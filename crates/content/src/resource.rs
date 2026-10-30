//! Direct bindings for runtime-selected resource IDs. Empty slots are null;
//! shared slots select one other ID. Payloads remain ordinary editable files.
use crate::{
    library::{Snapshot, validate_path},
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PATH: &str = "resources/mappings.json";
pub type Mappings = BTreeMap<u32, Option<Resource>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Resource {
    Geometry(String),
    /// One standard GLB clip, including its own joints and curve bindings.
    Animation(String),
    Textures(String),
    Collision(String),
    Script {
        script: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        messages: Option<String>,
    },
    /// Named parts of one model. Absent parts need no empty declaration.
    Parts(BTreeMap<String, Resource>),
    /// A direct reference to another mapping entry, never another alias.
    Alias(u32),
    Unavailable,
    Unsupported,
}

pub fn resolve(mappings: &Mappings, id: u32) -> Result<Option<&Resource>> {
    let value = mappings
        .get(&id)
        .with_context(|| format!("unmapped resource {id}"))?;
    if let Some(Resource::Alias(target)) = value {
        let target = mappings
            .get(target)
            .with_context(|| format!("resource {id} aliases missing {target}"))?;
        ensure!(
            !matches!(target, Some(Resource::Alias(_))),
            "resource {id} aliases another alias"
        );
        Ok(target.as_ref())
    } else {
        Ok(value.as_ref())
    }
}

impl Resource {
    fn visit(&self, depth: usize, action: &mut impl FnMut(&Resource) -> Result<()>) -> Result<()> {
        ensure!(depth <= 16, "resource parts exceed 16 levels");
        match self {
            Self::Parts(parts) => {
                for (name, part) in parts {
                    ensure!(
                        !name.is_empty() && !name.contains('/'),
                        "invalid resource part name"
                    );
                    part.visit(depth + 1, action)?;
                }
            }
            Self::Alias(_) => anyhow::bail!("aliases belong in the top-level resource mapping"),
            resource => {
                for path in resource.paths() {
                    validate_path(path)?;
                }
                action(resource)?;
            }
        }
        Ok(())
    }

    fn paths(&self) -> impl Iterator<Item = &str> {
        match self {
            Self::Geometry(path)
            | Self::Animation(path)
            | Self::Textures(path)
            | Self::Collision(path) => [Some(path.as_str()), None],
            Self::Script { script, messages } => [Some(script.as_str()), messages.as_deref()],
            _ => [None, None],
        }
        .into_iter()
        .flatten()
    }

    fn limitation(&self) -> &'static str {
        match self {
            Self::Unavailable => "the declared resource has no playable content",
            Self::Unsupported => "This resource requires a specialized field binding.",
            Self::Geometry(_) => "physical geometry requires a field model and material binding",
            Self::Animation(_) => "animation curves require a target model binding",
            Self::Textures(_) => "texture images require an overlay or material binding",
            Self::Collision(_) => "collision data requires a model binding",
            Self::Script { .. } => "script data is not a model or animation binding",
            Self::Parts(_) => "physical package members require a field resource binding",
            Self::Alias(_) => unreachable!("aliases are resolved before describing a resource"),
        }
    }
}

pub fn references(mappings: &Mappings) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for &id in mappings.keys() {
        if let Some(resource) = resolve(mappings, id)? {
            resource.visit(0, &mut |resource| {
                for path in resource.paths() {
                    paths.insert(path.into());
                }
                Ok(())
            })?;
        }
    }
    Ok(paths)
}

fn limitations(mappings: &Mappings, path: &str) -> Result<BTreeMap<u32, String>> {
    mappings
        .keys()
        .map(|&id| {
            let resource = resolve(mappings, id)?;
            if let Some(resource) = resource {
                resource.visit(0, &mut |_| Ok(()))?;
            }
            let reason = resource.map_or("the catalogue slot is empty", Resource::limitation);
            Ok((id, format!("{path} ID {id}: {reason}")))
        })
        .collect()
}

/// Current dynamic services need declarations to explain unsupported bindings,
/// not decoded copies of every possible mesh and animation. Capture only the
/// mapping. Concrete gameplay models are prepared through their own readers.
pub fn capture(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<BTreeMap<u32, String>> {
    ensure!(!cancelled(), "resource preparation cancelled");
    let mappings = snapshot.json(path, JSON_LIMIT)?;
    let result = limitations(&mappings, path)?;
    ensure!(!cancelled(), "resource preparation cancelled");
    Ok(result)
}

pub fn binding_limitations(
    files: &crate::prepared::Files,
    path: &str,
) -> Result<BTreeMap<u32, String>> {
    limitations(&files.json(path)?, path)
}

/// The importer can check all published payloads explicitly. Runtime declaration
/// capture never opens unrelated asset files or scans the library for changes.
pub fn admit(snapshot: &mut Snapshot, path: &str, cancelled: &impl Fn() -> bool) -> Result<()> {
    let mappings: Mappings = snapshot.json(path, JSON_LIMIT)?;
    let mut visited = BTreeSet::new();
    for &id in mappings.keys() {
        ensure!(!cancelled(), "resource preparation cancelled");
        let Some(resource) = resolve(&mappings, id)? else {
            continue;
        };
        resource.visit(0, &mut |resource| {
            ensure!(!cancelled(), "resource preparation cancelled");
            if !visited.insert(resource.clone()) {
                return Ok(());
            }
            match resource {
                Resource::Geometry(scene) => {
                    crate::physical_scene::capture(snapshot, scene, cancelled)?
                }
                Resource::Animation(path) => {
                    crate::animation::Motion::decode(&snapshot.read(path, ASSET_LIMIT)?)
                        .with_context(|| format!("decode animation {path}"))?;
                }
                Resource::Textures(path) => {
                    let catalogue = crate::physical_scene::TextureCatalogue::load(path, |path| {
                        snapshot.read(path, ASSET_LIMIT)
                    })?;
                    catalogue.validate()?;
                    for image in catalogue
                        .textures
                        .iter()
                        .flatten()
                        .flat_map(|texture| &texture.images)
                    {
                        ensure!(!cancelled(), "resource preparation cancelled");
                        snapshot.read(image, ASSET_LIMIT)?;
                    }
                }
                Resource::Collision(path) => {
                    crate::field::collision_mesh::decode_model(&snapshot.read(path, ASSET_LIMIT)?)
                        .with_context(|| format!("decode collision {path}"))?;
                }
                Resource::Script { script, messages } => {
                    snapshot.read(script, ASSET_LIMIT)?;
                    if let Some(path) = messages {
                        snapshot.read(path, JSON_LIMIT)?;
                    }
                }
                _ => (),
            }
            Ok(())
        })?;
    }
    Ok(())
}

//! Named physical resources for runtime-selected script IDs. Conversion and
//! gameplay binding are separate: a physical recipe is never a ready actor.
use crate::{
    library::{Mappings, Snapshot},
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 1;
pub const PATH: &str = "resources/catalogue.json";
pub const STANDALONE: &str = "resources/standalone/mappings.json";
pub const GROUPS: &str = "resources/groups/mappings.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub version: u32,
    pub standalone: String,
    /// Keys are the native group number, starting at one, not table offsets.
    pub groups: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub content: Resource,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Resource {
    Geometry {
        scene: String,
    },
    Animation {
        animation: String,
    },
    Textures {
        textures: String,
    },
    Collision {
        collision: String,
    },
    Script {
        script: String,
        messages: String,
    },
    /// Member mappings preserve the original scope and explicit native aliases.
    Members {
        mappings: String,
    },
    /// A declared resource with no playable content. Import source locations
    /// belong to external diagnostics, not to this runtime document.
    Unavailable,
    Empty,
    Unsupported {
        description: String,
    },
}

/// Skeleton-independent curves. Bone numbers identify native targets until a
/// model binds them; the curves themselves use the ordinary editable motion type.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub version: u32,
    pub duration_frames: f32,
    pub descriptors: Vec<AnimationDescriptor>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationDescriptor {
    pub name: Option<String>,
    pub indexed: bool,
    pub tracks: Vec<AnimationTrack>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationTrack {
    pub name: Option<String>,
    pub curve: crate::animation::Track,
}

fn definition(snapshot: &mut Snapshot, path: &str) -> Result<Definition> {
    let value: Definition = snapshot.json(path, JSON_LIMIT)?;
    ensure!(
        value.version == VERSION,
        "unsupported resource version in {path}"
    );
    Ok(value)
}

fn capture_resource(
    snapshot: &mut Snapshot,
    path: &str,
    visited: &mut BTreeMap<String, bool>,
    depth: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Resource> {
    ensure!(!cancelled(), "resource preparation cancelled");
    ensure!(depth <= 16, "resource nesting exceeds 16 levels at {path}");
    let value = definition(snapshot, path)?.content;
    if let Some(complete) = visited.get(path) {
        ensure!(*complete, "cyclic resource reference at {path}");
        return Ok(value);
    }
    visited.insert(path.into(), false);
    match &value {
        Resource::Geometry { scene } => crate::physical_scene::capture(snapshot, scene, cancelled)?,
        Resource::Animation { animation } => {
            let animation: Animation = snapshot.json(animation, JSON_LIMIT)?;
            ensure!(
                animation.version == VERSION
                    && animation.duration_frames.is_finite()
                    && animation.duration_frames >= 0.,
                "invalid unbound animation in {path}"
            );
        }
        Resource::Textures { textures } => {
            let catalogue: crate::physical_scene::TextureCatalogue =
                snapshot.json(textures, JSON_LIMIT)?;
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
        Resource::Collision { collision } => {
            let _: Vec<crate::field::CollisionGroup> = snapshot.json(collision, JSON_LIMIT)?;
        }
        Resource::Script { script, messages } => {
            snapshot.read(script, ASSET_LIMIT)?;
            snapshot.read(messages, JSON_LIMIT)?;
        }
        Resource::Members { mappings } => {
            let members = snapshot.mappings(mappings)?.clone();
            for (id, _) in members.iter() {
                let child = members.resolve(mappings, id, Some("resource.json"))?;
                capture_resource(snapshot, &child, visited, depth + 1, cancelled)?;
            }
        }
        Resource::Unsupported { description } => ensure!(
            !description.is_empty(),
            "unsupported resource needs a description in {path}"
        ),
        Resource::Empty | Resource::Unavailable => (),
    }
    visited.insert(path.into(), true);
    Ok(value)
}

/// Capture every declared choice before a dynamic field starts. Return explicit
/// binding limitations for the script service; loaded physical data is not a
/// substitute for the caller-specific model, palette or animation binding.
pub fn capture(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<BTreeMap<u32, String>> {
    ensure!(!cancelled(), "resource preparation cancelled");
    let catalogue: Catalogue = snapshot.json(path, JSON_LIMIT)?;
    ensure!(
        catalogue.version == VERSION,
        "unsupported resource catalogue in {path}"
    );
    let mut visited = BTreeMap::new();
    let mut limitations = BTreeMap::new();
    let standalone = snapshot.mappings(&catalogue.standalone)?.clone();
    for (id, _) in standalone.iter() {
        ensure!(
            id <= u32::from(u16::MAX),
            "standalone resource exceeds its ID scope"
        );
        let owner = standalone.resolve(&catalogue.standalone, id, Some("resource.json"))?;
        let resource = capture_resource(snapshot, &owner, &mut visited, 0, cancelled)?;
        limitations.insert(id, resource.binding_limitation(&owner));
    }
    let groups = snapshot.mappings(&catalogue.groups)?.clone();
    for (group, _) in groups.iter() {
        ensure!(
            (1..=u32::from(u16::MAX)).contains(&group),
            "invalid native resource group {group}"
        );
        let owner = groups.resolve(&catalogue.groups, group, Some("resource.json"))?;
        let resource = capture_resource(snapshot, &owner, &mut visited, 0, cancelled)?;
        if let Resource::Members { mappings } = resource {
            let members = snapshot.mappings(&mappings)?.clone();
            for (member, _) in members.iter() {
                ensure!(
                    member <= u32::from(u16::MAX),
                    "resource member exceeds its ID scope"
                );
                let path = members.resolve(&mappings, member, Some("resource.json"))?;
                let resource = definition(snapshot, &path)?.content;
                limitations.insert(group << 16 | member, resource.binding_limitation(&path));
            }
        } else {
            // Keep the group itself present even when its native table slot is empty.
            limitations.insert(group << 16, resource.binding_limitation(&owner));
        }
    }
    ensure!(!cancelled(), "resource preparation cancelled");
    Ok(limitations)
}

/// Describe unavailable dynamic bindings from the same bytes admitted with the
/// field. This never reopens a library or replaces a prepared model generation.
pub fn binding_limitations(
    files: &crate::prepared::Files,
    path: &str,
) -> Result<BTreeMap<u32, String>> {
    let catalogue: Catalogue = files.json(path)?;
    ensure!(
        catalogue.version == VERSION,
        "unsupported resource catalogue in {path}"
    );
    let mut result = BTreeMap::new();
    for (mapping, grouped) in [(&catalogue.standalone, false), (&catalogue.groups, true)] {
        let mappings: Mappings = files.json(mapping)?;
        for (id, _) in mappings.iter() {
            ensure!(
                id <= u32::from(u16::MAX) && (!grouped || id != 0),
                "invalid resource ID scope"
            );
            let owner = mappings.resolve(mapping, id, Some("resource.json"))?;
            let definition: Definition = files.json(&owner)?;
            ensure!(
                definition.version == VERSION,
                "unsupported resource version in {owner}"
            );
            if grouped && let Resource::Members { mappings } = &definition.content {
                let members: Mappings = files.json(mappings)?;
                for (member, _) in members.iter() {
                    ensure!(
                        member <= u32::from(u16::MAX),
                        "invalid resource member scope"
                    );
                    let owner = members.resolve(mappings, member, Some("resource.json"))?;
                    let definition: Definition = files.json(&owner)?;
                    ensure!(
                        definition.version == VERSION,
                        "unsupported resource version in {owner}"
                    );
                    result.insert(
                        id << 16 | member,
                        definition.content.binding_limitation(&owner),
                    );
                }
            } else {
                result.insert(
                    if grouped { id << 16 } else { id },
                    definition.content.binding_limitation(&owner),
                );
            }
        }
    }
    Ok(result)
}

impl Resource {
    fn binding_limitation(&self, owner: &str) -> String {
        let reason = match self {
            Self::Unavailable => "the declared resource has no playable content".into(),
            Self::Empty => "the catalogue slot is empty".into(),
            Self::Unsupported { description } => description.clone(),
            Self::Geometry { .. } => {
                "physical geometry requires a field model and material binding".into()
            }
            Self::Animation { .. } => "animation curves require a target model binding".into(),
            Self::Textures { .. } => "texture images require an overlay or material binding".into(),
            Self::Collision { .. } => "collision data requires a model binding".into(),
            Self::Script { .. } => "script data is not a model or animation binding".into(),
            Self::Members { .. } => {
                "physical package members require a field resource binding".into()
            }
        };
        format!("{owner}: {reason}")
    }
}

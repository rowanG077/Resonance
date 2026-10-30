//! Fields select shared models and animation banks. Only bindings
//! whose meaning belongs to one field are stored in that field's actor document.
use super::*;
use crate::SceneClip;

/// Script joint identifiers map to editable GLB node names once for the library.
pub const JOINT_MAPPINGS: &str = "meshes/joint-mappings.json";

mod models;
pub use models::{MODEL_GROUPS, ModelAlias, ModelMappings, ModelTarget};
mod animations;
pub use animations::{AnimationJoint, AnimationMappings, AnimationTarget};

pub const MODELS: &str = "meshes/mappings.json";
pub const ANIMATIONS: &str = "animations/mappings.json";
pub const ANIMATION_BINDINGS: &str = "animations/bindings.json";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actors {
    /// Resolve script-computed IDs at an explicit preparation boundary.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic_resources: bool,
    /// Field-owned IDs take precedence over the global standalone catalogue.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub local_resources: BTreeSet<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelBinding>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub animations: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub local_animations: BTreeMap<u32, Vec<AnimationBinding>>,
}

/// A named shared model, a reusable roster, or a field-specific actor. One list
/// preserves actor order while letting fields override any shared model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ModelBinding {
    Shared(String),
    Group { group: String },
    Local(Actor),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub resource: u32,
    pub model: String,
    /// Omit to use the model's default visibility. An empty override shows all nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_geometry: Option<Vec<String>>,
}

/// One GLB and gameplay slot can serve several actors and model parts. Keep
/// genuinely different rigs as separate bindings; never infer sharing from names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationBinding {
    pub actors: Vec<String>,
    pub parts: Vec<u16>,
    pub motion: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub joints: BTreeMap<String, Option<String>>,
    #[serde(default = "default_slot", skip_serializing_if = "is_default_slot")]
    pub resource_slot: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pose_chains: Vec<String>,
}

/// Shared bank targets refer to one reusable joint map. Local field motions
/// keep their joint overrides directly in `AnimationBinding`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedBankBindings {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rigs: BTreeMap<String, Vec<AnimationJoint>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<u32, Vec<AnimationJoint>>,
    pub joint_maps: BTreeMap<String, BTreeMap<String, Option<String>>>,
    pub banks: BTreeMap<u32, Vec<SharedAnimationBinding>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedAnimationBinding {
    pub actors: Vec<String>,
    pub parts: Vec<u16>,
    pub motion: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint_map: Option<String>,
    #[serde(default = "default_slot", skip_serializing_if = "is_default_slot")]
    pub resource_slot: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pose_chains: Vec<String>,
}

const fn default_slot() -> u16 {
    12
}
fn is_default_slot(slot: &u16) -> bool {
    *slot == default_slot()
}

impl Actors {
    /// Prepare only the model or motion bank chosen by a suspended script.
    pub fn capture_resource(
        snapshot: &mut crate::library::Snapshot,
        assets: &mut FieldAssets,
        resource: u32,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Vec<ActorAssets>> {
        let mappings: ModelMappings = snapshot.json(MODELS, crate::preparation::JSON_LIMIT)?;
        let animations: AnimationMappings =
            snapshot.json(ANIMATIONS, crate::preparation::JSON_LIMIT)?;
        let standalone = resource < 53 && !assets.local_resources.contains(&resource);
        let local_model = resource < 53 && assets.local_resources.contains(&resource);
        let animation = !local_model && animations.iter().any(|(id, _)| id == resource);
        let standalone_models: ModelMappings = if standalone && !animation {
            snapshot.json(
                "meshes/standalone/mappings.json",
                crate::preparation::JSON_LIMIT,
            )?
        } else {
            ModelMappings(BTreeMap::new())
        };
        let model = if standalone {
            standalone_models.0.contains_key(&resource)
        } else {
            mappings.0.contains_key(&resource)
        };
        if !animation && !model {
            let cameras: crate::library::Mappings =
                snapshot.json("cameras/mappings.json", crate::preparation::JSON_LIMIT)?;
            let path = cameras.resolve("cameras/mappings.json", resource, None)
                .with_context(|| format!("script resource {resource:#x} is absent from model, animation and camera mappings"))?;
            ensure!(!cancelled(), "resource preparation cancelled");
            let camera = crate::camera_motion::CameraMotion::decode(
                &snapshot.read(&path, crate::preparation::ASSET_LIMIT)?,
            )?;
            assets.camera_tracks.insert(resource, camera);
            return Ok(Vec::new());
        }
        let mut banks = assets
            .actors
            .iter()
            .flat_map(|actor| &actor.parts)
            .flat_map(|part| &part.clips)
            .filter_map(|clip| clip.animation_resource)
            .filter(|id| animations.target(*id).is_ok())
            .collect::<BTreeSet<_>>();
        if animation {
            banks = [resource].into();
        } else if standalone {
            banks.clear();
        }
        let selected = Self {
            models: if animation {
                Vec::new()
            } else if standalone {
                vec![ModelBinding::Local(Actor {
                    resource: super::super::STANDALONE_MODEL_RESOURCE_BASE | resource,
                    model: standalone_models.model_path(resource)?,
                    hidden_geometry: None,
                })]
            } else {
                vec![ModelBinding::Shared(mappings.0[&resource].name().into())]
            },
            animations: banks
                .into_iter()
                .map(|id| animations.target(id).map(str::to_owned))
                .collect::<Result<_>>()?,
            ..Default::default()
        };
        let mut read = |path: &str| {
            ensure!(!cancelled(), "script resource preparation cancelled");
            snapshot.read(path, crate::preparation::ASSET_LIMIT)
        };
        let mut actors = if animation {
            let mut actors = assets.actors.clone();
            Self::bind_animations(
                selected.animations,
                selected.local_animations,
                &mut actors,
                &mappings,
                &mut read,
            )?;
            actors
        } else {
            selected.assemble(&mut read)?
        };
        super::prepare_clips(
            actors.iter_mut().flat_map(|actor| &mut actor.parts),
            &mut read,
            animation.then_some(resource),
        )?;
        for part in actors.iter().flat_map(|actor| &actor.parts) {
            for path in std::iter::once(&part.mesh)
                .chain(&part.textures)
                .chain(part.clips.iter().map(|clip| &clip.motion))
            {
                read(path)?;
            }
        }
        Ok(actors)
    }

    pub(super) fn assemble(
        self,
        read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    ) -> Result<Vec<ActorAssets>> {
        let models = Self::expand_models(self.models, read)?;
        let mappings: ModelMappings = if models.iter().any(|b| matches!(b, ModelBinding::Shared(_)))
            || !self.animations.is_empty()
            || !self.local_animations.is_empty()
        {
            json(read, MODELS)?
        } else {
            ModelMappings(BTreeMap::new())
        };
        let names = mappings.by_name()?;
        let mut actors = Vec::with_capacity(models.len());
        let mut indices = BTreeMap::new();
        for model in models {
            let binding = match model {
                ModelBinding::Shared(name) => {
                    let &(resource, owner) = names
                        .get(name.as_str())
                        .with_context(|| format!("unknown model {name}"))?;
                    Actor {
                        resource,
                        model: format!("meshes/{owner}/model.json"),
                        hidden_geometry: None,
                    }
                }
                ModelBinding::Local(actor) => actor,
                ModelBinding::Group { .. } => unreachable!("expanded above"),
            };
            ensure!(
                indices.insert(binding.resource, actors.len()).is_none(),
                "duplicate actor resource {}",
                binding.resource
            );
            let mut model: Model = json(read, &binding.model)?;
            for part in &mut model.parts {
                part.prepare_bones(&read(&part.mesh)?)
                    .with_context(|| part.mesh.clone())?;
            }
            model.validate().with_context(|| binding.model.clone())?;
            let hidden_nodes = model
                .hidden_nodes(
                    binding
                        .hidden_geometry
                        .as_deref()
                        .unwrap_or(&model.hidden_geometry),
                )
                .with_context(|| binding.model.clone())?;
            actors.push(ActorAssets {
                resource: binding.resource,
                parts: model.parts,
                collision: match &model.collision {
                    Some(path) => collision_mesh::decode_model(&read(path)?)
                        .with_context(|| format!("decode {path}"))?,
                    None => ModelCollision::default(),
                },
                hidden_nodes,
            });
        }
        Self::bind_animations(
            self.animations,
            self.local_animations,
            &mut actors,
            &mappings,
            read,
        )?;
        Ok(actors)
    }

    fn bind_animations(
        animations: BTreeSet<String>,
        local_animations: BTreeMap<u32, Vec<AnimationBinding>>,
        actors: &mut [ActorAssets],
        mappings: &ModelMappings,
        read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    ) -> Result<()> {
        let names = mappings.by_name()?;
        let indices = actors
            .iter()
            .enumerate()
            .map(|(index, actor)| (actor.resource, index))
            .collect::<BTreeMap<_, _>>();
        let mut banks = local_animations;
        if !animations.is_empty() {
            let motions: AnimationMappings = json(read, ANIMATIONS)?;
            let definitions: SharedBankBindings = json(read, ANIMATION_BINDINGS)?;
            let mut bank_names = BTreeMap::new();
            for (id, name) in motions.iter() {
                ensure!(
                    bank_names.insert(name, id).is_none(),
                    "duplicate animation bank name {name}"
                );
            }
            for name in animations {
                let resource = *bank_names
                    .get(name.as_str())
                    .with_context(|| format!("unknown animation bank {name}"))?;
                ensure!(
                    !banks.contains_key(&resource),
                    "animation bank {resource} selected twice"
                );
                let bindings = definitions
                    .banks
                    .get(&resource)
                    .into_iter()
                    .flatten()
                    .map(|row| {
                        let joints = match &row.joint_map {
                            Some(name) => definitions
                                .joint_maps
                                .get(name)
                                .with_context(|| {
                                    format!(
                                        "animation bank {resource} has unknown joint map {name}"
                                    )
                                })?
                                .clone(),
                            None => BTreeMap::new(),
                        };
                        Ok(AnimationBinding {
                            actors: row.actors.clone(),
                            parts: row.parts.clone(),
                            motion: row.motion.clone(),
                            joints,
                            resource_slot: row.resource_slot,
                            pose_chains: row.pose_chains.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let fallback = definitions.fallback(resource, &motions, mappings, actors)?;
                ensure!(
                    fallback || !bindings.is_empty(),
                    "animation bank {resource} has no bindings for the loaded actors"
                );
                banks.insert(resource, bindings);
            }
        }
        for (resource, bank) in banks {
            let mut targets = BTreeSet::new();
            for binding in bank {
                validate_path(&binding.motion)?;
                ensure!(
                    !binding.actors.is_empty() && !binding.parts.is_empty(),
                    "animation bank {resource} has an empty target list"
                );
                for name in &binding.actors {
                    let &(actor, _) = names
                        .get(name.as_str())
                        .with_context(|| format!("unknown animation actor {name}"))?;
                    for part in &binding.parts {
                        ensure!(
                            targets.insert((actor, *part, binding.resource_slot)),
                            "duplicate animation target in bank {resource}: actor {actor}, part {part}, slot {}",
                            binding.resource_slot
                        );
                        // A shared bank can also serve characters absent from this field.
                        let Some(&index) = indices.get(&actor) else {
                            continue;
                        };
                        ensure!(
                            usize::from(*part) < actors[index].parts.len(),
                            "actor {actor} animation references missing part {part}"
                        );
                        let clip = SceneClip {
                            motion: binding.motion.clone(),
                            joints: binding.joints.clone(),
                            resource_slot: binding.resource_slot,
                            duration_seconds: 0.,
                            animation_resource: Some(resource),
                            pose_chains: binding.pose_chains.clone(),
                        };
                        let clips = &mut actors[index].parts[usize::from(*part)].clips;
                        if let Some(previous) = clips.iter_mut().find(|previous| {
                            previous.animation_resource == Some(resource)
                                && previous.resource_slot == binding.resource_slot
                        }) {
                            *previous = clip;
                        } else {
                            // Explicit bindings precede inherited defaults in
                            // this bank, preserving their own declaration order.
                            let at = clips
                                .iter()
                                .position(|existing| {
                                    existing.animation_resource == Some(resource)
                                        && !targets.contains(&(
                                            actor,
                                            *part,
                                            existing.resource_slot,
                                        ))
                                })
                                .unwrap_or(clips.len());
                            clips.insert(at, clip);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod installed_tests {
    use super::*;
    use std::path::Path;

    #[test]
    #[ignore = "requires the installed library; prepares representative shared-animation fields"]
    fn installed_shared_motions_prepare_representative_fields() -> Result<()> {
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let library = crate::library::Library::open(Path::new(&root), &[])?;
        for id in [332, 340, 359, 475] {
            let started = std::time::Instant::now();
            let mut snapshot = library.snapshot();
            let path = snapshot.field(id)?;
            let definition: super::super::Definition = snapshot.json(&path, 1024 * 1024)?;
            let mut reads = std::collections::BTreeMap::<String, usize>::new();
            let field = definition.assemble(id, |path| {
                *reads.entry(path.into()).or_default() += 1;
                snapshot.read(path, 64 * 1024 * 1024)
            })?;
            let mut motions = std::collections::BTreeMap::new();
            let parts = field
                .parts
                .iter()
                .chain(field.actors.iter().flat_map(|actor| &actor.parts));
            let mut bindings = 0;
            for part in parts {
                for clip in &part.clips {
                    assert_eq!(
                        reads[&clip.motion], 1,
                        "decode each shared file once per field"
                    );
                    bindings += 1;
                }
                crate::field::attachments::read(part, &mut motions, |path| {
                    snapshot.read(path, 64 * 1024 * 1024)
                })?;
            }
            println!(
                "{path}: {bindings} bindings, {} shared files, assembly and attachments {:.3}s",
                motions.len(),
                started.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires the installed library; reads current field bindings and mesh headers"]
    fn installed_fields_assemble_current_model_and_animation_bindings() -> Result<()> {
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let library = crate::library::Library::open(Path::new(&root), &[])?;
        let mut snapshot = library.snapshot();
        let mappings = snapshot.mappings(crate::library::FIELDS)?.clone();
        let mut count = 0;
        for (id, name) in mappings.iter() {
            let path = format!("fields/{name}/field.json");
            let definition: super::super::Definition = snapshot.json(&path, 1024 * 1024)?;
            let field =
                definition.assemble_documents(id, |path| snapshot.read(path, 64 * 1024 * 1024))?;
            for part in field
                .parts
                .iter()
                .chain(field.actors.iter().flat_map(|actor| &actor.parts))
            {
                part.validate(|_| true)?;
            }
            count += 1;
        }
        ensure!(count == 501, "expected all 501 fields, read {count}");
        println!(
            "All {count} fields assemble current models, collisions, visibility and animation bindings."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;

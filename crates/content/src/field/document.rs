//! Editable field files. Original IDs belong to mappings and actor bindings;
//! prepared gameplay objects are assembled from the current named resources.
use super::*;
use crate::{ScenePart, library::validate_path};
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::sync::Arc;

mod actors;
mod common;
pub use actors::{
    ANIMATIONS, Actor, Actors, AnimationBinding, AnimationJoint, AnimationMappings,
    AnimationTarget, JOINT_MAPPINGS, MODEL_GROUPS, MODELS, ModelAlias, ModelBinding, ModelMappings,
    ModelTarget, SharedAnimationBinding, SharedBankBindings,
};
pub use common::Common;

/// Capture the declared scene, including artwork only reached through effects
/// and overlay banks. Media preparation consumes `audio` and `movies` separately.
/// Every lookup uses the same effective content view and captured input bytes.
pub fn capture_scene(
    snapshot: &mut crate::library::Snapshot,
    id: u32,
    cancelled: impl Fn() -> bool,
) -> Result<(Definition, FieldAssets)> {
    let definition = Definition::read(snapshot, id)?;
    let field = definition.capture_scene(snapshot, id, cancelled, None, &mut |_, _| Ok(()))?;
    Ok((definition, field))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    pub scene: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<String>,
    pub actors: String,
    #[serde(
        default,
        skip_serializing_if = "crate::field_audio::document::Definition::is_empty"
    )]
    pub audio: crate::field_audio::document::Definition,
    /// All declared movie choices, independent of the current story branch.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub movies: BTreeSet<u32>,
}

impl Definition {
    pub fn read(snapshot: &mut crate::library::Snapshot, id: u32) -> Result<Self> {
        let path = snapshot.field(id)?;
        snapshot.json(&path, 1024 * 1024)
    }

    /// Capture scene dependencies from a definition read from this snapshot.
    /// `captured` publishes graphics before motion preparation, as in the
    /// checkpoint; every clip is still validated before gameplay admission.
    pub fn capture_scene(
        &self,
        snapshot: &mut crate::library::Snapshot,
        id: u32,
        cancelled: impl Fn() -> bool,
        common: Option<&Common>,
        captured: &mut dyn FnMut(&crate::library::Snapshot, &FieldAssets) -> Result<()>,
    ) -> Result<FieldAssets> {
        ensure!(!cancelled(), "field preparation cancelled");
        if let Some(common) = common {
            common.prune();
        }
        let path = snapshot.field(id)?;
        let mut field = self
            .assemble_documents(
                id,
                |path| {
                    ensure!(!cancelled(), "field preparation cancelled");
                    snapshot.read(path, 64 * 1024 * 1024)
                },
                common,
            )
            .with_context(|| format!("assemble {path}"))?;
        for part in field
            .parts
            .iter()
            .chain(field.actors.iter().flat_map(|actor| &actor.parts))
        {
            for texture in part
                .materials
                .iter()
                .flat_map(|material| material.color.iter().chain(&material.multiply))
                .filter_map(|binding| part.textures.get(binding.texture))
            {
                ensure!(!cancelled(), "field preparation cancelled");
                snapshot.read(texture, 64 * 1024 * 1024)?;
            }
        }
        snapshot.read(&field.toon_ramp, 64 * 1024 * 1024)?;
        captured(snapshot, &field)?;
        let mut read = |path: &str| {
            ensure!(!cancelled(), "field preparation cancelled");
            snapshot.read(path, 64 * 1024 * 1024)
        };
        prepare_clips(
            field
                .parts
                .iter_mut()
                .chain(field.actors.iter_mut().flat_map(|actor| &mut actor.parts)),
            &mut read,
            None,
            common,
            &mut Motions::new(),
        )?;
        field.validate().with_context(|| format!("field {id}"))?;
        for resource in self.references().chain(field.references()) {
            read(resource).with_context(|| format!("referenced by {path}"))?;
        }
        let settings = Common::settings(&read(&field.effects)?, common)
            .with_context(|| format!("decode {}", field.effects))?;
        let effects = &settings.effects;
        Common::validate_effects(&settings, common).with_context(|| field.effects.clone())?;
        for texture in effects.references() {
            read(texture).with_context(|| format!("referenced by {}", field.effects))?;
        }
        for overlay in field.overlays.values() {
            let art = crate::effect::OverlayArt::load(overlay, &mut read)?;
            art.validate().with_context(|| overlay.clone())?;
            for (index, texture) in art.textures.iter().enumerate() {
                if overlay == crate::input_prompts::FIELD_BANK_PATH
                    && !crate::input_prompts::PANEL_GLYPHS.contains(&index)
                {
                    continue;
                }
                if let Some(texture) = texture {
                    ensure!(
                        texture.size.is_some(),
                        "overlay {overlay} needs authored image coordinates"
                    );
                }
                for path in texture.iter().flat_map(|texture| &texture.images) {
                    read(path).with_context(|| format!("referenced by {overlay}"))?;
                }
            }
        }
        if field
            .overlays
            .values()
            .any(|path| path == crate::input_prompts::FIELD_BANK_PATH)
        {
            crate::input_prompts::capture(snapshot)?;
        }
        for path in field
            .unbound_geometry
            .iter()
            .flat_map(|geometry| &geometry.scenes)
        {
            crate::physical_scene::capture(snapshot, path, &cancelled)
                .with_context(|| format!("unbound geometry referenced by {}", self.scene))?;
        }
        Ok(field)
    }

    pub fn references(&self) -> impl Iterator<Item = &str> {
        [&self.script, &self.scene, &self.actors]
            .into_iter()
            .chain(self.messages.iter())
            .chain(self.collision.iter())
            .map(String::as_str)
    }

    pub fn validate(&self) -> Result<()> {
        for path in self.references() {
            validate_path(path)?;
        }
        Ok(())
    }

    /// The caller supplies captured bytes, so assembly and validation cannot
    /// observe different versions of a resource within this preparation.
    pub fn assemble(
        &self,
        map_id: u32,
        mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
    ) -> Result<FieldAssets> {
        let mut field = self.assemble_documents(map_id, &mut read, None)?;
        prepare_clips(
            field
                .parts
                .iter_mut()
                .chain(field.actors.iter_mut().flat_map(|actor| &mut actor.parts)),
            &mut read,
            None,
            None,
            &mut Motions::new(),
        )?;
        field
            .validate()
            .with_context(|| format!("field {map_id}"))?;
        Ok(field)
    }

    /// Assemble the same bindings for gameplay projection without decoding a
    /// shared motion again for each consumer. The caller supplies current clip
    /// durations and performs final field validation before using the result.
    pub(crate) fn assemble_documents(
        &self,
        map_id: u32,
        mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
        common: Option<&Common>,
    ) -> Result<FieldAssets> {
        self.validate()?;
        let mut scene: Scene = json(&mut read, &self.scene)?;
        let mut doors = Vec::new();
        for part in &mut scene.parts {
            let mesh = read(&part.mesh)?;
            if part.resource == 0 {
                doors.extend(super::doors::decode(&mesh, part.translation)?);
            }
            Common::prepare_geometry(part, mesh, common).with_context(|| part.mesh.clone())?;
        }
        let collision = match &self.collision {
            Some(path) => {
                collision_mesh::decode(&read(path)?).with_context(|| format!("decode {path}"))?
            }
            None => Collision::default(),
        };
        let bindings: Actors = json(&mut read, &self.actors)?;
        let dynamic_resources = bindings.dynamic_resources;
        let local_resources = bindings.local_resources.clone();
        let actors = bindings
            .assemble(&mut read, common)
            .with_context(|| self.actors.clone())?;
        let shared = Common::settings(&read(&scene.effects)?, common)
            .with_context(|| format!("decode {}", scene.effects))?;
        let particles = shared.resolve_particles(scene.particles);
        let save_point = scene.save_point.unwrap_or_else(|| {
            if actors
                .iter()
                .any(|actor| actor.resource == SAVE_POINT_RESOURCE)
            {
                shared.save_point.clone()
            } else {
                settings::SavePointText::default()
            }
        });
        let field = FieldAssets {
            joint_mappings: Common::joints(&read(JOINT_MAPPINGS)?, common)?,
            map_id,
            script: self.script.clone(),
            messages: self.messages.clone(),
            parts: scene.parts,
            panorama: scene.panorama,
            water: scene.water,
            environment: scene.environment.resolve(&shared.environment)?,
            ground: collision.ground,
            regions: collision.regions,
            doors,
            camera_tracks: scene
                .camera_tracks
                .into_iter()
                .map(|(id, path)| {
                    validate_path(&path)?;
                    Ok((
                        id,
                        crate::camera_motion::CameraMotion::decode(&read(&path)?)
                            .with_context(|| path)?,
                    ))
                })
                .collect::<Result<_>>()?,
            texture_animations: scene.texture_animations,
            actors,
            dynamic_resources,
            local_resources,
            unbound_geometry: scene.unbound_geometry,
            contact_shadow: scene
                .contact_shadow
                .unwrap_or_else(|| shared.contact_shadow.clone()),
            toon_ramp: scene.toon_ramp.unwrap_or_else(|| shared.toon_ramp.clone()),
            effects: scene.effects,
            blink: scene.blink.unwrap_or_else(|| shared.blink.clone()),
            particles,
            overlays: scene.overlays,
            save_point_tutorial: save_point.tutorial.0,
            save_point_unlock: save_point.unlock.0,
            save_point_no_gem: save_point.no_gem.0,
        };
        Ok(field)
    }
}

type Motions = BTreeMap<String, (Arc<[u8]>, Arc<crate::animation::NamedMotion>)>;

fn prepare_clips<'a>(
    parts: impl Iterator<Item = &'a mut ScenePart>,
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    bank: Option<u32>,
    common: Option<&Common>,
    motions: &mut Motions,
) -> Result<()> {
    // Match the checkpoint's source-grouped, four-worker preparation. Reads
    // remain on the snapshot thread; workers share only immutable captured bytes.
    struct Target<'a> {
        part: usize,
        clip: usize,
        bones: &'a [String],
        joints: &'a crate::JointMap,
        motion: Option<Arc<crate::animation::Motion>>,
    }
    struct Job<'a> {
        path: String,
        bytes: Arc<[u8]>,
        named: Option<Arc<crate::animation::NamedMotion>>,
        targets: Vec<Target<'a>>,
    }
    impl Job<'_> {
        fn prepare(&mut self, common: Option<&Common>) -> Result<()> {
            let named = match common {
                Some(common) => common.decode(&self.path, &self.bytes)?,
                None => match &self.named {
                    Some(named) => named.clone(),
                    None => Arc::new(crate::animation::NamedMotion::decode(&self.bytes)?),
                },
            };
            for target in &mut self.targets {
                target.motion = Some(
                    named
                        .bind_shared(target.bones, target.joints)
                        .with_context(|| format!("decode field motion {}", self.path))?,
                );
            }
            self.named = Some(named);
            Ok(())
        }
    }
    let parts: Vec<_> = parts.collect();
    let mut results: Vec<Vec<_>> = parts
        .iter()
        .map(|part| {
            (0..part.clips.len())
                .map(|index| {
                    part.retained_motion(index)
                        .map(|(bytes, motion)| (bytes.clone(), motion.clone()))
                })
                .collect()
        })
        .collect();
    let mut sources = vec![Vec::new(); parts.len()];
    let mut jobs = BTreeMap::<String, Job<'_>>::new();
    for (part_index, part) in parts.iter().enumerate() {
        for (clip_index, clip) in part.clips.iter().enumerate() {
            if bank.is_some_and(|bank| clip.animation_resource != Some(bank)) {
                continue;
            }
            if !jobs.contains_key(&clip.motion) {
                let bytes = read(&clip.motion)?;
                let named = motions
                    .get(&clip.motion)
                    .filter(|(captured, _)| crate::same_bytes(captured, &bytes))
                    .map(|(_, named)| named.clone());
                jobs.insert(
                    clip.motion.clone(),
                    Job {
                        path: clip.motion.clone(),
                        bytes,
                        named,
                        targets: Vec::new(),
                    },
                );
            }
            jobs.get_mut(&clip.motion).unwrap().targets.push(Target {
                part: part_index,
                clip: clip_index,
                bones: &part.bone_names,
                joints: &clip.joints,
                motion: None,
            });
        }
    }
    let mut jobs: Vec<_> = jobs.into_values().collect();
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4);
    if workers > 1 && jobs.len() >= 8 {
        let pending = std::sync::Mutex::new(jobs.iter_mut());
        std::thread::scope(|scope| -> Result<()> {
            let workers: Vec<_> = (0..workers)
                .map(|_| {
                    scope.spawn(|| -> Result<()> {
                        loop {
                            let Some(job) = pending.lock().unwrap().next() else {
                                return Ok(());
                            };
                            job.prepare(common)?;
                        }
                    })
                })
                .collect();
            // Always join every worker before returning or mutating any target.
            let results: Vec<_> = workers
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .map_err(|_| anyhow::anyhow!("animation preparation worker panicked"))?
                })
                .collect();
            results.into_iter().collect()
        })?;
    } else {
        jobs.iter_mut().try_for_each(|job| job.prepare(common))?;
    }
    for job in jobs {
        let named = job.named.unwrap();
        let mut retained_parts = BTreeSet::new();
        for target in job.targets {
            results[target.part][target.clip] = Some((job.bytes.clone(), target.motion.unwrap()));
            if common.is_some() && retained_parts.insert(target.part) {
                sources[target.part].push(named.clone());
            }
        }
        if common.is_none() {
            motions.insert(job.path, (job.bytes, named));
        }
    }
    for ((part, prepared), sources) in parts.into_iter().zip(results).zip(sources) {
        for (clip, motion) in part.clips.iter_mut().zip(&prepared) {
            if let Some((_, motion)) = motion {
                clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
            }
        }
        part.retain_motions_with_sources(prepared, sources);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub parts: Vec<ScenePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panorama: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<FieldWater>,
    #[serde(default, skip_serializing_if = "EnvironmentDocument::is_empty")]
    pub environment: EnvironmentDocument,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub camera_tracks: BTreeMap<u32, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texture_animations: Vec<FieldTextureAnimation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unbound_geometry: Vec<UnboundGeometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_shadow: Option<ContactShadow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toon_ramp: Option<String>,
    #[serde(rename = "settings")]
    pub effects: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blink: Option<crate::effect::BlinkCycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub particles: Option<BTreeMap<i32, crate::effect::FlutterRecipe>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overlays: BTreeMap<i32, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_point: Option<settings::SavePointText>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collision {
    pub ground: Vec<CollisionGroup>,
    pub regions: Vec<CollisionGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    #[serde(flatten, with = "crate::model_parts")]
    pub parts: Vec<ScenePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_geometry: Vec<String>,
}

impl Model {
    /// Resolve the selected visibility list against the current primary GLB.
    /// Callers choose model defaults or a field override before resolving names.
    pub fn hidden_nodes(&self, geometry: &[String]) -> Result<Vec<u16>> {
        let joints = self
            .parts
            .first()
            .map(|part| part.bone_names.as_slice())
            .unwrap_or_default();
        geometry
            .iter()
            .map(|name| {
                let mut matches = joints
                    .iter()
                    .enumerate()
                    .filter(|(_, joint)| *joint == name);
                let (index, _) = matches
                    .next()
                    .with_context(|| format!("unknown hidden geometry joint {name}"))?;
                ensure!(
                    matches.next().is_none(),
                    "ambiguous hidden geometry joint {name}"
                );
                Ok(u16::try_from(index)?)
            })
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        for part in &self.parts {
            part.validate(|_| true)?;
        }
        if let Some(path) = &self.collision {
            validate_path(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod visibility_tests;

/// Split a prepared field without changing any geometry, timing or gameplay data.
/// Publication supplies semantic paths and decides which model owners are shared.
pub fn split(
    mut field: FieldAssets,
    camera_tracks: BTreeMap<u32, String>,
    shared: &settings::Settings,
) -> Result<(Scene, Collision, Vec<ActorAssets>)> {
    let text = settings::SavePointText::from_field(&field);
    let expected_text = if field
        .actors
        .iter()
        .any(|actor| actor.resource == SAVE_POINT_RESOURCE)
    {
        shared.save_point.clone()
    } else {
        settings::SavePointText::default()
    };
    let particles = if field.particles == shared.resolve_particles(None) {
        None
    } else {
        for recipe in field.particles.values_mut() {
            if recipe.palette == shared.effects.palette {
                recipe.palette.clear();
            }
        }
        Some(field.particles)
    };
    let scene = Scene {
        parts: field.parts,
        panorama: field.panorama,
        water: field.water,
        environment: EnvironmentDocument::from_environment(
            &field.environment,
            &shared.environment,
        )?,
        camera_tracks,
        texture_animations: field.texture_animations,
        unbound_geometry: field.unbound_geometry,
        contact_shadow: (field.contact_shadow != shared.contact_shadow)
            .then_some(field.contact_shadow),
        toon_ramp: (field.toon_ramp != shared.toon_ramp).then_some(field.toon_ramp),
        effects: field.effects,
        blink: (field.blink != shared.blink).then_some(field.blink),
        particles,
        overlays: field.overlays,
        save_point: (text != expected_text).then_some(text),
    };
    Ok((
        scene,
        Collision {
            ground: field.ground,
            regions: field.regions,
        },
        field.actors,
    ))
}

fn json<T: DeserializeOwned>(
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    path: &str,
) -> Result<T> {
    validate_path(path)?;
    serde_json::from_slice(&read(path)?).with_context(|| format!("decode {path}"))
}

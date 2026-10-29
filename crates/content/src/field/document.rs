//! Editable field files. Original IDs belong to mappings and actor bindings;
//! prepared gameplay objects are assembled from the current named resources.
use super::*;
use crate::{ScenePart, library::validate_path};
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::sync::Arc;

mod actors;
pub use actors::{
    ANIMATIONS, Actor, Actors, AnimationBinding, AnimationJoint, AnimationMappings,
    AnimationTarget, JOINT_MAPPINGS, MODEL_GROUPS, MODELS, ModelAlias, ModelBinding, ModelMappings,
    ModelTarget, SharedAnimationBinding, SharedBankBindings,
};

/// Capture the declared scene, including artwork only reached through effects
/// and overlay banks. Media preparation consumes `audio` and `movies` separately.
/// Every lookup uses the same effective content view and captured input bytes.
pub fn capture_scene(
    snapshot: &mut crate::library::Snapshot,
    id: u32,
    cancelled: impl Fn() -> bool,
) -> Result<(Definition, FieldAssets)> {
    ensure!(!cancelled(), "field preparation cancelled");
    let path = snapshot.field(id)?;
    let definition: Definition = snapshot.json(&path, 1024 * 1024)?;
    let mut read = |path: &str| {
        ensure!(!cancelled(), "field preparation cancelled");
        snapshot.read(path, 64 * 1024 * 1024)
    };
    let field = definition
        .assemble(id, &mut read)
        .with_context(|| format!("assemble {path}"))?;
    for resource in definition.references().chain(field.references()) {
        read(resource).with_context(|| format!("referenced by {path}"))?;
    }
    let effects = json::<settings::Settings>(&mut read, &field.effects)?.effects;
    effects.validate().with_context(|| field.effects.clone())?;
    for texture in effects.references() {
        read(texture).with_context(|| format!("referenced by {}", field.effects))?;
    }
    for overlay in field.overlays.values() {
        let art = crate::effect::OverlayArt::load(overlay, &mut read)?;
        art.validate().with_context(|| overlay.clone())?;
        for texture in art.references() {
            read(texture).with_context(|| format!("referenced by {overlay}"))?;
        }
    }
    for path in field
        .unbound_geometry
        .iter()
        .flat_map(|geometry| &geometry.scenes)
    {
        crate::physical_scene::capture(snapshot, path, &cancelled)
            .with_context(|| format!("unbound geometry referenced by {}", definition.scene))?;
    }
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
        let mut field = self.assemble_documents(map_id, &mut read)?;
        prepare_clips(
            field
                .parts
                .iter_mut()
                .chain(field.actors.iter_mut().flat_map(|actor| &mut actor.parts)),
            &mut read,
            None,
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
    ) -> Result<FieldAssets> {
        self.validate()?;
        let mut scene: Scene = json(&mut read, &self.scene)?;
        let mut doors = Vec::new();
        for part in &mut scene.parts {
            let mesh = read(&part.mesh)?;
            if part.resource == 0 {
                doors.extend(super::doors::decode(&mesh, part.translation)?);
            }
            part.prepare_bones(&mesh)
                .with_context(|| part.mesh.clone())?;
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
            .assemble(&mut read)
            .with_context(|| self.actors.clone())?;
        let shared: settings::Settings = json(&mut read, &scene.effects)?;
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
            joint_mappings: json(&mut read, JOINT_MAPPINGS)?,
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
            contact_shadow: scene.contact_shadow.unwrap_or(shared.contact_shadow),
            toon_ramp: scene.toon_ramp.unwrap_or(shared.toon_ramp),
            effects: scene.effects,
            blink: scene.blink.unwrap_or(shared.blink),
            particles,
            overlays: scene.overlays,
            save_point_tutorial: save_point.tutorial.0,
            save_point_unlock: save_point.unlock.0,
            save_point_no_gem: save_point.no_gem.0,
        };
        Ok(field)
    }
}

fn prepare_clips<'a>(
    parts: impl Iterator<Item = &'a mut ScenePart>,
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    bank: Option<u32>,
) -> Result<()> {
    let mut motions = std::collections::BTreeMap::new();
    for part in parts {
        for clip in &mut part.clips {
            if bank.is_some_and(|bank| clip.animation_resource != Some(bank)) {
                continue;
            }
            if !motions.contains_key(&clip.motion) {
                motions.insert(
                    clip.motion.clone(),
                    crate::animation::NamedMotion::decode(&read(&clip.motion)?)?,
                );
            }
            let motion = motions[&clip.motion]
                .bind(&part.bone_names, &clip.joints)
                .with_context(|| format!("decode field motion {}", clip.motion))?;
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
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

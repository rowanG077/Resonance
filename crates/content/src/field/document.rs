//! Editable field files. Original IDs belong to mappings and actor bindings;
//! prepared gameplay objects are assembled from the current named resources.
use super::*;
use crate::{ScenePart, library::validate_path};
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::sync::Arc;

pub const VERSION: u32 = 1;

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
    let effects: crate::effect::FieldEffects = json(&mut read, &field.effects)?;
    effects.validate().with_context(|| field.effects.clone())?;
    for texture in effects.references() {
        read(texture).with_context(|| format!("referenced by {}", field.effects))?;
    }
    for overlay in field.overlays.values() {
        let art: crate::effect::OverlayArt = json(&mut read, overlay)?;
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
    if let Some(catalogue) = &field.resource_catalogue {
        crate::resource::capture(snapshot, catalogue, &cancelled)
            .with_context(|| format!("dynamic resources referenced by {}", definition.scene))?;
    }
    Ok((definition, field))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub script: String,
    pub messages: String,
    pub scene: String,
    pub collision: String,
    pub actors: String,
    pub audio: String,
    /// All declared movie choices, independent of the current story branch.
    #[serde(default)]
    pub movies: BTreeSet<u32>,
}

impl Definition {
    pub fn references(&self) -> impl Iterator<Item = &str> {
        [
            &self.script,
            &self.messages,
            &self.scene,
            &self.collision,
            &self.actors,
            &self.audio,
        ]
        .into_iter()
        .map(String::as_str)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported editable field version {}",
            self.version
        );
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
        self.validate()?;
        let scene: Scene = json(&mut read, &self.scene)?;
        let collision: Collision = json(&mut read, &self.collision)?;
        let bindings: Vec<Actor> = json(&mut read, &self.actors)?;
        let mut ids = BTreeSet::new();
        let mut actors = Vec::with_capacity(bindings.len());
        for binding in bindings {
            ensure!(
                ids.insert(binding.resource),
                "duplicate actor resource {} in {}",
                binding.resource,
                self.actors
            );
            validate_path(&binding.model)?;
            let mut model: Model = json(&mut read, &binding.model)?;
            model.validate().with_context(|| binding.model.clone())?;
            for (part, clips) in binding.animations {
                let part = model.parts.get_mut(usize::from(part)).with_context(|| {
                    format!(
                        "actor {} animation references missing part {part}",
                        binding.resource
                    )
                })?;
                let mut unique: BTreeMap<_, _> = part
                    .clips
                    .drain(..)
                    .map(|clip| ((clip.animation_resource, clip.resource_slot), clip))
                    .collect();
                let mut incoming = BTreeSet::new();
                for clip in clips {
                    let key = (clip.animation_resource, clip.resource_slot);
                    ensure!(
                        incoming.insert(key),
                        "duplicate actor animation binding {key:?}"
                    );
                    unique.insert(key, clip);
                }
                part.clips = unique.into_values().collect();
            }
            actors.push(ActorAssets {
                resource: binding.resource,
                parts: model.parts,
                collision: model.collision,
                hidden_nodes: binding.hidden_nodes,
            });
        }
        let mut field = FieldAssets {
            version: FIELD_VERSION,
            map_id,
            script: self.script.clone(),
            messages: self.messages.clone(),
            parts: scene.parts,
            panorama: scene.panorama,
            water: scene.water,
            environment: scene.environment,
            ground: collision.ground,
            regions: collision.regions,
            doors: scene.doors,
            camera_tracks: scene.camera_tracks,
            texture_animations: scene.texture_animations,
            actors,
            unbound_geometry: scene.unbound_geometry,
            resource_catalogue: scene.resource_catalogue,
            contact_shadow: scene.contact_shadow,
            toon_ramp: scene.toon_ramp,
            effects: scene.effects,
            blink: scene.blink,
            particles: scene.particles,
            overlays: scene.overlays,
            save_point_tutorial: scene.save_point_tutorial,
            save_point_unlock: scene.save_point_unlock,
            save_point_no_gem: scene.save_point_no_gem,
        };
        for part in field
            .parts
            .iter_mut()
            .chain(field.actors.iter_mut().flat_map(|actor| &mut actor.parts))
        {
            for clip in &mut part.clips {
                let motion = crate::animation::Motion::decode(&read(&clip.motion)?)
                    .with_context(|| format!("decode field motion {}", clip.motion))?;
                motion.validate_bones(part.bone_names.len())?;
                clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
            }
        }
        field
            .validate()
            .with_context(|| format!("field {map_id}"))?;
        Ok(field)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub parts: Vec<ScenePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panorama: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<FieldWater>,
    #[serde(default, skip_serializing_if = "FieldEnvironment::is_empty")]
    pub environment: FieldEnvironment,
    #[serde(default)]
    pub doors: Vec<Door>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub camera_tracks: BTreeMap<u32, Vec<crate::CameraKey>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texture_animations: Vec<FieldTextureAnimation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unbound_geometry: Vec<UnboundGeometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_catalogue: Option<String>,
    pub contact_shadow: ContactShadow,
    pub toon_ramp: String,
    pub effects: String,
    pub blink: crate::effect::BlinkCycle,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub particles: BTreeMap<i32, crate::effect::FlutterRecipe>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overlays: BTreeMap<i32, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_point_tutorial: Vec<crate::font::TextSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_point_unlock: Vec<crate::font::TextSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_point_no_gem: Vec<crate::font::TextSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collision {
    pub ground: Vec<CollisionGroup>,
    pub regions: Vec<CollisionGroup>,
}

/// Instance-local settings stay with the field instead of modifying a shared model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub resource: u32,
    pub model: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_nodes: Vec<u16>,
    /// Field-specific clips by model part. Matching native resource/slot pairs
    /// override a model default; other clips extend the model's motion bank.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub animations: BTreeMap<u16, Vec<crate::SceneClip>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub version: u32,
    pub parts: Vec<ScenePart>,
    #[serde(default)]
    pub collision: ModelCollision,
}

impl Model {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported editable model version {}",
            self.version
        );
        for part in &self.parts {
            part.validate(|_| true)?;
        }
        for group in self.collision.floors.iter().chain(&self.collision.solids) {
            group.validate()?;
        }
        Ok(())
    }
}

/// Split a prepared field without changing any geometry, timing or gameplay data.
/// Publication supplies semantic paths and decides which model owners are shared.
pub fn split(field: FieldAssets) -> (Scene, Collision, Vec<ActorAssets>) {
    let scene = Scene {
        parts: field.parts,
        panorama: field.panorama,
        water: field.water,
        environment: field.environment,
        doors: field.doors,
        camera_tracks: field.camera_tracks,
        texture_animations: field.texture_animations,
        unbound_geometry: field.unbound_geometry,
        resource_catalogue: field.resource_catalogue,
        contact_shadow: field.contact_shadow,
        toon_ramp: field.toon_ramp,
        effects: field.effects,
        blink: field.blink,
        particles: field.particles,
        overlays: field.overlays,
        save_point_tutorial: field.save_point_tutorial,
        save_point_unlock: field.save_point_unlock,
        save_point_no_gem: field.save_point_no_gem,
    };
    (
        scene,
        Collision {
            ground: field.ground,
            regions: field.regions,
        },
        field.actors,
    )
}

fn json<T: DeserializeOwned>(
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    path: &str,
) -> Result<T> {
    validate_path(path)?;
    serde_json::from_slice(&read(path)?).with_context(|| format!("decode {path}"))
}

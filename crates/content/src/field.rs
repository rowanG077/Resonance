//! High-level field data. Coordinates retain the authored Z-up world space.
use crate::{ScenePart, validate_asset_path};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FIELD_VERSION: u32 = 11;
/// Reserved resource range for static scenery, separate from character models.
pub const SCENERY_RESOURCE_BASE: u32 = 0x1000_0000;
/// Ordinary actor models embedded in a field archive, addressed by signed script IDs.
pub const LOCAL_MODEL_RESOURCES: std::ops::Range<u32> = 0xffee_0000..0xffef_0000;
/// Shared save-point model, addressed by the field service rather than scripts.
pub const SAVE_POINT_RESOURCE: u32 = 0x2000_0000;
/// Character-specific field service animations, separate from script banks.
pub const FIELD_SERVICE_MOTION_RESOURCE_BASE: u32 = 0x2100_0000;
/// Shared automatic flight accessory (native col_wing resource).
pub const COLETTE_WINGS_RESOURCE: u32 = 0x2200_0000;
#[derive(Clone, Copy)]
#[repr(u16)]
pub enum ServiceMotion {
    OpenDoor = 20,
    PullDoor = 24,
    HoldBlock = 32,
    PushBlock = 36,
    PullBlock = 40,
    CastRing = 52,
}
impl ServiceMotion {
    pub const ALL: [Self; 6] = [
        Self::OpenDoor,
        Self::PullDoor,
        Self::HoldBlock,
        Self::PushBlock,
        Self::PullBlock,
        Self::CastRing,
    ];
}
/// One model part's authored material orders. Actor bodies and outlines use two parts.
pub const MODEL_DRAW_SPAN: u32 = 1 << 16;

/// Field submission stages; each actor stage reserves both body and outline ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DrawStage {
    Background = 0,
    SecondaryBackground = 1,
    Actors = 2,
    Foreground = 4,
    /// Reserved for the native late actor pass, not yet represented by field actors.
    LateActors = 5,
    TranslucentScenery = 7,
}

impl DrawStage {
    pub const fn offset(self) -> u32 {
        self as u32 * MODEL_DRAW_SPAN
    }

    pub const fn scenery(section: u16) -> Option<Self> {
        match section {
            0 => Some(Self::Background),
            10 => Some(Self::SecondaryBackground),
            12 => Some(Self::Foreground),
            2 => Some(Self::TranslucentScenery),
            _ => None,
        }
    }
}
pub fn metadata_path(map: u32) -> String {
    format!("fields/map-{map}.json")
}

pub fn audio_path(map: u32) -> String {
    format!("fields/map-{map}-audio.json")
}
/// Native field treasure models: ordinary, reinforced and bag-shaped.
pub const TREASURE_RESOURCE_BASE: u32 = 0x7fff_0100;

pub fn preload_path(map: u32) -> String {
    format!("fields/map-{map}.preload.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionGroup {
    /// Surface classification and collision query mask used by field services.
    pub surface: u32,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum CollisionQuery {
    All = 0,
    Player = 1 << 19,
    Enemy = (1 << 19) | (1 << 20),
    Block = 1 << 21,
}

impl CollisionQuery {
    pub fn accepts(self, surface: u32) -> bool {
        surface & self as u32 == 0
    }
}

impl CollisionGroup {
    pub fn validate(&self) -> Result<()> {
        // Authored empty groups retain their slot and surface classification.
        ensure!(
            self.vertices.len() <= 65536,
            "invalid collision vertex count"
        );
        ensure!(
            self.triangles.len() <= 65536,
            "invalid collision triangle count"
        );
        ensure!(
            self.vertices.iter().flatten().all(|v| v.is_finite()),
            "nonfinite collision vertex"
        );
        ensure!(
            self.triangles
                .iter()
                .flatten()
                .all(|v| usize::from(*v) < self.vertices.len()),
            "collision index exceeds vertices"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAssets {
    pub version: u32,
    pub map_id: u32,
    pub source_sha256: String,
    pub script: String,
    pub messages: String,
    pub parts: Vec<ScenePart>,
    pub ground: Vec<CollisionGroup>,
    pub regions: Vec<CollisionGroup>,
    pub doors: Vec<Door>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub camera_tracks: BTreeMap<u32, Vec<crate::CameraKey>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texture_animations: Vec<FieldTextureAnimation>,
    #[serde(default)]
    pub actors: Vec<ActorAssets>,
    /// Geometry recipes requiring a caller texture binding before instantiation.
    pub unbound_geometry: Vec<UnboundGeometry>,
    /// Full resource catalogue used by scripts that select resources at runtime.
    pub resource_catalogue: Option<String>,
    pub contact_shadow: ContactShadow,
    pub toon_ramp: String,
    pub effects: String,
    pub blink: crate::effect::BlinkCycle,
    #[serde(default)]
    pub particles: BTreeMap<i32, crate::effect::FlutterRecipe>,
    pub overlays: BTreeMap<i32, String>,
    #[serde(default)]
    pub save_point_tutorial: Vec<crate::font::TextSpan>,
    #[serde(default)]
    pub save_point_unlock: Vec<crate::font::TextSpan>,
    #[serde(default)]
    pub save_point_no_gem: Vec<crate::font::TextSpan>,
}

/// Native field callback operands: either a fixed value or a live slot written
/// by ConfigureRendering. Actor and texture targets use the same slot table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenderValue {
    Fixed(i32),
    Setting(u8),
    SettingOffset { slot: u8, offset: i32 },
}
impl RenderValue {
    pub fn resolve(&self, settings: &BTreeMap<i32, i32>) -> i32 {
        match *self {
            Self::Fixed(value) => value,
            Self::Setting(slot) => settings.get(&i32::from(slot)).copied().unwrap_or(0),
            Self::SettingOffset { slot, offset } => settings
                .get(&i32::from(slot))
                .copied()
                .unwrap_or(0)
                .wrapping_add(offset),
        }
    }
}

mod texture_animation;
pub use texture_animation::{FieldTextureAnimation, FieldTextureWave, TextureClock, TextureMotion};

/// A scenery hinge and the standing pose used to open it before a field exit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Door {
    pub bone: u16,
    pub position: [f32; 3],
    pub approach: [f32; 3],
    pub heading: f32,
    pub pull: bool,
    pub angle: f32,
}

/// A soft textured ground quad, positioned from an animated actor joint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactShadow<Image = String> {
    pub texture: Image,
    pub uv_size: [f32; 2],
    pub half_size: f32,
    pub height_offset: f32,
    pub alpha: u8,
    pub anchor_node: u16,
}

impl<Image: AsRef<str>> ContactShadow<Image> {
    pub fn validate(&self) -> Result<()> {
        validate_asset_path(self.texture.as_ref())?;
        ensure!(
            self.uv_size
                .iter()
                .all(|v| v.is_finite() && *v > 0. && *v <= 1.)
                && self.half_size.is_finite()
                && (0. ..=1024.).contains(&self.half_size)
                && self.height_offset.is_finite()
                && (0. ..=32.).contains(&self.height_offset),
            "invalid contact shadow recipe"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorAssets {
    pub resource: u32,
    pub parts: Vec<ScenePart>,
    pub collision: ModelCollision,
    /// Attachment geometry disabled when this field actor is created.
    #[serde(default)]
    pub hidden_nodes: Vec<u16>,
}

/// Actor-local geometry: native package slots 29 (floors) and 30 (solid volumes).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCollision {
    pub floors: Vec<CollisionGroup>,
    pub solids: Vec<CollisionGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnboundGeometry {
    pub resource: u32,
    /// Physical scene descriptors retain meshes, draw recipes and palette requirements.
    pub scenes: Vec<String>,
}

impl FieldAssets {
    pub fn references(&self) -> impl Iterator<Item = &str> {
        [
            &self.script,
            &self.messages,
            &self.contact_shadow.texture,
            &self.toon_ramp,
            &self.effects,
        ]
        .into_iter()
        .chain(self.resource_catalogue.iter())
        .chain(
            self.unbound_geometry
                .iter()
                .flat_map(|geometry| &geometry.scenes),
        )
        .chain(self.overlays.values())
        .chain(self.particles.values().map(|recipe| &recipe.texture))
        .chain(
            self.parts
                .iter()
                .chain(self.actors.iter().flat_map(|actor| &actor.parts))
                .flat_map(|part| {
                    std::iter::once(&part.mesh)
                        .chain(&part.textures)
                        .chain(part.clips.iter().map(|clip| &clip.motion))
                }),
        )
        .map(String::as_str)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == FIELD_VERSION, "unsupported field assets");
        for animation in &self.texture_animations {
            animation.validate()?;
        }
        for actor in &self.actors {
            for group in actor.collision.floors.iter().chain(&actor.collision.solids) {
                group.validate()?;
            }
        }
        for track in self.camera_tracks.values() {
            ensure!(
                track.len() >= 2
                    && track.len() <= 100_000
                    && track.iter().all(|key| key.time.is_finite()
                        && key.time >= 0.
                        && key
                            .position
                            .iter()
                            .chain(&key.target)
                            .all(|v| v.is_finite()))
                    && track.windows(2).all(|keys| keys[0].time < keys[1].time),
                "invalid field camera track"
            );
        }
        for actor in &self.actors {
            ensure!(
                actor.parts.len() <= 2
                    && actor.parts.iter().all(|part| {
                        part.resource < 2
                            && part.materials.iter().all(|material| {
                                material.draw_order / MODEL_DRAW_SPAN == u32::from(part.resource)
                            })
                    }),
                "field actor exceeds its body/outline draw range"
            );
        }
        for part in &self.parts {
            ensure!(
                DrawStage::scenery(part.resource).is_some_and(|stage| {
                    part.materials
                        .iter()
                        .all(|material| material.draw_order / MODEL_DRAW_SPAN == stage as u32)
                }),
                "invalid field scenery draw range; recook the field"
            );
        }
        validate_asset_path(&self.script)?;
        self.blink.validate()?;
        ensure!(self.doors.len() <= 128, "too many scenery doors");
        let mut hinges = std::collections::BTreeSet::new();
        for door in &self.doors {
            ensure!(
                hinges.insert(door.bone)
                    && self
                        .parts
                        .iter()
                        .any(|part| part.resource == 0
                            && usize::from(door.bone) < part.bone_names.len())
                    && door
                        .position
                        .iter()
                        .chain(&door.approach)
                        .all(|v| v.is_finite())
                    && (0. ..360.).contains(&door.heading)
                    && (1. ..=180.).contains(&door.angle.abs()),
                "invalid scenery door"
            );
        }
        validate_asset_path(&self.messages)?;
        if let Some(path) = &self.resource_catalogue {
            validate_asset_path(path)?;
        }
        let mut geometry_resources = std::collections::BTreeSet::new();
        for geometry in &self.unbound_geometry {
            ensure!(
                geometry_resources.insert(geometry.resource)
                    && !self
                        .actors
                        .iter()
                        .any(|actor| actor.resource == geometry.resource)
                    && !geometry.scenes.is_empty(),
                "duplicate or empty unbound geometry"
            );
            for scene in &geometry.scenes {
                validate_asset_path(scene)?;
            }
        }
        let shadow = &self.contact_shadow;
        validate_asset_path(&self.toon_ramp)?;
        validate_asset_path(&self.effects)?;
        ensure!(self.particles.len() <= 256, "too many particle recipes");
        for text in [
            &self.save_point_tutorial,
            &self.save_point_unlock,
            &self.save_point_no_gem,
        ] {
            ensure!(text.len() <= 128, "system notice is too long");
            for span in text {
                span.validate()?;
            }
        }
        ensure!(
            !self
                .actors
                .iter()
                .any(|a| a.resource == SAVE_POINT_RESOURCE)
                || (!self.save_point_tutorial.is_empty()
                    && !self.save_point_unlock.is_empty()
                    && !self.save_point_no_gem.is_empty()),
            "memory-circle tutorial is missing; recook the field"
        );
        for overlay in self.overlays.values() {
            validate_asset_path(overlay)?;
        }
        for recipe in self.particles.values() {
            recipe.validate()?;
        }

        shadow.validate()?;

        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|v| v.is_ascii_hexdigit()),
            "invalid field source digest"
        );
        for group in self.ground.iter().chain(&self.regions) {
            group.validate()?;
        }
        for part in self
            .parts
            .iter()
            .chain(self.actors.iter().flat_map(|c| &c.parts))
        {
            part.validate(|_| true)?;
        }
        for actor in &self.actors {
            ensure!(
                actor.parts.first().is_some_and(|part| actor
                    .hidden_nodes
                    .iter()
                    .all(|node| usize::from(*node) < part.bone_names.len())),
                "invalid initial actor visibility"
            );
        }
        Ok(())
    }
}

impl crate::ScenePart {
    pub fn validate(&self, available: impl Fn(&str) -> bool) -> Result<()> {
        for chain in &self.secondary_motion.chains {
            chain.validate(self.bone_names.len())?;
        }
        ensure!(
            self.clips
                .iter()
                .flat_map(|c| &c.secondary_pose_nodes)
                .all(|node| usize::from(*node) < self.bone_names.len()),
            "secondary animation node exceeds skeleton"
        );
        ensure!(
            self.material_nodes.is_empty() || self.material_nodes.len() == self.materials.len(),
            "field material node mapping is incomplete"
        );
        ensure!(
            self.material_nodes
                .iter()
                .flatten()
                .all(|node| usize::from(*node) < self.bone_names.len()),
            "field material node index exceeds skeleton"
        );
        validate_asset_path(&self.mesh)?;
        ensure!(
            available(&self.mesh),
            "field mesh is missing from dependency inventory"
        );
        for texture in self
            .textures
            .iter()
            .chain(self.clips.iter().map(|clip| &clip.motion))
        {
            validate_asset_path(texture)?;
            ensure!(
                available(texture),
                "field texture is missing from dependency inventory"
            );
        }
        ensure!(
            self.translation.iter().all(|v| v.is_finite()),
            "invalid field translation"
        );
        for material in &self.materials {
            ensure!(
                material
                    .color
                    .iter()
                    .chain(&material.multiply)
                    .all(|b| b.texture < self.textures.len()),
                "invalid field material texture"
            );
        }
        Ok(())
    }
}

/// Prepared local geometry used by field ring effects.
pub const RING_BEAM_RESOURCE: u32 = LOCAL_MODEL_RESOURCES.start;
pub const RING_BOMB_RESOURCE: u32 = LOCAL_MODEL_RESOURCES.start + 12;

/// Room scenery required by abilities that use local geometry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RingScenery {
    #[default]
    None,
    Bomb,
    Bubble,
    Sunlight,
}
impl RingScenery {
    pub fn for_field(map: u32) -> Self {
        match map {
            412..=415 => Self::Bomb,
            492..=498 => Self::Bubble,
            511..=518 => Self::Sunlight,
            _ => Self::None,
        }
    }
}

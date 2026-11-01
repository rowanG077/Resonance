//! Cooked asset contracts shared by the importer and game.
pub(crate) mod defaults;
pub mod diagnostics;
use serde::{Deserialize, Serialize};
pub mod animation;
pub mod appearance;
pub mod arte;
pub mod battle_action;
pub mod battle_audio;
pub mod battle_conditions;
pub mod battle_defense_system;
pub mod battle_effect;
pub mod battle_enemy;
pub mod battle_event;
pub mod battle_formation;
pub mod battle_martial;
pub mod battle_model;
pub mod battle_motion;
pub mod battle_normal;
pub mod battle_profile;
pub mod battle_projectile;
pub mod battle_recoil;
pub mod battle_rules;
pub mod battle_scene;
pub mod battle_stage;
pub mod battle_ui;
pub mod battle_unison;
pub mod battle_victory;
pub mod battle_voice;
pub mod camera_motion;
pub mod characters;
mod collision_mesh;
pub mod cooking;
pub mod credits;
pub mod effect;
pub mod ex_skills;
pub mod field;
pub mod field_audio;
pub mod figurine;
pub mod font;
pub mod foraging;
pub mod game_over;
pub mod grade;
pub mod input;
pub mod input_prompts;
pub mod items;
pub mod library;
pub mod menu;
pub mod menu_data;
pub mod model_behavior;
pub(crate) mod model_parts;
pub mod model_preview;
pub mod monster;
pub mod movie;
pub mod music;
pub mod overworld;
pub mod physical_scene;
pub mod preparation;
pub mod prepared;
pub mod random;
pub mod resource;
pub(crate) mod scene_clips;
mod scene_materials;
pub mod secondary_motion;
pub mod session;
pub mod skit;
pub mod source;
pub mod startup;
pub mod texture;
mod texture_timeline;
pub mod titles;
mod ui_art;

pub const CONTENT_VERSION: u32 = 6;
pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
/// Authored animation time unit; simulation runs at its separate NTSC cadence.
pub const ANIMATION_HZ: f32 = 60.;
/// Scene rows retained inside the full-height UI/movie framebuffer.
pub const SCENE_HEIGHT: u32 = 448;

pub use startup::boot::Sequence as BootAssets;

pub fn validate_asset_path(name: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(name);
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains(['\\', ':', '#'])
            && !path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "unsafe asset path {name}"
    );
    Ok(())
}

pub type TitleAssets = Vec<startup::Artwork>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenePart {
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub resource: u16,
    pub mesh: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "scene_materials"
    )]
    pub materials: Vec<SceneMaterial>,
    #[serde(default, skip_serializing_if = "AppearanceTextures::is_empty")]
    pub appearance: Option<AppearanceTextures>,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub translation: [f32; 3],
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "scene_clips")]
    pub clips: Vec<SceneClip>,
    /// Play the field's default clip from scene entry, independently of actors.
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub autoplay: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texture_animations: Vec<TextureAnimation>,
    /// Prepared from the mesh's indexed nodes; the GLB owns joint names.
    #[serde(skip)]
    pub bone_names: Vec<String>,
    /// Prepared from each mesh node's nearest indexed GLB ancestor. A node
    /// toggle hides its attached geometry without hiding skeletal children.
    #[serde(skip)]
    pub material_nodes: Vec<Vec<u16>>,
    /// Color for untextured draws in this layer's inverted outline hull.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<[u8; 4]>,
    #[serde(
        default,
        skip_serializing_if = "secondary_motion::Definition::is_empty"
    )]
    pub secondary_motion: secondary_motion::Definition,
}

impl ScenePart {
    pub fn prepare_bones(&mut self, mesh: &[u8]) -> anyhow::Result<()> {
        (self.bone_names, self.material_nodes) = animation::scene_nodes_from_glb(mesh)?;
        Ok(())
    }

    pub fn outline_color_for(&self, material: &SceneMaterial) -> Option<[u8; 4]> {
        self.outline_color
            .filter(|_| material.color.is_none() && material.multiply.is_none())
    }
}

/// Optional vertical atlas channels supplied by character model metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceTextures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eyes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouth: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<AtlasChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costume: Option<usize>,
}
impl AppearanceTextures {
    fn is_empty(value: &Option<Self>) -> bool {
        value.as_ref().is_none_or(|value| {
            value.eyes.is_none()
                && value.mouth.is_none()
                && value.variant.is_none()
                && value.costume.is_none()
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasChannel {
    pub texture: usize,
    pub frames: u8,
}

/// Sampled UV translations at the scene's 60 Hz clock, with an optional intro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureAnimation<Texture = usize> {
    #[serde(default, skip_serializing_if = "texture_timeline::no_target")]
    pub texture: Texture,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub delay_ticks: u32,
    #[serde(
        default,
        rename = "loop_from",
        skip_serializing_if = "crate::defaults::is_default"
    )]
    pub loop_start: usize,
    #[serde(rename = "sequence", with = "texture_timeline")]
    pub offsets: Vec<[f32; 2]>,
}

impl<Texture> TextureAnimation<Texture> {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.offsets.is_empty()
                && self.offsets.len() <= 36000
                && self.loop_start < self.offsets.len()
                && self.offsets.iter().flatten().all(|v| v.is_finite()),
            "invalid texture animation"
        );
        Ok(())
    }

    /// Requires an animation checked with `Self::validate`.
    pub fn offset(&self, tick: u64) -> [f32; 2] {
        let elapsed = tick.saturating_sub(u64::from(self.delay_ticks));
        let length = self.offsets.len() as u64;
        let start = self.loop_start as u64;
        let index = if elapsed < length {
            elapsed
        } else {
            start + (elapsed - start) % (length - start)
        };
        self.offsets[index as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneClip {
    /// Shared authored curves, independent of the geometry and scene instance.
    pub motion: String,
    /// Overrides for model-joint -> clip-joint binding; null leaves a joint at rest.
    /// Other joints bind by name, so ordinary compatible rigs need no table.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub joints: std::collections::BTreeMap<String, Option<String>>,
    pub resource_slot: u16,
    /// Prepared from the current motion file. Editable bindings omit this value.
    #[serde(default, skip_serializing_if = "SceneClip::duration_is_derived")]
    pub duration_seconds: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_resource: Option<u32>,
    /// Named hair/cloth chains that use their pose-attraction setting for this clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pose_chains: Vec<String>,
}
impl SceneClip {
    pub fn decode_for_skeleton(
        &self,
        bytes: &[u8],
        skeleton: &animation::Skeleton,
    ) -> anyhow::Result<animation::Motion> {
        self.decode_motion(
            bytes,
            &skeleton
                .bones
                .iter()
                .map(|bone| bone.name.clone())
                .collect::<Vec<_>>(),
        )
    }

    pub fn decode_motion(
        &self,
        bytes: &[u8],
        bones: &[String],
    ) -> anyhow::Result<animation::Motion> {
        animation::NamedMotion::decode(bytes)?.bind(bones, &self.joints)
    }

    fn duration_is_derived(duration: &f32) -> bool {
        *duration == 0.
    }

    pub fn duration_ticks(&self) -> u32 {
        (self.duration_seconds * ANIMATION_HZ).round() as u32
    }
}

/// Ordinary material recipes. Original GPU command words stay in the importer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneMaterial<Order: Default + PartialEq = u32> {
    #[serde(default, skip_serializing_if = "SceneImage::is_palette")]
    pub image: SceneImage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TextureBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiply: Option<TextureBinding>,
    #[serde(
        default = "defaults::enabled",
        skip_serializing_if = "defaults::is_enabled"
    )]
    pub vertex_color: bool,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub blend: bool,
    #[serde(
        default = "defaults::enabled",
        skip_serializing_if = "defaults::is_enabled"
    )]
    pub depth_write: bool,
    #[serde(default, skip_serializing_if = "is_back_cull")]
    pub cull: CullFace,
    /// Runtime draws carry their priority; grouped authored surfaces use `()`
    /// because their mesh assignments carry each draw's priority separately.
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub draw_order: Order,
}

impl<Order: Default + PartialEq> SceneMaterial<Order> {
    pub(crate) fn with_draw_order<T: Default + PartialEq>(self, draw_order: T) -> SceneMaterial<T> {
        SceneMaterial {
            image: self.image,
            color: self.color,
            multiply: self.multiply,
            vertex_color: self.vertex_color,
            blend: self.blend,
            depth_write: self.depth_write,
            cull: self.cull,
            draw_order,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneImage {
    #[default]
    Palette,
    Capture,
}
impl SceneImage {
    fn is_palette(&self) -> bool {
        *self == Self::Palette
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CullFace {
    #[default]
    Back,
    Front,
    None,
}
fn is_back_cull(cull: &CullFace) -> bool {
    *cull == CullFace::Back
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureBinding {
    pub texture: usize,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub wrap_u: TextureWrap,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub wrap_v: TextureWrap,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub nearest_min: bool,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub nearest_mag: bool,
    #[serde(default, skip_serializing_if = "defaults::is_default")]
    pub mipmapped: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureWrap {
    Clamp,
    #[default]
    Repeat,
    Mirror,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraKey {
    pub time: f32,
    pub position: [f32; 3],
    pub target: [f32; 3],
    /// Camera-local orientation in the game's Z-up coordinates (XYZW).
    pub orientation: [f32; 4],
}

impl CameraKey {
    pub fn looking_at(time: f32, position: [f32; 3], target: [f32; 3]) -> Self {
        let forward = (glam::Vec3::from_array(target) - glam::Vec3::from_array(position))
            .try_normalize()
            .unwrap_or(glam::Vec3::Y);
        let up = if forward.cross(glam::Vec3::Z).length_squared() > 1e-8 {
            glam::Vec3::Z
        } else {
            glam::Vec3::Y
        };
        let view = glam::camera::rh::view::look_to_mat4(position.into(), forward, up);
        Self {
            time,
            position,
            target,
            orientation: glam::Quat::from_mat4(&view.inverse()).to_array(),
        }
    }

    pub fn up(&self) -> [f32; 3] {
        (glam::Quat::from_array(self.orientation) * glam::Vec3::Y).to_array()
    }
}

#[cfg(feature = "test-support")]
pub mod test_support;

pub mod options;
pub mod strategy;

#[cfg(test)]
mod battle_authoring_tests;

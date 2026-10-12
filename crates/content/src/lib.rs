//! Cooked asset contracts shared by the importer and game.
use serde::{Deserialize, Serialize};
pub mod animation;
pub mod appearance;
pub mod credits;
pub mod effect;
pub mod field;
pub mod field_audio;
pub mod field_preload;
pub mod figurine;
pub mod font;
pub mod grade;
pub mod menu;
pub mod menu_data;
pub mod model_behavior;
pub mod model_preview;
pub mod monster;
pub mod movie;
pub mod overworld;
pub mod prepared;
pub mod secondary_motion;
pub mod session;
pub mod skit;
pub mod source;
pub mod texture;

pub const CONTENT_VERSION: u32 = 6;
pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
/// Authored animation time unit; simulation runs at its separate NTSC cadence.
pub const ANIMATION_HZ: f32 = 60.;
/// Scene rows retained inside the full-height UI/movie framebuffer.
pub const SCENE_HEIGHT: u32 = 448;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootTexture {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub background: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootAssets {
    pub version: u32,
    pub source_sha256: String,
    pub textures: Vec<BootTexture>,
}

impl BootAssets {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 1 && self.textures.len() == 4,
            "unsupported startup logos; run cook-all"
        );
        for texture in &self.textures {
            validate_asset_path(&texture.path)?;
            anyhow::ensure!(
                (1..=640).contains(&texture.width) && (1..=480).contains(&texture.height),
                "invalid startup texture dimensions"
            );
        }
        Ok(())
    }
}

/// Playable stereo movie with timing retained from the original source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieAsset {
    pub version: u32,
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub frame_micros: u32,
    /// Zero-based audio stream within the shared movie container.
    pub audio_track: u16,
    pub sample_rate: u32,
    pub channels: u16,
    pub audio_frames: u64,
}

impl MovieAsset {
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_asset_path(&self.path)?;
        anyhow::ensure!(
            self.version == 2
                && (1..=1920).contains(&self.width)
                && (1..=1080).contains(&self.height)
                && (1..=100_000).contains(&self.frames)
                && (10_000..=100_000).contains(&self.frame_micros)
                && self.audio_track < 256
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid cooked movie manifest"
        );
        anyhow::ensure!(
            (8_000..=96_000).contains(&self.sample_rate)
                && self.channels == 2
                && (1..=u64::from(self.sample_rate) * 3600).contains(&self.audio_frames),
            "movie playback requires stereo audio with valid timing"
        );
        let video_micros = u64::from(self.frames) * u64::from(self.frame_micros);
        let audio_micros = self.audio_frames * 1_000_000 / u64::from(self.sample_rate);
        anyhow::ensure!(
            video_micros.abs_diff(audio_micros) < 100_000,
            "cooked movie audio and video durations disagree"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleSounds {
    pub version: u32,
    pub path: String,
    pub sha256: String,
}

impl TitleSounds {
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_asset_path(&self.path)?;
        anyhow::ensure!(
            self.version == 3
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "unsupported title sound manifest; recook title sounds"
        );
        Ok(())
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleAudio {
    pub version: u32,
    pub path: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl TitleAudio {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 3 && self.sample_rate == 32028 && self.channels == 2,
            "unsupported title audio format; recook title audio"
        );
        validate_asset_path(&self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleTexture {
    pub index: usize,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleAssets {
    pub version: u32,
    pub game_id: String,
    pub revision: u8,
    pub source_sha256: String,
    pub textures: Vec<TitleTexture>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenePart {
    pub resource: u16,
    pub mesh: String,
    pub textures: Vec<String>,
    pub materials: Vec<SceneMaterial>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<AppearanceTextures>,
    pub translation: [f32; 3],
    pub clips: Vec<SceneClip>,
    /// Play the field's default clip from scene entry, independently of actors.
    pub autoplay: bool,
    pub texture_animations: Vec<TextureAnimation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bone_names: Vec<String>,
    /// Script node indices that control each material's geometry visibility.
    /// A node toggle affects its attached geometry, not its skeletal children.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    pub fn outline_color_for(&self, material: &SceneMaterial) -> Option<[u8; 4]> {
        self.outline_color
            .filter(|_| material.color.is_none() && material.multiply.is_none())
    }
}

/// Optional vertical atlas channels supplied by character model metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceTextures {
    pub eyes: Option<usize>,
    pub mouth: Option<usize>,
    pub variant: Option<AtlasChannel>,
    pub costume: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasChannel {
    pub texture: usize,
    pub frames: u8,
}

/// Sampled UV translations at the scene's 60 Hz clock, with an optional intro.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureAnimation<Texture = usize> {
    pub texture: Texture,
    pub delay_ticks: u32,
    pub loop_start: usize,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneClip {
    /// Shared authored curves, independent of the geometry and scene instance.
    pub motion: String,
    pub resource_slot: u16,
    pub duration_seconds: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_resource: Option<u32>,
    /// Animated tracks that apply the stronger authored-pose attraction to cloth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_pose_nodes: Vec<u16>,
}
impl SceneClip {
    pub fn duration_ticks(&self) -> u32 {
        (self.duration_seconds * ANIMATION_HZ).round() as u32
    }
}

/// Ordinary material recipes. Original GPU command words stay in the importer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMaterial {
    #[serde(default, skip_serializing_if = "SceneImage::is_palette")]
    pub image: SceneImage,
    pub color: Option<TextureBinding>,
    pub multiply: Option<TextureBinding>,
    pub vertex_color: bool,
    pub blend: bool,
    pub depth_write: bool,
    #[serde(default, skip_serializing_if = "is_back_cull")]
    pub cull: CullFace,
    /// Authored scene draw sequence; lower values draw first.
    pub draw_order: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureBinding {
    pub texture: usize,
    pub wrap_u: TextureWrap,
    pub wrap_v: TextureWrap,
    pub nearest_min: bool,
    pub nearest_mag: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureWrap {
    Clamp,
    Repeat,
    Mirror,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraKey {
    pub time: f32,
    pub position: [f32; 3],
    pub target: [f32; 3],
}

impl TitleAssets {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == CONTENT_VERSION,
            "unsupported content version {}",
            self.version
        );
        anyhow::ensure!(
            self.game_id == "GQSEAF" && self.revision == 0,
            "expected GQSEAF revision 0"
        );
        anyhow::ensure!(
            self.textures.len() >= 18,
            "title package needs at least 18 textures"
        );
        for (index, texture) in self.textures.iter().enumerate() {
            anyhow::ensure!(
                texture.index == index
                    && (1..=4096).contains(&texture.width)
                    && (1..=4096).contains(&texture.height),
                "invalid title texture {index}"
            );
            validate_asset_path(&texture.path)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptAsset {
    pub path: String,
    pub sha256: String,
}

#[cfg(feature = "test-support")]
pub mod test_support;

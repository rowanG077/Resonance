//! Cooked asset contracts shared by the importer and game.
use serde::{Deserialize, Serialize};
pub mod effect;
pub mod field;
pub mod field_audio;
pub mod field_preload;
pub mod figurine;
pub mod font;
pub mod menu;
pub mod menu_data;
pub mod model_preview;
pub mod monster;
pub mod prepared;
pub mod secondary_motion;
pub mod session;
pub mod skit;

pub const CONTENT_VERSION: u32 = 5;
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
            "unsupported startup logos; run cook-boot"
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

/// Standard cooked movie with timing retained from the original source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieAsset {
    pub version: u32,
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub frame_micros: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub audio_frames: u64,
}

impl MovieAsset {
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_asset_path(&self.path)?;
        anyhow::ensure!(
            self.version == 1
                && (1..=1920).contains(&self.width)
                && (1..=1080).contains(&self.height)
                && (1..=100_000).contains(&self.frames)
                && (10_000..=100_000).contains(&self.frame_micros)
                && (8_000..=96_000).contains(&self.sample_rate)
                && self.channels == 2
                && self.audio_frames > 0
                && self.audio_frames <= u64::from(self.sample_rate) * 3600
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid cooked movie manifest"
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
    #[serde(default)]
    pub scene: Option<TitleScene>,
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
    /// Constant-color inverted hull, when this layer supplies actor outlines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_motion: Vec<secondary_motion::Chain>,
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
pub struct TextureAnimation {
    pub texture: usize,
    pub delay_ticks: u32,
    pub loop_start: usize,
    pub offsets: Vec<[f32; 2]>,
}

impl TextureAnimation {
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

    /// Requires a validated animation, as supplied by `TitleAssets::validate`.
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
    pub color: Option<TextureBinding>,
    pub multiply: Option<TextureBinding>,
    pub blend: bool,
    pub depth_write: bool,
    #[serde(default, skip_serializing_if = "is_back_cull")]
    pub cull: CullFace,
    /// Authored scene draw sequence; lower values draw first.
    pub draw_order: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleScene {
    pub script: ScriptAsset,
    pub source_sha256: String,
    pub code_source_sha256: String,
    pub parts: Vec<ScenePart>,
    pub glow: TitleGlow,
    pub cameras: Vec<Vec<CameraKey>>,
    pub fov_degrees: f32,
}

/// Baked world-space attachment paths for the title's two short glow trails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleGlow {
    pub texture: String,
    pub source_sha256: String,
    pub feather: Vec<[f32; 3]>,
    pub reflection: Vec<[f32; 3]>,
    pub landing: [f32; 3],
    pub landing_loop: Vec<[f32; 3]>,
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
        if let Some(scene) = &self.scene {
            validate_asset_path(&scene.script.path)?;
            anyhow::ensure!(
                scene.script.sha256.len() == 64
                    && scene.script.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid script digest"
            );
            validate_asset_path(&scene.glow.texture)?;
            anyhow::ensure!(
                scene.glow.feather.len() == 731
                    && scene.glow.reflection.len() == 731
                    && scene.glow.landing_loop.len() == 361
                    && scene
                        .glow
                        .feather
                        .iter()
                        .chain(&scene.glow.reflection)
                        .chain(&scene.glow.landing_loop)
                        .chain(std::iter::once(&scene.glow.landing))
                        .flatten()
                        .all(|v| v.is_finite()),
                "invalid glow attachment paths"
            );
            anyhow::ensure!(!scene.parts.is_empty(), "title scene has no parts");
            anyhow::ensure!(
                scene.fov_degrees.is_finite() && (1.0..179.0).contains(&scene.fov_degrees),
                "invalid field of view"
            );
            anyhow::ensure!(
                scene.cameras.len() == 2,
                "title scene needs two camera tracks"
            );
            let mut draw_orders = std::collections::BTreeSet::new();
            for part in &scene.parts {
                let mut animated_textures = std::collections::BTreeSet::new();
                for animation in &part.texture_animations {
                    animation.validate()?;
                    anyhow::ensure!(
                        animation.texture < part.textures.len()
                            && animated_textures.insert(animation.texture),
                        "invalid or duplicate animated texture index"
                    );
                }
                anyhow::ensure!(
                    part.translation.iter().all(|v| v.is_finite()),
                    "non-finite scene translation"
                );
                for (index, clip) in part.clips.iter().enumerate() {
                    anyhow::ensure!(
                        clip.duration_seconds.is_finite()
                            && (0.0..=600.0).contains(&clip.duration_seconds)
                            && clip.duration_seconds > 0.,
                        "invalid clip duration"
                    );
                    anyhow::ensure!(
                        if index == 0 {
                            clip.resource_slot == if part.autoplay { 0 } else { 12 }
                        } else {
                            clip.resource_slot > part.clips[index - 1].resource_slot
                        },
                        "invalid animation slots"
                    );
                }
                anyhow::ensure!(
                    !part.materials.is_empty(),
                    "scene part has no materials; recook title assets"
                );
                for material in &part.materials {
                    anyhow::ensure!(
                        draw_orders.insert(material.draw_order),
                        "duplicate authored scene draw order"
                    );
                    anyhow::ensure!(
                        material.draw_order < 1 << 24,
                        "scene draw order exceeds exact sort range"
                    );
                    anyhow::ensure!(
                        material.multiply.is_none() || material.color.is_some(),
                        "multiply texture requires a color texture"
                    );
                    for binding in material.color.iter().chain(&material.multiply) {
                        anyhow::ensure!(
                            binding.texture < part.textures.len(),
                            "material texture index exceeds scene textures"
                        );
                    }
                }
                for name in std::iter::once(&part.mesh).chain(&part.textures) {
                    validate_asset_path(name)?;
                }
            }
            for track in &scene.cameras {
                anyhow::ensure!(track.len() >= 2, "camera track needs at least two keys");
                let mut previous = -1.0;
                for key in track {
                    anyhow::ensure!(
                        key.time.is_finite() && key.time >= 0.0 && key.time > previous,
                        "camera key times must increase"
                    );
                    anyhow::ensure!(
                        key.position
                            .iter()
                            .chain(&key.target)
                            .all(|v| v.is_finite()),
                        "non-finite camera coordinate"
                    );
                    anyhow::ensure!(
                        key.position != key.target,
                        "camera looks at its own position"
                    );
                    previous = key.time;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptAsset {
    pub path: String,
    pub sha256: String,
}

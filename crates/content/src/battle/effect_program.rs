//! Authored effect timelines, geometry and material data. No source-memory layout remains.
mod models;
mod resources;
use super::effects::EffectId;
use crate::font::UiTexture;
use crate::menu_data::Element;
use anyhow::{Context, Result, bail, ensure};
pub use resources::{
    MaterialChoices, blend_choices, fresh_integer_scratch, group_materials, group_model_choices,
    integer_birth_ranges, model_indices, palette_indices, resource_indices, retained_materials,
    selected_model_index,
};
use serde::{Deserialize, Serialize};

pub const RETAINED_EFFECT_SLOTS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleEffectPrograms {
    pub programs: Vec<EffectProgram>,
    pub actors: Vec<EffectActor>,
    pub materials: Vec<EffectMaterial>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectProgram {
    pub id: EffectId,
    pub end_tick: u16,
    pub emissions: Vec<EffectEmission>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectEmission {
    pub tick: u16,
    pub repeat: Option<Repeat>,
    pub command: EffectCommand,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectCommand {
    Controller {
        /// Source actor identity remains distinct from the emitting program.
        actor: EffectId,
        controller: EffectController,
    },
    Particle {
        actor: EffectId,
        attachment: Attachment,
        modifiers: Vec<Modifier>,
    },
    ModifyRetained {
        slot: u8,
        modifiers: Vec<Modifier>,
    },
    Sound {
        sound: u16,
        priority: u8,
    },
}
/// Authored presentation commands execute without allocating a particle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectController {
    Camera {
        duration: u16,
        distance: f32,
        elevation: f32,
    },
    Shake {
        duration: u16,
        amplitude: u32,
    },
    StageColor {
        color: [u8; 4],
        duration: u16,
    },
    Caption {
        text: String,
    },
}

impl EffectController {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Camera {
                duration,
                distance,
                elevation,
            } => {
                ensure!(
                    *duration <= i16::MAX as u16 && distance.is_finite() && elevation.is_finite(),
                    "invalid camera bounds controller"
                );
            }
            Self::Shake { duration, .. } => {
                ensure!(
                    *duration <= i16::MAX as u16,
                    "negative camera shake duration"
                );
            }
            Self::StageColor { duration, .. } => {
                ensure!(
                    (1..=i16::MAX as u16).contains(duration),
                    "unsupported stage tint duration"
                );
            }
            Self::Caption { text } => {
                ensure!(
                    !text.is_empty() && !text.chars().any(char::is_control),
                    "invalid effect caption"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Repeat {
    pub count: u8,
    pub interval: u16,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum Attachment {
    Emitter,
    Bone(u8),
    BoneGroup(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectActor {
    pub id: EffectId,
    pub geometry: Geometry,
    pub material: Option<u16>,
    /// Prepared color-palette choices; the alpha palette and atlas stay fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<Palette>,
    /// Sample the captured scene through the authored atlas mask.
    #[serde(default)]
    pub screen_texture: Option<ScreenTexture>,
    /// Spell element can replace the atlas palette and outer RGB at birth.
    pub element_variants: Vec<ElementVariant>,
    pub use_element_variant: bool,
    pub blend: Blend,
    /// Prepared blend modes for constant birth modifiers, including the base mode.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub blend_variants: std::collections::BTreeSet<Blend>,
    pub uv: [i16; 4],
    pub uv_animation: Option<UvAnimation>,
    pub depth_test: bool,
    pub depth_write: bool,
    /// Force back-face culling, including additive/subtractive model effects.
    #[serde(default)]
    pub cull_back: bool,
    #[serde(default, deserialize_with = "deserialize_owner_layer")]
    pub owner_layer: Option<OwnerLayer>,
    /// Advances while ordinary battle actors and effects are paused.
    #[serde(default)]
    pub during_pause: bool,
    /// Keep this particle in its program's slot table for later authored changes.
    #[serde(default)]
    pub retained: bool,
    pub lifetime: Option<u16>,
    pub orientation: Orientation,
    #[serde(default)]
    pub space: EffectSpace,
    pub follow_emitter: bool,
    pub bottom_anchored: bool,
    /// Draw above the ground plane instead of inheriting the emitter's height.
    pub ground_relative: bool,
    /// Independent particles react when their world origin passes below the floor.
    #[serde(default)]
    pub ground: Option<GroundResponse>,
    /// Emit a same-bank child after each matching update, including age zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periodic: Option<PeriodicEmission>,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub acceleration_change: [f32; 3],
    pub angles: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub local_offset: [f32; 3],
    pub local_velocity: [f32; 3],
    /// Height, width and radius; each geometry gives these its authored meaning.
    pub dimensions: [f32; 3],
    pub dimension_velocity: [f32; 3],
    pub dimension_acceleration: [f32; 3],
    pub dimension_acceleration_until: Option<u16>,
    pub colors: [[i16; 4]; 2],
    /// Otherwise copy the first color to the second after spawn modifiers.
    pub color_gradient: bool,
    pub brighten: [u8; 4],
    pub darken: [u8; 4],
    pub brighten_until: u8,
    pub darken_from: u8,
    pub copies: u8,
    pub copy_rotation: u8,
}

/// Live owner binding used to place the particle's independent motion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectSpace {
    #[default]
    World,
    /// Inherit the complete live body-bone matrix; an out-of-range index selects root.
    OwnerBone { bone: u8 },
    /// Refresh the origin from a live body joint each update, without inheriting
    /// its rotation or scale. An out-of-range index selects joint zero.
    OwnerBonePosition { bone: u8 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroundResponse {
    Bounce {
        effect: Option<EffectId>,
    },
    EmitOnce {
        effect: EffectId,
        /// Capture the impact point first, then stop the parent at the floor.
        clamp: bool,
    },
    /// Stop the origin at the floor while retaining its velocity and acceleration.
    Clamp,
}

impl GroundResponse {
    pub fn effect(self) -> Option<EffectId> {
        match self {
            Self::Bounce { effect } => effect,
            Self::EmitOnce { effect, .. } => Some(effect),
            Self::Clamp => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeriodicEmission {
    pub effect: EffectId,
    pub period: std::num::NonZeroU8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenTexture {
    /// Signed offsets in the original scene's pixel coordinates.
    pub offset: [i8; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerLayer {
    Before,
    After,
}

fn deserialize_owner_layer<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<OwnerLayer>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Legacy(bool),
        Layer(Option<OwnerLayer>),
    }
    Ok(match Value::deserialize(deserializer)? {
        Value::Legacy(value) => value.then_some(OwnerLayer::After),
        Value::Layer(layer) => layer,
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Geometry {
    /// An allocated motion actor with no draw routine.
    NoDraw {
        #[serde(default)]
        motion: NoDrawMotion,
    },
    Model {
        model: ModelRef,
        animation: Option<u8>,
        loop_animation: bool,
        #[serde(default)]
        presentation: ModelPresentation,
    },
    Ring {
        segments: u8,
        flared: bool,
        lines: bool,
        repeat_uv: bool,
        uv_columns: u8,
    },
    Quad,
    /// One shared particle drawn at each live body bone in the authored effect group.
    BoneGroupQuad {
        group: u8,
    },
    /// Three strips connect concentric equilateral triangles.
    TriangleBand,
    /// Oriented panels spaced around a circle; height, width and orbit radius.
    RadialQuads {
        segments: u8,
        plane: RadialQuadPlane,
        advance_u: bool,
    },
    /// Ten latitude bands cover the full ellipsoid, always in world orientation.
    Ellipsoid {
        longitude_panels: u8,
    },
    /// Connected strips with a fixed noise pattern and three vertical atlas tiles.
    JitterRibbon {
        segments: u8,
        phase: u8,
        phase_period: i8,
        jitter_span: f32,
        plane: RibbonPlane,
    },
    /// Five latitude bands form the positive-Z half of an ellipsoid.
    Hemisphere {
        repeat_uv: bool,
        /// Last tile index on each axis; zero repeats a single tile.
        uv_columns: u8,
        uv_rows: u8,
    },
    /// Camera-facing sprites evenly spaced around an oriented circle.
    BillboardRing {
        segments: u8,
    },
    /// Camera-facing sprites sampled along a shrinking, rotating orbit.
    BillboardTrail {
        segments: u8,
        steps_per_segment: u8,
        /// Displacement subtracted per sample step, rotated by emitter heading.
        segment_offset: [f32; 3],
        /// Orbital degrees subtracted per sample step.
        angle_step: f32,
        /// Height, width and radius added per sample step, independently of time.
        size_step: [f32; 3],
    },
    VertexQuad {
        vertices: [[f32; 3]; 4],
        velocities: [[f32; 3]; 4],
        copy_axis: u8,
        rotate_copies_locally: bool,
    },
    Spiral {
        segments: u8,
        /// Number of step advances between adjacent strip segments.
        uv_rows: u8,
        /// Local displacement subtracted between strip samples; rotate by emitter heading.
        segment_offset: [f32; 3],
        /// Degrees subtracted from the radial phase between strip samples.
        angle_step: f32,
        /// Radius subtracted between strip samples, independent of time acceleration.
        radius_step: f32,
        repeat_uv: bool,
    },
    Disc {
        segments: u8,
    },
    CurvedShell {
        segments: u8,
        elliptical: bool,
        repeat_uv: bool,
        uv_columns: u8,
    },
}

/// Model origin and texture-zero row selection, independent of model scale and spin.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPresentation {
    /// Authored selector byte, independent of the animation bound before birth modifiers.
    #[serde(default)]
    pub animation_selector: u8,
    /// Reverse controller slot zero and release its endpoint stop at this particle age.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_at: Option<u8>,
    pub elevation: f32,
    pub texture_rows: u8,
    pub texture_frame: u8,
    /// Native selector storage; model drawing uses its own textures, not these atlas palettes.
    #[serde(default)]
    pub palettes: [u8; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<RetainedJoint>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external_animation: bool,
    #[serde(default)]
    pub orientation: ModelOrientation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrientation {
    #[default]
    Authored,
    /// Derive pitch/yaw from the followed emitter's displacement each effect tick.
    FollowMotion,
}

/// A live retained model in the same effect program supplies the complete root matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedJoint {
    pub slot: u8,
    /// Select the first authored joint whose label starts with `kk0{joint}`.
    pub joint: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadialQuadPlane {
    Tangent,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RibbonPlane {
    Flat,
    Depth,
}

impl Geometry {
    /// These parameters can also change through retained-particle modifiers.
    pub fn validate_dynamic(&self) -> Result<()> {
        match *self {
            Self::BoneGroupQuad { group } => ensure!(group < 16, "invalid effect bone group"),
            Self::Model {
                model,
                animation,
                presentation,
                ..
            } => ensure!(
                model.index() < 128
                    && match model {
                        ModelRef::EnemyAnimated {
                            animation_model, ..
                        } => animation_model < 128 && animation == Some(0),
                        _ => true,
                    }
                    && (!presentation.external_animation || animation.is_none())
                    && presentation.elevation.is_finite()
                    && presentation
                        .attachment
                        .is_none_or(|joint| usize::from(joint.slot) < RETAINED_EFFECT_SLOTS
                            && joint.slot == presentation.texture_frame
                            && joint.joint == presentation.texture_rows),
                "invalid model presentation or model index"
            ),
            Self::BillboardRing { segments } => ensure!(segments > 0, "empty billboard ring"),
            Self::RadialQuads { segments, .. } => ensure!(segments > 0, "empty radial quads"),
            Self::Ellipsoid { longitude_panels } => ensure!(
                matches!(longitude_panels, 8 | 16),
                "unsupported ellipsoid longitude count"
            ),
            Self::JitterRibbon {
                segments,
                phase,
                jitter_span,
                ..
            } => ensure!(
                segments > 0
                    && usize::from(phase) + usize::from(segments) <= 256
                    && jitter_span.is_finite()
                    && (2. ..=i32::MAX as f32).contains(&jitter_span),
                "invalid ribbon count, noise index or jitter span"
            ),
            _ => {}
        }
        Ok(())
    }
}
/// Where nondrawing actors integrate velocity. Origin motion keeps acceleration fixed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoDrawMotion {
    Local,
    #[default]
    Origin,
    /// Samples a moving radial point instead of advancing ordinary particle properties.
    PointHistory {
        capacity: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UvAnimation {
    pub frames: Vec<UvFrame>,
    pub loop_to: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_scroll: Option<ModelUvScroll>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UvFrame {
    pub duration: u8,
    #[serde(flatten)]
    pub update: UvUpdate,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UvUpdate {
    Rect {
        rect: [i16; 4],
    },
    Palette {
        material: u16,
    },
    /// Models retain this palette byte without replacing a model material.
    ModelPalette {
        index: u8,
    },
    /// Advance a retained atlas offset at the frame interval; dimensions remain unchanged.
    Scroll {
        origin: [i16; 2],
        step: [i16; 2],
    },
    /// Enter the terminal row. Sprites scroll at interval126; models scroll every update.
    End {
        end: UvEnd,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UvEnd {
    Rect {
        rect: [i16; 4],
    },
    Palette {
        material: u16,
        origin: [i16; 2],
        step: [i16; 2],
    },
    ModelPalette {
        origin: [i16; 2],
        step: [i16; 2],
    },
}

impl UvEnd {
    pub const INTERVAL: u8 = 126;
}

impl UvUpdate {
    pub fn material(self) -> Option<u16> {
        match self {
            Self::Palette { material }
            | Self::End {
                end: UvEnd::Palette { material, .. },
            } => Some(material),
            _ => None,
        }
    }

    pub fn scroll(self) -> Option<([i16; 2], [i16; 2])> {
        match self {
            Self::Scroll { origin, step }
            | Self::End {
                end: UvEnd::Palette { origin, step, .. },
            }
            | Self::End {
                end: UvEnd::ModelPalette { origin, step },
            } => Some((origin, step)),
            Self::End {
                end: UvEnd::Rect { rect },
            } => Some(([rect[0], rect[1]], [rect[2], rect[3]])),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelUvScroll {
    pub step: [i16; 2],
    pub period: [i16; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelRef {
    ColetteWeapon,
    Common {
        index: u8,
    },
    Enemy {
        monster: u8,
        index: u8,
    },
    /// Geometry changes after the original model animation has already been bound.
    EnemyAnimated {
        monster: u8,
        index: u8,
        animation_model: u8,
    },
    Magic {
        package: u16,
        index: u8,
    },
    Skill {
        package: u16,
        index: u8,
    },
}
impl ModelRef {
    /// Begin the next group birth's ordinary clip on its inherited current model.
    pub fn inherit_animation(self) -> Self {
        match self {
            Self::EnemyAnimated { monster, index, .. } => Self::EnemyAnimated {
                monster,
                index,
                animation_model: index,
            },
            _ => self,
        }
    }
    pub fn retains_animation(self) -> bool {
        matches!(self, Self::EnemyAnimated { .. })
    }
    pub fn index(self) -> u8 {
        match self {
            Self::ColetteWeapon => 0,
            Self::Common { index }
            | Self::Enemy { index, .. }
            | Self::EnemyAnimated { index, .. }
            | Self::Magic { index, .. }
            | Self::Skill { index, .. } => index,
        }
    }

    pub fn with_index(self, index: u8) -> Self {
        match self {
            Self::ColetteWeapon | Self::Common { .. } if index == 0 => Self::ColetteWeapon,
            Self::ColetteWeapon | Self::Common { .. } => Self::Common { index },
            Self::Enemy { monster, .. } => Self::Enemy { monster, index },
            Self::EnemyAnimated {
                monster,
                animation_model,
                ..
            } => Self::EnemyAnimated {
                monster,
                index,
                animation_model,
            },
            Self::Magic { package, .. } => Self::Magic { package, index },
            Self::Skill { package, .. } => Self::Skill { package, index },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    World,
    /// World Euler angles track emitter motion, including its initial direction.
    FollowMotion,
    /// Retain world Euler angles and local offset; use Y-X-Z rotation without emitter heading.
    FixedWorld,
    Billboard,
    CameraRelative,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Blend {
    Alpha = 0,
    Additive = 1,
    Subtractive = 2,
}
impl Blend {
    pub fn modified(self, operation: Arithmetic, value: IntegerValue) -> Result<Self> {
        let IntegerValue::Constant(value) = value else {
            anyhow::bail!("dynamic effect blend");
        };
        match operation.byte(self as u8, value as u8) {
            Some(0) => Ok(Self::Alpha),
            Some(1) => Ok(Self::Additive),
            Some(2) => Ok(Self::Subtractive),
            Some(flags) => anyhow::bail!("unsupported effect blend flags {flags:#x}"),
            None => anyhow::bail!("effect blend division by zero"),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectMaterial {
    /// Full atlas, with RGB and alpha palettes already combined. UV animation remains possible.
    pub texture: UiTexture,
    pub rgb_scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    pub index: u8,
    /// Present when source halfword operations also select the alpha palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<u8>,
    pub materials: std::collections::BTreeMap<u16, u16>,
}
impl Palette {
    pub fn material(&self) -> Option<u16> {
        let key = self.alpha.map_or(u16::from(self.index), |alpha| {
            u16::from_be_bytes([self.index, alpha])
        });
        self.materials.get(&key).copied()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ElementVariant {
    pub element: Element,
    pub material: Option<u16>,
    pub outer_color: [i16; 3],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum IntegerValue {
    Constant(i16),
    Temporary(u8),
}
impl IntegerValue {
    pub fn resolve(self, integers: &[i16; 4]) -> Result<i16> {
        match self {
            Self::Constant(value) => Ok(value),
            Self::Temporary(index) => integers
                .get(usize::from(index))
                .copied()
                .context("invalid integer temporary"),
        }
    }

    fn validate(self) -> Result<()> {
        self.resolve(&[0; 4]).map(|_| ())
    }

    // Earlier cooked lifetime operands were plain signed integers.
    fn deserialize_lifetime<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Operand {
            Literal(i16),
            Typed(IntegerValue),
        }
        Ok(match Operand::deserialize(deserializer)? {
            Operand::Literal(value) => Self::Constant(value),
            Operand::Typed(value) => value,
        })
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FloatValue {
    Constant(f32),
    Temporary(u8),
}
impl FloatValue {
    pub fn resolve(self, floats: &[f32; 4]) -> Result<f32> {
        let value = match self {
            Self::Constant(value) => value,
            Self::Temporary(index) => *floats
                .get(usize::from(index))
                .context("invalid float temporary")?,
        };
        ensure!(value.is_finite(), "non-finite effect modifier");
        Ok(value)
    }

    fn validate(self) -> Result<()> {
        self.resolve(&[0.; 4]).map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorAxis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectFlag {
    BottomAnchored,
    ColorGradient,
    RibbonDepth,
    FollowEmitter,
    GroundRelative,
    UseElementVariant,
    CullBack,
    DepthTest,
    DepthWrite,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum IntegerField {
    /// Signed halfword: color palette in the high byte, alpha palette in the low byte.
    PaletteSelection,
    /// Signed halfword: model index in the high byte, animation selector in the low byte.
    ModelSelection,
    /// Signed halfword: model texture rows/joint in the high byte, frame/parent slot in the low byte.
    ModelTextureSelection,
    U,
    V,
    Width,
    Height,
    Color {
        end: u8,
        channel: u8,
    },
    Temporary(u8),
    /// Signed arithmetic on a float scratch value's upper 16 bits, preserving its lower bits.
    FloatTemporaryHigh(u8),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "channel", rename_all = "snake_case")]
pub enum ByteField {
    Brighten(u8),
    Darken(u8),
    BrightenUntil,
    DarkenFrom,
    #[serde(alias = "uv_columns")]
    GeometryCount,
    NoisePhase,
    ModelIndex,
    ModelTextureFrame,
    /// Shared byte selecting an owner joint, an EF bone group, or model texture rows/joint.
    BoneSelector,
    Palette,
    Blend,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "axis", rename_all = "snake_case")]
pub enum FloatField {
    Position(u8),
    Velocity(u8),
    Acceleration(u8),
    Angle(u8),
    AngularVelocity(u8),
    LocalOffset(u8),
    LocalVelocity(u8),
    Dimension(u8),
    DimensionVelocity(u8),
    DimensionAcceleration(u8),
    /// Spatial displacement per rendered segment; distinct from particle acceleration.
    GeometrySegmentOffset(u8),
    GeometryAngleStep,
    Temporary(u8),
}
/// Whole authored vectors, including consecutive scratch slots.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum VectorField {
    Position,
    Velocity,
    Acceleration,
    Angle,
    AngularVelocity,
    LocalOffset,
    LocalVelocity,
    Dimension,
    DimensionVelocity,
    DimensionAcceleration,
    Temporary(u8),
}

impl VectorField {
    pub fn components(self) -> [FloatField; 3] {
        std::array::from_fn(|i| match self {
            Self::Position => FloatField::Position(i as u8),
            Self::Velocity => FloatField::Velocity(i as u8),
            Self::Acceleration => FloatField::Acceleration(i as u8),
            Self::Angle => FloatField::Angle(i as u8),
            Self::AngularVelocity => FloatField::AngularVelocity(i as u8),
            Self::LocalOffset => FloatField::LocalOffset(i as u8),
            Self::LocalVelocity => FloatField::LocalVelocity(i as u8),
            Self::Dimension => FloatField::Dimension(i as u8),
            Self::DimensionVelocity => FloatField::DimensionVelocity(i as u8),
            Self::DimensionAcceleration => FloatField::DimensionAcceleration(i as u8),
            Self::Temporary(start) => FloatField::Temporary(start.saturating_add(i as u8)),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arithmetic {
    #[default]
    Set,
    Add,
    Subtract,
    Multiply,
    Divide,
}
impl Arithmetic {
    pub fn integer(self, left: i16, right: i16) -> Option<i16> {
        Some(match self {
            Self::Set => right,
            Self::Add => left.wrapping_add(right),
            Self::Subtract => left.wrapping_sub(right),
            Self::Multiply => left.wrapping_mul(right),
            Self::Divide if right != 0 => left.wrapping_div(right),
            Self::Divide => return None,
        })
    }

    /// The source stores a signed halfword; zero disables expiration.
    pub fn lifetime(self, current: Option<u16>, value: i16) -> Result<Option<u16>> {
        let current = i16::try_from(current.unwrap_or(0))
            .map_err(|_| anyhow::anyhow!("unsupported signed effect lifetime"))?;
        let result = match self {
            Self::Set => value,
            Self::Add => current.wrapping_add(value),
            Self::Subtract => current.wrapping_sub(value),
            Self::Multiply => current.wrapping_mul(value),
            Self::Divide if value != 0 => current.wrapping_div(value),
            Self::Divide => anyhow::bail!("effect lifetime division by zero"),
        };
        ensure!(result >= 0, "unsupported negative effect lifetime");
        Ok((result != 0).then_some(result as u16))
    }

    /// Byte writes wrap; division treats both operands as signed bytes.
    pub fn byte(self, left: u8, right: u8) -> Option<u8> {
        Some(match self {
            Self::Set => right,
            Self::Add => left.wrapping_add(right),
            Self::Subtract => left.wrapping_sub(right),
            Self::Multiply => left.wrapping_mul(right),
            Self::Divide if right != 0 => (left as i8).wrapping_div(right as i8) as u8,
            Self::Divide => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Modifier {
    /// Proven single birth before any scratch writes; check, never overwrite, its scratch.
    RequireFreshIntegers,
    /// A preceding synchronous birth proves this range; assert it without changing scratch.
    RequireIntegerRange {
        index: u8,
        min: i16,
        max: i16,
    },
    PlayModelAnimation {
        animation: ModelAnimation,
    },
    /// Write the first embedded model controller, independently of clip selection.
    AnimationPosition {
        value: FloatValue,
    },
    AnimationRate {
        value: FloatValue,
    },
    Flag {
        field: EffectFlag,
        enabled: bool,
    },
    /// A signed-halfword operation on the authored expiration threshold.
    Lifetime {
        operation: Arithmetic,
        #[serde(deserialize_with = "IntegerValue::deserialize_lifetime")]
        value: IntegerValue,
    },
    #[serde(alias = "set_byte")]
    Byte {
        field: ByteField,
        #[serde(default)]
        operation: Arithmetic,
        value: IntegerValue,
    },
    Integer {
        field: IntegerField,
        operation: Arithmetic,
        value: IntegerValue,
    },
    Float {
        field: FloatField,
        operation: Arithmetic,
        value: FloatValue,
    },
    RandomInteger {
        field: IntegerField,
        modulus: u16,
    },
    /// Replace a retained particle's vector with one axis rotated by its spawn heading.
    SetEmitterAxisVector {
        field: VectorField,
        axis: VectorAxis,
        value: FloatValue,
    },
    RotateVector {
        field: VectorField,
        axis: VectorAxis,
        /// Degrees; temporary operands remain unscaled.
        angle: FloatValue,
    },
    /// Set [radius*cos(angle), 0, radius*sin(angle)]; angle is in degrees.
    PolarVector {
        field: VectorField,
        radius: FloatValue,
        angle: FloatValue,
    },
    /// Rotate [radius, 0, 0] around Z, X, then Y. Jitter uses signed remainders
    /// in tenths; each nonzero range consumes one draw, radius before angle.
    RandomPolarVector {
        field: VectorField,
        angles: [f32; 3],
        radius: f32,
        radius_jitter: i16,
        angle_jitter: i16,
    },
    /// Signed PRNG draw, optionally restricted before scaling.
    RandomFloat {
        field: FloatField,
        range: FloatRandomRange,
        scale: f32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FloatRandomRange {
    Remainder(u16),
    Signed,
}

/// A clip controlled by the effect program, independent of particle lifetime.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelAnimation {
    pub model: u8,
    pub clip: u8,
    pub blend_ticks: u16,
    pub rate: f32,
    pub hold: bool,
}

impl EffectProgram {
    /// Conservative dependency set; runtime slot allocation follows actual particle births.
    pub fn retained_actors<'a>(
        &'a self,
        content: &'a BattleEffectPrograms,
    ) -> impl Iterator<Item = &'a EffectActor> {
        self.emissions
            .iter()
            .filter_map(|emission| match emission.command {
                EffectCommand::Particle { actor, .. } => {
                    content.actor(actor).filter(|a| a.retained)
                }
                _ => None,
            })
    }
}

impl EffectActor {
    fn bone_selector(&self) -> Option<u8> {
        match self.geometry {
            Geometry::Model { presentation, .. } => Some(presentation.texture_rows),
            Geometry::BoneGroupQuad { group } => Some(group),
            _ => match self.space {
                EffectSpace::OwnerBone { bone } | EffectSpace::OwnerBonePosition { bone } => {
                    Some(bone)
                }
                EffectSpace::World => None,
            },
        }
    }

    /// All consumers of the authored selector observe the same byte write.
    pub fn modify_bone_selector(&mut self, operation: Arithmetic, value: u8) -> Result<()> {
        let previous = self
            .bone_selector()
            .context("effect has no bone selector consumer")?;
        let value = operation
            .byte(previous, value)
            .context("effect bone selector division by zero")?;
        match &mut self.space {
            EffectSpace::OwnerBone { bone } | EffectSpace::OwnerBonePosition { bone } => {
                *bone = value
            }
            EffectSpace::World => {}
        }
        match &mut self.geometry {
            Geometry::Model { presentation, .. } => {
                presentation.texture_rows = value;
                if let Some(joint) = &mut presentation.attachment {
                    joint.joint = value;
                }
            }
            Geometry::BoneGroupQuad { group } => *group = value,
            _ => {}
        }
        Ok(())
    }

    pub fn modify_model_texture_frame(&mut self, operation: Arithmetic, value: u8) -> Result<()> {
        let Geometry::Model { presentation, .. } = &mut self.geometry else {
            bail!("texture frame modifier on a non-model effect");
        };
        presentation.texture_frame = operation
            .byte(presentation.texture_frame, value)
            .context("effect texture frame division by zero")?;
        if let Some(joint) = &mut presentation.attachment {
            joint.slot = presentation.texture_frame;
        }
        Ok(())
    }

    pub fn modify_model_texture_selection(
        &mut self,
        operation: Arithmetic,
        value: i16,
    ) -> Result<()> {
        let Geometry::Model { presentation, .. } = self.geometry else {
            bail!("texture selection modifier on a non-model effect");
        };
        let current = i16::from_be_bytes([presentation.texture_rows, presentation.texture_frame]);
        let [rows, frame] = operation
            .integer(current, value)
            .context("effect texture selection division by zero")?
            .to_be_bytes();
        self.modify_bone_selector(Arithmetic::Set, rows)?;
        self.modify_model_texture_frame(Arithmetic::Set, frame)
    }

    pub fn child_effects(&self) -> impl Iterator<Item = EffectId> + '_ {
        self.ground
            .and_then(GroundResponse::effect)
            .into_iter()
            .chain(self.periodic.map(|emission| emission.effect))
    }

    /// Retained writes change selector storage, but skip the loader's geometry rebind.
    pub fn validate_retained_modifiers(&self, modifiers: &[Modifier]) -> Result<()> {
        for modifier in modifiers.iter().filter(|m| m.writes_model_index()) {
            modifier.validate()?;
            ensure!(
                matches!(self.geometry, Geometry::Model { .. }),
                "model selector on a non-model effect"
            );
        }
        let visible = modifiers
            .iter()
            .copied()
            .filter(|m| !m.writes_model_index())
            .collect::<Vec<_>>();
        self.validate_modifiers(&visible)
    }

    pub fn validate_modifiers(&self, modifiers: &[Modifier]) -> Result<()> {
        let precondition = |m: &Modifier| {
            matches!(
                m,
                Modifier::RequireFreshIntegers | Modifier::RequireIntegerRange { .. }
            )
        };
        ensure!(
            modifiers
                .iter()
                .skip_while(|m| precondition(m))
                .all(|m| !precondition(m)),
            "scratch preconditions must precede source instructions"
        );
        if let Geometry::Model {
            model,
            presentation,
            ..
        } = self.geometry
            && presentation.external_animation
        {
            let selected =
                selected_model_index(model.index(), presentation.animation_selector, modifiers)?;
            let mut played = false;
            for modifier in modifiers {
                if let Modifier::PlayModelAnimation { animation } = modifier {
                    ensure!(
                        animation.model == selected,
                        "effect animation has no matching final model consumer"
                    );
                    played = true;
                }
            }
            ensure!(
                selected == model.index() || played,
                "replacement effect model lacks its animation controller"
            );
        }
        if let Some(palette) = &self.palette {
            for index in palette_indices(palette.index, palette.alpha, modifiers)? {
                ensure!(
                    palette.materials.contains_key(&index),
                    "unprepared effect palette {index}"
                );
            }
        } else if !matches!(self.geometry, Geometry::Model { .. }) {
            ensure!(
                !modifiers.iter().any(|m| m.writes_palette_selection()
                    || matches!(
                        m,
                        Modifier::Byte {
                            field: ByteField::Palette,
                            ..
                        }
                    )),
                "effect has no prepared palettes"
            );
        }
        if let Geometry::Model {
            model,
            presentation,
            ..
        } = self.geometry
        {
            model_indices(model.index(), presentation.animation_selector, modifiers)?;
        }
        let mut blend = self.blend;
        // Scratch-dependent thresholds are checked when their operands are resolved.
        let mut lifetime = Some(self.lifetime);
        for modifier in modifiers {
            if matches!(
                self.geometry,
                Geometry::NoDraw {
                    motion: NoDrawMotion::PointHistory { .. }
                }
            ) {
                let history_storage = |field| {
                    matches!(
                        field,
                        FloatField::AngularVelocity(2)
                            | FloatField::DimensionVelocity(_)
                            | FloatField::DimensionAcceleration(_)
                    )
                };
                ensure!(
                    !match modifier {
                        Modifier::Float { field, .. } | Modifier::RandomFloat { field, .. } =>
                            history_storage(*field),
                        Modifier::SetEmitterAxisVector { field, .. }
                        | Modifier::RotateVector { field, .. }
                        | Modifier::PolarVector { field, .. }
                        | Modifier::RandomPolarVector { field, .. } =>
                            field.components().into_iter().any(history_storage),
                        _ => false,
                    },
                    "modifier writes unsupported point-history storage"
                );
            }
            if let Modifier::Lifetime { operation, value } = *modifier {
                lifetime = match (operation, value, lifetime) {
                    (_, IntegerValue::Constant(value), Some(current)) => {
                        Some(operation.lifetime(current, value)?)
                    }
                    (Arithmetic::Set, IntegerValue::Constant(value), None) => {
                        Some(operation.lifetime(None, value)?)
                    }
                    _ => None,
                };
            }
            if let Modifier::Byte {
                field: ByteField::Blend,
                operation,
                value,
            } = *modifier
            {
                blend = blend.modified(operation, value)?;
                ensure!(
                    (blend == self.blend || self.blend_variants.contains(&blend))
                        && self.screen_texture.is_none()
                        && !matches!(
                            self.geometry,
                            Geometry::Model { .. } | Geometry::NoDraw { .. }
                        ),
                    "unprepared or incompatible effect blend {blend:?}"
                );
            }
            modifier.validate()?;
            ensure!(
                match modifier {
                    Modifier::AnimationPosition { .. } | Modifier::AnimationRate { .. } => {
                        matches!(self.geometry, Geometry::Model { .. })
                    }
                    Modifier::PlayModelAnimation { .. } => matches!(
                        self.geometry,
                        Geometry::Model {
                            presentation: ModelPresentation {
                                external_animation: true,
                                ..
                            },
                            ..
                        }
                    ),
                    Modifier::Flag {
                        field: EffectFlag::FollowEmitter,
                        enabled: true,
                    } => self.orientation != Orientation::FixedWorld,
                    Modifier::Flag {
                        field: EffectFlag::UseElementVariant,
                        enabled: true,
                    } => !self.element_variants.is_empty(),
                    Modifier::Flag {
                        field: EffectFlag::CullBack,
                        ..
                    } => !matches!(self.geometry, Geometry::Model { .. }),
                    Modifier::Flag {
                        field: EffectFlag::DepthTest | EffectFlag::DepthWrite,
                        ..
                    } =>
                        self.screen_texture.is_none()
                            && !matches!(
                                self.geometry,
                                Geometry::Model { .. } | Geometry::NoDraw { .. }
                            ),
                    Modifier::Flag {
                        field: EffectFlag::RibbonDepth,
                        ..
                    } => matches!(self.geometry, Geometry::JitterRibbon { .. }),
                    Modifier::Byte {
                        field: ByteField::GeometryCount,
                        ..
                    } => matches!(
                        self.geometry,
                        Geometry::Ring { .. }
                            | Geometry::Hemisphere { .. }
                            | Geometry::CurvedShell { .. }
                            | Geometry::RadialQuads { .. }
                            | Geometry::BillboardRing { .. }
                            | Geometry::JitterRibbon { .. }
                    ),
                    Modifier::Byte {
                        field: ByteField::NoisePhase,
                        ..
                    } => matches!(self.geometry, Geometry::JitterRibbon { .. }),
                    Modifier::Byte {
                        field: ByteField::BoneSelector,
                        ..
                    } => self.bone_selector().is_some(),
                    Modifier::Integer {
                        field: IntegerField::ModelSelection,
                        ..
                    }
                    | Modifier::RandomInteger {
                        field: IntegerField::ModelSelection,
                        ..
                    } => matches!(
                        self.geometry,
                        Geometry::Model {
                            animation: None,
                            presentation: ModelPresentation {
                                external_animation: false,
                                ..
                            },
                            ..
                        }
                    ),
                    Modifier::Byte {
                        field: ByteField::ModelIndex,
                        ..
                    } => matches!(
                        self.geometry,
                        Geometry::Model { model, animation, .. } if animation.is_none() || model.retains_animation()
                    ),
                    Modifier::Byte {
                        field: ByteField::ModelTextureFrame,
                        ..
                    }
                    | Modifier::Integer {
                        field: IntegerField::ModelTextureSelection,
                        ..
                    }
                    | Modifier::RandomInteger {
                        field: IntegerField::ModelTextureSelection,
                        ..
                    } => matches!(self.geometry, Geometry::Model { .. }),
                    Modifier::Float {
                        field: FloatField::GeometryAngleStep | FloatField::GeometrySegmentOffset(_),
                        ..
                    }
                    | Modifier::RandomFloat {
                        field: FloatField::GeometryAngleStep | FloatField::GeometrySegmentOffset(_),
                        ..
                    } => matches!(
                        self.geometry,
                        Geometry::Spiral { .. } | Geometry::BillboardTrail { .. }
                    ),
                    _ => true,
                },
                "incompatible effect modifier {modifier:?} on {:?}",
                self.id
            );
        }
        Ok(())
    }
}

impl BattleEffectPrograms {
    pub fn program(&self, id: EffectId) -> Option<&EffectProgram> {
        self.programs.iter().find(|p| p.id == id)
    }
    pub fn actor(&self, id: EffectId) -> Option<&EffectActor> {
        self.actors.iter().find(|p| p.id == id)
    }
    pub fn assets(&self) -> impl Iterator<Item = &str> {
        self.materials.iter().map(|m| m.texture.path.as_str())
    }
    fn modifiers_for<'a>(&'a self, actor: &'a EffectActor) -> impl Iterator<Item = &'a [Modifier]> {
        self.programs.iter().flat_map(move |program| {
            program
                .emissions
                .iter()
                .filter_map(move |emission| match &emission.command {
                    EffectCommand::Particle {
                        actor: id,
                        modifiers,
                        ..
                    } if *id == actor.id => Some(modifiers.as_slice()),
                    EffectCommand::ModifyRetained { modifiers, .. }
                        if actor.retained
                            && program.retained_actors(self).any(|a| a.id == actor.id) =>
                    {
                        Some(modifiers.as_slice())
                    }
                    _ => None,
                })
        })
    }
    /// Includes authored replacement models before any particle can request them.
    pub fn models_for(&self, actor: EffectId) -> std::collections::BTreeSet<ModelRef> {
        let Some(actor) = self.actor(actor) else {
            return Default::default();
        };
        let Geometry::Model {
            model,
            presentation,
            ..
        } = actor.geometry
        else {
            return Default::default();
        };
        let mut models = std::collections::BTreeSet::from([model]);
        for (attachment, modifiers) in self
            .programs
            .iter()
            .flat_map(|program| &program.emissions)
            .filter_map(|emission| match &emission.command {
                EffectCommand::Particle {
                    actor: id,
                    modifiers,
                    attachment,
                } if *id == actor.id => Some((attachment, modifiers)),
                _ => None,
            })
        {
            if matches!(attachment, Attachment::BoneGroup(_)) {
                for (from, to) in
                    group_model_choices(model.index(), presentation.animation_selector, modifiers)
                        .expect("validated group model choices")
                {
                    let before = model.with_index(from).inherit_animation();
                    models.insert(before);
                    models.insert(before.with_index(to));
                }
            } else {
                for index in
                    model_indices(model.index(), presentation.animation_selector, modifiers)
                        .expect("validated effect resource choices")
                {
                    models.insert(model.with_index(index));
                }
            }
        }
        models
    }
    pub fn externally_animated(&self, binding: ModelRef) -> bool {
        self.actors.iter().any(|actor| {
            matches!(
                actor.geometry,
                Geometry::Model {
                    presentation: ModelPresentation {
                        external_animation: true,
                        ..
                    },
                    ..
                }
            ) && self.models_for(actor.id).contains(&binding)
        })
    }
    pub fn models(&self) -> impl Iterator<Item = ModelRef> + '_ {
        self.actors
            .iter()
            .flat_map(|actor| self.models_for(actor.id))
    }
    pub fn validate(&self) -> Result<()> {
        for (i, p) in self.programs.iter().enumerate() {
            ensure!(
                self.programs[..i].iter().all(|v| v.id != p.id),
                "duplicate battle effect program"
            );
            ensure!(
                p.emissions.windows(2).all(|v| v[0].tick <= v[1].tick),
                "unordered effect timeline"
            );
            for (emission_index, e) in p.emissions.iter().enumerate() {
                ensure!(e.tick <= p.end_tick, "invalid effect tick");
                ensure!(
                    e.repeat.is_none_or(|r| r.count > 0),
                    "empty effect repetition"
                );
                match &e.command {
                    EffectCommand::Controller { actor, controller } => {
                        ensure!(
                            self.actor(*actor).is_none(),
                            "effect actor is both particle and controller"
                        );
                        controller.validate()?;
                    }
                    EffectCommand::Particle {
                        actor,
                        attachment,
                        modifiers,
                    } => {
                        ensure!(
                            !modifiers
                                .iter()
                                .any(|m| matches!(m, Modifier::SetEmitterAxisVector { .. })),
                            "heading-relative vector birth requires ordered world-space initialization"
                        );
                        if modifiers
                            .iter()
                            .any(|m| matches!(m, Modifier::RequireFreshIntegers))
                        {
                            ensure!(
                                fresh_integer_scratch(&p.emissions[..emission_index])
                                    && e.repeat.is_none()
                                    && !matches!(attachment, Attachment::BoneGroup(_)),
                                "fresh scratch requires an unrepeated single birth after only unmodified births"
                            );
                        }
                        if modifiers
                            .iter()
                            .any(|m| matches!(m, Modifier::RequireIntegerRange { .. }))
                        {
                            ensure!(
                                e.repeat.is_none()
                                    && !matches!(attachment, Attachment::BoneGroup(_)),
                                "inherited scratch requires an unrepeated scalar birth"
                            );
                            resources::validate_birth_ranges(
                                &p.emissions[..emission_index],
                                e.tick,
                                modifiers,
                            )?;
                        }
                        let actor = self.actor(*actor).context("missing effect actor")?;
                        actor.validate_modifiers(modifiers)?;
                        if matches!(attachment, Attachment::BoneGroup(_)) {
                            group_materials(actor, modifiers)?.validate(actor)?;
                        }
                        if matches!(attachment, Attachment::BoneGroup(_))
                            && let Geometry::Model {
                                model,
                                presentation,
                                ..
                            } = actor.geometry
                        {
                            group_model_choices(
                                model.index(),
                                presentation.animation_selector,
                                modifiers,
                            )?;
                        }
                    }
                    EffectCommand::ModifyRetained { slot, modifiers } => {
                        ensure!(
                            !modifiers.iter().any(|m| matches!(
                                m,
                                Modifier::RequireFreshIntegers
                                    | Modifier::RequireIntegerRange { .. }
                            )),
                            "retained modification cannot require birth scratch"
                        );
                        ensure!(
                            usize::from(*slot) < RETAINED_EFFECT_SLOTS,
                            "invalid retained effect slot"
                        );
                        let mut actors = p.retained_actors(self).peekable();
                        ensure!(
                            actors.peek().is_some(),
                            "retained effect command has no retained actors"
                        );
                        for actor in actors {
                            actor.validate_retained_modifiers(modifiers)?;
                        }
                    }
                    EffectCommand::Sound { sound, .. } => {
                        ensure!((1..=255).contains(sound), "invalid effect sound")
                    }
                }
            }
            for (id, choices) in retained_materials(p, self)? {
                let actor = self.actor(id).unwrap();
                choices.validate(actor)?;
            }
        }
        for (i, a) in self.actors.iter().enumerate() {
            ensure!(
                self.actors[..i].iter().all(|v| v.id != a.id),
                "duplicate battle effect actor"
            );
            ensure!(
                a.blend_variants.is_empty()
                    || (a.blend_variants.contains(&a.blend)
                        && a.screen_texture.is_none()
                        && !matches!(a.geometry, Geometry::Model { .. } | Geometry::NoDraw { .. })),
                "incompatible effect blend variants"
            );
            if let Some(palette) = &a.palette {
                ensure!(
                    a.material == palette.material()
                        && !palette.materials.is_empty()
                        && palette
                            .materials
                            .values()
                            .all(|&m| usize::from(m) < self.materials.len())
                        && a.screen_texture.is_none()
                        && a.element_variants.is_empty()
                        && !matches!(a.geometry, Geometry::Model { .. } | Geometry::NoDraw { .. })
                        && a.uv_animation.as_ref().is_none_or(|uv| uv
                            .frames
                            .iter()
                            .all(|f| f.update.material().is_none())),
                    "invalid or conflicting effect palette variants"
                );
            }
            if let Some(index) = a.material {
                self.materials
                    .get(usize::from(index))
                    .context("missing effect material")?;
            }
            ensure!(
                a.screen_texture.is_none()
                    || (matches!(a.geometry, Geometry::Ring { lines: false, .. })
                        && a.blend == Blend::Alpha
                        && a.owner_layer.is_none()
                        && a.material.is_some()
                        && a.copies == 1
                        && a.uv_animation.is_none()
                        && a.element_variants.is_empty()
                        && !a.use_element_variant),
                "unsupported screen-textured effect"
            );
            for (i, variant) in a.element_variants.iter().enumerate() {
                ensure!(
                    a.element_variants[..i]
                        .iter()
                        .all(|v| v.element != variant.element)
                        && variant.outer_color.iter().all(|v| (0..=255).contains(v))
                        && variant
                            .material
                            .is_none_or(|m| usize::from(m) < self.materials.len()),
                    "invalid effect element variant"
                );
            }
            ensure!(
                a.element_variants.is_empty() || a.element_variants.len() == Element::ALL.len(),
                "incomplete effect element variants"
            );
            ensure!(
                !a.use_element_variant || !a.element_variants.is_empty(),
                "elemental effect has no palette variants"
            );
            ensure!(
                matches!(
                    a.geometry,
                    Geometry::Model { .. }
                        | Geometry::NoDraw { .. }
                        | Geometry::Disc { .. }
                        | Geometry::Hemisphere { .. }
                        | Geometry::Ring { .. }
                        | Geometry::RadialQuads { .. }
                        | Geometry::Ellipsoid { .. }
                        | Geometry::JitterRibbon { .. }
                        | Geometry::TriangleBand
                ) || a.material.is_some(),
                "textured effect has no material"
            );
            ensure!(
                [
                    a.position,
                    a.velocity,
                    a.acceleration,
                    a.acceleration_change,
                    a.angles,
                    a.angular_velocity,
                    a.local_offset,
                    a.local_velocity,
                    a.dimensions,
                    a.dimension_velocity,
                    a.dimension_acceleration
                ]
                .iter()
                .flatten()
                .all(|v| v.is_finite()),
                "non-finite effect motion"
            );
            ensure!(
                a.colors.iter().flatten().all(|v| (0..=255).contains(v)) && a.copies > 0,
                "invalid effect color or copy count"
            );
            ensure!(a.lifetime != Some(0), "zero effect lifetime");
            ensure!(
                !matches!(a.geometry, Geometry::NoDraw { .. })
                    || (a.material.is_none()
                        && a.uv_animation.is_none()
                        && a.element_variants.is_empty()),
                "nondrawing actor requests unsupported texture animation"
            );
            ensure!(
                match a.space {
                    EffectSpace::World => true,
                    EffectSpace::OwnerBone { .. } => {
                        matches!(a.geometry, Geometry::Ring { .. })
                            && matches!(a.orientation, Orientation::World)
                            && a.screen_texture.is_none()
                    }
                    EffectSpace::OwnerBonePosition { .. } => {
                        !matches!(
                            a.geometry,
                            Geometry::NoDraw {
                                motion: NoDrawMotion::PointHistory { .. }
                            }
                        )
                    }
                },
                "unsupported owner-bone effect geometry"
            );
            // Bone-emission vectors point into the loader's expired stack frame.
            // The stable owner-joint lookup takes precedence over that follow pointer.
            ensure!(
                a.follow_emitter
                    || matches!(a.space, EffectSpace::OwnerBonePosition { .. })
                    || !self.modifiers_for(a).flatten().any(|m| matches!(
                        m,
                        Modifier::Flag {
                            field: EffectFlag::FollowEmitter,
                            enabled: true
                        }
                    ))
                    || self
                        .programs
                        .iter()
                        .flat_map(|p| &p.emissions)
                        .all(|e| match e.command {
                            EffectCommand::Particle {
                                actor, attachment, ..
                            } if actor == a.id => matches!(attachment, Attachment::Emitter),
                            _ => true,
                        }),
                "following a temporary bone-emission vector is unsupported"
            );
            ensure!(
                a.orientation != Orientation::FixedWorld
                    || (matches!(a.geometry, Geometry::Ring { .. })
                        && matches!(a.space, EffectSpace::World)
                        && !a.follow_emitter),
                "fixed world orientation requires an independent ring"
            );
            ensure!(
                a.orientation != Orientation::FollowMotion
                    || matches!(a.geometry, Geometry::VertexQuad { .. })
                        && matches!(a.space, EffectSpace::World)
                        && a.follow_emitter,
                "motion-facing vertices must follow a world emitter"
            );
            a.geometry.validate_dynamic()?;
            for effect in a.child_effects() {
                ensure!(
                    effect.bank == a.id.bank && self.program(effect).is_some(),
                    "missing or cross-bank particle child {effect:?}"
                );
            }
            if matches!(
                a.geometry,
                Geometry::RadialQuads { .. }
                    | Geometry::Ellipsoid { .. }
                    | Geometry::JitterRibbon { .. }
                    | Geometry::TriangleBand
            ) {
                ensure!(a.copies == 1, "procedural effect does not support copies");
            }
            if matches!(
                a.geometry,
                Geometry::Model {
                    presentation: ModelPresentation {
                        orientation: ModelOrientation::FollowMotion,
                        ..
                    },
                    ..
                }
            ) {
                ensure!(
                    a.follow_emitter,
                    "motion-facing model must follow an emitter"
                );
            }
            if let Some(animation) = &a.uv_animation {
                if animation.model_scroll.is_some() {
                    ensure!(
                        matches!(a.geometry, Geometry::Model { .. })
                            && animation.frames.is_empty()
                            && animation.loop_to.is_none(),
                        "invalid model UV scroll"
                    );
                } else {
                    ensure!(
                        !animation.frames.is_empty()
                            && animation
                                .frames
                                .iter()
                                .all(|f| (1..=127).contains(&f.duration)
                                    || matches!(f.update, UvUpdate::Scroll { .. }))
                            && animation
                                .loop_to
                                .is_none_or(|i| usize::from(i) < animation.frames.len()),
                        "invalid effect UV animation"
                    );
                    if animation
                        .frames
                        .iter()
                        .any(|f| matches!(f.update, UvUpdate::Scroll { .. }))
                    {
                        ensure!(
                            !matches!(a.geometry, Geometry::Model { .. })
                                && animation.frames.len() == 1
                                && animation.loop_to.is_none()
                                && animation.frames[0].duration <= UvEnd::INTERVAL,
                            "invalid sprite UV scroll"
                        );
                    }
                    for (index, frame) in animation.frames.iter().enumerate() {
                        if matches!(frame.update, UvUpdate::End { .. }) {
                            ensure!(
                                index > 0
                                    && index + 1 == animation.frames.len()
                                    && frame.duration == UvEnd::INTERVAL
                                    && animation.loop_to.is_none(),
                                "invalid terminal effect UV row"
                            );
                        }
                        if let Some(material) = frame.update.material() {
                            ensure!(
                                !matches!(a.geometry, Geometry::Model { .. })
                                    && usize::from(material) < self.materials.len(),
                                "invalid animated effect palette"
                            );
                        }
                        if matches!(
                            frame.update,
                            UvUpdate::ModelPalette { .. }
                                | UvUpdate::End {
                                    end: UvEnd::ModelPalette { .. }
                                }
                        ) {
                            ensure!(
                                matches!(a.geometry, Geometry::Model { .. }),
                                "model palette row on a non-model effect"
                            );
                        }
                    }
                }
            }
            if let Geometry::VertexQuad {
                vertices,
                velocities,
                copy_axis,
                ..
            } = a.geometry
            {
                ensure!(
                    copy_axis < 3
                        && vertices
                            .iter()
                            .chain(&velocities)
                            .flatten()
                            .all(|v| v.is_finite()),
                    "invalid moving effect quad"
                );
            }
            if let Geometry::Spiral {
                segments,
                uv_rows,
                segment_offset,
                angle_step,
                radius_step,
                ..
            } = a.geometry
            {
                ensure!(
                    segments > 0
                        && uv_rows > 0
                        && angle_step.is_finite()
                        && radius_step.is_finite()
                        && segment_offset.iter().all(|v| v.is_finite()),
                    "invalid effect spiral"
                );
            }
            if let Geometry::BillboardTrail {
                segments,
                segment_offset,
                angle_step,
                size_step,
                ..
            } = a.geometry
            {
                ensure!(
                    segments > 0
                        && angle_step.is_finite()
                        && segment_offset
                            .iter()
                            .chain(&size_step)
                            .all(|v| v.is_finite()),
                    "invalid effect billboard trail"
                );
            }
        }
        for m in &self.materials {
            crate::validate_asset_path(&m.texture.path)?;
            ensure!(
                m.texture.width > 0
                    && m.texture.height > 0
                    && m.rgb_scale.is_finite()
                    && m.rgb_scale > 0.,
                "invalid effect texture"
            );
        }
        Ok(())
    }
}
impl Modifier {
    pub fn writes_model_index(&self) -> bool {
        self.writes_model_selection()
            || matches!(
                self,
                Self::Byte {
                    field: ByteField::ModelIndex,
                    ..
                }
            )
    }
    pub fn writes_material_selection(&self) -> bool {
        self.writes_palette_selection()
            || matches!(
                self,
                Self::Byte {
                    field: ByteField::Palette | ByteField::Blend,
                    ..
                }
            )
    }
    pub fn writes_palette_selection(&self) -> bool {
        matches!(
            self,
            Self::Integer {
                field: IntegerField::PaletteSelection,
                ..
            } | Self::RandomInteger {
                field: IntegerField::PaletteSelection,
                ..
            }
        )
    }
    fn writes_model_selection(&self) -> bool {
        matches!(
            self,
            Self::Integer {
                field: IntegerField::ModelSelection,
                ..
            } | Self::RandomInteger {
                field: IntegerField::ModelSelection,
                ..
            }
        )
    }

    fn validate(&self) -> Result<()> {
        match *self {
            Self::AnimationPosition { value } | Self::AnimationRate { value } => {
                value.validate()?
            }
            Self::PlayModelAnimation { animation } => ensure!(
                animation.model < 10
                    && animation.clip < 4
                    && animation.rate.is_finite()
                    && animation.rate != 0.,
                "invalid effect model animation"
            ),
            Self::RequireFreshIntegers | Self::Flag { .. } => {}
            Self::RequireIntegerRange { index, min, max } => ensure!(
                index < 4 && min <= max && i32::from(max) - i32::from(min) < 256,
                "invalid integer scratch range"
            ),
            Self::Lifetime { operation, value } => {
                value.validate()?;
                ensure!(
                    !matches!(
                        (operation, value),
                        (Arithmetic::Divide, IntegerValue::Constant(0))
                    ),
                    "effect lifetime division by zero"
                );
            }
            Self::Byte { field, value, .. } => {
                if let ByteField::Brighten(channel) | ByteField::Darken(channel) = field {
                    ensure!(channel < 4, "invalid effect color-rate component");
                }
                value.validate()?;
            }
            Self::Integer { field, value, .. } => {
                validate_integer(field)?;
                value.validate()?;
            }
            Self::Float { field, value, .. } => {
                validate_float(field)?;
                value.validate()?;
            }
            Self::SetEmitterAxisVector { field, value, .. } => {
                ensure!(
                    matches!(field, VectorField::Velocity | VectorField::Acceleration),
                    "unsupported heading-relative vector destination"
                );
                value.validate()?;
            }
            Self::RotateVector { field, angle, .. } | Self::PolarVector { field, angle, .. } => {
                for component in field.components() {
                    validate_float(component)?;
                }
                angle.validate()?;
                if let Self::PolarVector { radius, .. } = *self {
                    radius.validate()?;
                }
            }
            Self::RandomPolarVector {
                field,
                angles,
                radius,
                ..
            } => {
                for component in field.components() {
                    validate_float(component)?;
                }
                ensure!(
                    angles.into_iter().chain([radius]).all(f32::is_finite),
                    "non-finite polar effect modifier"
                );
            }
            Self::RandomInteger { field, modulus } => {
                validate_integer(field)?;
                ensure!(modulus > 0, "zero effect random modulus");
            }
            Self::RandomFloat {
                field,
                range,
                scale,
            } => {
                validate_float(field)?;
                ensure!(
                    !matches!(range, FloatRandomRange::Remainder(0)) && scale.is_finite(),
                    "invalid effect randomness"
                );
            }
        }
        Ok(())
    }
}
fn validate_integer(field: IntegerField) -> Result<()> {
    match field {
        IntegerField::Temporary(i) | IntegerField::FloatTemporaryHigh(i) => {
            ensure!(i < 4, "invalid integer temporary")
        }
        IntegerField::Color { end, channel } => {
            ensure!(end < 2 && channel < 4, "invalid color component")
        }
        _ => {}
    }
    Ok(())
}
fn validate_float(field: FloatField) -> Result<()> {
    let (i, limit) = match field {
        FloatField::GeometryAngleStep => return Ok(()),
        FloatField::Temporary(i) => (i, 4),
        FloatField::Position(i)
        | FloatField::Velocity(i)
        | FloatField::Acceleration(i)
        | FloatField::Angle(i)
        | FloatField::AngularVelocity(i)
        | FloatField::LocalOffset(i)
        | FloatField::LocalVelocity(i)
        | FloatField::Dimension(i)
        | FloatField::DimensionVelocity(i)
        | FloatField::DimensionAcceleration(i)
        | FloatField::GeometrySegmentOffset(i) => (i, 3),
    };
    ensure!(i < limit, "invalid effect vector component");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_animation_keeps_existing_json_shape() {
        let json = serde_json::json!({"frames":[{"duration":4,"rect":[1,65,30,30]}],"loop_to":0});
        let animation: UvAnimation = serde_json::from_value(json.clone()).unwrap();
        assert!(animation.model_scroll.is_none());
        assert_eq!(serde_json::to_value(animation).unwrap(), json);
    }

    #[test]
    fn lifetime_operands_read_legacy_literals_and_round_trip_typed_values() {
        for value in [
            serde_json::json!(30),
            serde_json::json!({"kind":"constant","value":30}),
            serde_json::json!({"kind":"temporary","value":3}),
        ] {
            let modifier: Modifier = serde_json::from_value(serde_json::json!({
                "kind":"lifetime", "operation":"add", "value":value
            }))
            .unwrap();
            let Modifier::Lifetime { value, .. } = modifier else {
                unreachable!()
            };
            assert_eq!(value.resolve(&[0, 0, 0, 30]).unwrap(), 30);
            let encoded = serde_json::to_value(modifier).unwrap();
            let decoded: Modifier = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
        }
    }

    #[test]
    fn owner_layers_preserve_legacy_boolean_catalogues() {
        for (json, expected) in [
            ("false", None),
            ("true", Some(OwnerLayer::After)),
            ("null", None),
            ("\"before\"", Some(OwnerLayer::Before)),
            ("\"after\"", Some(OwnerLayer::After)),
        ] {
            let mut input = serde_json::Deserializer::from_str(json);
            assert_eq!(deserialize_owner_layer(&mut input).unwrap(), expected);
        }
        let mut input = serde_json::Deserializer::from_str("\"other\"");
        assert!(deserialize_owner_layer(&mut input).is_err());
        assert_eq!(
            serde_json::to_string(&Some(OwnerLayer::Before)).unwrap(),
            "\"before\""
        );
    }
}

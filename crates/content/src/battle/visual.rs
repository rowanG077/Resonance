//! Converted battle geometry and native-indexed animation clips.
use crate::{menu_data::Costume, model_preview::ModelPreview};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualAssets {
    pub arenas: BTreeMap<u16, ArenaVisuals>,
    pub party: BTreeMap<u8, PartyVisuals>,
    pub enemies: BTreeMap<u8, ModelVisuals>,
    pub weapons: BTreeMap<u16, WeaponVisuals>,
    pub pow_weapons: BTreeMap<super::unison::PowWeapon, WeaponVisuals>,
    /// Slot zero changes; Presea's second carried instance remains equipped.
    pub pow_devastation: BTreeMap<u16, WeaponVisuals>,
    pub toon_ramp: String,
    pub shadow_texture: String,
    pub effect_models: Vec<EffectModel>,
}

/// Typed body/CAB selections for one character. Aliases point directly to a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyVisuals {
    pub bindings: BTreeMap<Costume, PartyVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PartyVariant {
    Model(Box<PartyModel>),
    Alias(Costume),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyModel {
    pub visual: ModelVisuals,
    /// Resolved again against this body's rig, as at native actor construction.
    pub head_bone: u16,
    /// Equipment-item motion banks decoded from this selection's CAB.
    pub weapon_motions: BTreeMap<u16, LinkedWeaponMotions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedWeaponMotions {
    /// Logical carried slots; geometry and textures remain in WeaponVisuals.
    pub rigs: BTreeMap<u8, Rig>,
    pub link: WeaponMotionLink,
}

impl PartyVisuals {
    /// Missing selections, missing targets and every alias chain fail preparation.
    pub fn resolve(&self, costume: Costume) -> Result<(Costume, &PartyModel)> {
        match self.bindings.get(&costume) {
            Some(PartyVariant::Model(model)) => Ok((costume, model)),
            Some(PartyVariant::Alias(canonical)) => match self.bindings.get(canonical) {
                Some(PartyVariant::Model(model)) => Ok((*canonical, model)),
                _ => anyhow::bail!(
                    "party costume {costume:?} alias target {canonical:?} is not a concrete model"
                ),
            },
            None => anyhow::bail!("unprepared party costume {costume:?}"),
        }
    }

    /// Explicit Standard access for callers that do not represent a selected actor.
    pub fn standard(&self) -> Result<&PartyModel> {
        self.resolve(Costume::Standard).map(|(_, model)| model)
    }

    pub fn concrete(&self) -> impl Iterator<Item = (Costume, &PartyModel)> {
        self.bindings
            .iter()
            .filter_map(|(&costume, binding)| match binding {
                PartyVariant::Model(model) => Some((costume, model.as_ref())),
                PartyVariant::Alias(_) => None,
            })
    }

    pub fn validate(&self, character: u8) -> Result<()> {
        for &costume in required_costumes(character)? {
            self.resolve(costume)
                .with_context(|| format!("required party costume {character}:{costume:?}"))?;
        }
        self.validate_bindings()?;
        for (costume, model) in self.concrete() {
            ensure!(
                model
                    .visual
                    .rig
                    .skeleton
                    .bones
                    .get(usize::from(model.head_bone))
                    .is_some_and(|bone| bone.name.eq_ignore_ascii_case("Bone_atama")),
                "invalid party head anchor {character}:{costume:?}"
            );
        }
        Ok(())
    }

    fn validate_bindings(&self) -> Result<()> {
        for &costume in self.bindings.keys() {
            self.resolve(costume)?;
        }
        let mut pairs = std::collections::BTreeSet::new();
        for (costume, model) in self.concrete() {
            ensure!(
                pairs.insert((&model.visual.model_sha256, &model.visual.animation_sha256)),
                "duplicate concrete body/CAB pair at {costume:?}; use a direct alias"
            );
        }
        Ok(())
    }
}

/// Original title overrides plus Standard and the Colette/Presea story defaults.
/// Source-table recovery verifies these 36 logical selections before cooking.
pub fn required_costumes(character: u8) -> Result<&'static [Costume]> {
    use Costume::*;
    Ok(match character {
        2 | 7 => &[Standard, Variant1, Variant2, Story, Variant4],
        1 | 3..=6 | 8 => &[Standard, Variant1, Variant2, Variant4],
        9 => &[Standard, Variant1],
        _ => anyhow::bail!("invalid party costume owner {character}"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "item", rename_all = "snake_case")]
pub enum WeaponModel {
    Equipment(u16),
    PowBlade,
    PowDevastation(u16),
    PowSpear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectModel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig: Option<Rig>,
    /// Rig joint for each rendered layer joint, preserving each layer's authored names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pose_joints: Vec<Vec<u16>>,
    pub binding: super::effect_program::ModelRef,
    pub model: ModelPreview,
}

impl EffectModel {
    pub fn validate_pose_joints(&self) -> Result<()> {
        let Some(rig) = &self.rig else {
            ensure!(
                self.pose_joints.is_empty(),
                "effect pose mapping has no rig"
            );
            return Ok(());
        };
        let count = rig.skeleton.bones.len();
        ensure!(
            self.pose_joints.len() == self.model.parts.len()
                && self
                    .pose_joints
                    .first()
                    .is_some_and(|joints| joints.len() == count
                        && joints
                            .iter()
                            .enumerate()
                            .all(|(index, &joint)| usize::from(joint) == index)),
            "effect model {:?} has incomplete pose layers or unordered primary joints",
            self.binding
        );
        for (index, (joints, part)) in self.pose_joints.iter().zip(&self.model.parts).enumerate() {
            ensure!(
                joints.len() == part.scene.bone_names.len()
                    && joints.iter().all(|&joint| usize::from(joint) < count),
                "effect model {:?} layer {index} has an incomplete or invalid pose joint mapping",
                self.binding
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponVisuals {
    pub source_sha256: String,
    /// Logical carried slot; instances may share model paths but keep separate poses.
    pub slots: BTreeMap<u8, ModelPreview>,
    pub rigs: BTreeMap<u8, Rig>,
    pub trails: BTreeMap<u8, TrailVisual>,
    pub motion_link: Option<WeaponMotionLink>,
}

impl WeaponVisuals {
    /// Expand package resources into logical carried instances without recooking geometry.
    pub fn with_instances(mut self, packages: &[u8]) -> Result<Self> {
        ensure!(
            !packages.is_empty() && packages.len() <= 8,
            "invalid weapon instance count"
        );
        let models = std::mem::take(&mut self.slots);
        let rigs = std::mem::take(&mut self.rigs);
        let trails = std::mem::take(&mut self.trails);
        for (slot, package) in packages.iter().enumerate() {
            let slot = slot as u8;
            self.slots.insert(
                slot,
                models
                    .get(package)
                    .context("missing carried model package")?
                    .clone(),
            );
            self.rigs.insert(
                slot,
                rigs.get(package)
                    .context("missing carried rig package")?
                    .clone(),
            );
            if let Some(trail) = trails.get(package) {
                self.trails.insert(slot, trail.clone());
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponMotionLink {
    pub source_sha256: String,
    pub owner: u8,
    pub offset: u16,
    pub fallback: u16,
    pub time_scale: f32,
}

/// Source-linked carried-controller bounds, independent of the body interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkedPlayback {
    pub clip: u16,
    pub start: f32,
    pub end: f32,
}

impl WeaponMotionLink {
    /// The native mirror preserves its fixed-scale start even after selecting
    /// the fallback resource. Without a blend, its clock normalizes before the
    /// first sample; the ordinary body's start/end interval rules do not apply.
    pub fn playback(
        &self,
        rig: &Rig,
        clip: u8,
        start: u8,
        end: Option<u8>,
        blend: u8,
    ) -> Result<LinkedPlayback> {
        ensure!(
            self.time_scale.is_finite() && self.time_scale > 0.,
            "invalid linked weapon time scale"
        );
        ensure!(
            rig.motions.contains_key(&self.fallback),
            "missing linked weapon fallback"
        );
        let clip = u16::from(clip)
            .checked_add(self.offset)
            .filter(|clip| rig.motions.contains_key(clip))
            .unwrap_or(self.fallback);
        let duration = rig
            .motions
            .get(&clip)
            .context("missing linked weapon motion")?
            .duration_frames;
        ensure!(
            duration.is_finite() && duration > 0.,
            "invalid linked weapon motion period"
        );
        let start = f32::from(start) * self.time_scale;
        let end = end.map_or(duration, |end| f32::from(end) * self.time_scale);
        ensure!(
            start.is_finite() && end.is_finite() && end <= duration,
            "linked weapon end exceeds selected clip"
        );
        // A blended entry can sample before clock normalization. Raw source SDK
        // sampling beyond the terminal period remains an unsupported boundary.
        ensure!(
            blend <= 1 || start <= end && start <= duration,
            "linked weapon blended start exceeds selected interval"
        );
        Ok(LinkedPlayback { clip, start, end })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVisuals {
    pub model_sha256: String,
    pub animation_sha256: String,
    /// Declared CAB slots, including authored nulls. Missing metadata never permits a no-op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_motions: Option<AuthoredMotionTable>,
    /// Model-local geometry and tracks retain authored Z-up coordinates.
    /// Clip slots are native motion indices, including gaps in the source table.
    pub model: ModelPreview,
    pub volumes: Vec<BoneVolume>,
    pub rig: Rig,
    pub alpha: u8,
    pub shadow: Option<Shadow>,
    pub bounds_joints: Vec<u16>,
    pub target_anchor: TargetAnchor,
    pub initial_pose: InitialPose,
    pub paired_body: Option<PairedBody>,
    pub texture_layers: Vec<TextureLayer>,
    /// An independent, persistent appearance row on the primary body texture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_texture: Option<TextureLayer>,
    pub victory: BTreeMap<VictoryStyle, VictoryMotion>,
    /// Embedded enemy weapons retain their own rig and native attachment slot.
    pub attachments: BTreeMap<u8, AttachmentVisual>,
    pub trails: BTreeMap<u8, TrailVisual>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthoredMotionTable {
    pub count: u16,
    pub nulls: std::collections::BTreeSet<u16>,
}

pub enum MotionSlot<'a> {
    Present(&'a super::pose::Motion),
    AuthoredNull,
}

impl AuthoredMotionTable {
    pub fn validate(&self, rig: &Rig, external: impl IntoIterator<Item = u16>) -> Result<()> {
        let external: std::collections::BTreeSet<_> = external.into_iter().collect();
        ensure!(
            (1..=u16::from(u8::MAX)).contains(&self.count)
                && self.nulls.iter().all(|&slot| slot < self.count)
                && (0..self.count)
                    .all(|slot| rig.motions.contains_key(&slot) != self.nulls.contains(&slot))
                && external
                    .iter()
                    .all(|&slot| slot >= self.count && rig.motions.contains_key(&slot))
                && rig
                    .motions
                    .keys()
                    .all(|slot| *slot < self.count || external.contains(slot)),
            "invalid authored actor motion table partition"
        );
        Ok(())
    }
}

impl ModelVisuals {
    /// The caller resolves external resources before checking their cooked slot.
    pub fn motion_slot(&self, slot: u16) -> Result<MotionSlot<'_>> {
        const EXTERNAL_SOURCE_START: u16 = 100;
        const COOKED_EXTERNAL_START: u16 = 256;
        ensure!(
            !(EXTERNAL_SOURCE_START..COOKED_EXTERNAL_START).contains(&slot),
            "unrelocated external actor motion {slot}"
        );
        let null = self
            .authored_motions
            .as_ref()
            .is_some_and(|table| slot < table.count && table.nulls.contains(&slot));
        if let Some(motion) = self.rig.motions.get(&slot) {
            ensure!(
                !null,
                "actor motion {slot} is both present and authored null"
            );
            return Ok(MotionSlot::Present(motion));
        }
        // Native indices >=100 use a separate external table. Genis's high CAB
        // slots belong to linked weapons and cannot authorize a body no-op.
        if slot < EXTERNAL_SOURCE_START && null {
            return Ok(MotionSlot::AuthoredNull);
        }
        anyhow::bail!("missing actor motion {slot}")
    }
}

/// Initial body binding and independent face clock, recovered from actor metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitialPose {
    pub clip: u16,
    pub blink: bool,
    /// This model rejects ordinary body commands below clip 30.
    pub fixed_motion: bool,
}

impl InitialPose {
    pub fn suppresses_motion(&self, clip: u16) -> bool {
        self.fixed_motion && clip < 30
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedBody {
    /// None follows the actor root; otherwise follows this primary body's posed joint.
    pub bone: Option<u16>,
    pub parts: Vec<u16>,
    pub rig: Rig,
    pub initial_clip: u16,
    /// Body commands address the paired clip at this offset in the shared motion table.
    pub clip_offset: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetAnchor {
    /// Root-relative in world axes when absent; yaw-relative to this joint otherwise.
    pub bone: Option<u16>,
    pub offset: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentVisual {
    pub bone: u16,
    pub parts: Vec<u16>,
    pub rig: Rig,
    /// Trail endpoints ordered by their authored KI index.
    pub trail_bones: Vec<u16>,
    pub trail_style: Option<TrailStyle>,
    pub toon: bool,
    pub follow_bone: bool,
}

/// Authored result variants; character and story rules determine availability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum VictoryStyle {
    Healthy = 0,
    Tired,
    Standard,
    Rare,
    Alternate,
}

impl VictoryStyle {
    pub const ALL: [Self; 5] = [
        Self::Healthy,
        Self::Tired,
        Self::Standard,
        Self::Rare,
        Self::Alternate,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VictoryMotion {
    pub source_sha256: String,
    /// Cooked clip slot replacing the shared native result-motion binding.
    pub clip: u16,
    pub animations: super::actions::AnimationProgram,
}

impl VictoryMotion {
    pub const NATIVE_CLIP: u8 = 23;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shadow {
    pub scale: f32,
    pub color: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureLayer {
    pub texture: u16,
    pub frames: u8,
}

/// Texture matrices use the first matching descriptor; the appearance row follows animation.
pub fn texture_offset(
    primary_body: bool,
    layers: &[TextureLayer],
    frames: &[u8; 4],
    variant: Option<(&TextureLayer, u8)>,
    texture: usize,
) -> f32 {
    if !primary_body {
        return 0.;
    }
    layers
        .iter()
        .zip(frames.iter().copied())
        .chain(variant)
        .find_map(|(layer, frame)| {
            (usize::from(layer.texture) == texture)
                .then(|| f32::from(frame) / f32::from(layer.frames))
        })
        .unwrap_or(0.)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailStyle {
    /// Palette variants selected by battle state; None explicitly means untextured.
    pub textures: BTreeMap<u8, Option<crate::font::UiTexture>>,
    pub palette: u8,
    pub rgb: [u8; 3],
    pub uv: [i16; 4],
    pub blend: super::effect_program::Blend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailVisual {
    pub bones: Vec<u16>,
    pub style: TrailStyle,
}

impl TrailVisual {
    pub fn validate(&self, rig: &Rig) -> Result<()> {
        ensure!(
            (2..=3).contains(&self.bones.len())
                && self
                    .bones
                    .iter()
                    .all(|&i| usize::from(i) < rig.skeleton.bones.len()),
            "invalid trail endpoints"
        );
        self.style.validate()
    }
}

impl TrailStyle {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.textures.contains_key(&self.palette),
            "missing default trail palette"
        );
        let textured = self.textures[&self.palette].is_some();
        for texture in self.textures.values() {
            ensure!(
                texture.is_some() == textured,
                "inconsistent trail texture variants"
            );
            if let Some(texture) = texture {
                crate::validate_asset_path(&texture.path)?;
                ensure!(
                    texture.width > 0 && texture.height > 0,
                    "empty trail texture"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rig {
    pub skeleton: super::pose::Skeleton,
    pub motions: BTreeMap<u16, super::pose::Motion>,
    pub attack_groups: BTreeMap<u8, Vec<u16>>,
    pub effect_groups: BTreeMap<u8, Vec<u16>>,
    pub weapon_bones: BTreeMap<u8, u16>,
}
impl Rig {
    pub fn validate(&self) -> Result<()> {
        self.skeleton.validate()?;
        for motion in self.motions.values() {
            motion.validate(&self.skeleton)?;
        }
        ensure!(
            self.attack_groups
                .values()
                .flatten()
                .chain(self.effect_groups.values().flatten())
                .chain(self.weapon_bones.values())
                .all(|&bone| usize::from(bone) < self.skeleton.bones.len()),
            "invalid battle rig group"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VolumeKind {
    BodyAndHurt,
    Hurt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoneVolume {
    pub bone: String,
    pub radius: f32,
    pub kind: VolumeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaVisuals {
    pub source_sha256: String,
    pub model: ModelPreview,
    /// Placement is in battle Y-up space: Ry(yaw) * Rx(-90°) on Z-up meshes.
    pub yaw_degrees: f32,
    pub camera_pitch_offset: f32,
    pub translation: [f32; 3],
    /// One playback rate per mesh layer; static layers retain their source rate.
    pub animation_rates: Vec<f32>,
    /// Authored order per mesh layer. The first channel matching a texture wins.
    pub uv_channels: Vec<Vec<ArenaUvChannel>>,
    pub ambient: [u8; 4],
    /// Actor lighting is independent of the stage color. Older catalogs must recook for corpses.
    #[serde(default)]
    pub actor_ambient: Option<[u8; 3]>,
    pub light_position: [f32; 3],
}

impl ArenaVisuals {
    pub fn validate(&self) -> Result<()> {
        self.model.validate()?;
        validate_digest(&self.source_sha256)?;
        ensure!(
            self.yaw_degrees.is_finite()
                && self.camera_pitch_offset.is_finite()
                && self.translation.iter().all(|v| v.is_finite())
                && self.light_position.iter().all(|v| v.is_finite())
                && self.animation_rates.len() == self.model.parts.len()
                && self.uv_channels.len() == self.model.parts.len()
                && self
                    .animation_rates
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.),
            "invalid battle arena transform or animation rate"
        );
        for channels in &self.uv_channels {
            ensure!(channels.len() <= 4, "too many arena UV channels");
            for channel in channels {
                channel.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArenaUvMode {
    #[default]
    Disabled,
    Frames,
    Scroll,
    Oscillate,
}

/// Parameters and initial state of one stepped texture translation.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArenaUvChannel {
    pub mode: ArenaUvMode,
    /// Matches material texture IDs; unmatched authored selectors are inert.
    pub texture: u16,
    pub interval: u8,
    pub frames: u8,
    pub speed: [f32; 2],
    /// Degrees added to each axis on an oscillation update.
    pub angular_speed: [f32; 2],
    pub initial_tick: u8,
    pub initial_frame: u8,
    pub initial_offset: [f32; 2],
    pub initial_angle: [f32; 2],
}

impl ArenaUvChannel {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.speed
                .iter()
                .chain(&self.angular_speed)
                .chain(&self.initial_offset)
                .chain(&self.initial_angle)
                .all(|v| v.is_finite())
                && (self.mode != ArenaUvMode::Frames || self.frames != 0),
            "invalid arena UV channel"
        );
        Ok(())
    }

    /// Sample the number of unfrozen arena updates, with identity at tick zero.
    pub fn offset(&self, tick: u64) -> [f32; 2] {
        // The byte counter increments before comparison; even 255 wraps first.
        // An interval of zero updates every tick, as does an interval of one.
        let first = 1 + u64::from(
            self.interval
                .saturating_sub(self.initial_tick.wrapping_add(1)),
        );
        if tick < first {
            return [0.; 2];
        }
        let steps = 1 + (tick - first) / u64::from(self.interval.max(1));
        match self.mode {
            ArenaUvMode::Disabled => [0.; 2],
            ArenaUvMode::Frames => {
                let frame = self.initial_frame.wrapping_add(1);
                let first = if frame >= self.frames { 0 } else { frame };
                [
                    0.,
                    ((u64::from(first) + (steps - 1) % u64::from(self.frames))
                        % u64::from(self.frames)) as f32
                        / f32::from(self.frames),
                ]
            }
            ArenaUvMode::Scroll => std::array::from_fn(|axis| {
                (f64::from(self.initial_offset[axis]) + f64::from(self.speed[axis]) * steps as f64)
                    as f32
            }),
            ArenaUvMode::Oscillate => {
                let angle: [f64; 2] = std::array::from_fn(|axis| {
                    (f64::from(self.initial_angle[axis])
                        + f64::from(self.angular_speed[axis]) * steps as f64)
                        .rem_euclid(360.)
                        .to_radians()
                });
                [
                    self.speed[0] * angle[0].cos() as f32,
                    self.speed[1] * angle[1].sin() as f32,
                ]
            }
        }
    }
}

impl VisualAssets {
    pub fn weapon(&self, binding: WeaponModel) -> Option<&WeaponVisuals> {
        match binding {
            WeaponModel::Equipment(item) => self.weapons.get(&item),
            WeaponModel::PowBlade => self.pow_weapons.get(&super::unison::PowWeapon::Blade),
            WeaponModel::PowDevastation(item) => self.pow_devastation.get(&item),
            WeaponModel::PowSpear => self.pow_weapons.get(&super::unison::PowWeapon::Spear),
        }
    }

    pub fn weapon_models(&self) -> impl Iterator<Item = (WeaponModel, &WeaponVisuals)> {
        self.weapons
            .iter()
            .map(|(&item, visual)| (WeaponModel::Equipment(item), visual))
            .chain(
                self.pow_weapons
                    .iter()
                    .filter(|(kind, _)| **kind != super::unison::PowWeapon::Devastation)
                    .map(|(kind, visual)| (kind.model(0), visual)),
            )
            .chain(
                self.pow_devastation
                    .iter()
                    .map(|(&item, visual)| (WeaponModel::PowDevastation(item), visual)),
            )
    }
    pub fn trail_styles(&self) -> impl Iterator<Item = &TrailStyle> {
        self.weapon_models()
            .map(|(_, weapon)| weapon)
            .flat_map(|w| w.trails.values().map(|t| &t.style))
            .chain(
                self.party
                    .values()
                    .flat_map(PartyVisuals::concrete)
                    .map(|(_, model)| &model.visual)
                    .chain(self.enemies.values())
                    .flat_map(|v| {
                        v.trails.values().map(|t| &t.style).chain(
                            v.attachments
                                .values()
                                .filter_map(|a| a.trail_style.as_ref()),
                        )
                    }),
            )
    }

    pub fn trail_textures(&self) -> impl Iterator<Item = &crate::font::UiTexture> {
        self.trail_styles()
            .flat_map(|s| s.textures.values().filter_map(Option::as_ref))
    }

    pub fn scenes(&self) -> impl Iterator<Item = &crate::ScenePart> {
        self.arenas
            .values()
            .map(|a| &a.model)
            .chain(
                self.party
                    .values()
                    .flat_map(PartyVisuals::concrete)
                    .map(|(_, model)| &model.visual)
                    .chain(self.enemies.values())
                    .map(|v| &v.model),
            )
            .chain(self.weapon_models().flat_map(|(_, w)| w.slots.values()))
            .chain(self.effect_models.iter().map(|v| &v.model))
            .flat_map(|m| m.parts.iter().map(|p| &p.scene))
    }

    fn validate_linked_weapons(&self, character: u8, model: &PartyModel) -> Result<()> {
        let expected = self.weapons.iter().filter_map(|(&item, weapon)| {
            weapon
                .motion_link
                .as_ref()
                .filter(|link| link.owner == character)
                .map(|_| item)
        });
        ensure!(
            model.weapon_motions.keys().copied().eq(expected),
            "selected party weapon motion inventory mismatch"
        );
        for (&item, bank) in &model.weapon_motions {
            let weapon = &self.weapons[&item];
            let base = weapon
                .motion_link
                .as_ref()
                .context("weapon has no motion link")?;
            ensure!(
                bank.link.owner == character
                    && bank.link.source_sha256 == model.visual.animation_sha256
                    && bank.link.offset == base.offset
                    && bank.link.fallback == base.fallback
                    && bank.link.time_scale == base.time_scale
                    && bank.link.time_scale.is_finite()
                    && bank.link.time_scale > 0.
                    && bank.rigs.keys().eq(weapon.rigs.keys())
                    && bank.rigs.keys().all(|slot| model
                        .visual
                        .rig
                        .weapon_bones
                        .contains_key(slot)),
                "selected weapon CAB, link or carried slot mismatch for item {item}"
            );
            for (&slot, rig) in &bank.rigs {
                rig.validate()?;
                let shared = &weapon.rigs[&slot];
                ensure!(
                    rig.skeleton.bones.len() == shared.skeleton.bones.len()
                        && rig
                            .skeleton
                            .bones
                            .iter()
                            .zip(&shared.skeleton.bones)
                            .all(|(a, b)| a.name == b.name
                                && a.parent == b.parent
                                && a.bind == b.bind)
                        && rig.attack_groups == shared.attack_groups
                        && rig.effect_groups == shared.effect_groups
                        && rig.weapon_bones == shared.weapon_bones
                        && rig.motions.contains_key(&bank.link.fallback),
                    "selected weapon rig does not match shared geometry for item {item}, slot {slot}"
                );
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(&self.toon_ramp)?;
        crate::validate_asset_path(&self.shadow_texture)?;
        for arena in self.arenas.values() {
            arena.validate()?;
        }
        for (&character, party) in &self.party {
            party.validate(character)?;
            for (costume, model) in party.concrete() {
                self.validate_linked_weapons(character, model)
                    .with_context(|| format!("party weapon motions {character}:{costume:?}"))?;
            }
        }
        for (owner, visual) in self
            .party
            .iter()
            .flat_map(|(character, party)| {
                party.concrete().map(move |(costume, model)| {
                    (format!("party {character}:{costume:?}"), &model.visual)
                })
            })
            .chain(
                self.enemies
                    .iter()
                    .map(|(id, model)| (format!("enemy {id}"), model)),
            )
        {
            visual.model.validate()?;
            validate_digest(&visual.model_sha256)?;
            validate_digest(&visual.animation_sha256)?;
            visual.rig.validate()?;
            if let Some(table) = &visual.authored_motions {
                table
                    .validate(
                        &visual.rig,
                        visual.victory.values().map(|motion| motion.clip),
                    )
                    .with_context(|| format!("{owner} motion table"))?;
            }
            let bones = &visual.rig.skeleton.bones;
            let names = &visual.model.parts[0].scene.bone_names;
            if let Some(index) = (0..bones.len().max(names.len()))
                .find(|&i| bones.get(i).map(|bone| &bone.name) != names.get(i))
            {
                anyhow::bail!(
                    "battle rig/model bone order mismatch for {owner} at index {index}: rig {:?} ({} bones), model {:?} ({} bones)",
                    bones.get(index).map(|bone| &bone.name),
                    bones.len(),
                    names.get(index),
                    names.len()
                );
            }
            ensure!(
                visual.target_anchor.offset.iter().all(|v| v.is_finite())
                    && visual
                        .target_anchor
                        .bone
                        .is_none_or(|bone| usize::from(bone) < visual.rig.skeleton.bones.len()),
                "invalid battle target anchor"
            );
            for trail in visual.trails.values() {
                trail.validate(&visual.rig)?;
            }
            ensure!(
                !visual.bounds_joints.is_empty()
                    && visual
                        .bounds_joints
                        .iter()
                        .all(|&i| usize::from(i) < visual.rig.skeleton.bones.len())
                    && visual.texture_layers.len() <= 4
                    && visual
                        .texture_layers
                        .iter()
                        .chain(visual.variant_texture.iter())
                        .all(|layer| layer.frames > 0
                            && usize::from(layer.texture)
                                < visual.model.parts[0].scene.textures.len()),
                "invalid battle bounds or texture layers"
            );
            if let Some(shadow) = &visual.shadow {
                ensure!(
                    shadow.scale.is_finite() && shadow.scale > 0.,
                    "invalid battle shadow bounds"
                );
            }
            ensure!(
                visual.volumes.iter().all(|v| v.radius.is_finite()
                    && v.radius >= 0.
                    && visual.rig.skeleton.bone(&v.bone).is_some()),
                "invalid battle body volume"
            );
            ensure!(
                !visual.model.parts[0].scene.clips.is_empty(),
                "battle actor has no motions"
            );
            for part in &visual.model.parts {
                let mut slots = std::collections::BTreeSet::new();
                ensure!(
                    part.scene
                        .clips
                        .iter()
                        .all(|clip| slots.insert(clip.resource_slot)
                            && clip.duration_seconds.is_finite()
                            && clip.duration_seconds >= 0.),
                    "invalid or duplicate battle motion"
                );
            }
            ensure!(
                visual
                    .rig
                    .motions
                    .get(&visual.initial_pose.clip)
                    .is_some_and(|motion| motion.duration_frames >= 1.
                        && motion.duration_frames <= f32::from(i16::MAX)),
                "invalid initial battle motion period"
            );
            let mut attached_parts = std::collections::BTreeSet::new();
            if let Some(paired) = &visual.paired_body {
                paired.rig.validate()?;
                let anchor = paired
                    .bone
                    .map(|bone| {
                        visual
                            .rig
                            .skeleton
                            .bones
                            .get(usize::from(bone))
                            .map(|bone| bone.name.as_str())
                            .context("invalid paired body anchor")
                    })
                    .transpose()?;
                ensure!(
                    paired.rig.motions.contains_key(&paired.initial_clip)
                        && !paired.parts.is_empty()
                        && paired.parts.iter().all(|&index| index != 0
                            && attached_parts.insert(index)
                            && visual
                                .model
                                .parts
                                .get(usize::from(index))
                                .is_some_and(|part| part.attached_to.as_deref() == anchor))
                        && paired.rig.skeleton.bones.len()
                            == visual.model.parts[usize::from(paired.parts[0])]
                                .scene
                                .bone_names
                                .len(),
                    "invalid paired battle body"
                );
            }
            for (&slot, attachment) in &visual.attachments {
                attachment.rig.validate()?;
                if let Some(style) = &attachment.trail_style {
                    style.validate()?;
                }
                let bone = visual.rig.skeleton.bones.get(usize::from(attachment.bone));
                ensure!(
                    slot < 8
                        && visual.rig.weapon_bones.get(&slot) == Some(&attachment.bone)
                        && bone.is_some()
                        && !attachment.parts.is_empty()
                        && attachment.parts.iter().all(|&index| {
                            attached_parts.insert(index)
                                && visual
                                    .model
                                    .parts
                                    .get(usize::from(index))
                                    .is_some_and(|part| {
                                        part.attached_to.as_deref() == bone.map(|b| b.name.as_str())
                                    })
                        })
                        && attachment.trail_bones.len() <= 3
                        && attachment.trail_style.is_some() == (attachment.trail_bones.len() >= 2)
                        && attachment.trail_bones.iter().all(|&index| {
                            usize::from(index) < attachment.rig.skeleton.bones.len()
                        }),
                    "invalid battle attachment"
                );
            }
        }
        for (_, weapon) in self.weapon_models() {
            validate_digest(&weapon.source_sha256)?;
            ensure!(!weapon.slots.is_empty(), "battle weapon has no models");
            for model in weapon.slots.values() {
                model.validate()?;
            }
            ensure!(
                weapon.slots.keys().eq(weapon.rigs.keys()),
                "battle weapon rig inventory mismatch"
            );
            for rig in weapon.rigs.values() {
                rig.validate()?;
            }
            for (slot, trail) in &weapon.trails {
                let rig = weapon
                    .rigs
                    .get(slot)
                    .ok_or_else(|| anyhow::anyhow!("trail has no weapon rig"))?;
                trail.validate(rig)?;
            }
            if let Some(link) = &weapon.motion_link {
                validate_digest(&link.source_sha256)?;
                ensure!(
                    (1..=9).contains(&link.owner)
                        && link.time_scale.is_finite()
                        && link.time_scale > 0.
                        && weapon
                            .rigs
                            .values()
                            .all(|rig| rig.motions.contains_key(&link.fallback)),
                    "invalid battle weapon motion link"
                );
            }
        }
        let mut bindings = std::collections::BTreeSet::new();
        for effect in &self.effect_models {
            ensure!(
                bindings.insert(effect.binding),
                "duplicate battle effect model"
            );
            effect.model.validate()?;
            effect.validate_pose_joints()?;
            if let Some(rig) = &effect.rig {
                rig.validate()?;
                ensure!(
                    rig.skeleton
                        .bones
                        .iter()
                        .map(|b| &b.name)
                        .eq(&effect.model.parts[0].scene.bone_names),
                    "effect rig/model bone order mismatch"
                );
            }
        }
        Ok(())
    }
}

fn validate_digest(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid battle visual digest"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_uv_preserves_initial_phase_byte_wrapping_and_continuous_motion() {
        let mut channel = ArenaUvChannel {
            mode: ArenaUvMode::Frames,
            interval: 3,
            frames: 4,
            initial_tick: 1,
            initial_frame: 2,
            ..Default::default()
        };
        channel.validate().unwrap();
        for (tick, expected) in [(0, 0.), (1, 0.), (2, 0.75), (4, 0.75), (5, 0.), (8, 0.25)] {
            assert_eq!(channel.offset(tick), [0., expected]);
        }
        channel.initial_tick = 255;
        channel.initial_frame = 0;
        assert_eq!(channel.offset(3), [0.; 2]);
        assert_eq!(channel.offset(4), [0., 0.25]);
        channel.interval = 0;
        channel.initial_frame = 255;
        assert_eq!(channel.offset(1), [0.; 2]);
        assert_eq!(channel.offset(2), [0., 0.25]);

        channel.mode = ArenaUvMode::Scroll;
        channel.initial_offset = [2., -3.];
        channel.speed = [0.25, -0.5];
        assert_eq!(channel.offset(0), [0.; 2]);
        assert_eq!(channel.offset(1), [2.25, -3.5]);
        assert_eq!(channel.offset(100), [27., -53.]);

        channel.mode = ArenaUvMode::Oscillate;
        channel.interval = 2;
        channel.initial_tick = 0;
        channel.speed = [2., 3.];
        channel.angular_speed = [90.; 2];
        channel.initial_angle = [270.; 2];
        for (tick, expected) in [
            (0, [0., 0.]),
            (1, [0., 0.]),
            (2, [2., 0.]),
            (4, [0., 3.]),
            (6, [-2., 0.]),
            (10, [2., 0.]),
        ] {
            for (actual, expected) in channel.offset(tick).into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.00001);
            }
        }
        channel.mode = ArenaUvMode::Disabled;
        assert_eq!(channel.offset(100), [0.; 2]);
        channel.angular_speed[0] = f32::NAN;
        assert!(channel.validate().is_err());
        channel.angular_speed[0] = 0.;
        channel.mode = ArenaUvMode::Frames;
        channel.frames = 0;
        assert!(channel.validate().is_err());
    }

    #[test]
    fn body_atlas_rows_keep_source_descriptor_priority_and_part_scope() {
        // Enemy100: animated eye texture1 has8 rows; appearance texture2 has4.
        let animated = [TextureLayer {
            texture: 1,
            frames: 8,
        }];
        let appearance = TextureLayer {
            texture: 2,
            frames: 4,
        };
        let variant = Some((&appearance, 3));
        assert_eq!(
            texture_offset(true, &animated, &[2, 0, 0, 0], variant, 1),
            0.25
        );
        assert_eq!(
            texture_offset(true, &animated, &[2, 0, 0, 0], variant, 2),
            0.75
        );
        assert_eq!(
            texture_offset(false, &animated, &[2, 0, 0, 0], variant, 2),
            0.
        );
        // The draw consumer stops at its first matching descriptor, even on aliases.
        let alias = TextureLayer {
            texture: 1,
            frames: 4,
        };
        assert_eq!(
            texture_offset(true, &animated, &[2, 0, 0, 0], Some((&alias, 3)), 1),
            0.25
        );
    }

    fn party_model(body: &str, cab: &str, namespace: &str) -> PartyModel {
        let bones = vec![super::super::pose::Bone {
            bind_channels: Default::default(),
            name: "Bone_atama".into(),
            parent: None,
            bind: Default::default(),
        }];
        PartyModel {
            head_bone: 0,
            weapon_motions: BTreeMap::new(),
            visual: ModelVisuals {
                model_sha256: body.repeat(64),
                animation_sha256: cab.repeat(64),
                authored_motions: None,
                model: ModelPreview {
                    scale: 1.,
                    elevation: 0.,
                    hidden_geometry: vec![],
                    node_scales: vec![],
                    parts: vec![crate::model_preview::PreviewPart {
                        animation: None,
                        scene: crate::ScenePart {
                            resource: 0,
                            mesh: format!("{namespace}/body.glb"),
                            textures: vec![format!("{namespace}/body.ktx2")],
                            materials: vec![],
                            appearance: None,
                            translation: [0.; 3],
                            clips: vec![crate::SceneClip {
                                resource_slot: 0,
                                duration_seconds: 1.,
                                animation_resource: None,
                                secondary_pose_nodes: vec![],
                            }],
                            autoplay: false,
                            texture_animations: vec![],
                            bone_names: vec!["Bone_atama".into()],
                            material_nodes: vec![],
                            outline_color: None,
                            secondary_motion: Default::default(),
                        },
                        attached_to: None,
                        additive: false,
                        uv_offsets: vec![],
                    }],
                },
                rig: Rig {
                    skeleton: super::super::pose::Skeleton { bones },
                    motions: [(
                        0,
                        super::super::pose::Motion {
                            duration_frames: 30.,
                            tracks: vec![],
                        },
                    )]
                    .into(),
                    attack_groups: [(0, vec![0])].into(),
                    effect_groups: BTreeMap::new(),
                    weapon_bones: BTreeMap::new(),
                },
                volumes: vec![],
                alpha: 255,
                shadow: None,
                bounds_joints: vec![0],
                target_anchor: Default::default(),
                initial_pose: Default::default(),
                paired_body: None,
                texture_layers: vec![],
                variant_texture: None,
                victory: BTreeMap::new(),
                attachments: BTreeMap::new(),
                trails: BTreeMap::new(),
            },
        }
    }

    fn assets(party: PartyVisuals) -> VisualAssets {
        VisualAssets {
            arenas: BTreeMap::new(),
            party: [(9, party)].into(),
            enemies: BTreeMap::new(),
            weapons: BTreeMap::new(),
            pow_weapons: BTreeMap::new(),
            pow_devastation: BTreeMap::new(),
            toon_ramp: "toon.ktx2".into(),
            shadow_texture: "shadow.ktx2".into(),
            effect_models: vec![],
        }
    }

    #[test]
    fn authored_motion_partition_rejects_gaps_conflicts_and_unrelocated_external_slots() {
        let mut model = party_model("a", "b", "body");
        let table = AuthoredMotionTable {
            count: 2,
            nulls: [1].into(),
        };
        table.validate(&model.visual.rig, []).unwrap();
        model.visual.authored_motions = Some(table.clone());
        assert!(matches!(
            model.visual.motion_slot(0).unwrap(),
            MotionSlot::Present(_)
        ));
        assert!(matches!(
            model.visual.motion_slot(1).unwrap(),
            MotionSlot::AuthoredNull
        ));
        assert!(model.visual.motion_slot(2).is_err());
        for invalid in [
            AuthoredMotionTable {
                count: 0,
                nulls: [].into(),
            },
            AuthoredMotionTable {
                count: 2,
                nulls: [].into(),
            },
            AuthoredMotionTable {
                count: 2,
                nulls: [0, 1].into(),
            },
            AuthoredMotionTable {
                count: 2,
                nulls: [1, 2].into(),
            },
        ] {
            assert!(invalid.validate(&model.visual.rig, []).is_err());
        }
        model
            .visual
            .rig
            .motions
            .insert(256, model.visual.rig.motions[&0].clone());
        assert!(table.validate(&model.visual.rig, []).is_err());
        table.validate(&model.visual.rig, [256]).unwrap();
        assert!(matches!(
            model.visual.motion_slot(256).unwrap(),
            MotionSlot::Present(_)
        ));
        model
            .visual
            .rig
            .motions
            .insert(100, model.visual.rig.motions[&0].clone());
        assert!(model.visual.motion_slot(100).is_err());
        model.visual.authored_motions = None;
        assert!(model.visual.motion_slot(1).is_err());
    }

    #[test]
    fn rig_model_mismatches_identify_owner_index_and_both_lengths() {
        for enemy in [false, true] {
            let model = party_model("a", "b", "body");
            let mut valid = assets(PartyVisuals {
                bindings: [
                    (
                        Costume::Standard,
                        PartyVariant::Model(Box::new(model.clone())),
                    ),
                    (Costume::Variant1, PartyVariant::Alias(Costume::Standard)),
                ]
                .into(),
            });
            let owner = if enemy {
                valid.party.clear();
                valid.enemies.insert(182, model.visual);
                "enemy 182"
            } else {
                "party 9:Standard"
            };
            valid.validate().unwrap();
            for (names, index) in [
                (vec!["wrong bone"], 0),
                (vec![], 0),
                (vec!["Bone_atama", "extra bone"], 1),
            ] {
                let mut invalid = valid.clone();
                let visual = if enemy {
                    invalid.enemies.get_mut(&182).unwrap()
                } else {
                    let PartyVariant::Model(model) = invalid
                        .party
                        .get_mut(&9)
                        .unwrap()
                        .bindings
                        .get_mut(&Costume::Standard)
                        .unwrap()
                    else {
                        unreachable!()
                    };
                    &mut model.visual
                };
                visual.model.parts[0].scene.bone_names =
                    names.iter().map(|s| (*s).into()).collect();
                let error = invalid.validate().unwrap_err().to_string();
                assert!(
                    error.contains(&format!("{owner} at index {index}")),
                    "{error}"
                );
                assert!(error.contains("(1 bones)"), "{error}");
                assert!(
                    error.contains(&format!("({} bones)", names.len())),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn costume_alias_resolution_preserves_canonical_identity_and_rejects_missing_selection() {
        use Costume::*;
        let party = PartyVisuals {
            bindings: [
                (
                    Standard,
                    PartyVariant::Model(Box::new(party_model("a", "b", "standard"))),
                ),
                (Variant1, PartyVariant::Alias(Standard)),
            ]
            .into(),
        };
        let (canonical, model) = party.resolve(Variant1).unwrap();
        assert_eq!(canonical, Standard);
        assert!(std::ptr::eq(model, party.standard().unwrap()));
        assert_eq!(party.concrete().count(), 1);
        assert!(
            party
                .resolve(Variant4)
                .unwrap_err()
                .to_string()
                .contains("Variant4")
        );
        let decoded: PartyVisuals =
            serde_json::from_slice(&serde_json::to_vec(&party).unwrap()).unwrap();
        assert_eq!(decoded.resolve(Variant1).unwrap().0, Standard);
        assets(party).validate().unwrap();
    }

    #[test]
    fn costume_aliases_reject_missing_targets_chains_self_links_and_cycles() {
        use Costume::*;
        for bindings in [
            [
                (Standard, PartyVariant::Alias(Variant1)),
                (Variant1, PartyVariant::Alias(Variant2)),
            ],
            [
                (Standard, PartyVariant::Alias(Standard)),
                (Variant1, PartyVariant::Alias(Standard)),
            ],
            [
                (Standard, PartyVariant::Alias(Variant1)),
                (Variant1, PartyVariant::Alias(Standard)),
            ],
            [
                (
                    Standard,
                    PartyVariant::Model(Box::new(party_model("a", "b", "body"))),
                ),
                (Variant1, PartyVariant::Alias(Variant2)),
            ],
        ] {
            assert!(
                PartyVisuals {
                    bindings: bindings.into()
                }
                .validate_bindings()
                .is_err()
            );
        }
        let party = PartyVisuals {
            bindings: [
                (
                    Standard,
                    PartyVariant::Model(Box::new(party_model("a", "b", "body"))),
                ),
                (Variant1, PartyVariant::Alias(Standard)),
                (Variant2, PartyVariant::Alias(Variant1)),
            ]
            .into(),
        };
        assert!(party.resolve(Variant2).is_err());
    }

    #[test]
    fn full_pair_dedup_keeps_same_body_different_cab_and_both_material_dependencies() {
        use Costume::*;
        let first = party_model("a", "b", "ordinary");
        let story = party_model("a", "c", "story");
        let party = PartyVisuals {
            bindings: [
                (Standard, PartyVariant::Model(Box::new(first.clone()))),
                (Variant1, PartyVariant::Alias(Standard)),
                (Story, PartyVariant::Model(Box::new(story))),
            ]
            .into(),
        };
        party.validate_bindings().unwrap();
        assert_eq!(party.resolve(Story).unwrap().0, Story);
        assert_eq!(party.concrete().count(), 2);
        let visuals = assets(party);
        assert_eq!(
            visuals
                .scenes()
                .map(|scene| scene.textures[0].as_str())
                .collect::<Vec<_>>(),
            ["ordinary/body.ktx2", "story/body.ktx2"]
        );
        visuals.validate().unwrap();
        let duplicate = PartyVisuals {
            bindings: [
                (Standard, PartyVariant::Model(Box::new(first.clone()))),
                (Variant1, PartyVariant::Model(Box::new(first))),
            ]
            .into(),
        };
        assert!(
            duplicate
                .validate_bindings()
                .unwrap_err()
                .to_string()
                .contains("body/CAB")
        );
    }

    #[test]
    fn reachable_costume_masks_require_all_36_title_and_story_selections() {
        use Costume::*;
        assert_eq!(
            (1..=9)
                .map(|owner| required_costumes(owner).unwrap().len())
                .sum::<usize>(),
            36
        );
        assert_eq!(required_costumes(9).unwrap(), [Standard, Variant1]);
        assert!(required_costumes(0).is_err());
        assert!(required_costumes(10).is_err());
        for owner in 1..=9 {
            let mut party = PartyVisuals {
                bindings: required_costumes(owner)
                    .unwrap()
                    .iter()
                    .map(|&costume| {
                        (
                            costume,
                            if costume == Standard {
                                PartyVariant::Model(Box::new(party_model("a", "b", "body")))
                            } else {
                                PartyVariant::Alias(Standard)
                            },
                        )
                    })
                    .collect(),
            };
            party.validate(owner).unwrap();
            for &costume in required_costumes(owner).unwrap() {
                let removed = party.bindings.remove(&costume).unwrap();
                assert!(party.validate(owner).is_err(), "{owner}:{costume:?}");
                party.bindings.insert(costume, removed);
            }
        }
    }

    #[test]
    fn selected_weapon_bank_requires_matching_cab_and_exact_shared_skeleton() {
        let mut model = party_model("a", "b", "body");
        model.visual.rig.weapon_bones.insert(0, 0);
        let rig = model.visual.rig.clone();
        let link = WeaponMotionLink {
            source_sha256: "b".repeat(64),
            owner: 3,
            offset: 60,
            fallback: 0,
            time_scale: 0.5,
        };
        model.weapon_motions.insert(
            17,
            LinkedWeaponMotions {
                rigs: [(0, rig.clone())].into(),
                link: link.clone(),
            },
        );
        let mut visuals = assets(PartyVisuals {
            bindings: BTreeMap::new(),
        });
        visuals.weapons.insert(
            17,
            WeaponVisuals {
                source_sha256: "c".repeat(64),
                slots: [(0, model.visual.model.clone())].into(),
                rigs: [(0, rig)].into(),
                trails: BTreeMap::new(),
                motion_link: Some(link),
            },
        );
        visuals.validate_linked_weapons(3, &model).unwrap();
        let mut changed = model.clone();
        changed
            .weapon_motions
            .get_mut(&17)
            .unwrap()
            .link
            .source_sha256 = "d".repeat(64);
        assert!(visuals.validate_linked_weapons(3, &changed).is_err());
        let mut changed = model.clone();
        changed
            .weapon_motions
            .get_mut(&17)
            .unwrap()
            .rigs
            .get_mut(&0)
            .unwrap()
            .skeleton
            .bones[0]
            .name = "wrong bone".into();
        assert!(visuals.validate_linked_weapons(3, &changed).is_err());
        let mut changed = model.clone();
        changed
            .weapon_motions
            .get_mut(&17)
            .unwrap()
            .rigs
            .get_mut(&0)
            .unwrap()
            .skeleton
            .bones[0]
            .bind
            .translation[0] = 1.;
        assert!(visuals.validate_linked_weapons(3, &changed).is_err());
        let mut changed = model.clone();
        changed.weapon_motions.clear();
        assert!(visuals.validate_linked_weapons(3, &changed).is_err());
    }

    #[test]
    fn action_admission_uses_the_selected_cab_period() {
        use super::super::actions::AnimationCommand;
        let command = AnimationCommand::Play {
            clip: 0,
            blend: 0,
            start: 0,
            end: Some(20),
            layer: 8,
            looping: false,
            mirror: false,
            resource: -1,
            rate: 1.,
        };
        let standard = party_model("a", "b", "body");
        let mut changed = party_model("a", "c", "story");
        changed
            .visual
            .rig
            .motions
            .get_mut(&0)
            .unwrap()
            .duration_frames = 10.;
        standard.validate_animation(command).unwrap();
        assert!(changed.validate_animation(command).is_err());
        changed.visual.rig.motions.clear();
        assert!(changed.validate_animation(command).is_err());
    }

    fn linked_playback_fixture() -> (WeaponMotionLink, Rig) {
        let mut rig = party_model("a", "b", "weapon").visual.rig;
        rig.motions = [
            (
                60,
                super::super::pose::Motion {
                    duration_frames: 2.,
                    tracks: vec![],
                },
            ),
            (
                90,
                super::super::pose::Motion {
                    duration_frames: 50.,
                    tracks: vec![],
                },
            ),
        ]
        .into();
        (
            WeaponMotionLink {
                source_sha256: "b".repeat(64),
                owner: 3,
                offset: 60,
                fallback: 60,
                time_scale: 0.5,
            },
            rig,
        )
    }

    #[test]
    fn linked_playback_preserves_unblended_long_start_on_native_fallback() {
        let (link, rig) = linked_playback_fixture();
        // Genis's native result clip23 has no carried clip83. Its later
        // unblended loop starts at224, even though fallback60 has only2 frames.
        for blend in [0, 1] {
            assert_eq!(
                link.playback(&rig, 23, 224, None, blend).unwrap(),
                LinkedPlayback {
                    clip: 60,
                    start: 112.,
                    end: 2.
                }
            );
        }
        // An available linked clip retains the carried fixed half-frame scale.
        assert_eq!(
            link.playback(&rig, 30, 10, Some(20), 4).unwrap(),
            LinkedPlayback {
                clip: 90,
                start: 5.,
                end: 10.
            }
        );
    }

    #[test]
    fn linked_playback_rejects_unsupported_blends_ends_and_missing_resources() {
        let (link, mut rig) = linked_playback_fixture();
        assert!(link.playback(&rig, 23, 224, None, 2).is_err());
        assert_eq!(
            link.playback(&rig, 23, 4, None, 2).unwrap(),
            LinkedPlayback {
                clip: 60,
                start: 2.,
                end: 2.
            }
        );
        assert!(link.playback(&rig, 30, 20, Some(10), 2).is_err());
        assert!(link.playback(&rig, 23, 0, Some(6), 0).is_err());
        for time_scale in [0., -0.5, f32::NAN, f32::INFINITY, f32::MAX] {
            let invalid = WeaponMotionLink {
                time_scale,
                ..link.clone()
            };
            assert!(invalid.playback(&rig, 23, 224, None, 0).is_err());
        }
        for duration in [0., -2., f32::NAN, f32::INFINITY] {
            rig.motions.get_mut(&60).unwrap().duration_frames = duration;
            assert!(link.playback(&rig, 23, 0, None, 0).is_err());
        }
        rig.motions.remove(&60);
        assert!(link.playback(&rig, 23, 0, None, 0).is_err());
        // Fallback must exist even if this particular body command has a link.
        assert!(link.playback(&rig, 30, 0, None, 0).is_err());
    }

    #[test]
    fn linked_fallback_admission_keeps_body_interval_guard_strict() {
        use super::super::actions::AnimationCommand;
        let (link, rig) = linked_playback_fixture();
        let mut model = party_model("a", "b", "body");
        model
            .visual
            .rig
            .motions
            .get_mut(&0)
            .unwrap()
            .duration_frames = 120.;
        model.weapon_motions.insert(
            17,
            LinkedWeaponMotions {
                rigs: [(0, rig)].into(),
                link,
            },
        );
        let command = AnimationCommand::Play {
            clip: 0,
            blend: 0,
            start: 224,
            end: None,
            layer: 8,
            looping: true,
            mirror: false,
            resource: -1,
            rate: 0.5,
        };
        model.validate_animation(command).unwrap();
        model
            .visual
            .rig
            .motions
            .get_mut(&0)
            .unwrap()
            .duration_frames = 30.;
        assert!(model.validate_animation(command).is_err());
    }
}

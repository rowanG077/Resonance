//! Actor templates. Session statistics and equipment are applied on loading.
use crate::{battle_conditions::ConditionSet, battle_voice::Sound};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PARTY_PATH: &str = "battle/party-profiles.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub source_sha256: String,
    /// Default strategy values, transposed from three ten-byte asset columns.
    pub default_strategy: [[u8; 3]; 10],
    pub companion_policy: CompanionPolicy,
    pub entry: Entry,
    /// Character slots used to select chanting and release lines.
    pub voice_sequences: Vec<VoicePolicy>,
    /// Ordered victim/recipient character pairs used by the ordinary death entry.
    pub death_voice_pairs: Vec<[u8; 2]>,
    pub contact_sounds: ContactSounds,
    /// Over Limit sounds for characters 1..=9.
    pub overlimit_voices: [Option<Sound>; 9],
    /// Character IDs 1..=11.
    pub records: Vec<Profile>,
    /// Rescue names: Angel Tear, Revive, Resurrect, Ring, Doll.
    pub lethal_rescue_names: [String; 5],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanionPolicy {
    pub tp_limits: [u8; 9],
    pub healing_limits: [u8; 9],
    pub support_level_limits: [i8; 9],
}

/// Artwork settings for battle entry; gameplay owns the introduction duration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub replacement_motion_rate: f32,
    /// RGB used by result fades.
    pub fade_color: [u8; 3],
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicePolicy {
    pub default: VoiceSequence,
    pub techniques: BTreeMap<u16, VoiceSequence>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSequence {
    pub chant: Option<Sound>,
    pub self_chant: Option<Sound>,
    pub release: Option<Sound>,
    pub fallback: Option<Sound>,
}

/// Neutral party and elemental contact cue tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContactSounds {
    pub party: [u16; 9],
    pub elements: [u8; 9],
}

/// Facial atlas channel; V advances by expression index / frame count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureChannel {
    pub texture: u8,
    pub frames: u8,
}

/// Half-closed and closed eye artwork, with expressions that suppress blinking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blink {
    pub channel: u8,
    pub frames: [u8; 2],
    pub excluded_expressions: Vec<u8>,
}
impl Blink {
    pub fn validate(&self, channels: &[TextureChannel]) -> Result<()> {
        let channel = channels
            .get(usize::from(self.channel))
            .filter(|_| self.channel < 4)
            .context("blink channel is not prepared")?;
        ensure!(
            self.frames.iter().all(|&frame| frame < channel.frames),
            "blink frame is outside its expression atlas"
        );
        Ok(())
    }
}

/// Actor capabilities resolved during asset import. False leaves ordinary behavior enabled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileTraits {
    pub flying: bool,
    pub hover_bobbing: bool,
    pub hide_enemy_shadow: bool,
    pub turning_disabled: bool,
    pub inactive_item_target: bool,
    pub knockdown_immune: bool,
    pub clear_pending_on_hit: bool,
    pub show_body_attachments: bool,
    pub auto_guard_disabled: bool,
    pub retain_defeated_body: bool,
    pub recover_in_air: bool,
    pub launch_immune: bool,
    pub body_motion_disabled: bool,
    pub push_immovable: bool,
    pub unrestricted_arena: bool,
    pub enemy_contact_recovery: bool,
    pub passes_allied_obstacles: bool,
    pub passable_for_allies: bool,
    pub suppress_hurt_motion: bool,
    pub push_obstacle_disabled: bool,
    pub item_target_excluded: bool,
    pub secondary_body_entry: bool,
    pub fixed_height: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponStyle {
    pub before_body: bool,
    pub toon: bool,
    pub additive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub walk_speed: f32,
    pub run_speed: f32,
    pub turn_ticks: u8,
    /// Idle delay; entry uses only the random variation.
    pub idle_ticks: u8,
    pub idle_variation: u8,
    pub initial_motion: u8,
    pub initial_motion_override: u8,
    /// Initial primary body alpha, separate from material texture alpha.
    pub body_alpha: u8,
    pub texture_channels: Vec<TextureChannel>,
    pub blink: Option<Blink>,
    /// Resting facial expressions; undeclared channels are zero.
    pub idle_expression: [u8; 4],
    /// Facial expressions restored after lethal rescue.
    pub rescue_expression: [u8; 4],
    /// Styles for every attachment slot, including those enabled by equipment.
    pub weapon_styles: [WeaponStyle; 8],
    pub initial_conditions: ConditionSet,
    pub intrinsic_conditions: ConditionSet,
    pub immunities: ConditionSet,
    pub weight: u8,
    /// Actor species category used by weapon damage bonuses.
    pub species: u16,
    pub stun_resistance: u8,
    pub stagger_threshold: u8,
    pub stagger_ticks: u8,
    pub guard_reduction: u8,
    pub guard_pressure_limit: u32,
    pub traits: ProfileTraits,
    pub armor: u8,
    pub center_offset: [f32; 3],
    pub model_scale: f32,
    pub shadow_scale: f32,
    pub shadow_color: [u8; 4],
    pub effect_scale: f32,
    pub camera_category: u8,
    /// Named speech; absence denotes an unvoiced actor.
    pub voices: Option<crate::battle_voice::Voices>,
    pub death_motion: u8,
    pub overlimit_gain: u8,
    pub initial_overlimit: i16,
    pub ground_offset: f32,
    pub cast_ticks: i16,
}

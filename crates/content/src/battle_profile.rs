//! Original actor templates. Session statistics and equipment are applied on loading.
use crate::source::{FloatOperand, Storage};
use serde::{Deserialize, Serialize};

pub const PARTY_PATH: &str = "battle/party-profiles.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub source_sha256: String,
    /// 1AB2C defaults, transposed from the original three ten-byte columns.
    pub default_strategy: [[u8; 3]; 10],
    pub companion_policy: CompanionPolicy,
    pub placement: Placement,
    pub entry: Entry,
    /// Genis's original four motion rows, including the terminal record.
    pub chant: Vec<crate::battle_action::Animation>,
    /// Original ten character slots used to select chanting and release lines.
    pub voice_sequences: Vec<Vec<VoiceSequence>>,
    /// Ordered victim/recipient character pairs used by the ordinary death entry.
    pub death_voice_pairs: Vec<[u8; 2]>,
    pub contact_sounds: ContactSounds,
    /// Original character IDs 1..=11, including the two additional source slots.
    pub records: Vec<Profile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanionPolicy {
    pub tp_limits: [u8; 9],
    pub healing_limits: [u8; 9],
    pub support_level_limits: [i8; 9],
}

/// Original ordinary-entry operands (40C8 / 10668 / 52AA8).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub bootstrap_eye: [f32; 3],
    pub bootstrap_focus: [f32; 3],
    pub initial_yaw: f32,
    pub radius: f32,
    pub focus_x: f32,
    pub focus_speed_scale: f32,
    pub replacement_motion_rate: f32,
    /// Ordinary 5C38/11880 fade RGB, stored separately from the captured field.
    pub fade_color: [u8; 3],
    /// Original frozen-screen triangle table and arithmetic operands (C020/BBA0).
    pub screen_break: ScreenBreak,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenBreak {
    pub points: Vec<[f32; 3]>,
    pub triangles: Vec<[u8; 3]>,
    pub viewport: [f32; 2],
    pub viewport_center: [f32; 2],
    pub center_weight: f32,
    pub center_expansion: f32,
    pub velocity_scale: f32,
    pub angular_base: f32,
    pub angular_variation: f32,
    pub radians_per_degree: f32,
    pub secondary_rotation_scale: f32,
    pub draw_depth: f32,
}

/// Ordinary party rows, selected by the member's strategy position.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Lateral offsets in the row containing the first formation member.
    pub leader_z: [f32; 4],
    pub front_x: f32,
    pub row_step: f32,
    pub member_x: f32,
    pub member_z: f32,
    pub other_row_center: f32,
    pub single_row_z: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSequence {
    pub technique: u16,
    pub chant: u16,
    pub release: u16,
}

/// Source 3B228 neutral party and resolved-element contact cue tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContactSounds {
    pub party: [u16; 9],
    pub elements: [u8; 9],
}

/// Original facial atlas channel; 5B498 translates V by amount / frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureChannel {
    pub texture: u8,
    pub frames: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub walk_speed: FloatOperand,
    pub run_speed: FloatOperand,
    pub turn_ticks: u8,
    /// Ordinary controller delay (35B18); entry uses only the random variation.
    pub idle_ticks: u8,
    pub idle_variation: u8,
    pub initial_motion: u8,
    pub initial_motion_override: u8,
    pub texture_channels: Vec<TextureChannel>,
    /// 2B18C restores the declared D7 facial channels; undeclared entries are zero.
    pub idle_expression: [u8; 4],
    /// Original weapon-slot draw flags, in attachment order (51914 / 155CC).
    pub weapon_draw_flags: Vec<u8>,
    pub condition_flags: [u32; 2],
    pub condition_immunity: [u32; 2],
    pub intrinsic_conditions: [u32; 2],
    pub weight: u8,
    pub stun_resistance: u8,
    pub stagger_threshold: u8,
    pub stagger_ticks: u8,
    pub guard_reduction: u8,
    pub guard_pressure_limit: i16,
    pub flags: u32,
    pub body_flags: u16,
    pub armor: u8,
    pub center_offset: [FloatOperand; 3],
    pub target_bone: u8,
    pub target_offset: [FloatOperand; 3],
    pub model_scale: FloatOperand,
    pub shadow_scale: FloatOperand,
    pub shadow_color: [u8; 4],
    pub effect_scale: FloatOperand,
    pub camera_yaw_offset: u8,
    pub camera_category: u8,
    pub camera_minimum_radius: i16,
    pub voice_base: u32,
    pub death_voice: u16,
    pub death_motion: u8,
    pub overlimit_gain: u8,
    pub ground_offset: FloatOperand,
    pub head_bone: u8,
    pub stun_offset: [FloatOperand; 3],
    pub casting: Casting,
    /// Fields without a current consumer remain source data, never generated code.
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Casting {
    pub base_ticks: i16,
    pub loop_start: u8,
    pub animation_rate: FloatOperand,
    pub command_index: i16,
    pub effect_interval: u8,
    pub motion_flags: u8,
    pub resume_start: u8,
    pub resume_blend: u8,
    pub resume_loop_start: u8,
    pub stored_recovery_clip: u8,
}

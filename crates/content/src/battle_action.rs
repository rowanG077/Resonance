//! Original action definitions, before runtime selection or command compilation.
use crate::source::{FloatOperand, Storage};
use serde::{Deserialize, Serialize};

pub const MARTIAL_PATH: &str = "battle/martial-actions.json";
pub const SPELL_PATH: &str = "battle/spell-actions.json";
pub const NORMAL_PATH: &str = "battle/normal-actions.json";

/// Enemy-local source pools, selected by the ordinary controller before an
/// authored sequence starts. Inactive declarations and storage remain intact.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyActions {
    pub rows: Vec<EnemyAction>,
    pub policy: EnemyPolicy,
    pub hit_rules: Vec<HitRule>,
    pub hits: Vec<Hit>,
    pub animations: Vec<Animation>,
    pub commands: Vec<Command>,
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyPolicy {
    pub native: u8,
    pub counter_chance: u8,
    pub counter_count: u8,
    pub ordinary_count: u8,
    pub back_row_count: u8,
    pub back_row_actions: [u8; 4],
    pub back_row_weights: [u8; 4],
    pub overlimit_first_action: u8,
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyAction {
    pub weight: i8,
    pub target_policy: u8,
    pub recovery_ticks: u8,
    pub recovery_clip: u8,
    pub recovery_rate: FloatOperand,
    pub requirements: u32,
    pub target_state: u16,
    pub duration: u16,
    pub range: [i16; 2],
    pub approach_range: i16,
    pub approach_minimum: i16,
    pub animation: u16,
    pub command: u16,
    pub hit: u16,
    pub combo_at: u16,
    pub followup_group: u8,
    pub effect: u8,
    pub guard_chance: u8,
    pub vulnerable: [u16; 2],
    pub movement_speed: FloatOperand,
    pub movement_rate: FloatOperand,
    pub movement_clip: u8,
    pub stagger_threshold: u8,
    pub followup_chance: u8,
    pub required_monster: u8,
    pub tp: u8,
    pub hit_recovery_clip: u8,
    pub recovery_command: u16,
    pub resource_decrement: u8,
    pub required_story_flag: u16,
    pub cast_voices: [u16; 2],
    pub technique: u16,
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalTable {
    pub source_sha256: String,
    pub weapon_flights: Vec<WeaponFlight>,
    /// Original character groups 1..=9, in source order.
    pub groups: Vec<NormalGroup>,
}

/// Detached equipped-weapon parameters selected by a negative hit emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponFlight {
    pub outbound_ticks: i16,
    pub speed: FloatOperand,
    pub return_speed: FloatOperand,
    pub direction_y: FloatOperand,
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalGroup {
    pub selectors: Vec<NormalSelector>,
    pub actions: Vec<NormalAction>,
    pub descriptors: Vec<NormalDescriptor>,
    pub hit_rules: Vec<HitRule>,
    pub hits: Vec<Hit>,
    pub animations: Vec<Animation>,
    pub commands: Vec<Command>,
    pub command_storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalSelector {
    pub action: u8,
    pub allowed_directions: u8,
    pub fallback: u8,
    pub storage: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalAction {
    pub descriptor: u32,
    pub hit: u32,
    pub animation: u32,
    pub command: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalDescriptor {
    pub duration: u16,
    pub recovery_ticks: u16,
    pub combo_at: [u16; 2],
    pub buffer_until: u8,
    pub recovery_clip: u8,
    pub recovery_rate: FloatOperand,
    pub startup_effect: i32,
    pub reach: [u16; 2],
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub source_sha256: String,
    /// Null source slots keep their indices.
    pub records: Vec<Option<Bundle>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Physical pool boundaries retained for source identity, never native pointers.
    pub pool_offsets: [u32; 4],
    /// Four phase descriptors, or one descriptor in the compact source layout.
    pub phases: Vec<Phase>,
    pub hit_rules: Vec<HitRule>,
    pub hits: Vec<Hit>,
    pub animations: Vec<Animation>,
    pub commands: Vec<Command>,
    /// Unconsumed bytes, including terminators shorter than a fixed-width row.
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub duration: u16,
    pub recovery_ticks: u16,
    pub buffer_until: u16,
    pub combo_at: u16,
    pub startup_effect: i32,
    /// Record indices into rules, hits, animations, then a word index into commands.
    pub indices: [u32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitRule {
    pub flags: u16,
    pub element: u8,
    pub hitstun: u8,
    pub contact_cooldown: u8,
    pub stun_chance: u8,
    pub stagger: u8,
    pub guard_pressure: u8,
    pub conditions: u32,
    pub condition_chance: u8,
    pub power_mode: u8,
    pub power: u16,
    pub sound: u16,
    /// Amount added to the defender's action-armor hit counter.
    pub armor_damage: u8,
    pub knockback_delay: u8,
    pub impact_effect: u8,
    pub condition_parameter: i8,
    pub impact_bank: u8,
    pub storage: Vec<Storage>,
}

/// A fixed-width source row, including inactive operands in end records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub start: i16,
    pub emission: i8,
    pub attachment_count: i8,
    pub emission_operands: [u8; 4],
    pub radius: FloatOperand,
    pub height: FloatOperand,
    pub shape: u8,
    pub damage_kind: u8,
    pub rule: u8,
    pub hit_class: u8,
    pub reaction: u8,
    pub projectile_modifier: u16,
    pub inner_radius: FloatOperand,
    pub storage: Vec<Storage>,
}

/// Source animation records can be dispatched or bound directly by a callback.
/// Keep both interpretations possible rather than cooking executable operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Animation {
    pub time: i16,
    pub clip: u8,
    pub blend: u8,
    pub start: u8,
    pub end: u8,
    pub layer_flags: u8,
    pub resource: i8,
    pub rate: FloatOperand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub word_index: u32,
    pub record: CommandRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandRecord {
    End,
    Loop,
    Command {
        time: i16,
        opcode: i16,
        operands: Vec<u16>,
    },
}

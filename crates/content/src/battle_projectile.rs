//! Original projectile templates. Allocation supplies actors, origins and hit bindings.
use crate::source::{FloatOperand, Storage};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/projectiles.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub source_sha256: String,
    pub records: Vec<Projectile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub bank: u8,
    /// Zero disables the effect; the stored bank is still retained.
    pub member: u8,
}

/// Source values are retained independently of runtime controller support.
/// Non-finite inactive operands and unused instance storage round-trip unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projectile {
    pub flags: u32,
    pub lifetime: i16,
    pub ground_effect: Effect,
    pub damage_kind: u8,
    pub hit_class: u8,
    pub reaction: u8,
    pub shape: u8,
    pub knockback: u8,
    pub repeat_limit: u8,
    pub velocity: [FloatOperand; 3],
    pub acceleration: [FloatOperand; 3],
    pub speed: FloatOperand,
    pub bounce_restitution: FloatOperand,
    pub radius: FloatOperand,
    pub height: FloatOperand,
    pub inner_radius: FloatOperand,
    pub growth: [FloatOperand; 2],
    pub birth_effect: Effect,
    pub trail_effect: Effect,
    pub spawn_offset: [FloatOperand; 3],
    pub velocity_jitter: [FloatOperand; 3],
    pub hit_offset: [FloatOperand; 3],
    pub active_start: i16,
    pub active_duration: i16,
    pub shadow_color: [u8; 4],
    pub trail_interval: i16,
    pub pulse_state: u8,
    pub steering_blend: FloatOperand,
    pub steering_end: u8,
    pub steering_start: u8,
    pub toward_bone: u8,
    pub from_bone: u8,
    pub update_mode: u8,
    pub velocity_reset_age: u8,
    pub storage: Vec<Storage>,
}

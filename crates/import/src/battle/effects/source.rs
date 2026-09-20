//! Authored values before an allocator supplies owners, resource banks and scratch state.
use super::*;
use serde::{Deserialize, Serialize};

pub(super) const RECORD_BYTES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EffectSelector {
    pub slot: u8,
    /// Zero disables the sequence; its slot remains authored data.
    pub id: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AuthoredProjectile {
    /// Control bits are retained even when their runtime behavior is not implemented.
    pub flags: u32,
    pub lifetime: u16,
    pub ground_effect: EffectSelector,
    pub shape: HitShape,
    pub knockback: KnockbackDirection,
    pub repeat_limit: u8,
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub speed: f32,
    pub bounce_restitution: f32,
    pub hit_growth: [f32; 2],
    pub spawn_effect: EffectSelector,
    pub trail_effect: EffectSelector,
    pub spawn_offset: [f32; 3],
    pub velocity_jitter: [f32; 3],
    pub hit_offset: [f32; 3],
    pub active_start: u16,
    pub active_duration: u16,
    pub shadow_color: [u8; 4],
    pub trail_interval: u16,
    pub pulse_state: u8,
    pub steering_blend: f32,
    pub steering_end: u8,
    pub steering_start: u8,
    pub toward_bone: u8,
    pub from_bone: u8,
    pub update_mode: u8,
    pub velocity_reset_age: u8,
}

impl AuthoredProjectile {
    /// A null table row still supplies the standard controller's zero-valued recipe.
    pub const NULL: Self = Self {
        flags: 0,
        lifetime: 0,
        ground_effect: EffectSelector { slot: 0, id: 0 },
        shape: HitShape {
            radius: 0.,
            height: 0.,
            kind: HitShapeKind::Box,
            inner_radius: 0.,
            damage_kind: 0,
            hit_class: 0,
            reaction: 0,
        },
        knockback: KnockbackDirection::Attacker,
        repeat_limit: 0,
        velocity: [0.; 3],
        acceleration: [0.; 3],
        speed: 0.,
        bounce_restitution: 0.,
        hit_growth: [0.; 2],
        spawn_effect: EffectSelector { slot: 0, id: 0 },
        trail_effect: EffectSelector { slot: 0, id: 0 },
        spawn_offset: [0.; 3],
        velocity_jitter: [0.; 3],
        hit_offset: [0.; 3],
        active_start: 0,
        active_duration: 0,
        shadow_color: [0; 4],
        trail_interval: 0,
        pulse_state: 0,
        steering_blend: 0.,
        steering_end: 0,
        steering_start: 0,
        toward_bone: 0,
        from_bone: 0,
        update_mode: 0,
        velocity_reset_age: 0,
    };

    pub fn decode(row: &[u8]) -> Result<Self> {
        ensure!(row.len() == RECORD_BYTES, "truncated projectile recipe");
        let scalar = |at| {
            let value = float(row, at)?;
            ensure!(value.is_finite(), "nonfinite projectile value at {at:#x}");
            Ok::<_, anyhow::Error>(value)
        };
        let vector = |at| Ok::<_, anyhow::Error>([scalar(at)?, scalar(at + 4)?, scalar(at + 8)?]);
        let effect = |at: usize| EffectSelector {
            slot: row[at],
            id: row[at + 1],
        };
        Ok(Self {
            flags: word(row, 8)?,
            lifetime: half(row, 12)?,
            ground_effect: effect(0xe),
            shape: HitShape {
                radius: scalar(0x44)?,
                height: scalar(0x48)?,
                inner_radius: scalar(0x4c)?,
                kind: match row[0x15] {
                    0 => HitShapeKind::Box,
                    1 => HitShapeKind::Cylinder,
                    2 => HitShapeKind::GroundCircle,
                    3 => HitShapeKind::Ring,
                    4 => HitShapeKind::Sphere,
                    kind => bail!("unsupported projectile shape {kind}"),
                },
                damage_kind: row[0x11],
                hit_class: row[0x12],
                reaction: row[0x13],
            },
            knockback: match row[0x16] {
                0 => KnockbackDirection::Attacker,
                1 => KnockbackDirection::Velocity,
                2 => KnockbackDirection::AwayFromProjectile,
                3 => KnockbackDirection::TowardProjectile,
                kind => bail!("unsupported knockback direction {kind}"),
            },
            repeat_limit: row[0x17],
            velocity: vector(0x24)?,
            acceleration: vector(0x30)?,
            speed: scalar(0x3c)?,
            bounce_restitution: scalar(0x40)?,
            hit_growth: [scalar(0x50)?, scalar(0x54)?],
            spawn_effect: effect(0x5c),
            trail_effect: effect(0x5e),
            spawn_offset: vector(0x60)?,
            velocity_jitter: vector(0x6c)?,
            hit_offset: vector(0x78)?,
            active_start: half(row, 0x84)?,
            active_duration: half(row, 0x86)?,
            shadow_color: row[0x88..0x8c].try_into()?,
            trail_interval: half(row, 0x90)?,
            pulse_state: row[0x92],
            steering_blend: scalar(0x94)?,
            steering_end: row[0x98],
            steering_start: row[0x99],
            toward_bone: row[0x9a],
            from_bone: row[0x9b],
            update_mode: row[0x9c],
            velocity_reset_age: row[0x9d],
        })
    }
}

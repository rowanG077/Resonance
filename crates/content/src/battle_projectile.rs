//! Projectile motion and contact settings. Encounters supply actor and resource bindings.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "battle/projectiles.json";

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    pub source_sha256: String,
    pub records: Vec<Projectile>,
    #[serde(default)]
    pub resident_spells: BTreeMap<u16, ResidentSpell>,
    #[serde(default)]
    pub support_durations: BTreeMap<u16, u16>,
    #[serde(default)]
    pub fire_ball: Option<FireBall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireBall {
    pub velocities: [[f32; 3]; 3],
    pub center_height_scale: f32,
    pub height_offset: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResidentSpell {
    pub duration: u16,
    pub hit: crate::battle_action::ActionHit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_hit: Option<crate::battle_action::ActionHit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub bank: u8,
    /// Zero disables the effect; the stored bank is still retained.
    pub member: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageKind {
    Slash,
    Thrust,
    Magic,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitShape {
    Box,
    Cylinder,
    GroundCircle,
    Ring { width: f32 },
    Sphere,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoilDirection {
    Travel,
    #[default]
    AwayFromOwner,
    AwayFromContact,
    TowardContact,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileSteering {
    pub blend: f32,
    pub start: u32,
    /// Exclusive; None leaves steering active indefinitely.
    pub end: Option<u32>,
    pub planar: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileResponse {
    pub bounce: bool,
    pub ricochet: bool,
    /// Rebound on guarded or guard-broken contacts and attack clashes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ricochet_on_deflect: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ground_ricochet: bool,
    pub restitution: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileOrigin {
    #[default]
    Actor,
    TargetFloor,
    Bone(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectileBoneDirection {
    pub from: u8,
    pub to: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileVelocityReset {
    pub age: u32,
    pub acceleration_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileDeparture {
    pub return_speed: f32,
    pub jump_speed: f32,
    pub ground_speed: f32,
    /// X/Z reference point for the tangent followed at ground departure.
    pub ground_reference: [f32; 2],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectileMotion {
    #[serde(default)]
    pub origin: ProjectileOrigin,
    /// Sample a body-bone direction once, independently of the emission origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone_direction: Option<ProjectileBoneDirection>,
    /// Hop toward the owner after contact, then depart along the ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub departure: Option<ProjectileDeparture>,
    /// Replace velocity after this age's movement; acceleration remains active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_reset: Option<ProjectileVelocityReset>,
    /// Use the supplied world direction without rotating it by the owner's heading.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub world_direction: bool,
    /// Update heading from a positive X or Z motion component.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub face_velocity: bool,
    /// Choose a fixed world direction toward the captured aim point at launch.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aim_at_target: bool,
    /// Keep the projectile at its first floor contact until its lifetime ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stop_on_ground: bool,
    /// Animated contact points in projectile-local space, indexed by age; hold the final sample.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contact_path: Vec<Vec<[f32; 3]>>,
    /// This flight may travel beyond the ordinary arena retirement radius.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unlimited_range: bool,
    /// Three-phase water-blade motion, prepared by the Aqua Edge binding.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aqua_edge: bool,
    pub velocity_jitter: [f32; 3],
    /// Direction-times-speed integration; None uses ballistic velocity.
    pub speed: Option<f32>,
    pub steering: Option<ProjectileSteering>,
    pub response: Option<ProjectileResponse>,
}

impl ProjectileMotion {
    pub fn validate_velocity_jitter(&self) -> Result<()> {
        ensure!(
            self.velocity_jitter
                .iter()
                .all(|amplitude| amplitude.is_finite()),
            "invalid projectile velocity jitter"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileShadow {
    pub color: [u8; 4],
    pub additive: bool,
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub repeat_limit: u8,
    pub shape: HitShape,
    pub recoil_direction: RecoilDirection,
    pub radius: f32,
    pub height: f32,
    pub growth: [f32; 2],
    pub offset: [f32; 3],
    pub survives_contact: bool,
    pub clashes: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projectile {
    pub lifetime: Option<u32>,
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub spawn_offset: [f32; 3],
    pub clamp_ground: bool,
    pub active: Option<[u32; 2]>,
    pub motion: ProjectileMotion,
    pub contact: Contact,
    pub birth_effect: Effect,
    pub ground_effect: Effect,
    pub trail_effect: Effect,
    pub trail_interval: Option<u32>,
    pub shadow: Option<ProjectileShadow>,
    /// Unsupported templates stay available for diagnostics without blocking import.
    pub unsupported_reason: Option<String>,
}

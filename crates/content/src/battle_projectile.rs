//! Projectile motion and contact settings. Encounters supply actor and resource bindings.
use anyhow::{Result, ensure};
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
    pub restitution: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectileMotion {
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

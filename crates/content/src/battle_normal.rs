//! Authored normal attacks, prepared on one elapsed gameplay timeline.
use crate::{
    battle_action::{ActionHit, ActionPose, MeleeContact},
    battle_voice::Sound,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attack {
    pub continuation: Continuation,
    pub duration: u16,
    pub chain_at: Option<u16>,
    pub recovery: u16,
    pub recovery_pose: ActionPose,
    pub reach: [u16; 2],
    pub events: Vec<(u16, Event)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Effect(u16),
    Pose(ActionPose),
    AnimationRate(f32),
    Contact(MeleeContact),
    /// Release a shared projectile. A missing hit retains the technique's recipe.
    Projectile {
        member: u8,
        hit: Option<ActionHit>,
        feedback: Option<crate::battle_martial::ProjectileFeedback>,
    },
    Throw {
        slot: u8,
        flight: WeaponFlight,
        radius: f32,
        hit: ActionHit,
    },
    Forward(f32),
    Vertical(f32),
    Gravity(f32),
    Turn(f32),
    SecondaryWind {
        enabled: bool,
        duration: u16,
    },
    CameraDistance {
        distance: f32,
        duration: u16,
    },
    PassThrough(bool),
    Braking(f32),
    WeaponVisibility {
        slot: u8,
        visible: bool,
    },
    Trail {
        slot: u8,
        duration: u16,
    },
    Sound(u16),
    Voice(Option<Sound>),
}

/// Directional successors and the selection used for any other input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Continuation {
    pub directions: u8,
    pub fallback: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeaponFlight {
    pub outbound_ticks: u16,
    pub speed: f32,
    pub return_speed: f32,
    pub direction_y: f32,
}

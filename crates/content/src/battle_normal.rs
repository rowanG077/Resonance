//! Authored normal attacks, prepared on one elapsed gameplay timeline.
use crate::{
    battle_action::{ActionHit, ActionPose, MeleeContact, WeaponSlot},
    battle_voice::Sound,
};
use serde::{Deserialize, Serialize};

pub mod document;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(
        serialize = "E: Serialize, C: Serialize + Default + PartialEq",
        deserialize = "E: Deserialize<'de>, C: Deserialize<'de> + Default"
    )
)]
pub struct Attack<E = Event, C = Continuation> {
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub continuation: C,
    pub duration: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_at: Option<u16>,
    pub recovery: u16,
    pub reach: [u16; 2],
    pub events: Vec<(u16, E)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "H: Deserialize<'de>"))]
pub enum Event<H = ActionHit> {
    Effect(u16),
    Pose(ActionPose),
    AnimationRate(f32),
    Contact(MeleeContact<H>),
    /// Release a shared projectile. A missing hit retains the technique's recipe.
    Projectile {
        member: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hit: Option<H>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<crate::battle_martial::ProjectileFeedback>,
    },
    Throw {
        slot: WeaponSlot,
        flight: WeaponFlight,
        radius: f32,
        hit: H,
    },
    Forward(f32),
    Vertical(f32),
    Gravity(f32),
    Turn(f32),
    /// Move toward an in-front target's far side, up to the authored travel limit.
    TraverseTarget {
        duration: u16,
        clearance: f32,
        max_start_distance: f32,
        max_distance: f32,
    },
    /// Suppress hit reactions for this many updates; zero clears the protection.
    ReactionArmor(u16),
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
        slot: WeaponSlot,
        visible: bool,
    },
    Trail {
        slot: WeaponSlot,
        duration: u16,
    },
    Sound(u16),
    Voice(Option<Sound>),
}

impl<H> Event<H> {
    pub(crate) fn hit(&self) -> Option<&H> {
        match self {
            Self::Contact(contact) => Some(&contact.hit),
            Self::Projectile { hit, .. } => hit.as_ref(),
            Self::Throw { hit, .. } => Some(hit),
            _ => None,
        }
    }
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

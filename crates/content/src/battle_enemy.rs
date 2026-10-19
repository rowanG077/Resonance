//! Enemy gameplay inputs, available independently of body and attachment artwork.
use crate::{
    battle_action::{EnemyActions, TargetPolicy},
    battle_profile::Profile,
};
use serde::{Deserialize, Serialize};

pub fn path(id: u8) -> String {
    format!("battle/enemies/{id:03}/definition.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryRow {
    Random,
    Front,
    Middle,
    Back,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub source_sha256: String,
    /// Empty visual slots in the authored body motion table.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub null_motions: std::collections::BTreeSet<u16>,
    pub profile: Profile,
    /// Initial visibility overrides, indexed by carried component slot.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub weapon_visibility: std::collections::BTreeMap<u8, WeaponVisibility>,
    /// Every decoded projectile row, including unsupported rows, in authored order.
    pub projectiles: Vec<Projectile>,
    pub actions: EnemyActions,
    pub target_strategy: Option<TargetPolicy>,
    pub entry_row: EntryRow,
    pub guard_recovery_bonus: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponVisibility {
    Hidden,
    /// Hidden on even formation slots, visible on odd slots (starting at zero).
    Alternating,
}

impl WeaponVisibility {
    pub fn visible(self, spawn_index: usize) -> bool {
        match self {
            Self::Hidden => false,
            Self::Alternating => !spawn_index.is_multiple_of(2),
        }
    }
}

/// Optional opening shot and the final enemy's defeat hold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presentation {
    pub opening: Option<Opening>,
    pub defeat: crate::battle_voice::TimedVoice,
}

/// An opening either holds for a voice or plays an authored entrance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Opening {
    Voice(crate::battle_voice::TimedVoice),
    Entrance(Entrance),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entrance {
    pub ticks: u16,
    pub gravity: f32,
    pub flash: Option<EntranceFlash>,
    /// Vertical impulses applied before movement, indexed by actor age.
    pub launches: Vec<(u16, f32)>,
    /// Presentation cues run after movement, indexed by the displayed update.
    pub cues: Vec<(u16, EntranceCue)>,
    pub camera: EntranceCamera,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntranceFlash {
    pub tick: u16,
    pub color: [u8; 3],
    pub step: u8,
    pub hold: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntranceCamera {
    pub radius: f32,
    pub height_radius: f32,
    pub focus_height: f32,
    pub yaw: f32,
    pub yaw_step: f32,
    pub elevation: f32,
    pub elevation_step: f32,
    pub minimum_elevation: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntranceCue {
    Height(f32),
    Vertical(f32),
    Pose {
        clip: u16,
        rate: f32,
        repeat: bool,
        blend: u8,
    },
    StageTint {
        color: [u8; 3],
        duration: u16,
        step: u8,
    },
    Feedback {
        duration: u16,
        limit: u8,
        alpha: u8,
    },
    Expression(u8),
    Effect(u16),
    Voice(crate::battle_voice::Sound),
    Sound(crate::battle_voice::Sound),
    Shake {
        duration: u32,
        amplitude: u32,
    },
}

/// An enemy projectile recipe and its contact response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projectile {
    pub definition: crate::battle_projectile::Projectile,
    pub recoil: u8,
}

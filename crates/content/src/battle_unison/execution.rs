//! Optional imported participant choreography, flattened into ordinary actions.
use crate::{battle_normal::Event, battle_voice::Sound};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub roles: [Role; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub voices: Vec<PairVoice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub phases: BTreeMap<String, u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<RolePlacement>,
    pub visible: bool,
    #[serde(default)]
    pub temporary_weapons: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<(u16, Callback)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RolePlacement {
    pub direction: [f32; 3],
    pub distance: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairVoice {
    pub characters: [String; 2],
    /// Index of the speaking role in this ordered pair.
    pub speaker: u8,
    pub sound: Sound,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    World([f32; 3]),
    ActorRoot,
    ActorAim,
    TargetRoot,
    TargetAim,
    TargetAimHeight { minimum_height: f32 },
    TargetRootOffset { offset: [f32; 3], rotated: bool },
    TargetAimOffset { offset: [f32; 3], rotated: bool },
    WorldOffset { offset: [f32; 3], rotated: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Callback {
    Effect {
        member: u16,
        origin: Origin,
        scale: Option<f32>,
        follow: bool,
    },
    CommonEffect {
        member: u16,
        origin: Origin,
        scale: Option<f32>,
        follow: bool,
    },
    Projectile {
        member: u8,
        hit_variant: u8,
        origin: Origin,
        heading: Option<f32>,
        height: Option<f32>,
    },
    Feedback {
        duration: u16,
        limit: u8,
    },
    PositionFromTarget {
        height: f32,
        retreat: f32,
    },
    Ordinary(Event),
}

impl Execution {
    pub fn validate(&self) -> Result<()> {
        const NAMES: [&str; 9] = [
            "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
        ];
        for role in &self.roles {
            ensure!(
                !role.phases.is_empty()
                    && role
                        .phases
                        .keys()
                        .all(|name| NAMES.contains(&name.as_str())),
                "invalid authored Unison role characters"
            );
            if let Some(placement) = role.placement {
                ensure!(
                    placement.direction.iter().all(|v| v.is_finite())
                        && placement.direction.iter().any(|v| *v != 0.)
                        && placement.distance.is_finite()
                        && placement.distance >= 0.,
                    "invalid authored Unison placement"
                );
            }
        }
        for voice in &self.voices {
            ensure!(
                voice.speaker < 2
                    && voice.characters[0] != voice.characters[1]
                    && self.roles[0].phases.contains_key(&voice.characters[0])
                    && self.roles[1].phases.contains_key(&voice.characters[1]),
                "invalid authored Unison pair voice"
            );
        }
        Ok(())
    }
}

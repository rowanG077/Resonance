//! A fixed-origin summon and its authored sequence of independent attack points.
use super::{HitRule, StoredSpellPresentation};
use crate::battle::effects::{EffectBank, EffectId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummonKind {
    Light,
    Birth,
}

impl SummonKind {
    pub fn native(self) -> u16 {
        match self {
            Self::Light => 290,
            Self::Birth => 292,
        }
    }
    pub fn lifetime(self) -> u16 {
        match self {
            Self::Light => 335,
            Self::Birth => 445,
        }
    }
    pub fn interval(self) -> u16 {
        match self {
            Self::Light => 6,
            Self::Birth => 15,
        }
    }
    pub fn count(self) -> usize {
        match self {
            Self::Light => 17,
            Self::Birth => 14,
        }
    }
    pub fn focus_ticks(self) -> u16 {
        match self {
            Self::Light => 130,
            Self::Birth => 135,
        }
    }
    pub fn magic_up(self) -> i16 {
        match self {
            Self::Light => 15,
            Self::Birth => 20,
        }
    }
    pub fn voice(self) -> u16 {
        match self {
            Self::Light => 0x8584,
            Self::Birth => 0x8594,
        }
    }
    pub fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self.native() - 200),
            id,
        }
    }
    pub fn strike_effect(self) -> EffectId {
        self.effect(match self {
            Self::Light => 2,
            Self::Birth => 3,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummonRecipe {
    pub kind: SummonKind,
    pub rule: HitRule,
    pub offsets: Vec<[f32; 3]>,
    pub presentation: StoredSpellPresentation,
    pub title: String,
}

impl SummonRecipe {
    pub const FIRST_STRIKE: u16 = 140;
    pub const VOICE_TICK: u16 = 90;
    pub const HEADING_BIAS: f32 = 20.0;
    pub const FOCUS_DISTANCE: f32 = 2150.0;
    pub const BLESSING_RADIUS: f32 = 512.0;

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.offsets.len() == self.kind.count()
                && self.offsets.iter().flatten().all(|n| n.is_finite())
                && self.offsets.iter().all(|point| point[1] == 0.)
                && self.presentation.color == [12, 12, 12, 255]
                && self.presentation.camera_distance
                    == match self.kind {
                        SummonKind::Light => 3450.,
                        SummonKind::Birth => 4450.,
                    }
                && self.presentation.camera_elevation == 15.5
                && self.title
                    == match self.kind {
                        SummonKind::Light => "-Luna-",
                        SummonKind::Birth => "-Maxwell-",
                    },
            "invalid fixed-origin summon recipe"
        );
        self.rule
            .impact_program_from(self.kind.effect(1).bank, Some(self.kind.native() - 200))?;
        Ok(())
    }
    /// The pattern heading is captured once; Light's contact heading remains live.
    pub fn strike(&self, age: u16, heading: f32) -> Option<[f32; 3]> {
        let elapsed = age.checked_sub(Self::FIRST_STRIKE)?;
        if elapsed % self.kind.interval() != 0 {
            return None;
        }
        let [x, y, z] = *self
            .offsets
            .get(usize::from(elapsed / self.kind.interval()))?;
        let (sin, cos) = heading.sin_cos();
        Some([cos * x + sin * z, y, -sin * x + cos * z])
    }
}

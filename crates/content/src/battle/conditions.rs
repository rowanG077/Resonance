use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Ailment identities retain their authored battle bit positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Poison = 0,
    DeadlyPoison = 1,
    Paralysis = 3,
    Weak = 4,
    Petrify = 5,
    Curse = 7,
    ItemRecoveryDown = 8,
    Slow = 9,
}

impl Condition {
    pub const ALL: [Self; 8] = [
        Self::Poison,
        Self::DeadlyPoison,
        Self::Paralysis,
        Self::Weak,
        Self::Petrify,
        Self::Curse,
        Self::ItemRecoveryDown,
        Self::Slow,
    ];
    pub const PHYSICAL_MASK: u64 = 0xab;
    pub const MAGICAL_MASK: u64 =
        Self::Weak.bit() | Self::ItemRecoveryDown.bit() | Self::Slow.bit();
    pub const MASK: u64 = Self::PHYSICAL_MASK | Self::MAGICAL_MASK;
    pub const fn bit(self) -> u64 {
        1 << self as u8
    }
    pub const fn persistent_bit(self) -> u32 {
        match self {
            Self::Poison => 0x20,
            Self::DeadlyPoison => 0x40,
            Self::Paralysis => 0x80,
            Self::Petrify => 0x100,
            Self::Curse => 0x200,
            // These magical ailments are cleared by battle cleanup.
            Self::Weak | Self::ItemRecoveryDown | Self::Slow => 0,
        }
    }
}

/// Script applications admit whole masks; immunity rejects the entire request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScriptConditions(u64);

impl ScriptConditions {
    pub const STUN: u64 = 4;

    pub fn new(bits: u64) -> Result<Self> {
        let flags = Self(bits);
        flags.validate()?;
        Ok(flags)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub fn validate(self) -> Result<()> {
        ensure!(
            self.0 & !(Condition::MASK | Self::STUN) == 0,
            "unsupported scripted battle condition mask {:#x}",
            self.0
        );
        Ok(())
    }
}

/// Temporary percentage penalties applied by seal artes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum StatDebuff {
    DefenseDown,
    AccuracyDown,
    EvasionDown,
    AttackDown,
}

impl StatDebuff {
    pub const ALL: [Self; 4] = [
        Self::DefenseDown,
        Self::AccuracyDown,
        Self::EvasionDown,
        Self::AttackDown,
    ];
    pub const fn bit(self) -> u64 {
        match self {
            Self::DefenseDown => 0x40000,
            Self::AccuracyDown => 0x80000,
            Self::EvasionDown => 0x400000,
            Self::AttackDown => 0x20000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ConditionTraits {
    pub immunity: u64,
    pub intrinsic: u64,
    pub chance_resistance: bool,
    pub paralysis_face: [u8; 4],
}

impl ConditionTraits {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.intrinsic & !Condition::MASK == 0,
            "unsupported battle condition traits"
        );
        Ok(())
    }
}

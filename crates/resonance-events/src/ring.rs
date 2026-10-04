//! Sorcerer's Ring abilities and scenario encodings; casting lives in the controller.
mod controller;
pub(crate) use controller::Controller;
use serde::{Deserialize, Serialize};

pub const ITEM: u16 = 55;
pub(crate) const SCRIPT_ACTOR: i32 = 99_992;

pub(crate) const CALLBACK: u32 = u32::MAX;
/// Shared native registry entry: scan expiry or insufficient TP, by ability.
pub(crate) const SECONDARY_CALLBACK: u32 = (-9999_i32) as u32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SorcerersRing {
    #[default]
    Disabled,
    Fire,
    Shrink,
    Mana,
    ElectricOrb(ElectricOrbKind),
    Radar,
    Water,
    Wind,
    LongRangeFire,
    Sunlight,
    Bomb,
    Lightning(LightningColor),
    Ice,
    Earthquake,
    Darkness,
    Sound,
    AnimalCall(CallColor),
    Bubble(BubblePhase),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElectricOrbKind {
    Sylvarant,
    Tethealla,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightningColor {
    Blue,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallColor {
    Pink,
    White,
    Blue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BubblePhase {
    Release,
    Float,
}

impl TryFrom<[u8; 2]> for SorcerersRing {
    type Error = String;

    fn try_from([mode, variant]: [u8; 2]) -> Result<Self, String> {
        Ok(match (mode, variant) {
            (0, 0) => Self::Disabled,
            (1 | 2, 0) => Self::Fire,
            (3, 0) => Self::Shrink,
            (4, 0) => Self::Mana,
            (5, 0) => Self::ElectricOrb(ElectricOrbKind::Sylvarant),
            (6, 0) => Self::Radar,
            (7, 0) => Self::Water,
            (8, 0) => Self::Wind,
            (9, 0) => Self::LongRangeFire,
            (10, 0) => Self::Sunlight,
            (11, 0) => Self::ElectricOrb(ElectricOrbKind::Tethealla),
            (12, 0) => Self::Bomb,
            (13, 0) => Self::Lightning(LightningColor::Blue),
            (13, 1) => Self::Lightning(LightningColor::Yellow),
            (13, 2) => Self::Lightning(LightningColor::Red),
            (14, 0) => Self::Ice,
            (15, 0) => Self::Earthquake,
            (16, 0) => Self::Darkness,
            (17, 0) => Self::Sound,
            (18, 0) => Self::AnimalCall(CallColor::Pink),
            (18, 1) => Self::AnimalCall(CallColor::White),
            (18, 2) => Self::AnimalCall(CallColor::Blue),
            (19, 0) => Self::Bubble(BubblePhase::Release),
            (19, 1) => Self::Bubble(BubblePhase::Float),
            _ => {
                return Err(format!(
                    "invalid Sorcerer's Ring mode/variant {mode}/{variant}"
                ));
            }
        })
    }
}

impl From<SorcerersRing> for [u8; 2] {
    fn from(ring: SorcerersRing) -> Self {
        use SorcerersRing::*;
        match ring {
            Disabled => [0, 0],
            Fire => [1, 0],
            Shrink => [3, 0],
            Mana => [4, 0],
            ElectricOrb(ElectricOrbKind::Sylvarant) => [5, 0],
            Radar => [6, 0],
            Water => [7, 0],
            Wind => [8, 0],
            LongRangeFire => [9, 0],
            Sunlight => [10, 0],
            ElectricOrb(ElectricOrbKind::Tethealla) => [11, 0],
            Bomb => [12, 0],
            Lightning(LightningColor::Blue) => [13, 0],
            Lightning(LightningColor::Yellow) => [13, 1],
            Lightning(LightningColor::Red) => [13, 2],
            Ice => [14, 0],
            Earthquake => [15, 0],
            Darkness => [16, 0],
            Sound => [17, 0],
            AnimalCall(CallColor::Pink) => [18, 0],
            AnimalCall(CallColor::White) => [18, 1],
            AnimalCall(CallColor::Blue) => [18, 2],
            Bubble(BubblePhase::Release) => [19, 0],
            Bubble(BubblePhase::Float) => [19, 1],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hit {
    Actor(i16),
    /// Area and transformation abilities identify the ring itself to the script.
    Pulse,
}

impl Hit {
    pub(crate) fn event_actor(self) -> i16 {
        // The scenario ring actor ID (99_992) is stored in an s16 event field.
        match self {
            Self::Actor(actor) => actor,
            Self::Pulse => SCRIPT_ACTOR as i16,
        }
    }
}

#[cfg(test)]
mod tests;

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

impl SorcerersRing {
    const MODES: &[(Self, [u8; 2])] = &[
        (Self::Disabled, [0, 0]),
        (Self::Fire, [1, 0]),
        (Self::Fire, [2, 0]),
        (Self::Shrink, [3, 0]),
        (Self::Mana, [4, 0]),
        (Self::ElectricOrb(ElectricOrbKind::Sylvarant), [5, 0]),
        (Self::Radar, [6, 0]),
        (Self::Water, [7, 0]),
        (Self::Wind, [8, 0]),
        (Self::LongRangeFire, [9, 0]),
        (Self::Sunlight, [10, 0]),
        (Self::ElectricOrb(ElectricOrbKind::Tethealla), [11, 0]),
        (Self::Bomb, [12, 0]),
        (Self::Lightning(LightningColor::Blue), [13, 0]),
        (Self::Lightning(LightningColor::Yellow), [13, 1]),
        (Self::Lightning(LightningColor::Red), [13, 2]),
        (Self::Ice, [14, 0]),
        (Self::Earthquake, [15, 0]),
        (Self::Darkness, [16, 0]),
        (Self::Sound, [17, 0]),
        (Self::AnimalCall(CallColor::Pink), [18, 0]),
        (Self::AnimalCall(CallColor::White), [18, 1]),
        (Self::AnimalCall(CallColor::Blue), [18, 2]),
        (Self::Bubble(BubblePhase::Release), [19, 0]),
        (Self::Bubble(BubblePhase::Float), [19, 1]),
    ];
}
impl TryFrom<[u8; 2]> for SorcerersRing {
    type Error = String;
    fn try_from(input: [u8; 2]) -> Result<Self, String> {
        Self::MODES
            .iter()
            .find(|(_, wire)| *wire == input)
            .map(|(ring, _)| *ring)
            .ok_or_else(|| {
                format!(
                    "invalid Sorcerer's Ring mode/variant {}/{}",
                    input[0], input[1]
                )
            })
    }
}
impl From<SorcerersRing> for [u8; 2] {
    fn from(ring: SorcerersRing) -> Self {
        SorcerersRing::MODES
            .iter()
            .find(|(kind, _)| *kind == ring)
            .unwrap()
            .1
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

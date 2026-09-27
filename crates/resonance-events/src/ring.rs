//! Sorcerer's Ring state and the original event ABI. Dungeon policy belongs in
//! scripts; numeric encodings are confined to this boundary.
use serde::{Deserialize, Serialize};

pub const ITEM: u16 = 55;

pub(crate) const CALLBACK: u32 = u32::MAX;
/// Shared native registry entry: scan expiry or insufficient TP, by ability.
pub(crate) const SECONDARY_CALLBACK: u32 = (-9999_i32) as u32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "[u8; 2]", into = "[u8; 2]")]
pub enum SorcerersRing {
    #[default]
    Disabled,
    Fire,
    /// Native mode 2 shares the ordinary fire controller; no retail station sets it.
    AlternateFire,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElectricOrbKind {
    Sylvarant,
    Tethealla,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightningColor {
    Blue,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallColor {
    Pink,
    White,
    Blue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubblePhase {
    Release,
    Float,
}

impl TryFrom<[u8; 2]> for SorcerersRing {
    type Error = String;

    fn try_from([mode, variant]: [u8; 2]) -> Result<Self, String> {
        Ok(match (mode, variant) {
            (0, 0) => Self::Disabled,
            (1, 0) => Self::Fire,
            (2, 0) => Self::AlternateFire,
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
            AlternateFire => [2, 0],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Actor(i16),
    /// Area and transformation abilities identify the ring itself to the script.
    Pulse,
}

impl Hit {
    pub(crate) fn event_actor(self) -> i16 {
        // The original ring actor ID (99_992) is stored in an s16 event field.
        const RING_ACTOR: i16 = 99_992u32 as i16;
        match self {
            Self::Actor(actor) => actor,
            Self::Pulse => RING_ACTOR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_loading_rejects_variants_that_do_not_belong_to_the_selected_ability() {
        for invalid in ["[1,1]", "[13,3]", "[18,3]", "[19,2]", "[20,0]"] {
            assert!(serde_json::from_str::<SorcerersRing>(invalid).is_err());
        }
        // Original Thunder Temple and Latheon station encodings in saved travel.
        assert_eq!(
            serde_json::from_str::<SorcerersRing>("[13,1]").unwrap(),
            SorcerersRing::Lightning(LightningColor::Yellow)
        );
        assert_eq!(
            serde_json::from_str::<SorcerersRing>("[19,1]").unwrap(),
            SorcerersRing::Bubble(BubblePhase::Float)
        );
    }
}

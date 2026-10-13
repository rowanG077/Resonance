//! Prepare party techniques with resolved action and feedback resources.
use super::{ActionDefinition, party::Character, voice::Sound};

mod advanced_seals;
mod attack;
mod authored;
mod colette;
mod fierce;
mod hammer;
mod physical;
mod seals;
mod steal;
mod sword;
mod tempest;
mod thrust;
mod tiger_blade;

/// Special Guard shares one sequence, with character-specific catalogue and feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpecialGuard {
    pub catalogue: u16,
    pub effect: u16,
    pub voice: Sound,
}

pub(crate) fn special_guard(character: u8) -> Option<SpecialGuard> {
    let index = usize::from(character.checked_sub(1)?);
    Some(SpecialGuard {
        catalogue: *[34, 202, 203, 203, 204, 34, 205, 206, 34].get(index)?,
        effect: if character == Character::Genis as u8 {
            2
        } else {
            1
        },
        voice: [
            Sound::Stream(92),
            Sound::Stream(202),
            Sound::Stream(329),
            Sound::Stream(445),
            Sound::Stream(548),
            Sound::Stream(677),
            Sound::Stream(782),
            Sound::Stream(892),
            Sound::Stream(995),
        ][index],
    })
}

pub(crate) fn is_special_guard(character: u8, catalogue: u16) -> bool {
    special_guard(character).is_some_and(|row| row.catalogue == catalogue)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Sequence {
    SpecialGuard,
    Authored,
    TigerBlade,
    Tempest,
    LightningBlade,
    RisingFalcon,
    FalconsCrest,
    Spell(u16),
    DemonFang,
    FierceDemonFang,
    RayThrust,
    HurricaneThrust,
    PowHammer,
    HammerVolley,
    ColetteProjectiles,
    ItemThief,
    Destruction,
    EagleDive,
    Seal,
    ForceSeal,
    CycloneSeal,
}

/// One supported party action and the resources needed by its sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Technique {
    character: Character,
    catalogue: u16,
    sequence: Sequence,
}

pub fn technique(character: u8, catalogue: u16) -> Option<Technique> {
    use Character::*;
    use Sequence::*;
    let character = Character::try_from(character).ok()?;
    let sequence = match (character, catalogue) {
        _ if is_special_guard(character as u8, catalogue) => SpecialGuard,
        (Lloyd, 1 | 2) | (Zelos | Kratos, 207 | 208) => DemonFang,
        (Lloyd, 4) | (Zelos | Kratos, 209) => FierceDemonFang,
        (Lloyd, 3 | 5 | 8 | 9 | 10 | 12 | 13 | 14 | 15 | 16 | 20 | 21 | 22 | 24 | 26 | 27 | 28)
        | (Colette, 38 | 45 | 46 | 48)
        | (Zelos | Kratos, 145..=148 | 150 | 210 | 212) => Authored,
        (Lloyd, 6 | 7 | 23 | 25) => TigerBlade,
        (Lloyd, 11) => HurricaneThrust,
        (Zelos | Kratos, 211) => HurricaneThrust,
        (Lloyd, 17..=19) => Tempest,
        (Lloyd, 29) => RisingFalcon,
        (Lloyd, 228) => FalconsCrest,
        (Zelos | Kratos, 149 | 151) => LightningBlade,
        (Colette, 35 | 36) => RayThrust,
        (Colette, 40) => PowHammer,
        (Colette, 41 | 42) => HammerVolley,
        (Colette, 37 | 39 | 43 | 44 | 50 | 52) => ColetteProjectiles,
        (Colette, 223 | 224) => ItemThief,
        (Colette, 56) => Spell(267),
        (Colette, 57) => Spell(268),
        (Colette, 59) => Spell(270),
        (Colette, 251) => Spell(296),
        (Colette | Zelos, 58) | (Kratos, 252) => Spell(269),
        (Genis, 62) => Spell(200),
        (Genis, 63) => Spell(201),
        (Genis, 64) => Spell(202),
        (Genis, 65) => Spell(203),
        (Genis, 85) => Spell(223),
        (Genis, 90) => Spell(228),
        (Genis, 94) => Spell(232),
        (Genis, 66) | (Zelos | Kratos, 213) => Spell(204),
        (Genis, 67) | (Zelos | Kratos, 214) => Spell(205),
        (Genis, 68) => Spell(206),
        (Genis, 69) => Spell(207),
        (Genis, 70) | (Zelos | Kratos, 215) => Spell(208),
        (Genis, 71) | (Zelos | Kratos, 216) => Spell(209),
        (Genis, 72) => Spell(210),
        (Genis, 73) => Spell(211),
        (Genis, 74) | (Zelos | Kratos, 217) => Spell(212),
        (Genis, 75) => Spell(213),
        (Genis, 76) => Spell(214),
        (Genis, 77) | (Zelos | Kratos, 218) => Spell(215),
        (Genis, 78) | (Zelos | Kratos, 219) => Spell(216),
        (Genis, 79) => Spell(217),
        (Genis, 82) => Spell(220),
        (Genis, 80) => Spell(218),
        (Genis, 81) | (Zelos | Kratos, 220) => Spell(219),
        (Genis, 83) => Spell(221),
        (Genis, 84) => Spell(222),
        (Genis, 86) => Spell(224),
        (Genis, 88) => Spell(226),
        (Genis, 89) => Spell(227),
        (Genis, 91) => Spell(229),
        (Genis, 92) => Spell(230),
        (Genis, 93) => Spell(231),
        (Genis, 95) => Spell(233),
        (Genis, 248) => Spell(280),
        (Raine, 98) | (Zelos | Kratos, 221) => Spell(236),
        (Raine, 99) => Spell(237),
        (Raine, 100) => Spell(238),
        (Raine, 101) => Spell(239),
        (Raine, 102) => Spell(240),
        (Raine, 103) => Spell(241),
        (Raine, 104) => Spell(242),
        (Raine, 105) => Spell(243),
        (Raine, 106) => Spell(244),
        (Raine, 107) => Spell(245),
        (Raine, 108) => Spell(246),
        (Raine, 109) => Spell(247),
        (Raine, 110) => Spell(248),
        (Raine, 111) => Spell(249),
        (Raine, 112) => Spell(250),
        (Raine, 113) => Spell(251),
        (Raine, 114) => Spell(252),
        (Raine, 115) => Spell(253),
        (Raine, 116) => Spell(254),
        (Zelos | Kratos, 117) => Spell(255),
        (Zelos | Kratos, 118) => Spell(256),
        (Raine, 119) => Spell(257),
        (Raine, 120) => Spell(258),
        (Raine, 121) => Spell(259),
        (Raine, 122) => Spell(260),
        (Raine, 123) => Spell(261),
        (Raine, 222) => Spell(263),
        (Raine, 246) => Spell(278),
        (Raine, 250) => Spell(283),
        (Sheena, 125..=136) => Seal,
        (Sheena, 140) => ForceSeal,
        (Sheena, 141) => Authored,
        (Sheena, 142) => Spell(295),
        (Sheena, 143) => CycloneSeal,
        (Sheena, 139 | 245) => Spell(super::corrine::SCENE),
        (Sheena, catalogue) if super::enchant_seals::native(catalogue).is_some() => {
            Spell(super::enchant_seals::native(catalogue)?)
        }
        (Sheena, catalogue) => Spell(super::summons::scene(catalogue)?),
        (Presea, 154..=157 | 170 | 171) => Destruction,
        (Presea, 159..=165 | 167..=169 | 172 | 173 | 175) => Authored,
        (Regal, 176..=184 | 188..=191 | 195 | 197 | 201) => Authored,
        (Regal, 185..=187) => EagleDive,
        (Regal, 192) => Spell(273),
        (Regal, 193) => Spell(274),
        (Regal, 194) => Spell(275),
        _ => return None,
    };
    Some(Technique {
        character,
        catalogue,
        sequence,
    })
}

impl Technique {
    pub(crate) fn is_special_guard(self) -> bool {
        self.sequence == Sequence::SpecialGuard
    }

    pub(crate) fn casting_voice(self) -> Option<u16> {
        match self.sequence {
            Sequence::Spell(voice) => Some(voice),
            _ => None,
        }
    }
}

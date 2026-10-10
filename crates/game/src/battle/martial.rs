//! Prepare party techniques with resolved action and feedback resources.
use super::{ActionDefinition, party::Character, voice::Sound};

mod attack;

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
    FireBall,
    DemonFang,
    RayThrust,
    Mirage,
    Destruction,
    Beast,
    Infliction,
    CrescentMoon,
    SpinKick,
    EagleDive,
    PyreSeal,
    PowerSeal,
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
        (Lloyd, 1) | (Zelos | Kratos, 207) => DemonFang,
        (Colette, 35) => RayThrust,
        (Genis, 66) => FireBall,
        (Sheena, 125) => PyreSeal,
        (Sheena, 126) => PowerSeal,
        (Presea, 154) => Destruction,
        (Presea, 163) => Infliction,
        (Presea, 172) => Beast,
        (Regal, 176) => CrescentMoon,
        (Regal, 177) => SpinKick,
        (Regal, 185) => EagleDive,
        (Regal, 201) => Mirage,
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
        (self.sequence == Sequence::FireBall).then_some(204)
    }
}

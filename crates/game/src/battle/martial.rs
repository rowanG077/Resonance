//! Prepare party techniques with resolved action and feedback resources.
use super::{ActionDefinition, party::Character, voice::Sound};
use resonance_battle::{
    AttackEvent, AttackPose, HitShape, PreparedAttack, ProjectileContact, ProjectileDefinition,
};

mod advanced_seals;
mod attack;
mod authored;
mod colette;
mod elemental;
mod fierce;
mod hammer;
mod physical;
pub(super) use physical::prepare_eagle_landing;
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

pub(crate) fn special_guard(character: Character) -> SpecialGuard {
    let index = character.index();
    SpecialGuard {
        catalogue: [34, 202, 203, 203, 204, 34, 205, 206, 34][index],
        effect: if character == Character::Genis { 2 } else { 1 },
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
    }
}

pub(crate) fn is_special_guard(character: Character, catalogue: u16) -> bool {
    special_guard(character).catalogue == catalogue
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Sequence {
    Asset,
    SpecialGuard,
    Elemental,
    Authored,
    TigerBlade,
    Tempest,
    LightningBlade,
    RisingFalcon,
    FalconsCrest,
    Spell(u16),
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

pub fn technique(character: Character, catalogue: u16) -> Option<Technique> {
    use Character::*;
    use Sequence::*;
    if let Some(variant) = resonance_content::arte::elemental::find(catalogue) {
        let mut technique = technique(character, variant.base)?;
        technique.catalogue = catalogue;
        technique.sequence = Elemental;
        return Some(technique);
    }
    let sequence = match (character, catalogue) {
        _ if is_special_guard(character, catalogue) => SpecialGuard,
        (Lloyd, 1 | 2) | (Zelos | Kratos, 207 | 208) => Authored,
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
        (Genis, 62..=86 | 88..=95)
        | (Raine, 98..=116 | 119..=123)
        | (Zelos | Kratos, 117..=118) => Spell(catalogue + 138),
        (Genis, 248) => Spell(280),
        (Zelos | Kratos, 213) => Spell(204),
        (Zelos | Kratos, 214) => Spell(205),
        (Zelos | Kratos, 215) => Spell(208),
        (Zelos | Kratos, 216) => Spell(209),
        (Zelos | Kratos, 217) => Spell(212),
        (Zelos | Kratos, 218) => Spell(215),
        (Zelos | Kratos, 219) => Spell(216),
        (Zelos | Kratos, 220) => Spell(219),
        (Zelos | Kratos, 221) => Spell(236),
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
        (Regal, 192..=194) => Spell(catalogue + 81),
        _ => return None,
    };
    Some(Technique {
        character,
        catalogue,
        sequence,
    })
}

impl Technique {
    pub(crate) fn asset(character: Character, catalogue: u16) -> Self {
        Self {
            character,
            catalogue,
            sequence: Sequence::Asset,
        }
    }
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

fn attack_definition(attack: PreparedAttack, tp_cost: u16) -> ActionDefinition {
    ActionDefinition {
        normal: None,
        tp_cost,
        execution: super::ActionExecution::Attack(attack),
    }
}

fn recovery_pose(attack: &PreparedAttack) -> Option<AttackPose> {
    attack.events.iter().find_map(|(at, event)| match event {
        AttackEvent::Pose(pose) if *at == attack.end_at => Some(*pose),
        _ => None,
    })
}

/// A stationary hit volume; each callback supplies its placement and visuals.
fn stationary_contact(
    hit: &resonance_content::battle_action::ActionHit,
    recoil: &crate::battle::recoil::Parameters,
    lifetime: u32,
    shape: HitShape,
    [radius, height]: [f32; 2],
) -> anyhow::Result<ProjectileDefinition> {
    Ok(ProjectileDefinition {
        lifetime: Some(lifetime),
        contact: Some(ProjectileContact {
            hit: crate::battle::hit::authored(hit, recoil)?,
            cooldown: hit.cooldown,
            repeat_limit: 0,
            radius,
            height,
            shape,
            offset: [0.; 3],
            radius_growth: 0.,
            height_growth: 0.,
            survives_contact: true,
            clashes: false,
        }),
        ..Default::default()
    })
}

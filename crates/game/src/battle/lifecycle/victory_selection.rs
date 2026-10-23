//! Victory celebrations chosen from the party's condition and prepared resources.
use crate::battle::results::PreparedGroup;
use resonance_battle::{Control, Sound};
use resonance_content::battle_victory::{Condition, SelectionRule};

use crate::battle::party::Character;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColetteState {
    Normal,
    Silent,
    Sealed,
    Alternate,
}

impl From<u32> for ColetteState {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Normal,
            301..=999 => Self::Silent,
            1000 => Self::Sealed,
            _ => Self::Alternate,
        }
    }
}

impl ColetteState {
    pub fn can_speak(self) -> bool {
        matches!(self, Self::Normal | Self::Alternate)
    }
}

#[derive(Debug, Clone)]
pub struct Participant {
    pub character: Character,
    pub available: bool,
    pub dead: bool,
    pub hp_percent: u8,
    pub close_to_lloyd: bool,
    pub distant_from_lloyd: bool,
    pub participation: u32,
    pub poisoned: bool,
    pub control: Control,
    pub kills: u32,
}

#[derive(Debug, Clone)]
pub struct Context<'a> {
    pub leader: Character,
    pub formation: u16,
    pub party: Vec<Participant>,
    pub colette: ColetteState,
    pub presea_recovered: bool,
    pub regal_recovered: bool,
    pub party_was_hit: bool,
    pub enemy_was_scanned: bool,
    pub level_difference: i16,
    pub enemy_count: usize,
    pub prepared_groups: &'a [PreparedGroup],
}

/// A prepared victory dialogue, independent of formation and pose selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VictoryGroupId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Solo(u8),
    Group(VictoryGroupId),
}

const INJURED_HP_PERCENT: u8 = 25;
const HEALTHY_HP_PERCENT: u8 = 75;
const STRONG_PARTICIPATION: u32 = 100;
const CHALLENGING_LEVEL_DIFFERENCE: i16 = 4;

fn fallen_companions(condition: Condition) -> &'static [Character] {
    use Character::*;
    match condition {
        Condition::RaineFallen => &[Raine],
        Condition::SheenaFallen => &[Sheena],
        Condition::RaineAndSheenaFallen => &[Raine, Sheena],
        Condition::ColetteFallen => &[Colette],
        Condition::KratosFallen => &[Kratos],
        Condition::GenisFallenBeforePreseaRecovery => &[Genis],
        _ => &[],
    }
}

impl Context<'_> {
    fn member_is(
        &self,
        character: Character,
        predicate: impl FnOnce(&Participant) -> bool,
    ) -> bool {
        self.party
            .iter()
            .find(|member| member.character == character)
            .is_some_and(predicate)
    }

    fn available(&self, character: Character) -> bool {
        self.member_is(character, |member| member.available && !member.dead)
    }

    fn close(&self, character: Character) -> bool {
        self.member_is(character, |member| member.close_to_lloyd)
    }

    fn dead(&self, character: Character) -> bool {
        self.member_is(character, |member| member.dead)
    }

    fn injured(&self, character: Character) -> bool {
        self.member_is(character, |member| member.hp_percent <= INJURED_HP_PERCENT)
    }

    fn poisoned(&self, character: Character) -> bool {
        self.member_is(character, |member| member.poisoned)
    }

    fn eligible(&self, condition: Condition) -> bool {
        use Character::*;
        let flawless = !self.party_was_hit;
        use Condition::*;
        if !fallen_companions(condition).iter().all(|&id| self.dead(id)) {
            return false;
        }
        match condition {
            SheenaAffinity => self.close(Sheena),
            GenisAffinity => self.close(Genis),
            EnemyScanned => self.enemy_was_scanned,
            LloydOutmatched => {
                self.member_is(Lloyd, |member| member.kills == 0)
                    && self.level_difference >= CHALLENGING_LEVEL_DIFFERENCE
                    && self.enemy_count > 1
            }
            ZelosAffinity => self.close(Zelos),
            Flawless => flawless,
            PreseaDistantBeforeRecovery => !self.close(Presea) && !self.presea_recovered,
            PreseaCloseAfterRecovery => self.close(Presea) && self.presea_recovered,
            GenisAndKratosDistantFlawless => flawless && !self.close(Genis) && !self.close(Kratos),
            GenisAndZelosDistantFlawless => flawless && !self.close(Genis) && !self.close(Zelos),
            ChildhoodFriendsInjured => [Lloyd, Colette, Genis]
                .into_iter()
                .all(|id| self.injured(id)),
            ColetteAffinity => self.close(Colette),
            LloydPoisoned => self.poisoned(Lloyd),
            ColetteHighParticipationFlawless => {
                flawless
                    && self.member_is(Colette, |member| {
                        member.participation >= STRONG_PARTICIPATION
                    })
            }
            ColetteFallen => self.close(Colette) && self.colette.can_speak(),
            GenisJealous => {
                self.close(Colette) && self.member_is(Genis, |member| member.distant_from_lloyd)
            }
            PreseaSheenaRainePoisoned => [Presea, Sheena, Raine]
                .into_iter()
                .all(|id| self.poisoned(id)),
            GenisInjured => self.injured(Genis),
            RegalRecovered => {
                self.regal_recovered && !self.member_is(Regal, |member| member.distant_from_lloyd)
            }
            GenisFallenBeforePreseaRecovery => !self.presea_recovered,
            PreseaDistantAfterRecoveryFlawless => {
                flawless
                    && self.presea_recovered
                    && self.member_is(Presea, |member| member.distant_from_lloyd)
            }
            LloydManualPartyAutomatic => {
                self.member_is(Lloyd, |member| member.control == Control::Manual)
                    && [Genis, Zelos, Regal]
                        .into_iter()
                        .all(|id| self.member_is(id, |member| member.control == Control::Auto))
            }
            Always | RaineFallen | SheenaFallen | RaineAndSheenaFallen | KratosFallen => true,
        }
    }
}

/// Earlier eligible exchanges have priority; each gets its own chance to speak.
pub fn select(context: &Context<'_>, mut next: impl FnMut() -> u16) -> Selection {
    let mut groups: Vec<_> = context.prepared_groups.iter().collect();
    groups.sort_by_key(|group| group.selection.priority());
    // Encounter exchanges take priority, including participants who were defeated.
    for group in &groups {
        if let SelectionRule::Encounter { formations, .. } = &group.selection
            && formations.contains(&context.formation)
            && group.participants.iter().all(|&character| {
                context
                    .party
                    .iter()
                    .any(|member| member.character == character)
            })
        {
            return Selection::Group(group.id);
        }
    }
    // Half of ordinary results proceed directly to a solo celebration.
    if next() & 1 == 0 {
        for group in groups {
            let SelectionRule::Ordinary { gate, .. } = group.selection else {
                continue;
            };
            let eligible = group
                .required_leader
                .is_none_or(|leader| leader == context.leader)
                && group.participants.iter().all(|&character| {
                    fallen_companions(group.condition).contains(&character)
                        || context.party.iter().any(|member| {
                            member.character == character && member.available && !member.dead
                        })
                })
                && (context.colette.can_speak()
                    || !group.participants.contains(&Character::Colette))
                && context.eligible(group.condition);
            if !eligible {
                continue;
            }
            if gate.accepts(next()) {
                return Selection::Group(group.id);
            }
        }
    }
    Selection::Solo(ordinary_pose(context, next()))
}

fn ordinary_pose(context: &Context<'_>, sample: u16) -> u8 {
    use Character::*;
    let varied_pose = |draw: u16| if draw.is_multiple_of(10) { 3 } else { 2 };
    if context.leader == Colette && context.colette == ColetteState::Normal {
        return varied_pose(sample);
    }
    if context.leader == Presea && !context.presea_recovered {
        return 2;
    }
    if !context.available(context.leader)
        || context.member_is(context.leader, |member| {
            member.hp_percent < INJURED_HP_PERCENT
        })
    {
        return 1;
    }
    if context
        .party
        .iter()
        .all(|member| member.available && !member.dead && member.hp_percent >= HEALTHY_HP_PERCENT)
    {
        return 0;
    }
    if context.leader == Colette || !context.party_was_hit {
        return 4;
    }
    varied_pose(sample)
}

/// Solo lines belong to the chosen celebration; only its paired variation is random.
pub fn ordinary_voice(
    character: Character,
    pose: u8,
    sample: u16,
    choices: &[[Option<Sound>; 2]],
) -> Option<Sound> {
    let (pose, variation) = match (character, pose) {
        (Character::Colette, 2) => (3, 0),
        (Character::Colette, 3) => (3, 1),
        (_, pose) => (usize::from(pose), usize::from(sample & 1)),
    };
    choices.get(pose)?[variation]
}

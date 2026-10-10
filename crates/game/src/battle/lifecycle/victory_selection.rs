//! Victory celebrations chosen from the party's condition and prepared resources.
use crate::battle::results::PreparedGroup;
use resonance_battle::Control;
use resonance_content::battle_victory::Condition;

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
    pub party: Vec<Participant>,
    pub colette: ColetteState,
    pub presea_recovered: bool,
    pub regal_recovered: bool,
    pub party_was_hit: bool,
    pub enemy_was_scanned: bool,
    pub level_difference: i16,
    pub enemy_count: usize,
    pub seen_groups: u64,
    pub prepared_groups: &'a [PreparedGroup],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub group: u8,
    pub pose: u8,
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

/// Choose once among prepared, eligible groups and an ordinary solo celebration.
/// Prefer dialogue the player has not heard yet; repeat it when all choices are familiar.
pub fn select(context: &Context<'_>, sample: u16) -> Selection {
    let mut eligible: Vec<u8> = context
        .prepared_groups
        .iter()
        .filter(|group| {
            group
                .required_leader
                .is_none_or(|leader| leader == context.leader as u8)
                && group.participants.iter().all(|&character| {
                    fallen_companions(group.condition)
                        .iter()
                        .any(|&id| id as u8 == character)
                        || context.party.iter().any(|member| {
                            member.character as u8 == character && member.available && !member.dead
                        })
                })
                && (context.colette.can_speak()
                    || !group.participants.contains(&(Character::Colette as u8)))
                && context.eligible(group.condition)
        })
        .map(|group| group.id)
        .collect();
    if eligible
        .iter()
        .any(|group| context.seen_groups & (1 << group) == 0)
    {
        eligible.retain(|group| context.seen_groups & (1 << group) == 0);
    }
    if !eligible.is_empty() {
        let choice = usize::from(sample) % (eligible.len() + 1);
        if let Some(&group) = eligible.get(choice) {
            return Selection { group, pose: 0 };
        }
    }
    Selection {
        group: 0,
        pose: ordinary_pose(context, sample),
    }
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
    if !context.available(context.leader) || context.injured(context.leader) {
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

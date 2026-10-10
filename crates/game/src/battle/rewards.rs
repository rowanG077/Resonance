//! Ordinary victory rewards. The session applies these to its candidate party
//! once, before publishing results; presentation ticks never recalculate them.
use anyhow::{Context, Result, ensure};
use resonance_battle::ActorAvailability;
use resonance_content::{
    arte::Catalogue, menu_data::Title, monster::Monster, session::SessionData,
};
use resonance_events::{GameplayRandom, party::Party};

const MAX_GALD: u32 = 99_999_999;
const GALD_FINDER_PERCENT_RANGE: u32 = 20;

/// One admitted enemy instance, in battle roster order. Excluded encounter
/// categories are removed by the caller before any reward random draws.
#[derive(Debug, Clone)]
pub struct EnemyReward {
    pub monster: u8,
    pub experience: u32,
    pub gald: u32,
    pub drops: [Option<u16>; 2],
    pub drop_chances: [u8; 2],
}

impl EnemyReward {
    pub fn from_monster(monster: &Monster, variant: usize) -> Result<Self> {
        let stats = monster
            .statistics
            .get(variant)
            .context("missing reward statistics variant")?;
        Ok(Self {
            monster: monster.id,
            experience: stats.experience,
            gald: stats.gald,
            drops: monster.drops,
            drop_chances: monster.drop_chances,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAward {
    pub item: u16,
    pub count: u16,
    /// Knowledge is awarded even when the inventory stack is already full.
    pub discoveries: Vec<(u8, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewards {
    pub experience: u32,
    pub combo_experience: u32,
    pub gald: u32,
    pub items: Vec<ItemAward>,
}

/// Successful Happiness bonuses for an active member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HappinessAward {
    /// One-based persistent character identity.
    pub character: u8,
    pub experience_success: bool,
    pub gald_success: bool,
    pub experience: u32,
    pub gald: u32,
}

/// Equipment and skill reward modifiers. Gald Finder applies after equipment multipliers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RewardModifiers {
    /// Maximum-vital growth for available active members, indexed by character and then HP/TP.
    pub maximum_vital_growth: [[bool; 2]; 9],
    /// Local EX72 or partywide EX92 takes precedence over local EX152.
    /// This bonus uses base EXP, excluding combo EXP.
    pub experience_bonus_percent: [u8; 9],
    /// Experience bonuses from each admitted member's committed equipment at reward time.
    pub equipment_experience_percent: [u8; 9],
    pub item_drop_bonus: u8,
    pub gald_one_and_a_half: bool,
    pub gald_double: bool,
    pub gald_finder: bool,
    /// Raine's luck for the enemy-drop bonus, when she is available.
    pub happiness_luck: Option<u16>,
    /// Raine's luck for recipient bonuses; each recipient's skill and availability determine eligibility.
    pub happiness_recipient_luck: Option<u16>,
    pub happiness_recipients: [bool; 9],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaximumVital {
    Hp,
    Tp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaximumVitalAward {
    pub character: u8,
    pub vital: MaximumVital,
    /// Actual base-stat increase after applying the stat cap.
    pub amount: u16,
}

pub struct RewardInput<'a> {
    pub enemies: &'a [EnemyReward],
    pub maximum_combo: u16,
    pub level_difference: i8,
    pub modifiers: RewardModifiers,
}

/// A complete reward transaction. Publish the party and random state together.
pub struct Awarded {
    pub party: Party,
    pub random: GameplayRandom,
    pub rewards: Rewards,
    pub advancement: Vec<Advancement>,
    pub maximum_vitals: Vec<MaximumVitalAward>,
    pub happiness: Vec<HappinessAward>,
    pub overflow: Vec<bool>,
}

/// Work on owned state so an invalid resource cannot publish partial rewards or random draws.
pub fn award(
    mut party: Party,
    session: &SessionData,
    techniques: &Catalogue,
    titles: &[Vec<Title>],
    input: RewardInput<'_>,
    mut random: GameplayRandom,
) -> Result<Awarded> {
    for enemy in input.enemies {
        ensure!(
            usize::from(enemy.monster) < resonance_content::monster::MONSTER_COUNT,
            "invalid reward monster"
        );
    }
    let rewards = Rewards::calculate(&input, &mut random)?;
    let overflow = rewards
        .items
        .iter()
        .map(|award| {
            let item = session
                .items
                .get(usize::from(award.item))
                .context("missing awarded item")?;
            Ok(
                u16::from(*party.items.get(&award.item).unwrap_or(&0)) + award.count
                    > u16::from(party.item_limit(item)),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let maximum_vitals = rewards.grow_vitals(&mut party, session, input.modifiers, &mut random)?;
    let (advancement, happiness) = rewards.apply(
        &mut party,
        session,
        techniques,
        titles,
        input.modifiers,
        &mut random,
    )?;
    Ok(Awarded {
        party,
        random,
        rewards,
        advancement,
        maximum_vitals,
        happiness,
        overflow,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advancement {
    /// One-based character ID, independent of its battle slot.
    pub character: u8,
    pub levels: u8,
    pub techniques: Vec<u16>,
    /// Last learned technique for each gained level.
    pub notices: Vec<u16>,
}

/// Each hit beyond the first adds 1% to base drop odds, up to double the base odds.
pub fn combo_drop_bonus(maximum_combo: u16) -> u8 {
    maximum_combo.saturating_sub(1).min(100) as u8
}

/// Scale the base drop chance before comparison.
fn drop_chance(base: u8, bonus: u8) -> u32 {
    let base = u32::from(base);
    base + base * u32::from(bonus) / 100
}

fn succeeds(chance: u32, total: u32, roll: impl FnOnce() -> u32) -> bool {
    chance != 0 && (chance >= total || roll() % total < chance)
}

fn happiness_chance(luck: u16) -> u32 {
    (u32::from(luck) / 20 + 5).min(100)
}

/// Increase and EXP Plus take precedence over the low-HP Tough bonus.
pub(super) fn experience_bonus_percent(increase: bool, tough: bool, hp: i32, maximum: i32) -> u8 {
    if increase {
        10
    } else if tough && maximum > 0 {
        ((3 - (i64::from(hp.max(0)) * 100 / i64::from(maximum)) / 25).max(0) * 5) as u8
    } else {
        0
    }
}

impl Rewards {
    fn calculate(input: &RewardInput<'_>, random: &mut GameplayRandom) -> Result<Self> {
        let RewardInput {
            enemies,
            maximum_combo,
            level_difference,
            modifiers,
        } = *input;
        ensure!(
            !enemies.is_empty()
                && enemies.len() <= resonance_battle::ENEMY_CAPACITY
                && (-8..=8).contains(&level_difference)
                && enemies.iter().all(|enemy| enemy
                    .drops
                    .iter()
                    .zip(enemy.drop_chances)
                    .all(|(item, chance)| item.is_none() || chance <= 100)),
            "invalid victory reward inputs"
        );
        let mut reward = Self {
            experience: 0,
            combo_experience: 0,
            gald: 0,
            items: Vec::new(),
        };
        let drop_bonus = combo_drop_bonus(maximum_combo);
        for enemy in enemies {
            let bonus_can_help = (0..2).any(|slot| {
                enemy.drops[slot].is_some()
                    && drop_chance(enemy.drop_chances[slot], drop_bonus)
                        + u32::from(modifiers.item_drop_bonus)
                        < 100
            });
            let mut item_bonus = modifiers.item_drop_bonus;
            if bonus_can_help
                && let Some(luck) = modifiers.happiness_luck
                && succeeds(happiness_chance(luck), 100, || random.next_u32())
            {
                item_bonus = item_bonus.saturating_add(10);
            }
            for slot in 0..2 {
                if let Some(item) = enemy.drops[slot]
                    && succeeds(
                        drop_chance(enemy.drop_chances[slot], drop_bonus) + u32::from(item_bonus),
                        100,
                        || random.next_u32(),
                    )
                {
                    let index = reward.items.iter().position(|award| award.item == item);
                    let award = if let Some(index) = index {
                        &mut reward.items[index]
                    } else {
                        reward.items.push(ItemAward {
                            item,
                            count: 0,
                            discoveries: Vec::new(),
                        });
                        reward.items.last_mut().unwrap()
                    };
                    award.count += 1;
                    award.discoveries.push((enemy.monster, slot));
                }
            }
            reward.experience = reward
                .experience
                .checked_add(enemy.experience)
                .context("victory experience overflow")?;
            reward.gald = reward
                .gald
                .checked_add(enemy.gald)
                .context("victory gald overflow")?;
        }
        if level_difference > 2 {
            let percent = (100 - 10 * (i32::from(level_difference) - 2)).max(50) as u64;
            reward.experience = (u64::from(reward.experience) * percent / 100) as u32;
        }
        reward.combo_experience = (u64::from(reward.experience) * u64::from(maximum_combo) / 100)
            .try_into()
            .context("victory combo experience overflow")?;
        if modifiers.gald_double {
            reward.gald = reward
                .gald
                .checked_mul(2)
                .context("Gald reward multiplier overflow")?;
        } else if modifiers.gald_one_and_a_half {
            reward.gald = reward
                .gald
                .checked_add(reward.gald >> 1)
                .context("Gald reward multiplier overflow")?;
        }
        if modifiers.gald_finder
            && reward.gald < MAX_GALD
            && u64::from(reward.gald) * u64::from(GALD_FINDER_PERCENT_RANGE - 1) / 100 != 0
        {
            let percent = u64::from(random.next_u32() % GALD_FINDER_PERCENT_RANGE);
            let bonus = u64::from(reward.gald) * percent / 100;
            let gald = u64::from(reward.gald)
                .checked_add(bonus)
                .context("Gald Finder reward overflow")?;
            reward.gald = gald.min(u64::from(MAX_GALD)) as u32;
        } else {
            reward.gald = reward.gald.min(MAX_GALD);
        }
        Ok(reward)
    }

    /// Roll maximum-vital growth for active members before adding experience.
    fn grow_vitals(
        &self,
        party: &mut Party,
        session: &SessionData,
        modifiers: RewardModifiers,
        random: &mut GameplayRandom,
    ) -> Result<Vec<MaximumVitalAward>> {
        let experience = self.experience.saturating_add(self.combo_experience);
        let mut awards = Vec::new();
        if experience == 0 {
            return Ok(awards);
        }
        for &id in party.formation.iter().take(4) {
            let index = usize::from(id.checked_sub(1).context("zero party character")?);
            let member = party
                .members
                .get_mut(index)
                .context("missing reward recipient")?;
            let skills = *modifiers
                .maximum_vital_growth
                .get(index)
                .context("invalid maximum-vital recipient")?;
            if member.level >= 250 || member.hp == 0 || member.ailments.petrified {
                continue;
            }
            for (index, enabled) in skills.into_iter().enumerate() {
                let before = member.base_stats[index];
                let gain = if index == 0 {
                    (before / 100) >> 1
                } else {
                    before / 100
                };
                let cap = if index == 0 { 9999 } else { 999 };
                if !enabled || gain == 0 || before >= cap {
                    continue;
                }
                let level = usize::from(member.level);
                let span = session
                    .experience
                    .get(level + 1)
                    .zip(session.experience.get(level))
                    .and_then(|(&next, &current)| next.checked_sub(current))
                    .filter(|&span| span != 0)
                    .context("invalid maximum-vital growth EXP interval")?;
                if !succeeds(experience, span, || random.next_u32()) {
                    continue;
                }
                member.base_stats[index] = before.saturating_add(gain).min(cap);
                awards.push(MaximumVitalAward {
                    character: id,
                    vital: if index == 0 {
                        MaximumVital::Hp
                    } else {
                        MaximumVital::Tp
                    },
                    amount: member.base_stats[index] - before,
                });
            }
        }
        Ok(awards)
    }

    fn apply(
        &self,
        party: &mut Party,
        session: &SessionData,
        techniques: &Catalogue,
        titles: &[Vec<Title>],
        modifiers: RewardModifiers,
        random: &mut GameplayRandom,
    ) -> Result<(Vec<Advancement>, Vec<HappinessAward>)> {
        let formation = party.formation.clone();
        ensure!(
            formation
                .iter()
                .all(|&id| (1..=9).contains(&id) && usize::from(id - 1) < party.members.len()),
            "invalid reward recipient"
        );
        let gald = i32::try_from(self.gald).context("victory gald exceeds session range")?;

        party.add_gald(gald);
        let experience = self.experience.saturating_add(self.combo_experience);
        for (slot, &id) in formation.iter().enumerate() {
            let index = usize::from(id - 1);
            let member = &mut party.members[index];
            if member.hp > 0 && !member.ailments.petrified {
                // Equipment and skill bonuses round separately and exclude combo experience.
                let bonus = if slot < 4 {
                    let base = u64::from(self.experience);
                    base * u64::from(modifiers.equipment_experience_percent[index]) / 100
                        + base * u64::from(modifiers.experience_bonus_percent[index]) / 100
                } else {
                    0
                };
                member.experience = (u64::from(member.experience) + u64::from(experience) + bonus)
                    .min(9_999_999) as u32;
            }
        }
        let mut happiness_awards = Vec::new();
        if let Some(luck) = modifiers.happiness_recipient_luck {
            // Only active recipients with Happiness receive its additional rolls.
            for &id in formation.iter().take(4) {
                {
                    let member = &party.members[usize::from(id - 1)];
                    if member.hp == 0
                        || member.ailments.petrified
                        || !modifiers.happiness_recipients[usize::from(id - 1)]
                    {
                        continue;
                    }
                }
                let exp_bonus = self.experience / 20;
                let exp_success = exp_bonus != 0
                    && party.members[usize::from(id - 1)].experience < 9_999_999
                    && succeeds(happiness_chance(luck), 100, || random.next_u32());
                let exp = if exp_success { exp_bonus } else { 0 };
                if exp_success {
                    let member = &mut party.members[usize::from(id - 1)];
                    member.experience = member.experience.saturating_add(exp).min(9_999_999);
                }
                let gald_bonus = self.gald / 20;
                let gald_success = gald_bonus != 0
                    && party.gald < MAX_GALD
                    && succeeds(happiness_chance(luck), 100, || random.next_u32());
                let gald = if gald_success { gald_bonus } else { 0 };
                if gald_success {
                    party.add_gald(
                        i32::try_from(gald).context("Happiness Gald exceeds session range")?,
                    );
                }
                if exp_success || gald_success {
                    happiness_awards.push(HappinessAward {
                        character: id,
                        experience_success: exp_success,
                        gald_success,
                        experience: exp,
                        gald,
                    });
                }
            }
        }
        let mut advancement = Vec::new();
        for (slot, &id) in formation.iter().enumerate() {
            let index = usize::from(id - 1);
            let member = &party.members[index];
            let before = member.level;
            if before >= 250
                || session
                    .experience
                    .get(usize::from(before) + 1)
                    .is_none_or(|&threshold| member.experience < threshold)
            {
                continue;
            }
            let growth = titles
                .get(index)
                .and_then(|rows| {
                    member
                        .title
                        .checked_sub(1)
                        .and_then(|id| rows.get(usize::from(id)))
                })
                .context("missing reward title growth")?
                .growth;
            let learned = party
                .gain_experience(
                    session,
                    index,
                    0,
                    growth,
                    |id| {
                        if slot >= 4 {
                            Ok(true)
                        } else {
                            techniques
                                .definition(usize::from(id))
                                .map(|row| row.learn_on_level_up)
                                .map_err(|error| error.to_string())
                        }
                    },
                    || random.next_u32(),
                )
                .map_err(anyhow::Error::msg)?;
            let levels = party.members[index].level - before;
            if levels != 0 || !learned.techniques.is_empty() {
                advancement.push(Advancement {
                    character: id,
                    levels,
                    techniques: learned.techniques,
                    notices: learned.notices,
                });
            }
        }
        for award in &self.items {
            let count = i8::try_from(award.count).context("victory item quantity exceeds limit")?;
            for &(monster, slot) in &award.discoveries {
                party.monsters.entry(monster).or_default().drops[slot] = true;
            }
            party
                .change_item(session, award.item, count)
                .map_err(anyhow::Error::msg)?;
        }
        Ok((advancement, happiness_awards))
    }
}

/// Only living, active actors with a nonzero maximum TP receive recovery.
pub fn recover_tp_for_actor(
    current: u16,
    maximum: u16,
    percent_bonus: u8,
    availability: ActorAvailability,
    hp: i32,
) -> Option<u16> {
    (availability == ActorAvailability::Active && hp > 0 && maximum != 0).then(|| {
        let amount = (u32::from(maximum) * (8 + u32::from(percent_bonus)) / 100).max(10);
        (u32::from(current) + amount).min(u32::from(maximum)) as u16
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use resonance_content::session::{CharacterDefinition, ItemDefinition, StatGrowth};
    pub(in crate::battle) fn reward_fixture() -> (SessionData, Catalogue, Vec<Vec<Title>>, Party) {
        let session = SessionData {
            rules: None,
            version: 1,
            executable_sha256: "0".repeat(64),
            experience: vec![0, 0, 10, 30],
            items: (0..51)
                .map(|_| ItemDefinition {
                    equipment_kind: None,
                    allowed_characters: 511,
                    stack_limit: 20,
                })
                .collect(),
            characters: (0..9)
                .map(|_| CharacterDefinition {
                    cooking: [0; resonance_content::menu_data::RECIPE_COUNT],
                    ex_skills: [0; 4],
                    ex_gems: [0; 4],
                    compound_ex_skills: vec![],
                    recent_compound_ex_skills: vec![],
                    technique_balance: 0,
                    affinity: 0,
                    level: 1,
                    experience: 8,
                    base_stats: [100, 20, 30, 40, 50, 60, 70],
                    luck: 10,
                    overlimit: 0,
                    equipment: [0; 6],
                    techniques: vec![],

                    allowed_techniques: vec![2, 1],
                    shortcuts: [0; 4],
                    growth: std::array::from_fn(|_| StatGrowth {
                        base: 1,
                        random: 1,
                        title_bonus: 0,
                    }),
                    level_techniques: [(2, vec![1, 2])].into(),
                })
                .collect(),
        };
        let mut techniques = Catalogue {
            definitions: vec![Default::default(); 3],
            learning: vec![],
        };
        techniques.definitions[1].learn_on_level_up = true;
        let titles = vec![
            vec![Title {
                growth: [0; 7],
                costume: None
            }];
            9
        ];
        let party = Party::new(&session, Default::default()).unwrap();
        (session, techniques, titles, party)
    }

    fn input(enemies: &[EnemyReward]) -> RewardInput<'_> {
        RewardInput {
            enemies,
            maximum_combo: 0,
            level_difference: 0,
            modifiers: Default::default(),
        }
    }

    fn enemy(experience: u32, gald: u32) -> EnemyReward {
        EnemyReward {
            monster: 36,
            experience,
            gald,
            drops: [None; 2],
            drop_chances: [0; 2],
        }
    }

    #[test]
    fn probability_boundaries_and_luck_have_the_expected_rates() {
        assert_eq!(happiness_chance(0), 5);
        assert_eq!(happiness_chance(100), 10);
        assert_eq!(happiness_chance(u16::MAX), 100);
        assert_eq!(drop_chance(49, 4), 50);
        assert_eq!(drop_chance(100, 100), 200);
        assert!(!succeeds(0, 100, || 0));
        assert!(succeeds(100, 100, || 99));
        assert!(succeeds(200, 100, || 99));
        assert!(succeeds(20, 100, || 19));
        assert!(!succeeds(20, 100, || 20));
    }

    #[test]
    fn combo_finder_and_happiness_raise_drop_chances() {
        for (hits, bonus) in [(0, 0), (1, 0), (6, 5), (101, 100), (u16::MAX, 100)] {
            assert_eq!(combo_drop_bonus(hits), bonus);
        }
        let (session, techniques, titles, party) = reward_fixture();
        for bonus in 0..3 {
            let enemies = [EnemyReward {
                drops: [Some(1), None],
                drop_chances: [if bonus == 0 { 50 } else { 90 }, 0],
                ..enemy(0, 0)
            }];
            let mut request = input(&enemies);
            match bonus {
                0 => request.maximum_combo = 101,
                1 => request.modifiers.item_drop_bonus = 10,
                _ => request.modifiers.happiness_luck = Some(u16::MAX),
            }
            let awarded = award(
                party.clone(),
                &session,
                &techniques,
                &titles,
                request,
                GameplayRandom::new(7),
            )
            .unwrap();
            assert_eq!(awarded.party.items[&1], 1);
            assert_eq!(awarded.rewards.items[0].discoveries, [(36, 0)]);
        }
    }

    #[test]
    fn bonuses_preserve_precedence_rounding_caps_and_reserve_eligibility() {
        let (mut session, techniques, titles, mut party) = reward_fixture();
        session.experience = vec![0, 0, 1000, 3000];
        party.formation = vec![1, 2, 3, 4, 5];
        party.members[1].hp = 0;
        party.members[2].ailments.petrified = true;
        let enemies = [enemy(19, 101)];
        let mut request = input(&enemies);
        request.maximum_combo = 40;
        request.modifiers.experience_bonus_percent = [10; 9];
        request.modifiers.equipment_experience_percent = [50; 9];
        request.modifiers.experience_bonus_percent[3] = 15;
        request.modifiers.equipment_experience_percent[3] = 100;
        request.modifiers.gald_double = true;
        request.modifiers.gald_one_and_a_half = true;
        let awarded = award(
            party,
            &session,
            &techniques,
            &titles,
            request,
            GameplayRandom::new(1),
        )
        .unwrap();
        assert_eq!(awarded.rewards.gald, 202);
        assert_eq!(awarded.rewards.combo_experience, 7);
        assert_eq!(
            awarded
                .party
                .members
                .iter()
                .map(|member| member.experience)
                .collect::<Vec<_>>(),
            [44, 8, 8, 55, 34, 8, 8, 8, 8]
        );
        assert!(awarded.advancement.is_empty());
        assert_eq!(experience_bonus_percent(false, true, i32::MAX, i32::MAX), 0);
        assert_eq!(
            experience_bonus_percent(false, true, i32::MAX / 2, i32::MAX),
            10
        );
        for (hp, expected) in [(1, 15), (249, 15), (250, 10), (500, 5), (750, 0), (1000, 0)] {
            assert_eq!(experience_bonus_percent(false, true, hp, 1000), expected);
            assert_eq!(experience_bonus_percent(true, true, hp, 1000), 10);
        }
    }

    #[test]
    fn level_penalty_combo_and_gald_finder_apply_to_their_base_awards() {
        let (mut session, techniques, titles, mut party) = reward_fixture();
        session.experience = vec![0, 0, 1000, 3000];
        party.formation = vec![1];
        for (level_difference, experience, combo) in [(3, 98, 8), (8, 54, 4)] {
            let enemies = [enemy(109, 101)];
            let mut request = input(&enemies);
            request.maximum_combo = 9;
            request.level_difference = level_difference;
            request.modifiers.gald_one_and_a_half = true;
            request.modifiers.gald_finder = true;
            let awarded = award(
                party.clone(),
                &session,
                &techniques,
                &titles,
                request,
                GameplayRandom::new(3),
            )
            .unwrap();
            assert_eq!(
                (awarded.rewards.experience, awarded.rewards.combo_experience),
                (experience, combo)
            );
            assert!((151..=179).contains(&awarded.rewards.gald));
        }
        let enemies = [enemy(0, 60_000_000)];
        let mut request = input(&enemies);
        request.modifiers.gald_double = true;
        request.modifiers.gald_finder = true;
        let awarded = award(
            party,
            &session,
            &techniques,
            &titles,
            request,
            GameplayRandom::new(3),
        )
        .unwrap();
        assert_eq!(awarded.rewards.gald, MAX_GALD);
        assert_eq!(awarded.party.gald, MAX_GALD);
    }

    #[test]
    fn drops_record_discoveries_even_at_capacity_and_levels_preserve_vitals() {
        let (mut session, techniques, titles, mut party) = reward_fixture();
        party.formation = vec![1, 2, 3, 4, 5];
        // Future and already learned active techniques need no catalogue row.
        session.characters[0].allowed_techniques.extend([300, 301]);
        session.characters[0].level_techniques.insert(3, vec![300]);
        session.characters[0]
            .level_techniques
            .get_mut(&2)
            .unwrap()
            .push(301);
        party.members[0].techniques.insert(301);
        // Reserve learning uses session level data without battle learning policy.
        session.characters[4].allowed_techniques.push(302);
        session.characters[4]
            .level_techniques
            .get_mut(&2)
            .unwrap()
            .push(302);
        for member in &mut party.members {
            member.hp = 73;
            member.tp = 12;
        }
        party.members[1].hp = 0;
        party.members[2].ailments.petrified = true;
        party.items.insert(1, 20);
        let enemies = [EnemyReward {
            drops: [Some(1), Some(50)],
            drop_chances: [100; 2],
            ..enemy(8, 12)
        }];
        let awarded = award(
            party,
            &session,
            &techniques,
            &titles,
            input(&enemies),
            GameplayRandom::new(1),
        )
        .unwrap();
        assert_eq!(awarded.overflow, [true, false]);
        assert_eq!(awarded.party.items[&1], 20);
        assert_eq!(awarded.party.items[&50], 1);
        assert_eq!(awarded.party.monsters[&36].drops, [true, true]);
        assert!(!awarded.party.monsters[&36].scanned);
        assert_eq!(awarded.party.members[0].shortcuts, [1, 0, 0, 0]);
        assert_eq!(awarded.party.members[4].shortcuts, [2, 1, 302, 0]);
        assert!(!awarded.party.members[0].techniques.contains(&300));
        assert!(awarded.party.members[0].techniques.contains(&301));
        assert_eq!(
            (awarded.party.members[0].hp, awarded.party.members[0].tp),
            (73, 12)
        );
        assert_eq!(
            awarded
                .advancement
                .iter()
                .map(|row| row.character)
                .collect::<Vec<_>>(),
            [1, 4, 5]
        );
    }

    #[test]
    fn happiness_and_maximum_vitals_apply_only_to_eligible_active_members() {
        let (mut session, techniques, titles, mut party) = reward_fixture();
        use resonance_content::menu_data::{
            CharacterExSkills, ExActivation, ExSkill, ExSkillData, ExTendency,
        };
        let (mut rules, _) = crate::battle::party::projection_tests::fixture().unwrap();
        rules.ex_skills = ExSkillData {
            skills: [(
                33,
                ExSkill {
                    stat_bonuses: vec![],
                    save_point_tp_cost: None,
                    tendency: Some(ExTendency::Technical),
                    activation: ExActivation::BattleEnd,
                },
            )]
            .into(),
            characters: vec![
                CharacterExSkills {
                    levels: [[33; 4]; 4],
                    compounds: vec![]
                };
                9
            ],
            gem_items: [1, 2, 3, 4, 5],
        };
        session.rules = Some(std::sync::Arc::new(rules));
        party.bind_rules(&session);
        session.experience = vec![0, 0, 100, 300];
        party.formation = vec![1, 2, 3, 4, 5];
        party.members[0].base_stats[..2].copy_from_slice(&[9990, 995]);
        party.members[1].hp = 0;
        party.members[2].ailments.petrified = true;
        for member in &mut party.members {
            member.ex_skills[0] = 33;
        }
        let enemies = [enemy(100, 200)];
        let mut request = input(&enemies);
        request.modifiers.maximum_vital_growth = [[true; 2]; 9];
        request.modifiers.happiness_recipient_luck = Some(u16::MAX);
        request.modifiers.happiness_recipients = [true; 9];
        let awarded = award(
            party,
            &session,
            &techniques,
            &titles,
            request,
            GameplayRandom::new(1),
        )
        .unwrap();
        assert!(awarded.maximum_vitals.contains(&MaximumVitalAward {
            character: 1,
            vital: MaximumVital::Hp,
            amount: 9
        }));
        assert!(awarded.maximum_vitals.contains(&MaximumVitalAward {
            character: 1,
            vital: MaximumVital::Tp,
            amount: 4
        }));
        assert!(awarded.maximum_vitals.iter().all(|row| row.character == 1));
        assert_eq!(
            awarded
                .happiness
                .iter()
                .map(|row| row.character)
                .collect::<Vec<_>>(),
            [1, 4]
        );
        assert!(
            awarded
                .happiness
                .iter()
                .all(|row| row.experience == 5 && row.gald == 10)
        );
        assert_eq!(awarded.party.gald, 220);
        assert_eq!(awarded.party.members[4].experience, 108);
    }

    #[test]
    fn reward_transactions_replay_from_the_same_state() {
        let (session, techniques, titles, party) = reward_fixture();
        let random = GameplayRandom::new(5);
        let enemies = [EnemyReward {
            drops: [Some(1), Some(50)],
            drop_chances: [30, 70],
            ..enemy(30, 100)
        }];
        let first = award(
            party.clone(),
            &session,
            &techniques,
            &titles,
            input(&enemies),
            random,
        )
        .unwrap();
        let second = award(
            party,
            &session,
            &techniques,
            &titles,
            input(&enemies),
            random,
        )
        .unwrap();
        assert_eq!(first.rewards, second.rewards);
        assert_eq!(first.random, second.random);
        assert_eq!(
            serde_json::to_value(first.party).unwrap(),
            serde_json::to_value(second.party).unwrap()
        );
    }

    #[test]
    fn result_tp_caps_actual_recovery_and_requires_a_living_active_actor() {
        assert_eq!(
            recover_tp_for_actor(28, 32, 0, ActorAvailability::Active, 100),
            Some(32)
        );
        assert_eq!(
            recover_tp_for_actor(0, 250, 5, ActorAvailability::Active, 100),
            Some(32)
        );
        for (availability, hp, maximum) in [
            (ActorAvailability::Active, 0, 250),
            (ActorAvailability::Dead, 100, 250),
            (ActorAvailability::Petrified, 100, 250),
            (ActorAvailability::Active, 100, 0),
        ] {
            assert_eq!(recover_tp_for_actor(0, maximum, 5, availability, hp), None);
        }
        assert_eq!(
            recover_tp_for_actor(249, 250, 5, ActorAvailability::Active, 100),
            Some(250)
        );
    }
}

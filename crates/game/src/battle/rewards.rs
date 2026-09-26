//! Ordinary victory rewards. The session applies these to its candidate party
//! once, before publishing results; presentation ticks never recalculate them.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    arte::Catalogue, menu_data::Title, monster::Monster, session::SessionData,
};
use resonance_events::party::Party;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advancement {
    /// One-based character ID, independent of its battle slot.
    pub character: u8,
    pub levels: u8,
    pub techniques: Vec<u16>,
    /// Last learned technique per level, in source level-growth order.
    pub notices: Vec<u16>,
}

impl Rewards {
    /// The opening route has no reward equipment/EX or new-game modifiers.
    /// Those branches must be prepared explicitly before calling this ordinary
    /// path for encounters that use them. Victory selection consumes RNG first.
    pub fn roll(
        enemies: &[EnemyReward],
        maximum_combo: u16,
        level_difference: i8,
        mut random: impl FnMut() -> u32,
    ) -> Result<Self> {
        ensure!(
            !enemies.is_empty()
                && enemies.len() <= 8
                && (-8..=8).contains(&level_difference)
                && enemies
                    .iter()
                    .all(|enemy| enemy.drop_chances.iter().all(|&chance| chance <= 100)),
            "invalid victory reward inputs"
        );
        let mut reward = Self {
            experience: 0,
            combo_experience: 0,
            gald: 0,
            items: Vec::new(),
        };
        for enemy in enemies {
            let roll = random() % 100;
            for slot in 0..2 {
                if let Some(item) = enemy.drops[slot]
                    && u32::from(enemy.drop_chances[slot]) > roll
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
        Ok(reward)
    }

    /// Vitals and combat conditions must already be copied into this candidate
    /// party. Reserve members receive the same ordinary EXP as active members.
    /// The separate growth RNG is the session's libc stream, not battle RNG.
    pub fn apply(
        &self,
        party: &mut Party,
        session: &SessionData,
        techniques: &Catalogue,
        titles: &[Vec<Title>],
        mut growth_random: impl FnMut() -> u32,
    ) -> Result<Vec<Advancement>> {
        let formation = party.formation.clone();
        let growth = formation
            .iter()
            .map(|&id| {
                let index = usize::from(id.checked_sub(1).context("zero party character")?);
                let member = party
                    .members
                    .get(index)
                    .context("missing reward recipient")?;
                let definition = session
                    .characters
                    .get(index)
                    .context("missing recipient definition")?;
                for &id in &definition.allowed_techniques {
                    techniques.definition(usize::from(id))?;
                }
                titles
                    .get(index)
                    .and_then(|rows| {
                        member
                            .title
                            .checked_sub(1)
                            .and_then(|id| rows.get(id as usize))
                    })
                    .map(|title| title.growth)
                    .context("missing reward title growth")
            })
            .collect::<Result<Vec<_>>>()?;
        let gald = i32::try_from(self.gald).context("victory gald exceeds session range")?;
        for award in &self.items {
            ensure!(
                session.items.get(usize::from(award.item)).is_some()
                    && award
                        .discoveries
                        .iter()
                        .all(|&(monster, slot)| usize::from(monster)
                            < resonance_content::monster::MONSTER_COUNT
                            && slot < 2),
                "invalid victory drop"
            );
        }

        party.add_gald(gald);
        let experience = self.experience.saturating_add(self.combo_experience);
        // All EXP additions precede the first level-growth draw in the original.
        for &id in &formation {
            let member = &mut party.members[usize::from(id - 1)];
            if member.hp > 0 && member.conditions & 0x8000_0100 == 0 {
                member.experience = member.experience.saturating_add(experience).min(9_999_999);
            }
        }
        let mut advancement = Vec::new();
        for (slot, (&id, growth)) in formation.iter().zip(growth).enumerate() {
            let index = usize::from(id - 1);
            let before = party.members[index].level;
            let learned = party
                .gain_experience(
                    session,
                    index,
                    0,
                    growth,
                    |id| {
                        slot >= 4
                            || techniques.definitions[usize::from(id)].flags & 0x8000_0000 != 0
                    },
                    &mut growth_random,
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
        Ok(advancement)
    }
}

/// Exact result-age-150 event, once per source-available active member. Returns actual
/// post-recovery TP and the nominal number displayed even near maximum TP.
pub fn recover_tp(current: u16, maximum: u16, percent_bonus: u8) -> (u16, u16) {
    let amount = (u32::from(maximum) * (8 + u32::from(percent_bonus)) / 100).max(10);
    (
        (u32::from(current) + amount).min(u32::from(maximum)) as u16,
        amount as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::session::{CharacterDefinition, ItemDefinition, StatGrowth};

    fn zombie() -> EnemyReward {
        EnemyReward {
            monster: 36,
            experience: 8,
            gald: 12,
            drops: [Some(1), Some(50)],
            drop_chances: [20, 8],
        }
    }

    #[test]
    fn opening_instances_share_each_drop_roll_and_preserve_award_order() {
        let ghost = EnemyReward {
            monster: 49,
            experience: 9,
            gald: 8,
            drops: [Some(1), Some(10)],
            drop_chances: [15, 5],
        };
        let mut draws = [4, 7].into_iter();
        let reward = Rewards::roll(&[ghost, zombie()], 1, 0, || draws.next().unwrap()).unwrap();
        assert_eq!(
            (reward.experience, reward.combo_experience, reward.gald),
            (17, 0, 20)
        );
        assert_eq!(draws.next(), None);
        assert_eq!(
            reward
                .items
                .iter()
                .map(|v| (v.item, v.count))
                .collect::<Vec<_>>(),
            [(1, 2), (10, 1), (50, 1)]
        );
        assert_eq!(reward.items[0].discoveries, [(49, 0), (36, 0)]);
        // A roll equal to the chance does not drop that slot.
        let reward = Rewards::roll(&[zombie()], 1, 0, || 8).unwrap();
        assert_eq!(reward.items.len(), 1);
        assert_eq!(reward.items[0].item, 1);
    }

    #[test]
    fn no_item_slots_still_consume_one_draw_and_level_penalty_precedes_combo() {
        let enemy = EnemyReward {
            experience: 109,
            drops: [None; 2],
            ..zombie()
        };
        let mut count = 0;
        let reward = Rewards::roll(std::slice::from_ref(&enemy), 9, 3, || {
            count += 1;
            0
        })
        .unwrap();
        assert_eq!(count, 1);
        assert!(reward.items.is_empty());
        assert_eq!((reward.experience, reward.combo_experience), (98, 8));
        let reward = Rewards::roll(&[enemy], 9, 8, || 99).unwrap();
        assert_eq!((reward.experience, reward.combo_experience), (54, 4));
    }

    #[test]
    fn result_tp_reports_nominal_recovery_and_caps_actual_amount() {
        assert_eq!(recover_tp(28, 32, 0), (32, 10));
        assert_eq!(recover_tp(44, 57, 0), (54, 10));
        assert_eq!(recover_tp(0, 250, 0), (20, 20));
        assert_eq!(recover_tp(0, 250, 5), (32, 32));
    }

    #[test]
    fn apply_awards_full_reserve_experience_without_healing_or_scanning() {
        let session = SessionData {
            ex_skills: None,
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
            combinations: vec![],
            learning_storage: [0; 5],
        };
        techniques.definitions[1].flags = 0x8000_0000;
        let titles = vec![
            vec![Title {
                name: "A".into(),
                description: String::new(),
                growth: [0; 7],
                costume: None
            }];
            9
        ];
        let mut party = Party::new(&session, Default::default()).unwrap();
        party.formation = vec![1, 2, 3, 4, 5];
        for member in &mut party.members {
            member.hp = 73;
            member.tp = 12;
        }
        party.members[1].hp = 0;
        party.members[2].conditions = 0x100;
        party.items.insert(1, 20);
        let rewards = Rewards::roll(&[zombie()], 1, 0, || 0).unwrap();
        let mut draws = 0;
        let advancement = rewards
            .apply(&mut party, &session, &techniques, &titles, || {
                draws += 1;
                1
            })
            .unwrap();
        assert_eq!(draws, 21);
        assert_eq!(
            party
                .members
                .iter()
                .map(|m| m.experience)
                .collect::<Vec<_>>(),
            [16, 8, 8, 16, 16, 8, 8, 8, 8]
        );
        assert_eq!(party.members[0].shortcuts, [1, 0, 0, 0]);
        assert_eq!(party.members[4].shortcuts, [2, 1, 0, 0]);
        assert_eq!((party.members[0].hp, party.members[0].tp), (73, 12));
        assert_eq!(party.members[0].base_stats[0], 102);
        assert_eq!(
            advancement.iter().map(|a| a.character).collect::<Vec<_>>(),
            [1, 4, 5]
        );
        assert_eq!(party.gald, 12);
        assert_eq!(party.items[&1], 20);
        assert_eq!(party.items[&50], 1);
        assert_eq!(party.monsters[&36].drops, [true, true]);
        assert!(!party.monsters[&36].scanned);
    }
}

use super::{Party, SessionData};
use anyhow::{Result, ensure};
use resonance_content::grade::{Benefit, MAX_GRADE, Shop};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub benefits: BTreeSet<Benefit>,
    pub purchases: Option<BTreeSet<Benefit>>,
    pub cleared: bool,
}

impl Party {
    pub fn record_clear(&mut self, shop: &Shop) -> Result<u8> {
        let previous = self.game_clears;
        if !self.new_game_plus.cleared {
            let refund = shop.cost(&self.new_game_plus.benefits)?;
            self.grade_hundredths = self.grade_hundredths.saturating_add(refund).min(MAX_GRADE);
            self.game_clears = self.game_clears.saturating_add(1).min(100);
            self.new_game_plus.cleared = true;
        }
        Ok(previous)
    }

    pub fn buy_new_game_plus(&mut self, shop: &Shop, purchases: BTreeSet<Benefit>) -> Result<()> {
        ensure!(
            self.new_game_plus.cleared && self.new_game_plus.purchases.is_none(),
            "Grade Shop requires a completed game"
        );
        let cost = shop.cost(&purchases)?;
        ensure!(cost <= self.grade_hundredths, "not enough Grade");
        self.grade_hundredths -= cost;
        self.new_game_plus.purchases = Some(purchases);
        Ok(())
    }

    /// Build from starting definitions, then transfer only purchased categories.
    pub fn start_new_game_plus(&mut self, data: &SessionData) -> Result<()> {
        let selected = self
            .new_game_plus
            .purchases
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("New Game Plus purchases are missing"))?;
        let has = |benefit| selected.contains(&benefit);
        let mut next = Self::new(data, self.settings.clone())?;
        next.game_clears = self.game_clears;
        next.grade_hundredths = self.grade_hundredths;
        next.new_game_plus.benefits = selected.clone();
        if has(Benefit::Gald) {
            next.gald = self.gald;
        }
        if has(Benefit::ExGems) {
            let rules = data
                .ex_skills
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("EX gem definitions are missing"))?;
            for id in rules.gem_items {
                if let Some(&count) = self.items.get(&id) {
                    next.change_item(data, id, count as i8)
                        .map_err(anyhow::Error::msg)?;
                }
            }
        }
        if has(Benefit::Recipes) {
            next.cooking.known = self.cooking.known;
        }
        if has(Benefit::Figurines) {
            next.figurines = self.figurines.clone();
            next.figurine_book_complete = self.figurine_book_complete;
        }
        if has(Benefit::MonsterList) {
            next.monsters = self.monsters.clone();
            next.monster_book_complete = self.monster_book_complete;
        }
        if has(Benefit::CollectorsBook) {
            next.found_items = self.found_items.clone();
            next.collectors_book_complete = self.collectors_book_complete;
        }
        if has(Benefit::WorldMap) {
            next.travel.visited_locations = self.travel.visited_locations.clone();
            next.travel.visited_shops = self.travel.visited_shops.clone();
        }
        if has(Benefit::BattleInfo) {
            next.battles = self.battles.clone();
        }
        for (member, previous) in next.members.iter_mut().zip(&self.members) {
            member.name = previous.name.clone();
            if has(Benefit::ExSkills) {
                member.ex_skills = previous.ex_skills;
                member.ex_gems = previous.ex_gems;
                member.compound_ex_skills = previous.compound_ex_skills.clone();
            }
            if has(Benefit::Affection) {
                member.affinity = previous.affinity;
            }
            if has(Benefit::CookingAbility) {
                member.cooking = previous.cooking;
            }
            if has(Benefit::Titles) {
                member.title = previous.title;
                member.titles = previous.titles.clone();
            }
            if has(Benefit::Tech) {
                member.techniques = previous.techniques.clone();
            }
            if has(Benefit::TechUsage) {
                member.technique_uses = previous.technique_uses.clone();
            }
            if has(Benefit::IncreasedHp) {
                const STARTING_HP_BONUS: u16 = 500;
                member.base_stats[0] = member.base_stats[0]
                    .saturating_add(STARTING_HP_BONUS)
                    .min(9999);
            }
            if has(Benefit::MinimumHp) {
                const MINIMUM_STARTING_HP: u16 = 160;
                member.base_stats[0] = MINIMUM_STARTING_HP;
            }
            [member.hp, member.tp] = member.maximum_vitals();
        }
        next.validate(data)?;
        *self = next;
        Ok(())
    }

    pub fn stack_limit(&self) -> u8 {
        if self.new_game_plus.benefits.contains(&Benefit::ThirtyItems) {
            30
        } else {
            resonance_content::session::DEFAULT_ITEM_STACK_LIMIT
        }
    }

    pub fn item_limit(&self, item: &resonance_content::session::ItemDefinition) -> u8 {
        if item.stack_limit > 1 && self.new_game_plus.benefits.contains(&Benefit::ThirtyItems) {
            item.stack_limit.max(self.stack_limit())
        } else {
            item.stack_limit
        }
    }
}

impl crate::GameWorld {
    pub(crate) fn start_new_game_plus(
        &mut self,
        memory: &mut symphonia_script_vm::Memory,
        data: &SessionData,
    ) -> Result<()> {
        let party = self
            .party
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("party is missing"))?;
        party.start_new_game_plus(data)?;
        let has = |benefit| party.new_game_plus.benefits.contains(&benefit);
        // These ranges are part of the shipped scripts' persistent-state schema.
        const MEMORY_CIRCLE_FLAGS: std::ops::RangeInclusive<u16> = 850..=898;
        const WORLD_MAP_FLAGS: std::ops::RangeInclusive<u16> = 900..=999;
        const MINIGAME_GLOBALS: std::ops::Range<u16> = 0x188..0x1b8;
        self.event_flags.retain(|flag| {
            has(Benefit::MemoryCircles) && MEMORY_CIRCLE_FLAGS.contains(flag)
                || has(Benefit::WorldMap) && WORLD_MAP_FLAGS.contains(flag)
        });
        for offset in
            (crate::persistent::STORY_GLOBALS_START..crate::persistent::GLOBAL_BYTES).step_by(4)
        {
            if !has(Benefit::MiniGames) || !MINIGAME_GLOBALS.contains(&offset) {
                memory.write(offset, symphonia_script::Width::S32, 0)?;
            }
        }
        if !has(Benefit::PlayTime) {
            self.played_ticks = 0;
            self.reset_play_time = true;
        }
        self.script_state.clear();
        self.event_records.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::grade::Purchase;

    #[test]
    fn a_new_run_keeps_only_purchased_progress_and_resets_story_and_vitals() {
        let data = crate::party::tests::data();
        for carry in [false, true] {
            let mut party = Party::new(&data, Default::default()).unwrap();
            party.game_clears = 2;
            party.grade_hundredths = 12345;
            party.gald = 789;
            party.members[0].name = Some("Player".into());
            party.members[0].hp = 1;
            party.members[0].conditions = 32;
            party.members[0].affinity = 42;
            party.members[0].techniques.insert(10);
            party.members[0].technique_uses.insert(10, 123);
            party.change_item(&data, 1, 10).unwrap();
            party.new_game_plus.purchases = Some(if carry {
                [
                    Benefit::Gald,
                    Benefit::Affection,
                    Benefit::TechUsage,
                    Benefit::IncreasedHp,
                    Benefit::PlayTime,
                    Benefit::MemoryCircles,
                    Benefit::MiniGames,
                    Benefit::ThirtyItems,
                ]
                .into()
            } else {
                Default::default()
            });
            let mut world = crate::GameWorld {
                party: Some(party),
                played_ticks: 900,
                event_flags: [850, 900, 2000].into(),
                ..Default::default()
            };
            let mut memory = symphonia_script_vm::Memory::default();
            memory
                .write(0x40, symphonia_script::Width::S32, 99_999_999)
                .unwrap();
            memory
                .write(0x188, symphonia_script::Width::S32, 200)
                .unwrap();
            world.start_new_game_plus(&mut memory, &data).unwrap();
            assert_eq!(memory.read(0x40, symphonia_script::Width::S32).unwrap(), 0);
            assert_eq!(
                memory.read(0x188, symphonia_script::Width::S32).unwrap(),
                if carry { 200 } else { 0 }
            );
            assert_eq!(world.played_ticks, if carry { 900 } else { 0 });
            assert_eq!(world.reset_play_time, !carry);
            assert_eq!(
                world.event_flags,
                if carry {
                    [850].into()
                } else {
                    Default::default()
                }
            );
            let party = world.party.as_mut().unwrap();
            assert_eq!((party.game_clears, party.grade_hundredths), (2, 12345));
            assert!(!party.new_game_plus.cleared);
            assert!(party.new_game_plus.purchases.is_none());
            assert_eq!(party.gald, if carry { 789 } else { 0 });
            assert!(party.items.is_empty());
            let member = &party.members[0];
            assert_eq!(member.name.as_deref(), Some("Player"));
            assert_eq!(member.conditions, 0);
            assert_eq!(member.hp, if carry { 600 } else { 100 });
            assert_eq!(member.affinity, if carry { 42 } else { 0 });
            assert!(member.techniques.is_empty());
            assert_eq!(
                member.technique_uses.get(&10).copied(),
                carry.then_some(123)
            );
            party.change_item(&data, 1, 50).unwrap();
            party.change_item(&data, 3, 50).unwrap();
            assert_eq!(party.items[&1], if carry { 30 } else { 20 });
            assert_eq!(party.items[&3], 1);
            party.validate(&data).unwrap();
        }
    }

    #[test]
    fn purchases_are_atomic_and_previous_benefits_are_refunded_once() {
        let shop = Shop {
            options: vec![Purchase {
                benefit: Benefit::Gald,
                price: 10,
                name: "Gald".into(),
                description: "Keep Gald".into(),
                excludes: vec![],
            }],
            labels: Default::default(),
        };
        let mut party = Party::new(&crate::party::tests::data(), Default::default()).unwrap();
        party.grade_hundredths = 250;
        party.new_game_plus.benefits.insert(Benefit::Gald);
        assert_eq!(party.record_clear(&shop).unwrap(), 0);
        party.record_clear(&shop).unwrap();
        assert_eq!((party.game_clears, party.grade_hundredths), (1, 1250));
        assert!(
            party
                .buy_new_game_plus(&shop, [Benefit::Tech].into())
                .is_err()
        );
        assert_eq!(party.grade_hundredths, 1250);
        party
            .buy_new_game_plus(&shop, [Benefit::Gald].into())
            .unwrap();
        assert_eq!(party.grade_hundredths, 250);
        assert!(
            party
                .buy_new_game_plus(&shop, [Benefit::Gald].into())
                .is_err()
        );
        assert_eq!(party.grade_hundredths, 250);
    }
}

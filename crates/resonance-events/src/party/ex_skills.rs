use super::*;
use resonance_content::menu_data::{ExSkillData, ExStatBonus};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(super) struct Rules {
    pub data: Arc<resonance_content::menu_data::MenuData>,
    pub character: usize,
}

fn skill_allowed(choices: &[[u8; 4]; 4], level: u8, skill: u8) -> bool {
    match level {
        1..=4 => choices[usize::from(level - 1)].contains(&skill),
        5 => choices.iter().flatten().any(|&id| id == skill),
        _ => false,
    }
}

impl Member {
    pub fn active_compound_ex(&self, data: &ExSkillData, character: usize) -> Vec<u8> {
        data.characters[character]
            .equipped_compounds(&self.ex_skills)
            .filter_map(|(i, _)| {
                self.compound_ex_skills
                    .contains(&(i as u8))
                    .then_some(i as u8)
            })
            .collect()
    }

    pub(super) fn ex_bonuses<'a>(
        &'a self,
        data: Option<&'a ExSkillData>,
        character: usize,
    ) -> impl Iterator<Item = &'a ExStatBonus> {
        assert!(
            data.is_some() || self.ex_skills == [0; 4],
            "EX skill rules were not prepared"
        );
        data.into_iter().flat_map(move |data| {
            self.ex_skills
                .into_iter()
                .filter(|&id| id != 0)
                .chain(
                    data.characters[character]
                        .equipped_compounds(&self.ex_skills)
                        .map(|(_, recipe)| recipe.skill),
                )
                .flat_map(move |id| &data.skills[&id].stat_bonuses)
        })
    }

    pub(super) fn validate_ex(
        &self,
        data: Option<&ExSkillData>,
        character: usize,
    ) -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        if self.ex_gems == [0; 4]
            && self.ex_skills == [0; 4]
            && self.compound_ex_skills.is_empty()
            && self.recent_compound_ex_skills.is_empty()
        {
            return Ok(());
        }
        let data = data.context("saved EX skills require prepared rules")?;
        let choices = &data.characters[character].levels;
        ensure!(
            self.ex_gems
                .iter()
                .zip(self.ex_skills)
                .enumerate()
                .all(|(slot, (&level, id))| level <= 5
                    && (id == 0
                        || skill_allowed(choices, level, id)
                            && !self.ex_skills[..slot].contains(&id)))
                && self
                    .compound_ex_skills
                    .iter()
                    .all(|&i| usize::from(i) < data.characters[character].compounds.len())
                && self
                    .recent_compound_ex_skills
                    .is_subset(&self.compound_ex_skills),
            "invalid saved EX skills for character {character}"
        );
        Ok(())
    }
}

impl Party {
    pub fn has_ex_skill(&self, character: u8, skill: u8) -> bool {
        self.formation.contains(&character)
            && character
                .checked_sub(1)
                .and_then(|index| self.members.get(usize::from(index)))
                .is_some_and(|member| member.ex_skills.contains(&skill))
    }

    /// Sheena gives the party a luck-dependent opportunity to pass a field enemy.
    pub(crate) fn evade_field_encounter(
        &self,
        data: &resonance_content::menu_data::MenuData,
        random: &mut impl FnMut() -> u32,
    ) -> bool {
        use resonance_content::menu_data::ex_effect::ENCOUNTER_EVASION;
        self.has_ex_skill(5, ENCOUNTER_EVASION)
            && (random() % 101) * 3 < u32::from(self.members[4].stats(data).luck) * 2
    }

    /// Called only while moving; Colette slows Holy Bottle consumption in both worlds.
    pub fn step_encounter_modifier(&mut self, tick: u32) {
        use resonance_content::menu_data::ex_effect::HOLY_BOTTLE_DURATION;
        let extended = self.has_ex_skill(2, HOLY_BOTTLE_DURATION);
        if let Some(modifier) = &mut self.encounter_modifier {
            if modifier.rate == 1 && extended && tick % 2 != 0 {
                return;
            }
            modifier.remaining = modifier.remaining.saturating_sub(1);
            if modifier.remaining == 0 {
                self.encounter_modifier = None;
            }
        }
    }

    /// Town/dungeon movement effects. Overworld travel only advances bottle duration.
    pub fn step_field_ex(
        &mut self,
        data: &SessionData,
        field: u32,
        random: &mut impl FnMut() -> u32,
    ) -> Result<Option<u16>, String> {
        use resonance_content::menu_data::ex_effect::{FIELD_FORAGING, FIELD_RECOVERY};
        const RECOVERY_INTERVAL: u16 = 256;
        const FORAGING_CYCLE: u16 = 4096;
        const EARLY_FORAGE: u16 = 512;
        self.field_steps = (self.field_steps + 1) % FORAGING_CYCLE;
        if self.field_steps % RECOVERY_INTERVAL == 0 && self.has_ex_skill(9, FIELD_RECOVERY) {
            let member = &mut self.members[8];
            let maximum = member.maximum_vitals()[0];
            member.hp = member.hp.saturating_add(maximum / 100).min(maximum);
        }
        if !matches!(self.field_steps, 0 | EARLY_FORAGE) || !self.has_ex_skill(7, FIELD_FORAGING) {
            return Ok(None);
        }
        let rules = data
            .rules
            .as_ref()
            .ok_or("EX skill rules were not prepared")?;
        let forage = &rules.ex_skills.foraging;
        let Some(choices) = forage.fields.get(&field) else {
            return Ok(None);
        };
        let pool = &forage.pools[choices[if choices.len() == 1 {
            0
        } else {
            random() as usize % choices.len()
        }]];
        let item = pool[random() as usize % pool.len()];
        Ok(self.change_item(data, item, 1)?.then_some(item))
    }

    /// Lloyd's Personal benefits the field leader while Lloyd is in the party.
    pub fn field_movement_multiplier(&self) -> f32 {
        use resonance_content::menu_data::ex_effect::FIELD_MOBILITY;
        if self.has_ex_skill(1, FIELD_MOBILITY) {
            1.35
        } else {
            1.
        }
    }

    pub fn bind_rules(&mut self, session: &SessionData) {
        for (character, member) in self.members.iter_mut().enumerate() {
            member.rules = session.rules.clone().map(|data| Rules { data, character });
        }
    }

    /// Replacing a gem consumes the new one and destroys the old one.
    pub fn set_ex_gem(
        &mut self,
        session: &SessionData,
        member: usize,
        slot: usize,
        level: u8,
    ) -> Result<bool, String> {
        let rules = &session
            .rules
            .as_ref()
            .ok_or("EX skill rules were not prepared")?
            .ex_skills;
        let target = self.members.get(member).ok_or("unknown party member")?;
        let old = *target.ex_gems.get(slot).ok_or("unknown EX gem slot")?;
        if !(1..=5).contains(&level) || old == level {
            return Ok(false);
        }
        let item = rules.gem_items[usize::from(level - 1)];
        if self.items.get(&item).copied().unwrap_or(0) == 0 {
            return Ok(false);
        }
        self.change_item(session, item, -1)?;
        self.bind_rules(session);
        let target = &mut self.members[member];
        target.ex_gems[slot] = level;
        target.ex_skills[slot] = 0;
        target.clamp_vitals();
        Ok(true)
    }

    pub fn set_ex_skill(
        &mut self,
        session: &SessionData,
        member: usize,
        slot: usize,
        skill: u8,
    ) -> Result<bool, String> {
        let data = &session
            .rules
            .as_ref()
            .ok_or("EX skill rules were not prepared")?
            .ex_skills;
        let target = self.members.get(member).ok_or("unknown party member")?;
        let level = *target.ex_gems.get(slot).ok_or("unknown EX skill slot")?;
        let choices = &data.characters[member].levels;
        if skill == 0 || target.ex_skills.contains(&skill) || !skill_allowed(choices, level, skill)
        {
            return Ok(false);
        }
        self.bind_rules(session);
        let target = &mut self.members[member];
        target.ex_skills[slot] = skill;
        target.clamp_vitals();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exploration_data() -> SessionData {
        let mut data = super::super::tests::data();
        let mut menu = super::super::tests::technique_menu();
        for id in [18, 34, 44, 53] {
            menu.ex_skills
                .skills
                .insert(id, menu.ex_skills.skills[&31].clone());
        }
        menu.ex_skills.foraging = resonance_content::menu_data::FieldForaging {
            pools: vec![vec![1, 2], vec![3]],
            fields: [(100, vec![0, 1])].into(),
        };
        data.rules = Some(Arc::new(menu));
        data
    }

    #[test]
    fn colette_extends_only_holy_bottles_while_present_and_equipped() {
        let mut party = Party::new(&exploration_data(), Default::default()).unwrap();
        party.formation = vec![1, 2];
        party.members[1].ex_skills[0] = 18;
        party.encounter_modifier = Some(EncounterModifier {
            rate: 1,
            remaining: 3,
        });
        for tick in 0..4 {
            party.step_encounter_modifier(tick);
        }
        assert_eq!(party.encounter_modifier.as_ref().unwrap().remaining, 1);
        party.step_encounter_modifier(4);
        assert!(party.encounter_modifier.is_none());
        for (rate, present, equipped) in [(2, true, true), (1, false, true), (1, true, false)] {
            party.formation = if present { vec![2] } else { vec![1] };
            party.members[1].ex_skills[0] = if equipped { 18 } else { 0 };
            party.encounter_modifier = Some(EncounterModifier { rate, remaining: 1 });
            party.step_encounter_modifier(1);
            assert!(party.encounter_modifier.is_none());
        }
    }

    #[test]
    fn movement_healing_and_foraging_obey_cadence_membership_and_inventory_limits() {
        let data = exploration_data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.formation = vec![1, 7, 9];
        party.members[6].ex_skills[0] = 44;
        party.members[8].ex_skills[0] = 53;
        party.members[8].hp = 98;
        for _ in 0..255 {
            assert_eq!(
                party
                    .step_field_ex(&data, 100, &mut || panic!("too early to forage"))
                    .unwrap(),
                None
            );
        }
        assert_eq!(party.members[8].hp, 98);
        party.step_field_ex(&data, 100, &mut || 1).unwrap();
        assert_eq!(party.members[8].hp, 99);
        for _ in 256..511 {
            party.step_field_ex(&data, 100, &mut || 1).unwrap();
        }
        assert_eq!(party.step_field_ex(&data, 100, &mut || 1).unwrap(), Some(3));
        assert_eq!(party.members[8].hp, 100);
        assert_eq!(party.items[&3], 1);
        party.field_steps = 4095;
        assert_eq!(
            party.step_field_ex(&data, 100, &mut || 1).unwrap(),
            None,
            "full stack"
        );
        party.field_steps = 511;
        assert_eq!(
            party
                .step_field_ex(&data, 101, &mut || panic!("field has no food"))
                .unwrap(),
            None
        );
        party.formation = vec![1];
        party.members[8].hp = 98;
        party.field_steps = 511;
        assert_eq!(
            party
                .step_field_ex(&data, 100, &mut || panic!("Presea absent"))
                .unwrap(),
            None
        );
        assert_eq!(party.members[8].hp, 98);
    }

    #[test]
    fn sheena_evasion_uses_luck_and_requires_party_membership() {
        let data = exploration_data();
        let menu = data.rules.as_ref().unwrap();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.formation = vec![1, 5];
        assert!(!party.evade_field_encounter(menu, &mut || panic!("skill absent")));
        party.members[4].ex_skills[0] = 34;
        party.members[4].luck = 75;
        assert!(party.evade_field_encounter(menu, &mut || 49));
        assert!(!party.evade_field_encounter(menu, &mut || 50));
        party.members[4].luck = 151;
        assert!(party.evade_field_encounter(menu, &mut || 100));
        party.formation = vec![1];
        assert!(!party.evade_field_encounter(menu, &mut || panic!("Sheena absent")));
    }

    #[test]
    fn personal_benefits_any_leader_only_while_lloyd_is_present_and_equipped() {
        use resonance_content::menu_data::ex_effect::FIELD_MOBILITY;
        let mut party = Party::new(&super::super::tests::data(), Default::default()).unwrap();
        party.formation = vec![1, 2];
        assert_eq!(party.field_movement_multiplier(), 1.);
        party.members[0].ex_skills[0] = FIELD_MOBILITY;
        assert_eq!(party.field_movement_multiplier(), 1.35);
        party.field_leader = 2;
        assert_eq!(party.field_movement_multiplier(), 1.35);
        party.formation = vec![2];
        assert_eq!(party.field_movement_multiplier(), 1.);
        party.formation.push(1);
        party.members[0].ex_skills[0] = 0;
        assert_eq!(party.field_movement_multiplier(), 1.);
    }
}

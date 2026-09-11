use super::*;
use resonance_content::menu_data::{ExSkillData, ExStatBonus};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(super) struct Rules {
    pub data: Arc<ExSkillData>,
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
            .compounds
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                (self.compound_ex_skills.contains(&(i as u8))
                    && c.required.iter().all(|s| self.ex_skills.contains(s)))
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
                    self.active_compound_ex(data, character)
                        .into_iter()
                        .map(move |i| data.characters[character].compounds[usize::from(i)].skill),
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
    pub fn bind_ex_skills(&mut self, session: &SessionData) {
        for (character, member) in self.members.iter_mut().enumerate() {
            member.ex_rules = session
                .ex_skills
                .clone()
                .map(|data| Rules { data, character });
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
        let rules = session
            .ex_skills
            .as_ref()
            .ok_or("EX skill rules were not prepared")?;
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
        self.bind_ex_skills(session);
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
        let data = session
            .ex_skills
            .as_ref()
            .ok_or("EX skill rules were not prepared")?;
        let target = self.members.get(member).ok_or("unknown party member")?;
        let level = *target.ex_gems.get(slot).ok_or("unknown EX skill slot")?;
        let choices = &data.characters[member].levels;
        if skill == 0 || target.ex_skills.contains(&skill) || !skill_allowed(choices, level, skill)
        {
            return Ok(false);
        }
        self.bind_ex_skills(session);
        let target = &mut self.members[member];
        target.ex_skills[slot] = skill;
        target.clamp_vitals();
        Ok(true)
    }
}

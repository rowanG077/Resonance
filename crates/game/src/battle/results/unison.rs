//! Unison page edits borrow the suspended battle candidate.
use super::*;
use crate::menu::unison::{Input, Page, UNLOCK_STORY, Unison, Visit};
use resonance_battle::BattlePhase;

impl Candidate {
    pub fn unison_page<'a>(&'a self, state: &'a Unison) -> Page<'a> {
        state.page(self.menu_party(), &self.session, &self.menus)
    }

    pub(in crate::battle) fn begin_unison(
        &mut self,
        battle: &Battle,
        remembered: usize,
    ) -> Result<Unison> {
        ensure!(
            battle.phase() == BattlePhase::Combat,
            "U. Attack requires active combat"
        );
        ensure!(
            self.setup.story >= UNLOCK_STORY as u32,
            "U. Attack is story locked"
        );
        self.begin_menu(battle)?;
        Unison::opening(remembered, self.menu_party())
    }

    pub(in crate::battle) fn step_unison(
        &mut self,
        battle: &mut Battle,
        state: &mut Unison,
        input: Input,
    ) -> Result<Visit> {
        if input == Some(crate::menu::MenuAction::Confirm)
            && state.focus == crate::menu::unison::Focus::List
            && self.unison_page(state).selection().is_some_and(|selected| {
                Self::tech_actor(&self.setup, selected.character)
                    .ok()
                    .and_then(|actor| {
                        battle.prepared_technique(
                            actor,
                            resonance_battle::TechniqueId(selected.technique),
                        )
                    })
                    .is_none()
            })
        {
            return Ok(Visit {
                cue: Some(4),
                changed: false,
                closed: false,
            });
        }
        let setup = &self.setup;
        let visit = state.step_with_edit(
            input,
            self.menu_party
                .as_mut()
                .context("Unison page has no party")?,
            &self.session,
            &self.menus,
            |party, edit| Self::apply_tech_edit(setup, battle, party, edit),
        )?;
        if visit.changed {
            self.publish_menu_edits();
        }
        Ok(visit)
    }
}

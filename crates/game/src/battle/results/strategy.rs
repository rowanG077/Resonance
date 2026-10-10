//! Apply Strategy page edits to the suspended battle candidate.
use super::*;
use crate::menu::strategy::{Input, Page, Strategy, Visit};
use resonance_battle::StrategyRefresh;

impl Candidate {
    pub fn strategy_page<'a>(&'a self, state: &'a Strategy) -> Page<'a> {
        state.page(&self.party, &self.menus)
    }

    pub(in crate::battle) fn begin_strategy(&mut self, battle: &Battle) -> Result<()> {
        self.sync_party(battle)
    }

    pub(in crate::battle) fn step_strategy(
        &mut self,
        state: &mut Strategy,
        input: Input,
    ) -> Result<Visit> {
        state.step(input, &mut self.party, &self.menus)
    }

    pub(in crate::battle) fn finish_strategy(&mut self, battle: &mut Battle) -> Result<()> {
        let rows: Vec<_> = self
            .setup
            .actors
            .iter()
            .map(|&(actor, character)| StrategyRefresh {
                actor,
                choices: self.party.members[usize::from(character - 1)].strategy,
            })
            .collect();
        battle.refresh_strategy(&rows)
    }
}

//! Apply Strategy page edits to the suspended battle candidate.
use super::*;
use crate::menu::strategy::{Input, Page, Strategy, Visit};
use resonance_battle::StrategyRefresh;

impl Candidate {
    pub fn order_name(&self, order: QuickOrder) -> Result<&str> {
        let Some(index) = order.preset_index() else {
            return Ok("Cancel orders");
        };
        if let Some(presets) = &self.party.strategy_presets {
            Ok(&presets[index].name)
        } else {
            Ok(&self
                .menus
                .strategy_text()?
                .presets
                .as_ref()
                .context("Strategy presets were not prepared")?[index])
        }
    }

    pub(in crate::battle) fn active_order(&self) -> QuickOrder {
        self.active_order
    }

    pub fn strategy_page<'a>(&'a self, state: &'a Strategy) -> Page<'a> {
        state.page(self.menu_party(), &self.menus)
    }

    pub(in crate::battle) fn begin_strategy(&mut self, battle: &Battle) -> Result<()> {
        self.begin_menu(battle)
    }

    pub(in crate::battle) fn step_strategy(
        &mut self,
        state: &mut Strategy,
        input: Input,
    ) -> Result<Visit> {
        let visit = state.step(
            input,
            self.menu_party
                .as_mut()
                .context("Strategy page has no party")?,
            &self.menus,
        )?;
        if visit.changed {
            self.publish_menu_edits();
        }
        Ok(visit)
    }

    pub(in crate::battle) fn finish_strategy(&mut self, battle: &mut Battle) -> Result<()> {
        self.apply_order(battle, self.active_order)
    }

    pub(in crate::battle) fn apply_order(
        &mut self,
        battle: &mut Battle,
        order: QuickOrder,
    ) -> Result<()> {
        let preset = order.preset_index().map(|index| {
            self.party
                .strategy_presets
                .as_ref()
                .map_or(&self.menus.strategy.presets[index], |presets| {
                    &presets[index].members
                })
        });
        let rows: Vec<_> = self
            .setup
            .actors
            .iter()
            .map(|&(actor, character)| StrategyRefresh {
                actor,
                choices: preset.map_or(self.party.members[character.index()].strategy, |members| {
                    members[character.index()]
                }),
            })
            .collect();
        battle.refresh_strategy(&rows)?;
        self.active_order = order;
        Ok(())
    }
}

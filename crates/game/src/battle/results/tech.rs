//! Tech edits validate against live battle state before changing the party or page.
use super::*;
use crate::menu::techniques::{
    BattlePage, Context, Edit, EditResult, Page, Step, TargetKind, Tech, Visit,
};
use anyhow::{Context as _, Result, ensure};
use resonance_battle::{ActorId, Battle, BattlePhase, Control};

impl Candidate {
    pub fn battle_tech_view<'a>(
        &'a self,
        state: &'a Tech,
        connected: &'a [bool; 4],
        battle: &'a Battle,
    ) -> Page<'a> {
        state.page(
            &self.party,
            &self.session,
            &self.menus,
            Context::Battle {
                battle,
                actors: &self.setup.actors,
                connected,
            },
        )
    }

    pub(in crate::battle) fn tech_actor(setup: &Setup, character: usize) -> Result<ActorId> {
        let character = u8::try_from(
            character
                .checked_add(1)
                .context("Tech character index overflow")?,
        )?;
        setup
            .actors
            .iter()
            .find(|&&(_, saved)| saved == character)
            .map(|&(actor, _)| actor)
            .context("Tech character has no live actor")
    }

    pub(in crate::battle) fn queue_tech_target(
        &mut self,
        battle: &mut Battle,
        target: crate::battle::command::TechTarget,
    ) -> Result<bool> {
        let Some(selected) = target.selected else {
            return Ok(false);
        };
        let Some(prepared) = battle.prepared_technique(target.actor, target.technique) else {
            return Ok(false);
        };
        battle.queue_technique_target_from(target.actor, prepared.action, selected, target.issuer)
    }

    pub(in crate::battle) fn begin_tech(
        &mut self,
        battle: &mut Battle,
        remembered: usize,
        connected: [bool; 4],
    ) -> Result<BattlePage> {
        ensure!(
            battle.phase() == BattlePhase::Combat,
            "Tech requires active combat"
        );
        self.sync_party(battle)?;
        let state = Tech::opening(
            remembered,
            Context::Battle {
                battle,
                actors: &self.setup.actors,
                connected: &connected,
            },
            &self.party,
            &self.session,
            &self.menus,
        )?;
        Ok(BattlePage { state, connected })
    }

    pub(super) fn apply_tech_edit(
        setup: &Setup,
        battle: &mut Battle,
        party: &mut resonance_events::party::Party,
        edit: Edit,
    ) -> Result<EditResult> {
        match edit {
            Edit::Control { slot, value } => {
                ensure!(
                    slot < party.formation.len().min(4) && value < 3,
                    "invalid Tech control edit"
                );
                let actor = setup
                    .actors
                    .get(slot)
                    .map(|&(actor, _)| actor)
                    .context("Tech control slot has no live actor")?;
                let mode = match value {
                    0 => Control::Manual,
                    1 => Control::SemiAuto,
                    _ => Control::Auto,
                };
                let changed = party.settings.battle_controls[slot] != value;
                battle.set_control_mode(actor, mode)?;
                party.settings.battle_controls[slot] = value;
                Ok(EditResult { changed, cue: None })
            }
            Edit::Shortcut {
                member,
                slot,
                selected,
            } => {
                ensure!(slot < 6, "invalid Tech shortcut slot");
                let owner = Self::tech_actor(setup, member)?;
                if slot < 4 {
                    let replacement = selected
                        .map(|shortcut| -> Result<_> {
                            ensure!(
                                shortcut.character == member,
                                "player shortcut must keep its owner"
                            );
                            battle
                                .prepared_technique(owner, shortcut.technique)
                                .map(|entry| entry.action)
                                .context("Tech shortcut action is not prepared")
                        })
                        .transpose()?;
                    let edit = battle.prepare_shortcut(owner, slot, replacement)?;
                    let changed = party
                        .assign_technique(member, slot, selected)
                        .map_err(anyhow::Error::msg)?;
                    edit.commit();
                    Ok(EditResult { changed, cue: None })
                } else {
                    let replacement = selected
                        .map(|shortcut| -> Result<_> {
                            let actor = Self::tech_actor(setup, shortcut.character)?;
                            let entry = battle
                                .prepared_technique(actor, shortcut.technique)
                                .context("Tech assist action is not prepared")?;
                            Ok((actor, entry.action))
                        })
                        .transpose()?;
                    let edit = battle.prepare_assist_shortcut(owner, slot - 4, replacement)?;
                    let changed = party
                        .assign_technique(member, slot, selected)
                        .map_err(anyhow::Error::msg)?;
                    edit.commit();
                    Ok(EditResult { changed, cue: None })
                }
            }
            Edit::Enabled {
                member,
                technique,
                enabled,
            } => {
                let target = party
                    .members
                    .get(member)
                    .context("Tech member is missing")?;
                ensure!(
                    target.techniques.contains(&technique),
                    "cannot enable an unlearned technique"
                );
                let changed = target.disabled_techniques.contains(&technique) == enabled;
                if changed {
                    let actor = Self::tech_actor(setup, member)?;
                    let action = battle
                        .prepared_technique(actor, technique)
                        .context("Tech enabled action is not prepared")?
                        .action;
                    battle.set_technique_enabled(actor, action, enabled)?;
                    if enabled {
                        party.members[member].disabled_techniques.remove(&technique);
                    } else {
                        party.members[member].disabled_techniques.insert(technique);
                    }
                }
                Ok(EditResult { changed, cue: None })
            }
            Edit::FieldForget { .. } | Edit::FieldCast { .. } => {
                anyhow::bail!("field Tech edits are unavailable in battle")
            }
        }
    }

    pub(in crate::battle) fn step_tech(
        &mut self,
        battle: &mut Battle,
        page: &mut BattlePage,
        issuer: ActorId,
        input: crate::menu::Input,
    ) -> Result<Visit> {
        let focus = page.state.focus;
        let step = page.state.request_step(
            input,
            &self.party,
            &self.session,
            &self.menus,
            Context::Battle {
                battle,
                actors: &self.setup.actors,
                connected: &page.connected,
            },
        )?;
        let visit = match step {
            Step::Navigate(visit) => visit,
            Step::Edit(edit) => {
                let result = Self::apply_tech_edit(&self.setup, battle, &mut self.party, edit)?;
                page.state.finish_edit(
                    edit,
                    result,
                    &self.party,
                    &self.session,
                    &self.menus,
                    Context::Battle {
                        battle,
                        actors: &self.setup.actors,
                        connected: &page.connected,
                    },
                )
            }
        };
        if let Some(exit) = visit.exit
            && let Some(technique) = exit.technique
        {
            let actor = Self::tech_actor(&self.setup, exit.member)?;
            let prepared = *battle
                .prepared_technique(actor, technique)
                .context("Tech queue action is not prepared")?;
            let accepted = match prepared.capabilities.target {
                TargetKind::Enemy => battle.technique_queue_admitted(actor, prepared.action)?,
                kind => {
                    let target = if kind == TargetKind::SelfTarget {
                        Some(actor)
                    } else {
                        self.setup.actors.get(exit.target).map(|row| row.0)
                    };
                    if let Some(target) = target {
                        battle.queue_technique_target_from(
                            actor,
                            prepared.action,
                            target,
                            if kind == TargetKind::SelfTarget {
                                actor
                            } else {
                                issuer
                            },
                        )?
                    } else {
                        false
                    }
                }
            };
            if !accepted {
                page.state.focus = focus;
                return Ok(Visit {
                    cue: Some(4),
                    changed: false,
                    exit: None,
                });
            }
        }
        Ok(visit)
    }
}

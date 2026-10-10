//! Recognition, transition primitives and explicit terminal ownership. The game
//! authors the result timeline; recognition never destroys the live battle.
use crate::conditions::{Condition, ConditionSet};
use crate::state::ActorTask;
use crate::{ActorId, Battle, BattleFrame, BattleOutcome, BattleResult, Cue, Side};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattlePhase {
    Entry,
    Combat,
    Ending,
    Results,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    FadeOut,
    FadeIn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionFrame {
    pub color: [u8; 3],
    pub alpha: u8,
}

struct Transition {
    frame: TransitionFrame,
    kind: TransitionKind,
    speed: u8,
}

#[derive(Default)]
pub(crate) struct Terminal {
    pub result: Option<BattleResult>,
    // Complete one camera update when recognizing results. Later ending updates only rebuild
    // matrices.
    pub ordinary_camera_finished: bool,
    retired: bool,
    escape: Option<bool>,
    transition: Option<Transition>,
    completion: Option<Arc<()>>,
}

impl Battle {
    pub fn phase(&self) -> BattlePhase {
        if self.ended {
            BattlePhase::Finished
        } else if self.terminal.retired {
            BattlePhase::Results
        } else if self.terminal.result.is_some() {
            BattlePhase::Ending
        } else if self.entry_remaining > 0 {
            BattlePhase::Entry
        } else {
            BattlePhase::Combat
        }
    }

    /// Availability determines result eligibility independently of HP and motion completion.
    /// Check before the world update.
    pub fn recognize_result(&mut self) -> Option<BattleResult> {
        if self.phase() == crate::BattlePhase::Entry {
            return None;
        }
        if self.ended || self.terminal.result.is_some() {
            return self.terminal.result;
        }
        // Only an active spell transition postpones recognition; a released spell slot alone
        // does not.
        if self.target_selector_active() {
            return None;
        }
        let unavailable = |side| {
            self.actors.iter().any(|actor| actor.side == side)
                && self
                    .actors
                    .iter()
                    .filter(|actor| actor.side == side)
                    .all(|actor| !actor.available())
        };
        self.terminal.result = if self.terminal.escape == Some(true) {
            Some(BattleResult::Escaped)
        } else if unavailable(Side::Party) {
            Some(BattleResult::Defeat)
        } else if unavailable(Side::Enemy) {
            Some(BattleResult::Victory)
        } else if self.terminal.escape == Some(false) {
            Some(BattleResult::Escaped)
        } else {
            None
        };
        if self.terminal.result.is_some() {
            // Idle actors stop when combat ends; AI no longer owns their movement.
            for (actor, runtime) in self.actors.iter_mut().zip(&self.runtime) {
                if actor.available()
                    && matches!(runtime.task(), ActorTask::None)
                    && !actor.guard.active
                    && actor.guard.recovery == 0
                {
                    actor.movement.forward = 0.;
                    actor.movement.locomotion = crate::Locomotion::Idle;
                }
            }
        }
        if self.terminal.result == Some(BattleResult::Escaped) {
            self.enter_escape();
        }
        self.terminal.result
    }

    /// Called after Escape succeeds.
    pub fn recognize_escape(&mut self, forced: bool) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat,
            "battle is already ending"
        );
        self.terminal.escape = Some(forced);
        Ok(())
    }

    pub fn forced_escape(&self) -> bool {
        self.terminal.escape == Some(true)
    }

    /// Retire combat effects and released spells while retaining actor commands, models, voices,
    /// and random state.
    pub fn retire_combat(&mut self) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == BattlePhase::Ending,
            "combat retirement is not pending"
        );
        let mut cues = Vec::new();
        for (action, _) in std::mem::take(&mut self.volleys) {
            cues.push(Cue::Interrupted { action });
        }
        self.projectiles.clear();
        self.retire_weapon_flights();
        cues.push(Cue::CombatRetired);
        self.terminal.retired = true;
        self.terminal.ordinary_camera_finished = true;
        Ok(cues)
    }

    /// Hide enemies after title awards regardless of death fade. Their ordinary lifecycle
    /// continues until cleanup.
    pub fn hide_result_enemies(&mut self) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "enemy result hiding outside results"
        );
        for (index, actor) in self.actors.iter_mut().enumerate() {
            if actor.side == Side::Enemy {
                actor.availability = crate::ActorAvailability::Dead;
                self.model_requests.push(crate::ModelRequest::Result {
                    actor: ActorId(index as u8),
                    visible: false,
                });
            }
        }
        Ok(())
    }

    /// Replace party combat controls with result controls. Select pose before normalizing
    /// availability; preserve zero HP and petrification.
    pub fn reset_result_actor(&mut self, id: ActorId) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "actor reset outside results"
        );
        ensure!(
            self.actor(id)?.side == Side::Party,
            "result reset needs a party actor"
        );
        let index = id.index();
        // End Hourglass without changing its screen transition.
        self.clear_hourglass();
        let mut cues = Vec::new();
        self.interrupt_actor(id, &mut cues);
        self.runtime[index].companion_policy = None;
        self.runtime[index].control = None;
        self.weapon_flights.retain(|&(owner, _), _| owner != id);
        let actor = &mut self.actors[index];
        actor.position[1] = 0.;
        actor.hit_stop = 0;
        actor.movement = crate::Movement::default();
        actor.guard.active = false;
        if actor.availability != crate::ActorAvailability::Petrified {
            actor.availability = crate::ActorAvailability::Active;
        }
        self.model_requests.push(crate::ModelRequest::Result {
            actor: id,
            visible: true,
        });

        Ok(cues)
    }

    /// Hide the selected Colette leader’s weapons after restoring result actors. Retain
    /// attachments and playback.
    pub fn hide_result_weapons(&mut self, id: ActorId) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "weapon visibility outside results"
        );
        ensure!(
            self.actor(id)?.side == Side::Party,
            "result weapons need a party actor"
        );
        self.model_requests
            .push(crate::ModelRequest::PrimaryWeapons {
                actor: id,
                visible: false,
            });
        Ok(())
    }

    /// Reload prepared party state without replacing its result animation or controller; support
    /// KO and petrification.
    #[allow(clippy::too_many_arguments)]
    pub fn refresh_result_member(
        &mut self,
        id: ActorId,
        hp: i32,
        max_hp: i32,
        tp: u16,
        max_tp: u16,
        overlimit: crate::OverLimit,
        petrified: bool,
        layers: crate::conditions::Layers,
    ) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results && self.actor(id)?.side == Side::Party,
            "member refresh outside party results"
        );
        let mut conditions = self.actor(id)?.conditions.clone();
        conditions.reload_layers(layers);
        self.refresh_result_member_conditions(
            id, hp, max_hp, tp, max_tp, overlimit, petrified, conditions,
        )
    }

    /// Publish a fully prepared result reload without entering combat cure or
    /// recovery controllers. Callers prepare every recipient before spending.
    #[allow(clippy::too_many_arguments)]
    pub fn refresh_result_member_conditions(
        &mut self,
        id: ActorId,
        hp: i32,
        max_hp: i32,
        tp: u16,
        max_tp: u16,
        overlimit: crate::OverLimit,
        petrified: bool,
        conditions: crate::conditions::Conditions,
    ) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results && self.actor(id)?.side == Side::Party,
            "member refresh outside party results"
        );
        self.set_actor_vitals(id, hp, max_hp, tp, max_tp)?;
        let actor = &mut self.actors[id.index()];
        actor.conditions = conditions;
        if !actor
            .conditions
            .effective()
            .intersects(ConditionSet::of(&[Condition::Quartz, Condition::Enchanted]))
        {
            actor.elements.enchantment = None;
        }
        actor.overlimit = overlimit;
        actor.availability = if hp == 0 {
            crate::ActorAvailability::Dead
        } else if petrified {
            crate::ActorAvailability::Petrified
        } else {
            crate::ActorAvailability::Active
        };

        Ok(())
    }

    pub fn set_actor_vitals(
        &mut self,
        actor: ActorId,
        hp: i32,
        maximum_hp: i32,
        tp: u16,
        maximum_tp: u16,
    ) -> Result<()> {
        ensure!(
            !self.ended && maximum_hp > 0 && (0..=maximum_hp).contains(&hp) && tp <= maximum_tp,
            "invalid result vitals"
        );
        let actor = self
            .actors
            .get_mut(actor.index())
            .context("unknown result actor")?;
        actor.hp = hp;
        actor.equipment.max_hp = maximum_hp;
        actor.tp = tp;
        actor.equipment.max_tp = maximum_tp;
        Ok(())
    }

    /// Start a screen fade without consuming either random stream.
    pub fn begin_transition(
        &mut self,
        kind: TransitionKind,
        color: [u8; 3],
        speed: u8,
    ) -> Result<()> {
        ensure!(!self.ended && speed != 0, "invalid battle transition");
        self.terminal.transition = Some(Transition {
            frame: TransitionFrame {
                color,
                alpha: if kind == TransitionKind::FadeIn {
                    255
                } else {
                    0
                },
            },
            kind,
            speed,
        });
        Ok(())
    }

    /// Advance the screen transition, reporting when its owner may continue.
    pub fn advance_transition(&mut self) -> Result<bool> {
        let transition = self
            .terminal
            .transition
            .as_mut()
            .context("no battle transition")?;
        match transition.kind {
            TransitionKind::FadeIn => {
                transition.frame.alpha = transition.frame.alpha.saturating_sub(transition.speed);
                Ok(transition.frame.alpha == 0)
            }
            TransitionKind::FadeOut => {
                transition.frame.alpha = transition.frame.alpha.saturating_add(transition.speed);
                Ok(transition.frame.alpha == 255)
            }
        }
    }

    pub fn transition(&self) -> Option<&TransitionFrame> {
        self.terminal
            .transition
            .as_ref()
            .map(|transition| &transition.frame)
    }

    /// Retire live task and weapon ownership before issuing the completed result.
    pub fn finish_result(&mut self) -> Result<BattleFrame> {
        ensure!(!self.ended, "battle outcome was already consumed");
        let result = self
            .terminal
            .result
            .context("battle has no recognized result")?;
        self.invalidate();
        let completion = Arc::new(());
        self.terminal.completion = Some(completion.clone());
        let mut frame = self.publish(Vec::new());
        frame.outcome = Some(BattleOutcome { result, completion });
        Ok(frame)
    }

    pub fn owns_outcome(&self, outcome: &BattleOutcome) -> bool {
        self.ended
            && self.terminal.result == Some(outcome.result)
            && self
                .terminal
                .completion
                .as_ref()
                .is_some_and(|completion| Arc::ptr_eq(completion, &outcome.completion))
    }

    /// Stop active work and invalidate any outcome. A successful finish issues
    /// its completion only after this cleanup; a failed update stays terminal.
    pub fn invalidate(&mut self) {
        self.terminal.completion = None;
        self.items.pending = None;
        self.pending_cues.clear();
        self.volleys.clear();
        self.projectiles.clear();
        self.retire_weapon_flights();
        for index in 0..self.runtime.len() {
            self.set_task(index, ActorTask::None);
        }
        self.timed_hold = None;
        self.ended = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{actor, prepared};

    #[test]
    fn cooking_member_reload_preserves_movement_and_restores_availability() {
        let mut battle = prepared(vec![actor(Side::Party)], 0).finish().unwrap();
        assert!(
            battle
                .refresh_result_member(
                    ActorId(0),
                    50,
                    100,
                    10,
                    20,
                    crate::OverLimit::new(50).unwrap(),
                    false,
                    Default::default(),
                )
                .is_err()
        );
        battle.recognize_escape(true).unwrap();
        battle.recognize_result().unwrap();
        battle.retire_combat().unwrap();
        let movement = battle.actors[0].movement;
        for (hp, petrified, expected) in [
            (50, false, crate::ActorAvailability::Active),
            (50, true, crate::ActorAvailability::Petrified),
            (0, true, crate::ActorAvailability::Dead),
        ] {
            battle.actors[0].availability = crate::ActorAvailability::Dead;
            battle
                .refresh_result_member(
                    ActorId(0),
                    hp,
                    100,
                    10,
                    20,
                    crate::OverLimit::new(70).unwrap(),
                    petrified,
                    Default::default(),
                )
                .unwrap();
            let actor = &battle.actors[0];
            assert_eq!(actor.availability, expected);
            assert_eq!(actor.movement, movement);
            assert_eq!(actor.overlimit.charge(), 70);
            assert!(!actor.overlimit.is_active());
        }
    }
}

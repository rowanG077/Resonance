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

/// Why the party leaves without victory rewards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepartureReason {
    Escape,
    ForcedEscape,
    EnemyRetreat,
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
    departure: Option<DepartureReason>,
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
        } else if self.enemy_presentation.opening()
            || self.entry_remaining > crate::prepare::ENTRY_SETTLE_UPDATES
        {
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
        if self.target_selector_active() || self.unison.active() {
            return None;
        }
        if self.enemy_presentation.delays_result(&self.actors) {
            return None;
        }
        if self.terminal.departure.is_none()
            && self.phase() == BattlePhase::Combat
            && !self.clock.paused()
            && !self.is_paused()
            && self
                .actors
                .iter()
                .zip(&self.prepared.actor_setup)
                .any(|(actor, setup)| {
                    actor.side == Side::Enemy
                        && actor.available()
                        && setup.enemy_decision.as_ref().is_some_and(|decision| {
                            matches!(decision.behavior.as_ref(),
                                Some(crate::EnemyBehavior::Retreat { hp_percent, after_ticks })
                                if actor.hp_percent() <= i16::from(*hp_percent)
                                    || self.ledger.elapsed_ticks >= *after_ticks)
                        })
                })
        {
            self.terminal.departure = Some(DepartureReason::EnemyRetreat);
        }
        let unavailable = |side| {
            self.actors.iter().any(|actor| actor.side == side)
                && self
                    .actors
                    .iter()
                    .filter(|actor| actor.side == side)
                    .all(|actor| !actor.available())
        };
        self.terminal.result = if matches!(
            self.terminal.departure,
            Some(DepartureReason::ForcedEscape | DepartureReason::EnemyRetreat)
        ) {
            Some(BattleResult::Escaped)
        } else if unavailable(Side::Party) {
            Some(BattleResult::Defeat)
        } else if unavailable(Side::Enemy) {
            Some(BattleResult::Victory)
        } else if self.terminal.departure == Some(DepartureReason::Escape) {
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
        if self.terminal.result == Some(BattleResult::Victory) {
            self.enemy_presentation.victory();
        }
        self.terminal.result
    }

    /// Called after Escape succeeds.
    pub fn recognize_escape(&mut self, forced: bool) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat,
            "battle is already ending"
        );
        self.terminal.departure = Some(if forced {
            DepartureReason::ForcedEscape
        } else {
            DepartureReason::Escape
        });
        Ok(())
    }

    pub fn forced_escape(&self) -> bool {
        self.terminal.departure == Some(DepartureReason::ForcedEscape)
    }

    pub fn departure_reason(&self) -> Option<DepartureReason> {
        self.terminal.departure
    }

    /// Some enemies retain a complete defeat performance before the victory wash.
    pub fn victory_animation_finished(&self) -> bool {
        self.prepared
            .actor_setup
            .iter()
            .enumerate()
            .filter(|(index, setup)| {
                setup.wait_for_defeat_animation && self.actors[*index].side == Side::Enemy
            })
            .all(|(index, _)| {
                self.models
                    .as_ref()
                    .and_then(|models| models.main_motion_observation(ActorId(index as u8)))
                    .is_none_or(|motion| motion.finished)
            })
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

    /// Keep the encounter's boss alive until its event owner releases the hold.
    /// This also runs while the event has paused combat.
    pub fn hold_event_boss_hp(&mut self, actor: Option<ActorId>) -> Result<()> {
        ensure!(!self.ended, "event health hold after battle completion");
        if let Some(id) = actor {
            let actor = self.actor(id)?;
            ensure!(
                actor.side == crate::Side::Enemy && actor.equipment.max_hp >= 2,
                "invalid event health hold actor"
            );
        }
        self.event_boss = actor;
        if let Some(actor) = actor {
            self.restore_event_boss_hp(actor);
        }
        Ok(())
    }

    pub(crate) fn restore_event_boss_hp(&mut self, actor: ActorId) -> bool {
        if self.event_boss != Some(actor) {
            return false;
        }
        let actor = &mut self.actors[actor.index()];
        actor.hp = actor.hp.max(actor.equipment.max_hp / 2);
        true
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
                let complete = transition.frame.alpha == 0;
                transition.frame.alpha = transition.frame.alpha.saturating_sub(transition.speed);
                Ok(complete)
            }
            TransitionKind::FadeOut => {
                let complete = transition.frame.alpha == 255;
                transition.frame.alpha = transition.frame.alpha.saturating_add(transition.speed);
                Ok(complete)
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
        self.summon_entry = Default::default();
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
    fn victory_waits_only_for_an_authored_defeat_performance() -> Result<()> {
        use crate::{
            ActorAvailability, ActorSetup, BattleClock, BattleInput, Models, PreparedBattle,
        };
        let mut enemy = actor(Side::Enemy);
        enemy.hp = 0;
        enemy.availability = ActorAvailability::Dead;
        let mut battle = PreparedBattle::new(
            vec![
                (actor(Side::Party), ActorSetup::default()),
                (
                    enemy.clone(),
                    ActorSetup {
                        wait_for_defeat_animation: true,
                        ..Default::default()
                    },
                ),
                (enemy, ActorSetup::default()),
            ],
            Default::default(),
            1,
        )?
        .finish()?;
        let model = |resource, duration| {
            let mut definition =
                crate::tests::model_definition(resource, [(0, 100.), (1, duration)]);
            definition.reactions.defeated = Some(1);
            Some(Arc::new(definition))
        };
        battle.set_models(Models::new(
            battle.actors(),
            vec![None, model(1, 3.), model(2, 100.)],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));
        assert!(!battle.victory_animation_finished());
        for _ in 0..40 {
            let mut frame = battle.step(BattleInput::default())?;
            battle.advance_models(&mut frame, BattleClock::Running, true)?;
            if battle.victory_animation_finished() {
                let models = battle.models().unwrap();
                let defeated = models.main_motion_observation(ActorId(1)).unwrap();
                assert_eq!(defeated.clip, 1);
                assert!(defeated.finished);
                assert!(!models.main_motion_observation(ActorId(2)).unwrap().finished);
                return Ok(());
            }
        }
        anyhow::bail!("authored defeat performance did not finish")
    }

    #[test]
    fn enemy_retreat_uses_health_or_elapsed_time_and_waits_while_paused() -> Result<()> {
        use crate::{ActionKey, ActionRequest, BattleInput, EnemyBehavior, PreparedBattle};

        for (maximum, hp, retreats) in [
            (40_000, 20_400, false),
            (40_000, 20_399, true),
            (80_000, 20_400, true),
        ] {
            let mut enemy = actor(Side::Enemy);
            enemy.hp = maximum;
            enemy.equipment.max_hp = maximum;
            let mut prepared = PreparedBattle::new(
                vec![
                    (actor(Side::Party), Default::default()),
                    (enemy, Default::default()),
                ],
                vec![crate::tests::cast(80, 0)].into(),
                1,
            )?;
            crate::tests::assign_action(&mut prepared, 1, ActionKey(0));
            prepared.resources.actor_setup[1]
                .enemy_decision
                .as_mut()
                .unwrap()
                .behavior = Some(EnemyBehavior::Retreat {
                hp_percent: 50,
                after_ticks: 3600,
            });
            let mut battle = prepared.with_entry().finish()?;
            for _ in 0..crate::prepare::ENTRY_RESUME_UPDATE {
                battle.step(BattleInput::default())?;
            }
            assert_eq!(
                battle.ledger.elapsed_ticks,
                u32::from(crate::prepare::ENTRY_RESUME_UPDATE)
            );
            assert!(battle.ledger.combat_ticks < battle.ledger.elapsed_ticks);
            battle.step(BattleInput {
                actions: vec![ActionRequest {
                    actor: ActorId(1),
                    action: ActionKey(0),
                    target: ActorId(0),
                }],
                ..Default::default()
            })?;
            assert!(battle.casting_remaining(ActorId(1)).is_some());
            battle.actors[1].hp = hp;
            battle.step(BattleInput {
                paused: true,
                ..Default::default()
            })?;
            assert_eq!(battle.recognize_result(), None);
            assert!(battle.casting_remaining(ActorId(1)).is_some());

            let party_hp = battle.actors[0].hp;
            let combat_ticks = battle.ledger().combat_ticks;
            battle.step(BattleInput::default())?;
            assert_eq!(
                battle.recognize_result(),
                retreats.then_some(BattleResult::Escaped)
            );
            assert_eq!(
                battle.departure_reason(),
                retreats.then_some(DepartureReason::EnemyRetreat)
            );
            assert_eq!(battle.casting_remaining(ActorId(1)).is_none(), retreats);
            assert_eq!(battle.actors[0].hp, party_hp);
            assert_eq!(
                battle.ledger().combat_ticks,
                combat_ticks + u32::from(!retreats)
            );
            assert!(battle.ledger().ordinary_escape.is_none());
            assert!(!battle.forced_escape());

            if !retreats {
                battle.ledger.elapsed_ticks = 3599;
                assert_eq!(battle.recognize_result(), None);
                battle.step(BattleInput {
                    paused: true,
                    ..Default::default()
                })?;
                assert_eq!(battle.ledger.elapsed_ticks, 3599);
                assert_eq!(battle.recognize_result(), None);
                battle.step(BattleInput::default())?;
                assert_eq!(battle.ledger.elapsed_ticks, 3600);
                assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
                assert_eq!(
                    battle.departure_reason(),
                    Some(DepartureReason::EnemyRetreat)
                );
                assert_eq!(battle.phase(), BattlePhase::Ending);
                assert!(battle.casting_remaining(ActorId(1)).is_some());
                let combat_ticks = battle.ledger.combat_ticks;
                battle.step(BattleInput::default())?;
                assert!(battle.casting_remaining(ActorId(1)).is_none());
                assert_eq!(battle.ledger.combat_ticks, combat_ticks);
                assert_eq!(battle.actors[0].hp, party_hp);
                assert!(battle.ledger.ordinary_escape.is_none());
            }
        }
        Ok(())
    }

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

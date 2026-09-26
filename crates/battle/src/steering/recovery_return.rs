//! Ordinary recovery destination callback 301A4 / 2A9CC / 3068C / 2F788.
use super::*;
use crate::{Activity, Cue, MotionBinding};
use anyhow::Context;

/// Bound motion roles and ordinary profile operands, independent of actor IDs.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryReturnDefinition {
    pub actor: ActorId,
    pub motion: Option<MotionBinding>,
    pub stop: Option<MotionBinding>,
    pub speed: f32,
    /// Profile+24, used independently of the selected walking speed.
    pub run_speed: f32,
    pub motion_rate: f32,
    pub turn_ticks: u8,
    /// The enemy difficulty/descriptor condition in 2A9CC.
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Moving,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReturnState {
    phase: Phase,
    age: i16,
    visited: bool,
}

impl PreparedBattle {
    pub fn with_recovery_returns(
        mut self,
        definitions: Vec<RecoveryReturnDefinition>,
    ) -> Result<Self> {
        for definition in definitions {
            let index = definition.actor.index();
            ensure!(
                index < self.actors.len()
                    && matches!(self.actors[index].control, Control::Auto | Control::Enemy),
                "recovery return needs an automatic actor"
            );
            ensure!(
                self.recovery_returns[index].is_none(),
                "duplicate recovery return binding"
            );
            ensure!(
                [
                    definition.speed,
                    definition.run_speed,
                    definition.motion_rate
                ]
                .iter()
                .all(|value| value.is_finite() && *value > 0.)
                    && definition.turn_ticks != 0,
                "invalid recovery return parameters"
            );
            for motion in [definition.motion, definition.stop].into_iter().flatten() {
                self.models[index]
                    .as_ref()
                    .context("recovery return motion needs an actor model")?
                    .duration(motion)?;
            }
            self.recovery_returns[index] = Some(definition);
        }
        Ok(self)
    }
}

impl Battle {
    fn controlled_target(&self) -> Option<ActorId> {
        self.actors
            .iter()
            .position(|actor| actor.side == Side::Party && actor.control != Control::Auto)
            .or_else(|| {
                self.actors
                    .iter()
                    .position(|actor| actor.side == Side::Party)
            })
            .and_then(|index| self.target(ActorId(index as u8)))
    }

    /// Handles only the reason5 return branch. The caller performs direct idle
    /// recovery otherwise; both 2A9CC outcomes perform exactly one shared reset.
    pub(crate) fn begin_recovery_return(
        &mut self,
        owner: ActorId,
        _cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = owner.index();
        let Some(definition) = self.prepared.recovery_returns[index] else {
            return Ok(false);
        };
        if !matches!(self.actors[index].control, Control::Auto | Control::Enemy)
            || self.controlled_target() == Some(owner)
            || self.controls[index]
                .as_ref()
                .is_some_and(|control| control.chained_technique.is_some())
        {
            return Ok(false);
        }
        if self.actors[index].side == Side::Party {
            let strategy = self.prepared.companions[index]
                .as_ref()
                .context("missing recovery return position policy")?
                .strategy[2];
            ensure!(
                (2..=6).contains(&strategy),
                "recovery position policy needs original grid selector validation"
            );
            // 22CDC compares finite planar distance against zero and therefore
            // cannot reject this destination for the opening policies2..6.
            self.actors[index].movement.steering.home = self.formation_home(owner)?.0;
        }
        let home = self.actors[index].movement.steering.home;
        let close = distance::length([
            home[0] - self.actors[index].position[0],
            0.,
            home[2] - self.actors[index].position[2],
        ]) < 45.;
        let row_disabled = self.enemy_selected[index]
            .and_then(|selected| {
                self.prepared.enemy_decisions[index]
                    .as_ref()
                    .and_then(|enemy| enemy.choices.get(selected))
            })
            .is_some_and(|row| row.requirements & 0x20 != 0);
        self.reset_ordinary_actor(owner);
        if close || definition.disabled || row_disabled {
            self.play_idle_pose(owner, 8)?;
        } else {
            if let Some(motion) = definition.motion {
                self.models[index].as_mut().unwrap().play(
                    motion,
                    0.,
                    definition.motion_rate,
                    true,
                    4,
                )?;
            }
            self.actors[index]
                .guard
                .reset_auto_chance(50, &mut self.random);
            self.actors[index].reaction.idle_initialization = None;
            self.actors[index].activity = Activity::Approaching;
            self.actors[index].movement.steering.returning = Some(ReturnState {
                phase: Phase::Moving,
                age: 0,
                visited: false,
            });
        }
        // 301A4 does this even when 2A9CC chose the close-distance idle branch.
        self.actors[index].movement.steering.side = approach_side(&self.actors[index], home);
        let target = self
            .target(owner)
            .context("recovery return requires a target")?;
        let destination = self.steer_approach(owner, target, home)?;
        let actor = &mut self.actors[index];
        actor.movement.direction =
            distance::planar_direction(destination, actor.position, actor.movement.direction);
        if actor.movement.hover_ready() {
            let direction = actor.movement.direction;
            crate::control::face_cached(actor, direction, 180. / f32::from(definition.turn_ticks));
        }
        Ok(true)
    }

    pub(crate) fn advance_recovery_return(&mut self, owner: ActorId) -> Result<()> {
        let index = owner.index();
        let Some(mut returning) = self.actors[index].movement.steering.returning.take() else {
            return Ok(());
        };
        if !self.actors[index].available() || self.actors[index].activity != Activity::Approaching {
            return Ok(());
        }
        returning.visited = true;
        if returning.phase == Phase::Moving {
            if self.models[index]
                .as_ref()
                .is_none_or(|model| !model.blending())
            {
                returning.age = returning.age.wrapping_add(1);
            }
            let target = self
                .target(owner)
                .context("recovery return requires a target")?;
            let destination =
                self.steer_approach(owner, target, self.actors[index].movement.steering.home)?;
            let actor = &mut self.actors[index];
            let desired =
                distance::planar_direction(destination, actor.position, actor.movement.direction);
            if distance::length(desired) > 0.1 {
                let desired = distance::normalize(desired);
                actor.movement.direction = distance::normalize(std::array::from_fn(|i| {
                    actor.movement.direction[i] * (1. - 0.1_f32) + desired[i] * 0.1_f32
                }));
            }
            actor.facing_direction = actor.movement.direction;
        }
        self.actors[index].movement.steering.returning = Some(returning);
        Ok(())
    }

    pub(crate) fn recovery_return_hover(&self, index: usize) -> Option<bool> {
        self.actors[index]
            .movement
            .steering
            .returning
            .filter(|state| state.visited)
            .map(|state| state.phase == Phase::Moving)
    }

    pub(crate) fn recovery_return_blocks_home(&self, index: usize) -> bool {
        !self.actors[index]
            .movement
            .steering
            .returning
            .is_some_and(|state| state.phase == Phase::Moving)
    }

    /// 3068C faces after hovering, then accelerates, before constraining.
    pub(crate) fn after_recovery_return_integration(&mut self, index: usize) -> Result<()> {
        if !self.actors[index]
            .movement
            .steering
            .returning
            .is_some_and(|state| state.visited && state.phase == Phase::Moving)
        {
            return Ok(());
        }
        let definition = self.prepared.recovery_returns[index].unwrap();
        let actor = &mut self.actors[index];
        if actor.movement.hover_ready() {
            let direction = actor.movement.direction;
            crate::control::face_cached(actor, direction, 180. / f32::from(definition.turn_ticks));
        }
        actor.movement.forward = (actor.movement.forward + 0.5).min(definition.speed);
        Ok(())
    }

    pub(crate) fn finish_recovery_return(&mut self, index: usize) -> Result<()> {
        let Some(mut returning) = self.actors[index].movement.steering.returning.take() else {
            return Ok(());
        };
        if !returning.visited {
            self.actors[index].movement.steering.returning = Some(returning);
            return Ok(());
        }
        let definition = self.prepared.recovery_returns[index].unwrap();
        if returning.phase == Phase::Stopping {
            let unfinished = definition.stop.is_some()
                && self.models[index]
                    .as_ref()
                    .is_some_and(|model| !model.finished());
            let actor = &mut self.actors[index];
            let braked = actor
                .movement
                .brake(actor.position[1], actor.activity, false, unfinished);
            if braked || actor.movement.flying || actor.movement.fixed_height {
                self.reset_ordinary_actor(ActorId(index as u8));
                self.play_idle_pose(ActorId(index as u8), 8)?;
                return Ok(());
            }
        } else {
            let actor = &self.actors[index];
            let home = actor.movement.steering.home;
            let close =
                distance::length([home[0] - actor.position[0], 0., home[2] - actor.position[2]])
                    <= actor.movement.forward / 0.55 + 4. * definition.run_speed;
            let near_enemy = returning.age > 60
                && self.target_gaps[index] < 60.
                && self
                    .actors
                    .iter()
                    .any(|other| other.side != actor.side && other.available());
            let pending = self.controls[index]
                .as_ref()
                .is_some_and(|control| control.chained_technique.is_some());
            if close
                || near_enemy
                || returning.age > 360
                || actor.movement.steering.arena_contact() != ArenaContact::None
                || self.controlled_target() == Some(ActorId(index as u8))
                || pending
            {
                returning.phase = Phase::Stopping;
                if let Some(stop) = definition.stop {
                    self.models[index]
                        .as_mut()
                        .unwrap()
                        .play(stop, 0., 0.5, false, 8)?;
                }
            }
            returning.age = returning.age.wrapping_add(1);
        }
        self.actors[index].movement.steering.returning = Some(returning);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BattleInput, EnemyChoice, EnemyDecisionDefinition,
        tests::{actor, prepared},
    };
    use std::sync::Arc;

    fn battle() -> Battle {
        let mut leader = actor(Side::Party);
        leader.position = [-500., 0., -400.];
        let mut other = actor(Side::Enemy);
        other.position = [-300., 0., 400.];
        let mut owner = actor(Side::Enemy);
        owner.control = Control::Enemy;
        owner.position = [400., 0., 0.];
        let mut prepared =
            Arc::try_unwrap(prepared("pub task run() {}", vec![leader, other, owner], 1)).unwrap();
        let row = |requirements| EnemyChoice {
            action: 99,
            weight: 1,
            requirements,
            target_policy: 1,
            guard_chance: 0,
            combo_at: i16::MAX as u16,
            followup_chance: 0,
            range: [0, 1000],
            tp: 0,
            approach_minimum: 0.,
            approach_range: 100.,
        };
        prepared.enemy_decisions[2] = Some(EnemyDecisionDefinition {
            actor: ActorId(2),
            strategy: 1,
            difficulty: 0,
            choices: vec![row(0), row(0x20)],
            back_row: vec![],
            walk_speed: 3.,
            turn_ticks: 8,
            body_flags: 0,
        });
        prepared.enemy_selected[2] = Some(0);
        let prepared = prepared
            .with_recovery_returns(vec![RecoveryReturnDefinition {
                actor: ActorId(2),
                motion: None,
                stop: None,
                speed: 3.,
                run_speed: 6.,
                motion_rate: 0.5,
                turn_ticks: 8,
                disabled: false,
            }])
            .unwrap();
        let mut battle = Battle::new(Arc::new(prepared));
        battle.actors[2].position[0] = 0.;
        battle.actors[2].activity = Activity::Recovering;
        battle
    }

    #[test]
    fn recovery_return_resets_then_draws_guard50_and_integrates_on_next_visit() {
        let mut battle = battle();
        let mut expected = battle.random;
        expected.next(); // 2B18C guard75.
        let guard = (50_i16 + (expected.next() as i16) % 5) as u8;
        assert!(
            battle
                .begin_recovery_return(ActorId(2), &mut vec![])
                .unwrap()
        );
        assert_eq!(battle.random.state(), expected.state());
        assert_eq!(battle.actors[2].guard.auto_chance, guard);
        assert_eq!(battle.actors[2].activity, Activity::Approaching);
        assert!(battle.actors[2].reaction.idle_initialization.is_none());
        assert_eq!(battle.recovery_return_hover(2), None);
        assert!(!battle.recovery_return_blocks_home(2));
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[2].position[0], 0.);
        assert_eq!(battle.actors[2].movement.forward, 0.5);
        assert_eq!(battle.actors[2].movement.steering.returning.unwrap().age, 2);
        battle.step(BattleInput::default()).unwrap();
        assert!((battle.actors[2].position[0] - 0.5).abs() < 0.000001);
        assert_eq!(battle.actors[2].movement.forward, 1.);
    }

    #[test]
    fn only_current_enemy_row_can_disable_roaming_and_close_still_runs_one_reset() {
        let mut battle = battle();
        battle.enemy_selected[2] = Some(1);
        let mut expected = battle.random;
        expected.next();
        assert!(
            battle
                .begin_recovery_return(ActorId(2), &mut vec![])
                .unwrap()
        );
        assert_eq!(battle.random.state(), expected.state());
        assert_eq!(battle.actors[2].activity, Activity::Idle);
        assert_eq!(battle.actors[2].reaction.idle_initialization, Some(false));
        assert!(battle.actors[2].movement.steering.returning.is_none());
        battle.enemy_selected[2] = Some(0);
        battle.actors[2].position[0] = 390.;
        expected.next();
        assert!(
            battle
                .begin_recovery_return(ActorId(2), &mut vec![])
                .unwrap()
        );
        assert_eq!(battle.random.state(), expected.state());
        assert_eq!(battle.actors[2].activity, Activity::Idle);
    }

    #[test]
    fn close_recovery_at_the_edge_does_not_need_an_unused_detour_side() -> Result<()> {
        let mut battle = battle();
        battle.actors[2].position = [700., 0., 0.];
        battle.actors[2].movement.steering.home = battle.actors[2].position;
        let mut expected = battle.random;
        expected.next(); // The ordinary close branch still resets guard75.

        assert!(battle.begin_recovery_return(ActorId(2), &mut vec![])?);

        assert_eq!(battle.random.state(), expected.state());
        let actor = &battle.actors[2];
        assert_eq!(actor.activity, Activity::Idle);
        assert_eq!(actor.reaction.idle_initialization, Some(false));
        assert!(actor.movement.steering.returning.is_none());
        assert_eq!(actor.movement.steering.side, DetourSide::Unproved);
        assert_eq!(actor.position, [700., 0., 0.]);
        Ok(())
    }

    #[test]
    fn controlled_target_skips_return_without_rng_and_stop_resets_once() {
        let mut battle = battle();
        battle.targets[0] = ActorId(2);
        let state = battle.random.state();
        assert!(
            !battle
                .begin_recovery_return(ActorId(2), &mut vec![])
                .unwrap()
        );
        assert_eq!(battle.random.state(), state);
        battle.targets[0] = ActorId(1);
        assert!(
            battle
                .begin_recovery_return(ActorId(2), &mut vec![])
                .unwrap()
        );
        battle.actors[2].position[0] = 395.;
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(
            battle.actors[2].movement.steering.returning.unwrap().phase,
            Phase::Stopping
        );
        assert_eq!(battle.recovery_return_hover(2), Some(false));
        let mut expected = battle.random;
        expected.next();
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.random.state(), expected.state());
        assert_eq!(battle.actors[2].activity, Activity::Idle);
        assert!(battle.actors[2].movement.steering.returning.is_none());
        assert_eq!(battle.actors[2].movement.forward, 0.);
    }
}

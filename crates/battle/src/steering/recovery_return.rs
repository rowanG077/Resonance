use super::*;
use crate::state::ActorTask;
use anyhow::Context;

const RETURN_ACCELERATION: f32 = 0.5;
const RETURN_TIMEOUT_TICKS: u16 = 240;

/// Locomotion used when an automatic actor returns to a safe position.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryReturnDefinition {
    pub speed: f32,
    pub turn_ticks: u8,
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
    elapsed: u16,
}

impl PreparedBattle {
    pub(crate) fn validate_actor_recovery_returns(&self) -> Result<()> {
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            let Some(definition) = &setup.recovery_return else {
                continue;
            };
            ensure!(
                index < self.actors.len()
                    && (matches!(self.actors[index].control, Control::Auto | Control::Enemy)
                        || self.resources.actor_setup[index].control.is_some()),
                "recovery return needs an automatic actor or prepared party controller"
            );

            ensure!(
                definition.speed.is_finite() && definition.speed > 0. && definition.turn_ticks != 0,
                "invalid recovery return parameters"
            );
        }
        Ok(())
    }
}

impl Battle {
    pub(crate) fn recovery_return_stopping(&self, index: usize) -> bool {
        matches!(
            self.runtime[index].task(),
            ActorTask::Returning(ReturnState {
                phase: Phase::Stopping,
                ..
            })
        )
    }

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

    fn return_interrupted(&self, owner: ActorId) -> bool {
        self.controlled_target() == Some(owner)
            || self.has_pending_item(owner)
            || self.pending_technique(owner).is_some()
            || self.runtime[owner.index()].combo.buffered.is_some()
    }

    pub(crate) fn begin_recovery_return(&mut self, owner: ActorId) -> Result<bool> {
        let index = owner.index();
        let Some(definition) = self.prepared.actor_setup[index].recovery_return else {
            return Ok(false);
        };
        if !matches!(self.actors[index].control, Control::Auto | Control::Enemy)
            || self.return_interrupted(owner)
        {
            return Ok(false);
        }
        let row_disabled = self.runtime[index]
            .enemy_choice
            .and_then(|selected| {
                self.prepared.actor_setup[index]
                    .enemy_decision
                    .as_ref()
                    .and_then(|enemy| enemy.choices.get(selected))
            })
            .is_some_and(|row| !row.return_to_formation);
        let actor = &self.actors[index];
        let home = if actor.side == Side::Party {
            if self.companion_position(owner)? == 1 {
                let target = self
                    .target(owner)
                    .context("recovery return requires a target")?;
                let preferred = recovery_position::retreat(actor, &self.actors[target.index()]);
                recovery_position::select(&self.actors, owner, preferred).unwrap_or(actor.position)
            } else {
                self.formation_home(owner)?
            }
        } else {
            recovery_position::select(&self.actors, owner, actor.movement.steering.home)
                .unwrap_or(actor.position)
        };
        let close = planar_length([home[0] - actor.position[0], 0., home[2] - actor.position[2]])
            <= ARRIVAL_DISTANCE;
        self.enter_idle(owner);
        if close || definition.disabled || row_disabled {
            return Ok(true);
        }
        self.model_requests.push(crate::ModelRequest::Common {
            actor: owner,
            pose: crate::CommonPose::Returning,
        });
        let actor = &mut self.actors[index];
        actor.movement.locomotion = crate::Locomotion::Run;
        actor.movement.steering.home = home;
        actor.movement.steering.side = approach_side(actor, home);
        self.set_task(
            index,
            ActorTask::Returning(ReturnState {
                phase: Phase::Moving,
                elapsed: 0,
            }),
        );
        Ok(true)
    }

    /// Own movement for exactly one tick, including the tick that finishes the return.
    pub(crate) fn update_recovery_return(
        &mut self,
        owner: ActorId,
        mut state: ReturnState,
    ) -> Result<bool> {
        let index = owner.index();
        if !self.actors[index].available() {
            self.set_task(index, ActorTask::None);
            return Ok(false);
        }
        let definition = self.prepared.actor_setup[index].recovery_return.unwrap();
        state.elapsed = state.elapsed.saturating_add(1);
        let actor = &self.actors[index];
        let home = actor.movement.steering.home;
        let remaining =
            planar_length([home[0] - actor.position[0], 0., home[2] - actor.position[2]]);
        if state.phase == Phase::Moving
            && (remaining <= ARRIVAL_DISTANCE
                || state.elapsed >= RETURN_TIMEOUT_TICKS
                || self.return_interrupted(owner)
                || !matches!(actor.control, Control::Auto | Control::Enemy))
        {
            state.phase = Phase::Stopping;
            self.actors[index].movement.locomotion = crate::Locomotion::Stop;
            self.model_requests.push(crate::ModelRequest::Common {
                actor: owner,
                pose: crate::CommonPose::Stopping,
            });
        }
        if state.phase == Phase::Moving {
            let destination = self.steer_to(owner, home, None)?;
            let actor = &mut self.actors[index];
            let desired =
                distance::planar_direction(destination, actor.position, actor.movement.direction);
            let turn = 180. / f32::from(definition.turn_ticks);
            actor.movement.direction =
                crate::control::turn_direction(actor.movement.direction, desired, turn);
            actor.facing_direction = actor.movement.direction;
            let facing = actor.facing_direction;
            crate::control::face(actor, facing, turn);
            let limit = actor.run_limit(definition.speed, actor.conditions.effective());
            actor.movement.forward = (actor.movement.forward + RETURN_ACCELERATION)
                .min(limit)
                .min(remaining);
        } else {
            let actor = &mut self.actors[index];
            actor
                .movement
                .brake(actor.position[1], crate::Activity::Approaching);
        }
        let actor = &mut self.actors[index];
        actor.movement.integrate(&mut actor.position);
        self.advance_hover(index, state.phase == Phase::Moving)?;
        let stopped = state.phase == Phase::Stopping && self.actors[index].movement.forward == 0.;
        if stopped {
            self.enter_idle(owner);
        } else {
            self.set_task(index, ActorTask::Returning(state));
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Activity;
    use crate::tests::actor;

    fn battle() -> Battle {
        let mut leader = actor(Side::Party);
        leader.position = [-500., 0., -400.];
        let mut target = actor(Side::Enemy);
        target.position = [-300., 0., 400.];
        let mut owner = actor(Side::Enemy);
        owner.control = Control::Enemy;
        owner.position = [400., 0., 0.];
        let prepared = PreparedBattle::new(
            vec![
                (leader, Default::default()),
                (target, Default::default()),
                (
                    owner,
                    crate::ActorSetup {
                        recovery_return: Some(RecoveryReturnDefinition {
                            speed: 3.,

                            turn_ticks: 8,
                            disabled: false,
                        }),
                        ..Default::default()
                    },
                ),
            ],
            Default::default(),
            1,
        )
        .unwrap();
        let mut battle = prepared.finish().unwrap();
        battle.actors[2].position = [0.; 3];
        battle
    }

    #[test]
    fn selected_enemy_action_can_stay_in_place_after_recovery() -> Result<()> {
        let mut battle = battle();
        let owner = ActorId(2);
        battle.prepared.actor_setup[2].enemy_decision = Some(crate::EnemyDecisionDefinition {
            strategy: crate::TargetPolicy::Nearest,
            difficulty: 0,
            choices: vec![crate::EnemyChoice {
                action: crate::ActionKey(0),
                weight: 1,
                requirements: Default::default(),
                target_policy: None,
                return_to_formation: false,
                guard_chance: 0,
                range: [0, 0],
                tp: 0,
                approach_minimum: 0.,
                approach_range: 100.,
            }],
            back_row: vec![],
            walk_speed: 3.,
            walk_motion: None,
            turn_ticks: 8,
        });
        battle.runtime[2].enemy_choice = Some(0);
        let position = battle.actors[2].position;
        assert!(battle.begin_recovery_return(owner)?);
        assert_eq!(battle.actors[2].position, position);
        assert!(battle.actor_command_ready(ActorId(2)));
        assert_ne!(battle.activity(ActorId(2)), Activity::Approaching);
        Ok(())
    }

    #[test]
    fn return_reaches_home_and_respects_pause() -> Result<()> {
        let mut battle = battle();
        let owner = ActorId(2);
        assert!(battle.begin_recovery_return(owner)?);
        battle.actors[2].movement.direction = [-1., 0., 0.];

        let start = battle.actors[2].position;
        battle.step(crate::BattleInput::default())?;
        assert_ne!(battle.actors[2].position, start);
        let paused = battle.actors[2].position;
        battle.step(crate::BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(battle.actors[2].position, paused);
        for _ in 0..RETURN_TIMEOUT_TICKS {
            battle.step(crate::BattleInput::default())?;
            if battle.actor_command_ready(owner) {
                break;
            }
        }
        assert!(battle.actor_command_ready(owner));
        assert!((battle.actors[2].position[0] - 400.).abs() < ARRIVAL_DISTANCE * 2.);

        Ok(())
    }

    #[test]
    fn selected_enemy_holds_position_and_target_change_interrupts_return() -> Result<()> {
        let mut battle = battle();
        battle.runtime[0].target = ActorId(2);
        assert!(!battle.begin_recovery_return(ActorId(2))?);
        battle.runtime[0].target = ActorId(1);
        assert!(battle.begin_recovery_return(ActorId(2))?);
        battle.step(crate::BattleInput::default())?;
        battle.runtime[0].target = ActorId(2);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.activity(ActorId(2)), Activity::Idle);
        assert!(battle.actor_command_ready(ActorId(2)));
        Ok(())
    }
}

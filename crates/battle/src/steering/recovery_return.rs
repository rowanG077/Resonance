use super::*;
use crate::state::ActorTask;
use anyhow::Context;

const RETURN_TIMEOUT_TICKS: u16 = 360;
const RETURN_DIRECTION_BLEND: f32 = 0.1;
const RETURN_START_DISTANCE: f32 = 45.;
const RETURN_STOP_LOOKAHEAD: f32 = 4.;
const RETURN_TARGET_DELAY: u16 = 60;
const RETURN_TARGET_DISTANCE: f32 = 60.;

/// Locomotion used when an automatic actor returns to a safe position.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryReturnDefinition {
    pub speed: f32,
    pub run_speed: f32,
    pub turn_ticks: u8,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Moving,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ReturnState {
    phase: Phase,
    elapsed: u16,
    speed: Option<f32>,
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
                definition.speed.is_finite()
                    && definition.speed > 0.
                    && definition.run_speed.is_finite()
                    && definition.run_speed > 0.
                    && definition.turn_ticks != 0,
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

    fn return_interrupted(&self, owner: ActorId) -> bool {
        self.preferred_party_leader()
            .and_then(|actor| self.target(actor))
            == Some(owner)
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
        let row = self.runtime[index].enemy_choice.and_then(|selected| {
            self.prepared.actor_setup[index]
                .enemy_decision
                .as_ref()
                .and_then(|enemy| enemy.choices.get(selected.0))
        });
        let row_disabled = row.is_some_and(|row| !row.return_to_formation);
        let returning = if self.runtime[index].enemy_walk_override.is_some() {
            None
        } else {
            row.map(|row| (row.returning, row.return_speed))
        };
        let actor = &self.actors[index];
        let home = if actor.side == Side::Party {
            if self
                .companion_policy(owner)
                .context("actor has no companion policy")?
                .position
                == crate::companion::PositionStrategy::Frontlines
            {
                let target = self
                    .target(owner)
                    .context("recovery return requires a target")?;
                #[cfg(any(test, feature = "test-support"))]
                let mut observed = self
                    .random_inputs
                    .as_mut()
                    .map(|inputs| inputs.positioning(self.ledger.elapsed_ticks, owner, target))
                    .transpose()?
                    .flatten()
                    .map(Vec::into_iter);
                let home = recovery_position::clear_position(&self.actors, owner, target, || {
                    #[cfg(any(test, feature = "test-support"))]
                    if let Some(values) = &mut observed {
                        return values.next().context("extra positioning draw");
                    }
                    Ok(self.random.next_u16())
                })?;
                #[cfg(any(test, feature = "test-support"))]
                ensure!(
                    observed.as_ref().is_none_or(|values| values.len() == 0),
                    "unused positioning draws"
                );
                home.unwrap_or(actor.movement.steering.home)
            } else {
                self.formation_home(owner)?
            }
        } else {
            actor.movement.steering.home
        };
        let close = planar_length([home[0] - actor.position[0], 0., home[2] - actor.position[2]])
            < RETURN_START_DISTANCE;
        let staying = close || definition.disabled || row_disabled;
        if staying {
            self.finish_recovery_return(owner);
        } else {
            self.enter_idle(owner);
            if let Some((motion, _)) = returning {
                let actor = &self.actors[index];
                self.request_pose(
                    owner,
                    motion,
                    crate::Pose {
                        rate: actor.motion_rate(0.5, actor.conditions.effective()),
                        repeat: true,
                        restart: false,
                        ..Default::default()
                    },
                );
            } else {
                self.model_requests.push(crate::ModelRequest::Common {
                    actor: owner,
                    pose: crate::CommonPose::Returning,
                });
            }
            let actor = &mut self.actors[index];
            actor.movement.locomotion = if self.runtime[index].enemy_walk_override.is_some() {
                crate::Locomotion::Walk
            } else {
                crate::Locomotion::Run
            };
            actor.movement.steering.home = home;
        }
        let actor = &mut self.actors[index];
        actor.movement.steering.side = if planar_length(actor.position) >= DETOUR_EDGE_RADIUS {
            match approach_side(actor, home) {
                DetourSide::Automatic => DetourSide::Right,
                side => side,
            }
        } else {
            DetourSide::Automatic
        };
        // Recovery faces home even when the actor stays in place.
        let destination = self.recovery_destination(owner, home);
        let actor = &mut self.actors[index];
        actor.movement.direction =
            distance::planar_direction(destination, actor.position, actor.movement.direction);
        if !actor.movement.turning_disabled && actor.movement.hover_ready() {
            let facing = actor.movement.direction;
            crate::control::face(actor, facing, 180. / f32::from(definition.turn_ticks));
            actor.facing_direction = crate::control::direction_from_heading(actor.heading);
        }
        if !staying {
            self.set_task(
                index,
                ActorTask::Returning(ReturnState {
                    phase: Phase::Moving,
                    elapsed: 0,
                    speed: returning.and_then(|(_, speed)| speed),
                }),
            );
        }
        Ok(true)
    }

    fn recovery_destination(&self, owner: ActorId, mut destination: [f32; 3]) -> [f32; 3] {
        let actor = &self.actors[owner.index()];
        // Each pass routes around one body; the roster bounds cyclic detours.
        for _ in 0..self.actors.len() {
            let direction =
                distance::planar_direction(destination, actor.position, actor.movement.direction);
            let normal = [-direction[2], 0., direction[0]];
            let detour = self.actors.iter().enumerate().find_map(|(index, other)| {
                if index == owner.index()
                    || self
                        .target(owner)
                        .is_some_and(|target| target.index() == index)
                {
                    return None;
                }
                detour_destination(actor, other, destination, normal)
            });
            let Some(detour) = detour else {
                break;
            };
            destination = detour;
        }
        destination
    }

    /// Own movement for exactly one tick, including the tick that finishes the return.
    pub(crate) fn update_recovery_return(
        &mut self,
        owner: ActorId,
        mut state: ReturnState,
        cues: &mut Vec<crate::Cue>,
    ) -> bool {
        let index = owner.index();
        if !self.actors[index].available() {
            self.set_task(index, ActorTask::None);
            return false;
        }
        let definition = self.prepared.actor_setup[index].recovery_return.unwrap();
        let home = self.actors[index].movement.steering.home;
        if state.phase == Phase::Moving {
            let motion_ready = self
                .models
                .as_ref()
                .is_none_or(|models| models.motion_ready_this_update(owner));
            state.elapsed = state.elapsed.saturating_add(u16::from(motion_ready));
            self.emit_movement_effect(owner, state.elapsed, cues);
            let near_target = state.elapsed > RETURN_TARGET_DELAY
                && self
                    .target(owner)
                    .is_some_and(|target| self.body_gap(owner, target) < RETURN_TARGET_DISTANCE);
            let interrupted = self.return_interrupted(owner)
                || !matches!(self.actors[index].control, Control::Auto | Control::Enemy);
            let destination = self.recovery_destination(owner, home);
            let actor = &mut self.actors[index];
            let desired =
                distance::planar_direction(destination, actor.position, actor.movement.direction);
            actor.movement.direction = distance::normalize(std::array::from_fn(|axis| {
                actor.movement.direction[axis] * (1. - RETURN_DIRECTION_BLEND)
                    + desired[axis] * RETURN_DIRECTION_BLEND
            }));
            actor.movement.integrate(&mut actor.position);
            self.advance_hover(index, true);
            let actor = &mut self.actors[index];
            actor.facing_direction = actor.movement.direction;
            let facing = actor.facing_direction;
            crate::control::face(actor, facing, 180. / f32::from(definition.turn_ticks));
            let speed = self.runtime[index]
                .enemy_walk_override
                .or(state.speed)
                .unwrap_or(definition.speed);
            let limit = actor.run_limit(speed, actor.conditions.effective());
            actor.movement.forward =
                (actor.movement.forward + crate::movement::RUN_ACCELERATION).min(limit);
            let remaining =
                planar_length([home[0] - actor.position[0], 0., home[2] - actor.position[2]]);
            let stopping_distance = actor.movement.forward / actor.movement.braking
                + RETURN_STOP_LOOKAHEAD * definition.run_speed;
            if remaining <= stopping_distance
                || state.elapsed > RETURN_TIMEOUT_TICKS
                || near_target
                || interrupted
            {
                state.phase = Phase::Stopping;
                actor.movement.locomotion = crate::Locomotion::Stop;
                if actor.side == Side::Party || state.speed.is_some() {
                    self.model_requests.push(crate::ModelRequest::Common {
                        actor: owner,
                        pose: crate::CommonPose::Stopping,
                    });
                }
            }
            state.elapsed = state.elapsed.saturating_add(1);
        } else {
            let actor = &mut self.actors[index];
            actor.movement.integrate(&mut actor.position);
            actor
                .movement
                .brake(actor.position[1], crate::Activity::Approaching);
            if actor.movement.flying || actor.movement.fixed_height {
                actor.movement.forward = 0.;
            }
            self.advance_hover(index, false);
        }
        let actor = &self.actors[index];
        let stop_finished = actor.movement.flying
            || actor.movement.fixed_height
            || self
                .models
                .as_ref()
                .is_none_or(|models| models.stop_finished_this_update(owner, actor.hit_stop == 0));
        if state.phase == Phase::Stopping && actor.movement.forward == 0. && stop_finished {
            self.finish_recovery_return(owner);
        } else {
            self.set_task(index, ActorTask::Returning(state));
        }
        true
    }

    fn finish_recovery_return(&mut self, owner: ActorId) {
        self.enter_idle(owner);
        self.model_requests.push(crate::ModelRequest::Common {
            actor: owner,
            pose: crate::CommonPose::Idle,
        });
        self.set_task(
            owner.index(),
            ActorTask::Recovering {
                remaining: 0,
                after_hit: false,
                idle_pose: None,
            },
        );
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
                            run_speed: 3.,
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
            walk_speed: 3.,
            ..crate::tests::enemy_decision(vec![crate::EnemyChoice {
                approach_range: 100.,
                ..crate::tests::enemy_choice(crate::ActionKey(0))
            }])
        });
        battle.runtime[2].enemy_choice = Some(crate::EnemyChoiceId(0));
        // A body directly on the route chooses the positive lateral detour.
        battle.actors[1].position = [200., 0., 0.];
        battle.actors[2].heading = -90.;
        let position = battle.actors[2].position;
        assert!(battle.begin_recovery_return(owner)?);
        assert_eq!(battle.actors[2].position, position);
        assert!(battle.actors[2].heading > -90.);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[2].position, position);
        assert!(battle.actor_command_ready(ActorId(2)));
        assert_ne!(battle.activity(ActorId(2)), Activity::Approaching);

        let actor = &mut battle.actors[2];
        actor.movement.flying = true;
        actor.movement.hover_height = 50.;
        actor.position[1] = 50.;
        actor.movement.vertical = 1.;
        actor.movement.settle_hover(&mut actor.position[1]);
        actor.heading = -90.;
        let facing = crate::control::direction_from_heading(actor.heading);
        actor.facing_direction = facing;
        assert!(battle.begin_recovery_return(owner)?);
        assert_eq!(battle.actors[2].facing_direction, facing);
        let actor = &mut battle.actors[2];
        actor.movement.vertical = 1.;
        actor.movement.settle_hover(&mut actor.position[1]);
        battle.maintain_ai_idle(owner);
        assert!((battle.actors[2].heading + 90.).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn return_reaches_home_and_respects_pause() -> Result<()> {
        let mut battle = battle();
        let owner = ActorId(2);
        let effect = crate::EffectAppearance {
            resource: crate::ResourceId(1),
            member: 9,
        };
        battle.prepared.actor_setup[owner.index()].movement_effect = Some((effect, 3));
        assert!(battle.begin_recovery_return(owner)?);

        let start = battle.actors[2].position;
        let frame = battle.step(crate::BattleInput::default())?;
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, crate::Cue::InitializedEffect(_)))
        );
        assert_eq!(battle.actors[2].position, start);
        let paused = battle.actors[2].position;
        let frame = battle.step(crate::BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, crate::Cue::InitializedEffect(_)))
        );
        assert_eq!(battle.actors[2].position, paused);
        let frame = battle.step(crate::BattleInput::default())?;
        assert!(frame.cues.iter().any(|cue| matches!(cue,
            crate::Cue::InitializedEffect(request)
                if request.owner == owner && request.appearance == effect
                    && request.follow == Some(crate::EffectFollow::Actor(owner)))));
        assert_ne!(battle.actors[2].position, start);
        for _ in 0..RETURN_TIMEOUT_TICKS {
            battle.step(crate::BattleInput::default())?;
            if battle.actor_command_ready(owner) {
                break;
            }
        }
        assert!(battle.actor_command_ready(owner));
        assert!((battle.actors[2].position[0] - 400.).abs() < ARRIVAL_DISTANCE * 2.);

        for (has_stop, flying, fixed_height) in [
            (true, false, false),
            (false, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            let mut stopping = self::battle();
            stopping.actors[2].movement.flying = flying;
            stopping.actors[2].movement.fixed_height = fixed_height;
            let mut definition = crate::tests::model_definition(7, [(0, 20.), (18, 2.)]);
            definition.idle_motions = [Some(0), None];
            definition.reactions.stopping = has_stop.then_some(18);
            definition.initial = crate::Playback {
                clip: if has_stop { 18 } else { 0 },
                frame: 1.,
                rate: 0.5,
                repeat: false,
            };
            if !has_stop {
                definition.motions.remove(&18);
            }
            stopping.set_models(crate::Models::new(
                &stopping.actors,
                vec![None, None, Some(std::sync::Arc::new(definition))],
                Default::default(),
                Default::default(),
            )?);
            assert_eq!(
                stopping
                    .models()
                    .unwrap()
                    .main_motion_observation(owner)
                    .unwrap()
                    .frame,
                1.5
            );
            stopping.set_task(
                owner.index(),
                ActorTask::Returning(ReturnState {
                    phase: Phase::Stopping,
                    elapsed: 0,
                    speed: Some(3.),
                }),
            );
            stopping.step(crate::BattleInput::default())?;
            if has_stop && !flying && !fixed_height {
                assert_eq!(stopping.activity(owner), Activity::Approaching);
                let motion = stopping
                    .models()
                    .unwrap()
                    .main_motion_observation(owner)
                    .unwrap();
                assert_eq!((motion.clip, motion.frame, motion.repeat), (18, 2., false));
                stopping.step(crate::BattleInput {
                    paused: true,
                    ..Default::default()
                })?;
                assert_eq!(stopping.activity(owner), Activity::Approaching);
                assert_eq!(
                    stopping.models().unwrap().main_motion_observation(owner),
                    Some(motion)
                );
                stopping.step(crate::BattleInput::default())?;
            }
            assert_eq!(stopping.activity(owner), Activity::Recovering);
            let motion = stopping
                .models()
                .unwrap()
                .main_motion_observation(owner)
                .unwrap();
            // Without a stopping clip the idle motion is already active. Idle
            // publication turns on looping without restarting its elapsed frame.
            assert_eq!(
                (motion.clip, motion.frame, motion.repeat),
                (0, if has_stop { 0. } else { 2. }, true)
            );
            stopping.step(crate::BattleInput::default())?;
            assert_eq!(stopping.activity(owner), Activity::Idle);
            assert!(stopping.actor_command_ready(owner));
        }

        Ok(())
    }

    #[test]
    fn selected_enemy_holds_position_and_target_change_interrupts_return() -> Result<()> {
        let mut battle = battle();
        let owner = ActorId(2);
        battle.runtime[0].target = ActorId(2);
        assert!(!battle.begin_recovery_return(ActorId(2))?);
        battle.runtime[0].target = ActorId(1);
        assert!(battle.begin_recovery_return(ActorId(2))?);
        battle.step(crate::BattleInput::default())?;
        battle.runtime[0].target = ActorId(2);
        for _ in 0..8 {
            battle.step(crate::BattleInput::default())?;
            if battle.actor_command_ready(owner) {
                break;
            }
        }
        assert_eq!(battle.activity(ActorId(2)), Activity::Idle);
        assert!(battle.actor_command_ready(ActorId(2)));
        Ok(())
    }
}

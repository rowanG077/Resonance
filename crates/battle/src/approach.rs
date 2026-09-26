//! Shared automatic admission (298C0, 30C4C, 30B4C). Policy chooses the action;
//! this state retains movement until the ordinary actor callback can start it.
use crate::{ActionPhase, ActionRequest, Activity, ActorId, Battle, Cue, MotionBinding};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy)]
pub struct ApproachParameters {
    pub minimum: f32,
    pub maximum: f32,
    pub motion: Option<MotionBinding>,
    pub stop_motion: Option<MotionBinding>,
    pub motion_rate: f32,
    pub speed: f32,
    pub turn_ticks: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Arc};

    fn battle(position: f32, minimum: f32, maximum: f32) -> Battle {
        let sources = BTreeMap::from([("test".into(), "script battle; use battle; pub task action() { await battle::at_age(ticks(60)); battle::finish(); }".into())]);
        let compiled =
            symphonia_script_compiler::compile("test", &sources, &crate::native_declarations())
                .unwrap();
        let entry = compiled.program.authored().unwrap().functions[0].entry;
        let action = crate::ActionDefinition {
            id: 1,
            phase: ActionPhase::Actor,
            program: Arc::new(compiled.program),
            entry,
            duration: 60,
            tp_cost: 0,
            resources: vec![],
        };
        let mut owner = crate::tests::actor(crate::Side::Party);
        owner.control = crate::Control::Auto;
        owner.heading = 90.;
        owner.movement.direction = [1., 0., 0.];
        owner.facing_direction = [1., 0., 0.];
        owner.body.approach_points.push(crate::HurtPoint {
            center: [0.; 3],
            radius: 10.,
        });
        let mut target = crate::tests::actor(crate::Side::Enemy);
        target.position[0] = position;
        target.body.center = target.position;
        target.body.approach_points.push(crate::HurtPoint {
            center: target.position,
            radius: 10.,
        });
        let prepared =
            crate::PreparedBattle::new(vec![owner, target], vec![action], 0, vec![], vec![])
                .unwrap();
        let mut battle = Battle::new(Arc::new(prepared));
        assert!(
            battle
                .request_approach(
                    ActorId(0),
                    ActorId(1),
                    1,
                    ApproachParameters {
                        minimum,
                        maximum,
                        motion: None,
                        stop_motion: None,
                        motion_rate: 0.5,
                        speed: 6.,
                        turn_ticks: 8
                    }
                )
                .unwrap()
        );
        battle
    }

    #[test]
    fn automatic_request_waits_for_moving_callback_before_acceleration() -> Result<()> {
        let mut battle = battle(400., 0., 120.);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 0.);
        assert_eq!(battle.actors[0].movement.forward, 0.);
        assert_eq!(battle.approach_hover(0), Some(true));
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 0.);
        assert_eq!(battle.actors[0].movement.forward, 0.5);
        battle.step(crate::BattleInput::default())?;
        assert!((battle.actors[0].position[0] - 0.5).abs() < 0.000001);
        assert_eq!(battle.actors[0].movement.forward, 1.);
        Ok(())
    }

    #[test]
    fn target_direction_samples_old_centers_before_petrified_center_refresh() -> Result<()> {
        let mut battle = battle(400., 0., 120.);
        battle.actors[0].petrified = true;
        battle.actors[0].body.center = [10., 80., 20.];
        battle.actors[1].body.center = [110., 100., 120.];
        battle.actors[0].position = [40., 0., 60.];
        let expected = crate::distance::planar_direction([110., 0., 120.], [10., 0., 20.], [0.; 3]);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[0].movement.target_direction, expected);
        assert_eq!(battle.actors[0].body.center, [40., 0., 60.]);
        battle.actors[0].position = [70., 0., 90.];
        battle.step(crate::BattleInput {
            menu_open: true,
            ..Default::default()
        })?;
        assert_eq!(battle.actors[0].movement.target_direction, expected);
        assert_eq!(battle.actors[0].body.center, [40., 0., 60.]);
        Ok(())
    }

    #[test]
    fn in_range_admission_copies_retained_target_direction() -> Result<()> {
        let mut battle = battle(100., 0., 1200.);
        let parameters = battle.approaches[0].unwrap().parameters;
        battle.approaches[0] = None;
        battle.actors[0].activity = Activity::Idle;
        battle.actors[0].position = [-700., 0., 150.];
        battle.actors[0].heading = f32::from_bits(0x42c8_eb2a); // Route01 Genis C314.
        // Request C315 sees C314 roots, but31C88 cached the C313 centers.
        battle.actors[1].position = [0x4379_ab44, 0, 0xc1cd_5e03].map(f32::from_bits);
        let cached = crate::distance::planar_direction(
            [0x437a_f7b4, 0, 0xc1cd_da38].map(f32::from_bits),
            battle.actors[0].position,
            [0.; 3],
        );
        battle.actors[0].movement.target_direction = cached;
        assert!(battle.request_approach(ActorId(0), ActorId(1), 1, parameters)?);
        assert_eq!(battle.actors[0].movement.direction, cached);
        assert_eq!(battle.actors[0].facing_direction, cached);
        battle.advance_approach(ActorId(0), &mut vec![])?;
        assert_eq!(battle.actors[0].heading.to_bits(), 0x42c8_f081); // Source C315.
        Ok(())
    }

    #[test]
    fn arriving_turns_this_visit_without_replacing_the_cached_movement_direction() -> Result<()> {
        let mut battle = battle(400., 0., 120.);
        // Source04 C60: roots after Colette integration, before Zombie's visit.
        battle.actors[0].position = [3276183705, 0, 3280194849].map(f32::from_bits);
        battle.actors[1].position = [1132351902, 0, 3199538140].map(f32::from_bits);
        battle.actors[0].heading = 87.76288;
        // 31C88 read the retained centers from C59, which contain C58 roots.
        let cached = crate::distance::planar_direction(
            [1132436347, 0, 3191454354].map(f32::from_bits),
            [3277271147, 0, 3280202146].map(f32::from_bits),
            [0.; 3],
        );
        battle.actors[0].movement.target_direction = cached;
        let approach = battle.approaches[0].as_mut().unwrap();
        approach.phase = Phase::Moving;
        approach.moved = true;
        approach.in_range = true;
        approach.parameters.turn_ticks = 10;
        let random = battle.random_state();
        let position = battle.actors[0].position;
        battle.finish_approach(0)?;
        assert_eq!(battle.actors[0].heading, 69.76288);
        assert_eq!(battle.actors[0].movement.direction, cached);
        assert_eq!(battle.actors[0].facing_direction, cached);
        assert_eq!(battle.actors[0].position, position);
        assert_eq!(battle.approaches[0].unwrap().phase, Phase::Turning);
        assert_eq!(battle.random_state(), random);
        // The next30B4C visit can now dispatch immediately. A missing arrival
        // turn would consume this entire visit turning by another18degrees.
        let mut cues = vec![];
        battle.advance_approach(ActorId(0), &mut cues)?;
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Started {
                actor: ActorId(0),
                ..
            }
        )));
        Ok(())
    }

    #[test]
    fn retreat_at_radial_limit_stops_without_requiring_minimum_gap() -> Result<()> {
        let mut battle = battle(100., 400., 500.);
        battle.actors[0].position[0] = 780.;
        // Preserve a too-close pair of sampled body volumes at this callback.
        battle.step(crate::BattleInput::default())?;
        battle.step(crate::BattleInput::default())?;
        let approach = battle.approaches[0].unwrap();
        assert!(approach.retreat_at_boundary);
        assert_eq!(approach.phase, Phase::Stopping);
        assert!(approach.retry_after_stop);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Requested,
    Moving,
    Stopping,
    Turning,
    Started,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Approach {
    action: u16,
    target: ActorId,
    parameters: ApproachParameters,
    phase: Phase,
    in_range: bool,
    retreat: bool,
    age: i16,
    hold: bool,
    moved: bool,
    retry_after_stop: bool,
    retreat_at_boundary: bool,
    hover: Option<bool>,
}

impl Battle {
    pub(crate) fn request_approach(
        &mut self,
        actor: ActorId,
        target: ActorId,
        mut action: u16,
        parameters: ApproachParameters,
    ) -> Result<bool> {
        let index = actor.index();
        let definition = self
            .prepared
            .actions
            .iter()
            .find(|a| a.id == action)
            .context("unprepared approach action")?;
        ensure!(
            matches!(definition.phase, ActionPhase::Actor | ActionPhase::Casting),
            "approach needs an actor action"
        );
        ensure!(
            target.index() < self.actors.len()
                && self.actors[target.index()].side != self.actors[index].side,
            "invalid approach target"
        );
        ensure!(
            [
                parameters.minimum,
                parameters.maximum,
                parameters.motion_rate,
                parameters.speed
            ]
            .iter()
            .all(|v| v.is_finite())
                && parameters.minimum >= 0.
                && parameters.maximum > parameters.minimum
                && parameters.motion_rate > 0.
                && parameters.speed > 0.
                && parameters.turn_ticks != 0,
            "invalid approach parameters"
        );
        if let Some(motion) = parameters.motion {
            self.models[index]
                .as_ref()
                .context("approach motion needs an actor model")?
                .duration(motion)?;
        }
        if let Some(motion) = parameters.stop_motion {
            self.models[index]
                .as_ref()
                .context("stop motion needs an actor model")?
                .duration(motion)?;
        }
        if !self.decision_ready(actor) || self.actors[index].tp < definition.tp_cost {
            return Ok(false);
        }
        // 298C0 clears Auto's upward/air selector before testing the already
        // selected range; it deliberately does not call 1F548 again here.
        if self.actors[index].control == crate::Control::Auto
            && let Some(controls) = &self.prepared.controls[index]
            && controls
                .normals
                .iter()
                .enumerate()
                .any(|(i, n)| n.action == action && (i == 1 || i >= 5))
        {
            action = controls.normals[0].action;
        }
        let gap = crate::control::body_gap(&self.actors[index], &self.actors[target.index()]);
        let manual = self.actors[index].control == crate::Control::Manual;
        let in_range = manual || gap < parameters.maximum && gap > parameters.minimum;
        let direction = if in_range {
            // 298C0's grounded non-Manual admission copies the18FC cache.
            self.actors[index].movement.target_direction
        } else {
            crate::distance::planar_direction(
                self.actors[target.index()].position,
                self.actors[index].position,
                self.actors[index].movement.direction,
            )
        };
        self.actors[index]
            .guard
            .reset_auto_chance(50, &mut self.random);
        self.actors[index].attack_power = 100;
        if !manual
            && (!in_range
                || self.actors[index].position[1] <= 0.1
                || self.actors[index].movement.flying)
        {
            self.actors[index].movement.direction = if gap < parameters.minimum {
                direction.map(|v| -v)
            } else {
                direction
            };
            self.actors[index].facing_direction = self.actors[index].movement.direction;
        }
        if !in_range && self.actors[index].control == crate::Control::SemiAuto {
            // 31290 publishes full run speed on the request visit itself.
            self.actors[index].movement.forward = parameters.speed;
        }
        self.actors[index].activity = Activity::Approaching;
        if !in_range {
            self.begin_approach_steering(actor, target)?;
        }
        if !in_range && let Some(motion) = parameters.motion {
            self.models[index].as_mut().unwrap().play(
                motion,
                0.,
                parameters.motion_rate,
                true,
                4,
            )?;
        }
        self.approaches[index] = Some(Approach {
            action,
            target,
            parameters,
            phase: Phase::Requested,
            in_range,
            retreat: false,
            age: 0,
            hold: false,
            moved: false,
            retry_after_stop: false,
            retreat_at_boundary: false,
            hover: Some(true),
        });
        Ok(true)
    }

    pub(crate) fn advance_approach(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let index = actor.index();
        self.reevaluate_approach(actor, cues)?;
        let Some(mut approach) = self.approaches[index].take() else {
            return Ok(());
        };
        if !self.actors[index].available() || self.actors[index].activity != Activity::Approaching {
            return Ok(());
        }
        let mut target = self.target(actor).unwrap_or(approach.target);
        approach.target = target;
        approach.hold = false;
        approach.moved = false;
        approach.hover = None;
        match approach.phase {
            Phase::Requested => {
                approach.hover = Some(true);
                // The idle callback selected the next state; its callback is next visit.
                approach.phase = if approach.in_range {
                    Phase::Turning
                } else {
                    Phase::Moving
                };
                // The requesting31290 idle callback still performs its final
                // automatic24D24. Manual/Semi's enabled1 path is heading-only.
                if matches!(
                    self.actors[index].control,
                    crate::Control::Auto | crate::Control::Enemy
                ) {
                    let direction = self.actors[index].facing_direction;
                    crate::control::face_cached(
                        &mut self.actors[index],
                        direction,
                        180. / f32::from(approach.parameters.turn_ticks),
                    );
                }
            }
            Phase::Turning => {
                approach.hold = true;
                let direction = self.actors[index].movement.direction;
                if self.actors[index].movement.turning_disabled
                    || self.actors[index].side == crate::Side::Party
                        && self.actors[index].position[1] > 0.1
                    || (!self.actors[index].movement.flying
                        || self.actors[index].movement.hover_ready())
                        && crate::control::face_cached(
                            &mut self.actors[index],
                            direction,
                            180. / f32::from(approach.parameters.turn_ticks),
                        )
                {
                    let normal =
                        self.start_automatic_normal(actor, approach.action, target, cues)?;
                    if !normal {
                        self.start(
                            ActionRequest {
                                actor,
                                target,
                                action: approach.action,
                            },
                            cues,
                        )?;
                        // Prepared ordinary enemy rows have technique zero and
                        // share 3DF34's no-integration initializer branch.
                        approach.hold =
                            self.prepared.enemy_decisions[index]
                                .as_ref()
                                .is_some_and(|d| {
                                    d.choices.iter().any(|row| row.action == approach.action)
                                });
                    }
                    approach.phase = Phase::Started;
                }
            }
            Phase::Moving => {
                approach.hover = Some(true);
                approach.moved = true;
                approach.age = approach.age.wrapping_add(1);
                let gap =
                    crate::control::body_gap(&self.actors[index], &self.actors[target.index()]);
                approach.retreat = gap < approach.parameters.minimum;
                approach.retreat_at_boundary = approach.retreat
                    && crate::distance::length([
                        self.actors[index].position[0],
                        0.,
                        self.actors[index].position[2],
                    ]) > 765.;
                if !approach.retreat
                    && self.actors[index].side == crate::Side::Enemy
                    && gap > 5. * approach.parameters.maximum
                {
                    target = self.decision_target(actor, 1)?;
                    self.set_decision_target(actor, target)?;
                    approach.target = target;
                }
                let destination = if approach.retreat {
                    let direction = crate::distance::planar_direction(
                        self.actors[index].position,
                        self.actors[target.index()].position,
                        self.actors[index].movement.direction,
                    );
                    std::array::from_fn(|i| {
                        self.actors[index].position[i] + direction[i] * approach.parameters.minimum
                    })
                } else {
                    let center = self.actors[target.index()].body.center;
                    [center[0], 0., center[2]]
                };
                let direction = if self.actors[index].movement.fixed_height {
                    // 30C4C's body flag 0x4000 branch neither steers nor integrates.
                    approach.hold = true;
                    crate::distance::planar_direction(
                        self.actors[target.index()].position,
                        self.actors[index].position,
                        self.actors[index].movement.direction,
                    )
                } else {
                    let destination = self.steer_approach(actor, target, destination)?;
                    let desired = crate::distance::planar_direction(
                        destination,
                        self.actors[index].position,
                        self.actors[index].movement.direction,
                    );
                    let current = self.actors[index].movement.direction;
                    if crate::distance::length(desired) > 0.1 {
                        let desired = crate::distance::normalize(desired);
                        crate::distance::normalize(std::array::from_fn(|i| {
                            current[i] * (1. - 0.1_f32) + desired[i] * 0.1_f32
                        }))
                    } else {
                        current
                    }
                };
                self.actors[index].movement.direction = direction;
                self.actors[index].facing_direction = direction;
                if !self.actors[index].movement.flying || self.actors[index].movement.hover_ready()
                {
                    crate::control::face_cached(
                        &mut self.actors[index],
                        direction,
                        180. / f32::from(approach.parameters.turn_ticks),
                    );
                }
                // The body gap was sampled before integration in 31C88.
                approach.in_range = gap
                    > approach.parameters.minimum - self.actors[index].movement.forward
                    && gap < approach.parameters.maximum;
            }
            Phase::Stopping => {
                approach.moved = true;
                approach.hover = Some(false);
            }
            Phase::Started => return Ok(()),
        }
        self.approaches[index] = Some(approach);
        Ok(())
    }

    pub(crate) fn approach_normal(&self, actor: ActorId) -> Option<u8> {
        let approach = self.approaches[actor.index()].as_ref()?;
        let controls = self.prepared.controls[actor.index()].as_ref()?;
        controls
            .normals
            .iter()
            .position(|normal| normal.action == approach.action)
            .map(|i| i as u8)
    }

    pub(crate) fn reselect_approach_normal(&mut self, actor: ActorId, selector: u8) -> Result<()> {
        ensure!(
            self.approach_normal(actor).is_some(),
            "approach is not an ordinary normal"
        );
        let index = actor.index();
        let normal = *self.prepared.controls[index]
            .as_ref()
            .unwrap()
            .normals
            .get(usize::from(selector))
            .context("invalid approach normal selector")?;
        let approach = self.approaches[index].as_mut().unwrap();
        approach.action = normal.action;
        approach.parameters.maximum = normal.reach;
        // 1F548 zeroes the minimum for every mode except Auto.
        approach.parameters.minimum = if self.actors[index].control == crate::Control::Auto {
            normal.minimum_reach
        } else {
            0.
        };
        Ok(())
    }

    fn reevaluate_approach(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let index = actor.index();
        if self.actors[index].side != crate::Side::Party
            || self.actors[index].control == crate::Control::Manual
            || !self.actors[index].available()
            || self.actors[index].activity != Activity::Approaching
            || self.approaches[index].is_none_or(|a| {
                a.phase != Phase::Moving
                    && !(a.phase == Phase::Requested
                        && a.in_range
                        && self.actors[index].control == crate::Control::Auto)
            })
            || self.approach_normal(actor).is_none()
        {
            return Ok(());
        }
        let target = self
            .target(actor)
            .unwrap_or(self.approaches[index].unwrap().target);
        let decisions: Vec<_> = self
            .sequences
            .iter()
            .filter(|(_, s)| s.actor == actor && s.definition.phase == ActionPhase::Decision)
            .map(|(&id, _)| id)
            .collect();
        for id in decisions {
            let mut sequence = self.sequences.remove(&id).unwrap();
            sequence.target = target;
            crate::script::step_approach_visit(self, id, &mut sequence, cues)?;
            self.sequences.insert(id, sequence);
        }
        Ok(())
    }

    pub(crate) fn finish_approach(&mut self, index: usize) -> Result<()> {
        let Some(mut approach) = self.approaches[index].take() else {
            return Ok(());
        };
        if !approach.moved || self.actors[index].activity != Activity::Approaching {
            self.approaches[index] = Some(approach);
            return Ok(());
        }
        let target_position = self.actors[approach.target.index()].position;
        let actor = &mut self.actors[index];
        if approach.phase == Phase::Stopping {
            let unfinished = approach.parameters.stop_motion.is_some()
                && self.models[index]
                    .as_ref()
                    .is_some_and(|model| !model.finished());
            if actor
                .movement
                .brake(actor.position[1], actor.activity, false, unfinished)
            {
                if approach.retry_after_stop {
                    let direction = crate::distance::planar_direction(
                        target_position,
                        actor.position,
                        actor.movement.direction,
                    );
                    if crate::control::face(
                        actor,
                        direction,
                        180. / f32::from(approach.parameters.turn_ticks),
                    ) {
                        actor.movement.direction = direction;
                        actor.facing_direction = direction;
                        actor.guard.reset_auto_chance(50, &mut self.random);
                        actor.attack_power = 100;
                        approach.phase = Phase::Turning;
                    }
                } else {
                    actor.activity = Activity::Idle;
                    actor.guard.reset_auto_chance(75, &mut self.random);
                    actor.reset_hud_targets();
                    return Ok(());
                }
            }
        } else if approach.phase == Phase::Moving {
            if !actor.movement.fixed_height {
                actor.movement.forward =
                    (actor.movement.forward + 0.5).min(approach.parameters.speed);
            }
            let cancelled = actor.control != crate::Control::SemiAuto
                && (approach.age > 300
                    || actor.movement.steering.arena_contact() != crate::ArenaContact::None);
            if cancelled || approach.retreat && (approach.in_range || approach.retreat_at_boundary)
            {
                approach.phase = Phase::Stopping;
                approach.retry_after_stop = !cancelled;
                if let Some(motion) = approach.parameters.stop_motion {
                    self.models[index]
                        .as_mut()
                        .unwrap()
                        .play(motion, 0., 0.5, false, 8)?;
                }
            } else if approach.in_range {
                // 30C4C copies the pre-callback18FC cache, then separately
                // turns toward the current roots using heading-only24A8C.
                actor.movement.direction = actor.movement.target_direction;
                actor.facing_direction = actor.movement.target_direction;
                let direction =
                    std::array::from_fn(|axis| target_position[axis] - actor.position[axis]);
                crate::control::face(
                    actor,
                    direction,
                    180. / f32::from(approach.parameters.turn_ticks),
                );
                approach.phase = Phase::Turning;
            }
        }
        self.approaches[index] = Some(approach);
        Ok(())
    }

    pub(crate) fn approach_holds_movement(&self, index: usize) -> bool {
        self.approaches[index].is_some_and(|approach| approach.hold)
    }

    pub(crate) fn approach_hover(&self, index: usize) -> Option<bool> {
        self.approaches[index].and_then(|approach| approach.hover)
    }

    pub(crate) fn approach_blocks_home(&self, index: usize) -> bool {
        !self.approaches[index].is_some_and(|approach| approach.phase == Phase::Moving)
    }
}

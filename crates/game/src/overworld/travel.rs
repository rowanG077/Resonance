//! Player travel and mount lifetime, independent of scene and audio ownership.
use super::{Position, Rules, collision};
use anyhow::{Result, ensure};
use resonance_content::overworld::MovementParameters;
pub use resonance_content::overworld::{Mount, TravelState as State};
use std::{
    collections::BTreeSet,
    f32::consts::{FRAC_PI_2, PI, TAU},
    sync::Arc,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct Input {
    /// Camera-relative right/forward, normalized to [-1,1]. In vehicles these
    /// are steering/pitch; forward propulsion belongs to throttle.
    pub stick: [f32; 2],
    /// Right stick: direct vehicle travel, or camera rotation on foot.
    pub secondary: [f32; 2],
    pub throttle: bool,
    pub toggle_noishe: bool,
    /// Call Rheairds/ship or request landing/disembarkation (B).
    pub vehicle: bool,
    pub rotate_camera: f32,
    pub toggle_perspective: bool,
    pub cycle_map: bool,
}
impl Input {
    pub(super) fn validate(self) -> Result<()> {
        ensure!(
            self.stick
                .into_iter()
                .chain(self.secondary)
                .chain([self.rotate_camera])
                .all(|v| v.is_finite() && (-1.0..=1.0).contains(&v)),
            "invalid world travel input"
        );
        Ok(())
    }
}

pub struct Context<'a> {
    pub event_flags: &'a BTreeSet<u16>,
    pub rheairds_owned: bool,
    /// World events and landmark contacts can veto a landing before descent.
    pub landing_clear: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Denied,
    /// Play the party leader's mounting/dismounting clip. Completion is tied to
    /// this token, so an old clip cannot unlock a subsequent transition.
    Animation {
        token: u64,
        mounting: bool,
    },
    Started {
        from: Mount,
        to: Mount,
    },
    Finished(Mount),
}

#[derive(Debug, Clone)]
enum Phase {
    Noishe,
    Flight,
    Ship {
        destination: Position,
        anchorage: Position,
        ticks: u8,
    },
}
#[derive(Debug, Clone)]
struct Transition {
    from: Mount,
    phase: Phase,
    animation: Option<u64>,
    zoom_done: bool,
    phase_done: bool,
}

/// All clocks advance on the ordinary simulation cadence. Menus/battle suspend
/// this owner; rendering observes it without moving the player or ending waits.
#[derive(Clone)]
pub struct Travel {
    state: State,
    parameters: Arc<MovementParameters>,
    transition: Option<Transition>,
    next_animation: u64,
    camera_distance: f32,
    map_opacity: [u8; 2],
    stick_divisor: f32,
    speed: f32,
    vehicle_speed: f32,
    turn: f32,
    pitch: f32,
    response: i8,
    slope: [f32; 3],
}
impl Travel {
    pub fn new(state: State, parameters: Arc<MovementParameters>) -> Result<Self> {
        parameters.validate()?;
        state.validate_shape()?;
        let camera_distance = parameters.camera_distances[usize::from(state.alternate_perspective)]
            [usize::from(state.mount.long_range())];
        let stick_divisor = if state.mount.long_range() {
            parameters.mounted_stick_divisor
        } else {
            parameters.foot_stick_divisor
        };
        Ok(Self {
            map_opacity: state.map_display.opacity(),
            state,
            parameters,
            transition: None,
            next_animation: 1,
            camera_distance,
            stick_divisor,
            speed: 0.,
            vehicle_speed: 0.,
            turn: 0.,
            pitch: 0.,
            response: -1,
            slope: [0.; 3],
        })
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub(super) fn reject_displacement(&mut self, previous: Position) {
        self.stop();
        self.state.position = previous;
        if !self.state.mount.airborne() {
            self.state.altitude = previous.map()[2];
        }
    }
    pub(super) fn stop(&mut self) {
        self.speed = 0.;
        self.vehicle_speed = 0.;
    }
    pub fn player_has_control(&self) -> bool {
        self.transition.is_none()
    }
    pub fn speed(&self) -> f32 {
        self.speed
    }
    pub fn bank(&self) -> f32 {
        self.turn
    }
    pub fn pitch(&self) -> f32 {
        self.pitch
    }
    pub fn response(&self) -> i8 {
        self.response
    }
    pub fn slope(&self) -> [f32; 3] {
        self.slope
    }
    pub fn camera_distance(&self) -> f32 {
        self.camera_distance
    }
    pub fn map_opacity(&self) -> [u8; 2] {
        self.map_opacity
    }
    pub fn displayed_mount(&self) -> Mount {
        if self.state.mount == Mount::Foot {
            self.transition.as_ref().map_or(Mount::Foot, |t| t.from)
        } else {
            self.state.mount
        }
    }
    /// Boarding grows the ship at the sea endpoint while the player waits on
    /// land. Disembarking leaves it there to shrink after the player returns.
    pub fn ship_pose(&self) -> Option<(Position, f32)> {
        if let Some(Transition {
            phase: Phase::Ship {
                anchorage, ticks, ..
            },
            ..
        }) = &self.transition
        {
            let scale = if self.state.mount == Mount::Ship {
                f32::from(*ticks) / 30.
            } else {
                f32::from(30 - *ticks) / 30.
            };
            Some((*anchorage, 0.6 * scale))
        } else {
            (self.state.mount == Mount::Ship).then_some((self.state.position, 0.6))
        }
    }
    pub fn speed_fraction(&self) -> f32 {
        let multiplier = match self.displayed_mount() {
            Mount::Rheairds => self.parameters.flight_speed_multiplier,
            Mount::Ship => self.parameters.ship_speed_multiplier,
            _ => 1.,
        };
        (self.speed / (multiplier * self.parameters.stick_maximum / self.stick_divisor))
            .clamp(0., 1.)
    }
    pub fn checkpoint(&self) -> Result<State> {
        ensure!(
            self.transition.is_none(),
            "cannot save during a mount transition"
        );
        Ok(self.state.clone())
    }
    pub fn finish_animation(&mut self, token: u64) -> bool {
        if let Some(transition) = &mut self.transition
            && transition.animation == Some(token)
        {
            transition.animation = None;
            return true;
        }
        false
    }

    /// Failed terrain preparation or invalid input never partly changes travel.
    pub fn step(
        &mut self,
        input: Input,
        context: &Context<'_>,
        terrain: &collision::Terrain,
        rules: &Rules,
    ) -> Result<Vec<Cue>> {
        input.validate()?;
        let mut candidate = self.clone();
        let cues = candidate.advance(input, context, terrain, rules)?;
        *self = candidate;
        Ok(cues)
    }

    fn advance(
        &mut self,
        input: Input,
        context: &Context<'_>,
        terrain: &collision::Terrain,
        rules: &Rules,
    ) -> Result<Vec<Cue>> {
        let mut cues = Vec::new();
        let query = terrain.query(self.state.position, self.parameters.collision_radius)?;
        let mode = if self.state.mount == Mount::Ship {
            collision::Mode::Ship
        } else {
            collision::Mode::Ground
        };
        self.response = query.surface(mode).map_or(-1, |s| s.response);
        if self.transition.is_none() {
            if input.toggle_perspective {
                self.state.alternate_perspective = !self.state.alternate_perspective;
            }
            if input.cycle_map {
                self.state.map_display = self.state.map_display.next();
            }
            self.actions(input, context, rules, &mut cues)?;
        }
        for (alpha, goal) in self
            .map_opacity
            .iter_mut()
            .zip(self.state.map_display.opacity())
        {
            *alpha = if *alpha < goal {
                alpha.saturating_add(16).min(goal)
            } else {
                alpha.saturating_sub(16).max(goal)
            };
        }
        if self.transition.is_some() {
            self.speed = 0.;
            self.advance_transition(&mut cues);
            return Ok(cues);
        }

        let p = &self.parameters;
        let distances = p.camera_distances[usize::from(self.state.alternate_perspective)];
        self.camera_distance = approach(
            self.camera_distance,
            distances[usize::from(self.state.mount.long_range())],
            (distances[1] - distances[0]) / p.zoom_transition_ticks,
        );
        let mut camera_input = input.rotate_camera * p.stick_maximum;
        if matches!(self.state.mount, Mount::Foot | Mount::Noishe) {
            let [x, y] = input.stick;
            self.speed = (x.hypot(y).min(1.) * p.stick_maximum) / self.stick_divisor;
            if self.speed > 0. {
                self.state.heading = (x.atan2(-y) - self.state.camera_yaw).rem_euclid(TAU);
            }
            camera_input += input.secondary[0] * p.stick_maximum;
        } else {
            self.state.heading = (PI - self.state.camera_yaw).rem_euclid(TAU);
            self.vehicle_motion(input);
            camera_input = self.turn.trunc();
        }
        let motion = query.motion(self.speed, self.state.heading, mode)?;
        let mut delta = motion.delta;
        self.slope = motion.slope;
        if self.state.mount.airborne() {
            // Flight retains the ground query for landing, but ignores horizontal
            // ground rejection. Height is displayed from the separate altitude.
            delta[0] = self.speed * collision::sine(self.state.heading);
            delta[1] = -self.speed * collision::cosine(self.state.heading);
            self.response = motion.response.unwrap_or(-1);
        } else if let Some(response) = motion.response {
            self.response = response;
        }
        self.state.position = self.state.position.translated(delta)?;
        self.state.camera_yaw = (self.state.camera_yaw
            + camera_input * self.parameters.camera_turn_degrees.to_radians())
        .rem_euclid(TAU);
        if !self.state.mount.airborne() {
            self.state.altitude = self.state.position.map()[2];
        }
        Ok(cues)
    }

    fn actions(
        &mut self,
        input: Input,
        context: &Context<'_>,
        rules: &Rules,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if input.vehicle {
            match self.state.mount {
                Mount::Rheairds => {
                    if context.landing_clear && rules.can_land(self.state.world, self.response) {
                        self.begin(Mount::Foot, Phase::Flight, cues)?;
                    } else {
                        cues.push(Cue::Denied);
                    }
                }
                Mount::Ship => {
                    if context.event_flags.contains(&23)
                        && let Some(destination) = rules.disembark_destination(self.state.position)
                    {
                        // The player returns to land immediately; the ship then
                        // scales away over thirty updates.
                        let anchorage = self.state.position;
                        self.state.position = destination;
                        self.begin(
                            Mount::Foot,
                            Phase::Ship {
                                destination,
                                anchorage,
                                ticks: 0,
                            },
                            cues,
                        )?;
                    } else {
                        cues.push(Cue::Denied);
                    }
                }
                Mount::Foot | Mount::Noishe => {
                    if context.rheairds_owned && context.event_flags.contains(&22) {
                        self.state.altitude = self.state.position.map()[2];
                        self.begin(Mount::Rheairds, Phase::Flight, cues)?;
                    } else if context.event_flags.contains(&23)
                        && let Some(destination) =
                            rules.embark_destination(self.state.position, self.response)
                    {
                        self.begin(
                            Mount::Ship,
                            Phase::Ship {
                                destination,
                                anchorage: destination,
                                ticks: 0,
                            },
                            cues,
                        )?;
                    } else {
                        cues.push(Cue::Denied);
                    }
                }
            }
        }
        if input.toggle_noishe
            && self.transition.is_none()
            && matches!(self.state.mount, Mount::Foot | Mount::Noishe)
        {
            let mounting = self.state.mount == Mount::Foot;
            if !mounting
                || rules.noishe_available(
                    self.state.world,
                    self.state.position,
                    context.event_flags,
                )
            {
                self.begin(
                    if mounting { Mount::Noishe } else { Mount::Foot },
                    Phase::Noishe,
                    cues,
                )?;
            } else {
                cues.push(Cue::Denied);
            }
        }
        Ok(())
    }

    fn begin(&mut self, target: Mount, phase: Phase, cues: &mut Vec<Cue>) -> Result<()> {
        let from = self.state.mount;
        let animation = if matches!(phase, Phase::Noishe) {
            let token = self.next_animation;
            self.next_animation = token
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("world animation token overflow"))?;
            cues.push(Cue::Animation {
                token,
                mounting: target == Mount::Noishe,
            });
            Some(token)
        } else {
            None
        };
        cues.push(Cue::Started { from, to: target });
        self.state.mount = target;
        self.speed = 0.;
        self.vehicle_speed = 0.;
        self.turn = 0.;
        self.pitch = 0.;
        self.transition = Some(Transition {
            from,
            phase,
            animation,
            zoom_done: from.long_range() == target.long_range(),
            phase_done: false,
        });
        Ok(())
    }

    fn advance_transition(&mut self, cues: &mut Vec<Cue>) {
        let p = &self.parameters;
        let transition = self.transition.as_mut().expect("active transition");
        if !transition.zoom_done {
            let distances = p.camera_distances[usize::from(self.state.alternate_perspective)];
            let goal = distances[usize::from(self.state.mount.long_range())];
            self.camera_distance = approach(
                self.camera_distance,
                goal,
                (distances[1] - distances[0]) / p.zoom_transition_ticks,
            );
            transition.zoom_done = self.camera_distance == goal;
        }
        match &mut transition.phase {
            Phase::Noishe => {
                transition.phase_done = transition.animation.is_none();
            }
            Phase::Flight => {
                let ground = self.state.position.map()[2];
                let height = self.state.altitude - ground;
                if self.state.mount == Mount::Rheairds {
                    let step = (p.lift_step * (PI * height / p.flight_clearance).sin())
                        .max(p.minimum_lift_step);
                    self.state.altitude =
                        (self.state.altitude + step).min(ground + p.flight_clearance);
                    transition.phase_done = self.state.altitude == ground + p.flight_clearance;
                } else {
                    let step = if height < p.descent_easing_height {
                        (p.lift_step * (FRAC_PI_2 * height / p.flight_clearance).sin())
                            .max(p.minimum_lift_step)
                    } else {
                        p.lift_step
                    };
                    self.state.altitude = (self.state.altitude - step).max(ground);
                    transition.phase_done = self.state.altitude == ground;
                }
            }
            Phase::Ship {
                destination, ticks, ..
            } => {
                *ticks = (*ticks + 1).min(30);
                if *ticks == 30 {
                    self.state.position = *destination;
                    self.state.altitude = destination.map()[2];
                    transition.phase_done = true;
                }
            }
        }
        if transition.zoom_done && transition.animation.is_none() {
            self.stick_divisor = if self.state.mount.long_range() {
                p.mounted_stick_divisor
            } else {
                p.foot_stick_divisor
            };
        }
        if transition.zoom_done && transition.phase_done && transition.animation.is_none() {
            self.transition = None;
            cues.push(Cue::Finished(self.state.mount));
        }
    }

    fn vehicle_motion(&mut self, input: Input) {
        let p = &self.parameters;
        let flying = self.state.mount == Mount::Rheairds;
        let multiplier = if flying {
            p.flight_speed_multiplier
        } else {
            p.ship_speed_multiplier
        };
        let straight_maximum = multiplier * (p.stick_maximum / self.stick_divisor);
        let maximum = straight_maximum
            - if flying {
                (self.turn / p.flight_bank_divisor).abs()
            } else {
                (self.turn * p.altitude_per_pitch).abs()
            };
        let acceleration = maximum
            / if flying {
                p.flight_acceleration_ticks
            } else {
                p.ship_acceleration_ticks
            };
        self.vehicle_speed = if input.throttle {
            (self.vehicle_speed + acceleration).min(maximum)
        } else {
            (self.vehicle_speed - acceleration).max(0.)
        };
        self.speed = 0.;
        let desired_turn = input.stick[0].signum() * p.turn_limit;
        self.turn = approach(
            self.turn,
            if input.stick[0] == 0. {
                0.
            } else {
                desired_turn
            },
            if flying {
                p.flight_turn_step
            } else {
                p.ship_turn_step
            },
        );
        if flying {
            let desired_pitch = if input.stick[1] == 0. {
                0.
            } else {
                input.stick[1].signum() * p.pitch_limit
            };
            self.pitch = approach(self.pitch, desired_pitch, p.pitch_step);
            self.state.altitude -= self.pitch.trunc() * p.altitude_per_pitch;
            self.state.altitude = self
                .state
                .altitude
                .min(p.maximum_altitude)
                .max(self.state.position.map()[2] + p.flight_clearance);
        }
        let [x, y] = input.secondary;
        if flying && (x != 0. || y != 0.) {
            self.state.heading = (x.atan2(-y) - self.state.camera_yaw).rem_euclid(TAU);
            self.speed = (x.hypot(y) * p.stick_maximum / p.direct_vehicle_stick_divisor)
                .min(straight_maximum);
        } else if !flying && y != 0. {
            self.state.heading =
                ((if y > 0. { PI } else { 0. }) - self.state.camera_yaw).rem_euclid(TAU);
            self.speed =
                (y.abs() * p.stick_maximum / p.direct_vehicle_stick_divisor).min(straight_maximum);
        }
        // Controller polling writes direct movement first. The inertial update
        // then replaces its speed while propulsion is still nonzero.
        if self.vehicle_speed != 0. {
            self.speed = self.vehicle_speed;
        }
    }
}

fn approach(current: f32, target: f32, step: f32) -> f32 {
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

#[cfg(test)]
pub(super) mod tests;

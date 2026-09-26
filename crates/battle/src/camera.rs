//! Ordinary battle framing and the stored-scene return (F1CC / FE9C).
mod result;
use crate::{Activity, Actor, ActorId, Control, distance};
use anyhow::{Result, ensure};
pub use result::ResultCameraParameters;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ActorFraming {
    /// Degrees added to the target's framing angle by its original profile.
    pub yaw_offset: f32,
    pub minimum_radius: f32,
    pub large: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub eye: [f32; 3],
    pub focus: [f32; 3],
    /// Angles are in degrees; positions and radius use battle world units.
    pub pitch: f32,
    pub yaw: f32,
    pub radius: f32,
}

#[derive(Debug, Clone)]
pub struct CameraDefinition {
    pub leader: ActorId,
    pub target: ActorId,
    pub stage_pitch: f32,
    /// The original player's camera option: frame both combatants, or stay
    /// nearer the controlled actor at a fixed base radius.
    pub adaptive: bool,
    pub initial: CameraPose,
}

/// Source operands for the ordinary entry slide, separate from stored scenes.
#[derive(Debug, Clone, Copy)]
pub struct EntryCamera {
    pub initial_yaw: f32,
    pub radius: f32,
    pub focus_x: f32,
    pub focus_speed_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Tracking,
    Entry,
    Returning,
}

/// The camera visit owner; original pause bits belong to the source adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pause {
    None,
    Command,
    Target,
}

// FE9C's target-change interpolation operands. These are the values at
// US_r_Top2Btl .rodata+740, +750, +794, +7AC and +7B4 respectively.
// Keep them separate from the ordinary tracking operands: +296 is sampled
// before the actor-common visit decrements it, and changes both FE9C rates.
// +740 is an f64 whose exact bits encode the original f32 0.1.
const FE9C_TARGET_YAW_BASE: f64 = 0.10000000149011612;
const FE9C_TARGET_YAW_DIVISOR: f64 = 120.;
const FE9C_TARGET_FOCUS_SLOPE: f32 = 0.01;
const FE9C_FOCUS_INTERCEPT: f32 = 6.;
const FE9C_FOCUS_LIMIT: f32 = 25.;
// FE9C +7C0/+7C8. The source promotes these operands to f64.
const FE9C_TRACKING_RADIUS_INTERCEPT: f64 = 4.;
const FE9C_TRACKING_RADIUS_SLOPE: f64 = 0.07500000298023224;
const FE9C_TRANSITION_RADIUS_INTERCEPT: f64 = 15.;
const FE9C_TRANSITION_RADIUS_SLOPE: f64 = 0.15;
const FE9C_COMMAND_FOCUS_SLOPE: f32 = 0.2;
const FE9C_COMMAND_FOCUS_INTERCEPT: f32 = 6.;
// Selector bit4: F1CC +6E4; FE9C +778, +798 and f64 +7B8.
const F1CC_SELECTOR_PITCH: f32 = 7.5;
const FE9C_SELECTOR_PITCH_STEP: f32 = 0.75;
const FE9C_SELECTOR_FOCUS_STEP: f32 = 15.;
const FE9C_SELECTOR_RADIUS_SLOPE: f64 = 0.15000000596046448;

#[derive(Debug, Clone)]
pub(crate) struct Camera {
    pub definition: CameraDefinition,
    pub pose: CameraPose,
    pub phase: Phase,
    reversed: bool,
    turning: Option<u16>,
    pub minimum_radius: f32,
    pub minimum_pitch: f32,
    pub remaining: u16,
    ordinary_entry: Option<(EntryCamera, f32)>,
    result_parameters: Option<ResultCameraParameters>,
    result_orbit: Option<result::Orbit>,
}

impl Camera {
    pub fn new(definition: CameraDefinition, actors: &[Actor]) -> Result<Self> {
        ensure!(
            definition.leader.index() < actors.len() && definition.target.index() < actors.len(),
            "invalid prepared camera actor"
        );
        ensure!(
            matches!(
                actors[definition.leader.index()].control,
                Control::Manual | Control::SemiAuto
            ),
            "automatic camera framing is not prepared"
        );
        let pose = definition.initial;
        ensure!(
            definition.stage_pitch.is_finite()
                && pose
                    .eye
                    .into_iter()
                    .chain(pose.focus)
                    .chain([pose.pitch, pose.yaw, pose.radius])
                    .all(f32::is_finite)
                && pose.radius > 0.,
            "invalid prepared battle camera"
        );
        Ok(Self {
            definition,
            pose,
            phase: Phase::Tracking,
            reversed: false,
            turning: None,
            minimum_radius: 0.,
            minimum_pitch: 0.,
            remaining: 0,
            ordinary_entry: None,
            result_parameters: None,
            result_orbit: None,
        })
    }

    pub fn initialize_entry(&mut self, actors: &[Actor], entry: EntryCamera) -> Result<()> {
        ensure!(
            [
                entry.initial_yaw,
                entry.radius,
                entry.focus_x,
                entry.focus_speed_scale
            ]
            .into_iter()
            .all(f32::is_finite)
                && entry.radius > 0.
                && entry.focus_x != 0.
                && entry.focus_speed_scale > 0.,
            "invalid ordinary battle entry camera"
        );
        self.pose.yaw = entry.initial_yaw;
        // 10820 selects the target framing, offsets its yaw by -5, then
        // 10668 selects it once more before committing the initial pose.
        self.step_inner(actors, None, Some((entry, false)), Pause::None)?;
        self.pose.yaw -= 5.;
        self.step_inner(actors, None, Some((entry, true)), Pause::None)
    }

    pub fn entry_pending(&self) -> bool {
        self.ordinary_entry.is_some()
    }

    pub fn constrain(&mut self, duration: u16, radius: f32, pitch: f32) {
        if self.remaining != 0 {
            self.remaining = self.remaining.max(duration);
            self.minimum_radius = self.minimum_radius.max(radius);
            self.minimum_pitch = self.minimum_pitch.max(pitch);
        } else {
            self.remaining = duration;
            self.minimum_radius = radius;
            self.minimum_pitch = pitch;
        }
    }

    pub fn step(&mut self, actors: &[Actor], entry: Option<ActorId>) -> Result<()> {
        if self.result_orbit.is_some() {
            return Ok(());
        }
        self.step_inner(actors, entry, None, Pause::None)
    }

    /// Source command visits hold desired yaw but still run camera approach,
    /// projection and radius/focus clocks on the retained frame.
    pub fn step_command_pause(&mut self, actors: &[Actor]) -> Result<()> {
        if self.result_orbit.is_some() {
            return Ok(());
        }
        self.step_inner(actors, None, None, Pause::Command)
    }

    /// 3648 retains selector bit4 through camera, drawing and the release visit.
    pub fn step_target_selector(&mut self, actors: &[Actor]) -> Result<()> {
        if self.result_orbit.is_some() {
            return Ok(());
        }
        self.step_inner(actors, None, None, Pause::Target)
    }

    fn step_inner(
        &mut self,
        actors: &[Actor],
        entry: Option<ActorId>,
        initialize: Option<(EntryCamera, bool)>,
        pause: Pause,
    ) -> Result<()> {
        let command_pause = pause == Pause::Command;
        let target_selector = pause == Pause::Target;
        let ordinary = initialize
            .map(|(entry, _)| entry)
            .or_else(|| self.ordinary_entry.map(|(entry, _)| entry));
        let leader = &actors[self.definition.leader.index()];
        let target = &actors[self.definition.target.index()];
        let mut endpoints = [
            clamp_endpoint(leader.position),
            clamp_endpoint(target.position),
        ];
        if self.reversed {
            endpoints.swap(0, 1);
        }
        let separation = planar_length(sub(endpoints[0], endpoints[1]));
        let direction = distance::planar_direction(endpoints[1], endpoints[0], [1., 0., 0.]);
        let mut yaw = self.pose.yaw;
        // F1CC tests bit4 before deriving yaw, so changing selector targets
        // must not reverse the endpoint order or start a turn.
        if !target_selector {
            yaw = (f64::from(direction[0]).atan2(f64::from(-direction[2])) as f32) * 57.295_79
                + target.framing.yaw_offset;
            if distance::length(direction) >= 0.1 {
                // Whole turns preserve the original +/-270 window without an
                // unbounded loop for a malformed external starting angle.
                if yaw < self.pose.yaw - 270. {
                    yaw += ((self.pose.yaw - yaw - 270.) / 360.).ceil() * 360.;
                } else if yaw > self.pose.yaw + 270. {
                    yaw -= ((yaw - self.pose.yaw - 270.) / 360.).ceil() * 360.;
                }
                if (yaw - self.pose.yaw).abs() > 40. {
                    self.turning.get_or_insert(0);
                }
                yaw -= 5.;
            }
            if (yaw - self.pose.yaw).abs() > 95. {
                self.reversed = !self.reversed;
                self.phase = Phase::Tracking;
                self.turning = None;
                yaw = self.pose.yaw;
            }
        }
        let mut focus = if ordinary.is_some() {
            [0.; 3]
        } else if let Some(owner) = entry {
            actors[owner.index()].position
        } else if self.definition.adaptive {
            add(endpoints[0], scale(direction, separation * 0.5))
        } else if target_selector {
            // E9B4 fixed-camera selector follows the target itself.
            target.position
        } else {
            add(
                leader.position,
                scale(
                    distance::planar_direction(target.position, leader.position, [1., 0., 0.]),
                    (separation * 0.5).min(150.),
                ),
            )
        };
        focus[1] = 0.;
        let single_enemy = actors
            .iter()
            .filter(|actor| actor.side == crate::Side::Enemy && actor.hp > 0 && !actor.petrified)
            .count()
            <= 1;
        let mut radius = if let Some(ordinary) = ordinary.filter(|_| self.definition.adaptive) {
            ordinary.radius
        } else if self.definition.adaptive {
            let base = if single_enemy { 1000. } else { 1400. };
            3.5_f32.mul_add(separation.max(300.), base)
        } else {
            2400.
        };
        if self.remaining != 0 {
            radius = radius.max(self.minimum_radius);
        }
        if self.definition.adaptive {
            if target.framing.large {
                radius = radius.max(2800.);
            }
            radius = radius.max(target.framing.minimum_radius);
            radius = radius.max(if single_enemy { 1950. } else { 2400. });
            radius = radius.min(6000. - planar_length(focus));
            if let Some(owner) = entry {
                radius = 1900.;
                if owner == self.definition.target {
                    radius = radius.max(target.framing.minimum_radius);
                }
            }
        }
        // F1CC applies the arena-edge focus correction after selecting radius.
        let mut line = sub(endpoints[0], endpoints[1]);
        line[1] = 0.;
        let edge = if f64::from(distance::length(line)) > 0.01 {
            let line = distance::normalize(line);
            distance::dot(scale(endpoints[0], -1.), [-line[2], 0., line[0]]).abs()
        } else {
            0.
        };
        let chord = (-edge).mul_add(edge, 722500.);
        let chord = if chord > 0. { chord.sqrt() } else { chord };
        if 2. * chord <= 552.5 {
            let toward = distance::planar_direction(self.pose.focus, self.pose.eye, [0., 0., 1.]);
            focus = add(focus, scale(toward, edge - 646185.94_f32.sqrt()));
        }
        let mut pitch = 8. + self.definition.stage_pitch;
        if target_selector {
            pitch += F1CC_SELECTOR_PITCH;
        }
        if self.remaining != 0 {
            pitch = pitch.max(self.minimum_pitch);
        }
        if pause != Pause::None || entry.is_some() || self.phase != Phase::Tracking {
            yaw = self.pose.yaw;
        }
        if pause == Pause::None && entry.is_none() && self.phase == Phase::Tracking {
            self.remaining = self.remaining.saturating_sub(1);
        }
        if let Some((ordinary, commit)) = initialize {
            self.pose.yaw = yaw;
            if commit {
                // 10668's initial eye has no FE9C per-update height addition.
                let old_focus = focus;
                focus[0] = ordinary.focus_x;
                focus[2] = 0.;
                let motion = scale(sub(old_focus, focus), ordinary.focus_speed_scale);
                focus[1] = 120.;
                let angle = |degrees: f32| f64::from(degrees * 0.017_453_289);
                let offset = [
                    angle(yaw).cos() as f32,
                    angle(pitch).sin() as f32,
                    angle(yaw).sin() as f32,
                ];
                self.pose = CameraPose {
                    eye: add(focus, scale(offset, radius)),
                    focus,
                    pitch,
                    yaw,
                    radius,
                };
                self.ordinary_entry = Some((ordinary, distance::length(motion)));
                self.turning = None;
            }
            return Ok(());
        }
        let delta = f64::from((yaw - self.pose.yaw).abs());
        // FE9C's +296 focus branch is nested under mode != 2 and lifecycle
        // state > 2. Camera::new admits only the manual/semi-auto modes, and
        // ActorAvailability::Active is the host's live-owner state; the
        // paired-opening watch proves the native owner is mode 1/state 3
        // (0x08031000). Do not let a retained highlight affect a KO/auto
        // owner while the unmodeled native low bits are unavailable here.
        let target_highlight = leader.hud.target_highlight != 0
            && leader.available()
            && matches!(leader.control, Control::Manual | Control::SemiAuto);
        let yaw_step = if matches!(leader.activity, Activity::Action { .. } | Activity::Hurt) {
            if delta > 45. {
                (0.1_f32 as f64 + delta / 80.) as f32
            } else {
                0.1
            }
        } else if target_highlight {
            // FE9C reads obj+0x296 after the action (5/9) branch. The native
            // target-change path is 0.1 + |delta| / 120, before the ordinary
            // 15/40-degree and transition thresholds.
            (FE9C_TARGET_YAW_BASE + delta / FE9C_TARGET_YAW_DIVISOR) as f32
        } else if let Some(age) = &mut self.turning {
            if *age <= 60 {
                *age += 1;
                (0.1_f32 as f64 + delta / 80.) as f32
            } else {
                (0.2_f32 as f64 + delta / 30.) as f32
            }
        } else if delta < 40. {
            if delta < 15. {
                0.1
            } else {
                (0.1_f32 as f64 + delta / 80.) as f32
            }
        } else {
            (0.2_f32 as f64 + delta / 60.) as f32
        };
        if approach(&mut self.pose.yaw, yaw, yaw_step) {
            self.turning = None;
        }
        if entry.is_none() {
            approach(
                &mut self.pose.pitch,
                pitch,
                if target_selector {
                    FE9C_SELECTOR_PITCH_STEP
                } else {
                    0.375
                },
            );
        }
        // Original rounded coefficient, followed by double-precision trig.
        let angle = |degrees: f32| f64::from(degrees * 0.017_453_289);
        let offset = [
            angle(self.pose.yaw).cos() as f32,
            angle(self.pose.pitch).sin() as f32,
            angle(self.pose.yaw).sin() as f32,
        ];
        let mut eye = scale(offset, self.pose.radius);
        eye[1] += 120.;
        focus[1] = if entry.is_some() {
            50_f32.mul_add(1. - (6000. - self.pose.radius) / 3600., 100.)
        } else {
            120.
        };
        let distance = planar_length(sub(focus, self.pose.focus));
        let focus_step = if let Some((_, speed)) = self.ordinary_entry {
            speed
        } else if self.phase != Phase::Tracking {
            0.1_f32.mul_add(distance, 6.).min(180.)
        } else if target_selector {
            FE9C_SELECTOR_FOCUS_STEP * if self.definition.adaptive { 1. } else { 1.5 }
        } else if command_pause && (leader.control == Control::Auto || !leader.available()) {
            FE9C_COMMAND_FOCUS_SLOPE
                .mul_add(distance, FE9C_COMMAND_FOCUS_INTERCEPT)
                .min(FE9C_FOCUS_LIMIT)
        } else if target_highlight {
            // FE9C's +296 branch uses +7AC*t + +794 and the shared +7B4 cap.
            // Retain the existing adaptive mode multiplier used by this host
            // for the corresponding ordinary tracking branch.
            (FE9C_TARGET_FOCUS_SLOPE.mul_add(distance, FE9C_FOCUS_INTERCEPT)
                * if self.definition.adaptive { 1. } else { 1.5 })
            .min(FE9C_FOCUS_LIMIT)
        } else {
            (0.175_f32.mul_add(distance, 6.) * if self.definition.adaptive { 1. } else { 1.5 })
                .min(25.)
        };
        let focus_arrived = approach_position(&mut self.pose.focus, focus, focus_step);
        // Eye placement uses the previous radius, before this approach.
        let difference = f64::from((self.pose.radius - radius).abs());
        let radius_step = if target_selector {
            FE9C_SELECTOR_RADIUS_SLOPE.mul_add(difference, FE9C_TRANSITION_RADIUS_INTERCEPT)
        } else if self.phase != Phase::Tracking {
            FE9C_TRANSITION_RADIUS_SLOPE.mul_add(difference, FE9C_TRANSITION_RADIUS_INTERCEPT)
        } else {
            FE9C_TRACKING_RADIUS_SLOPE.mul_add(difference, FE9C_TRACKING_RADIUS_INTERCEPT)
        } as f32;
        approach(&mut self.pose.radius, radius, radius_step);
        eye = add(eye, self.pose.focus);
        let eye_step = 0.2_f32.mul_add(distance::length(sub(eye, self.pose.eye)), 12.);
        if approach_position(&mut self.pose.eye, eye, eye_step) && self.phase == Phase::Returning {
            self.phase = Phase::Tracking;
        }
        if focus_arrived {
            self.ordinary_entry = None;
        }
        ensure!(
            self.pose
                .eye
                .into_iter()
                .chain(self.pose.focus)
                .chain([self.pose.pitch, self.pose.yaw, self.pose.radius])
                .all(f32::is_finite),
            "battle camera overflow"
        );
        Ok(())
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn scale(a: [f32; 3], scale: f32) -> [f32; 3] {
    a.map(|v| v * scale)
}
fn planar_length([x, _, z]: [f32; 3]) -> f32 {
    distance::length([x, 0., z])
}
fn clamp_endpoint(position: [f32; 3]) -> [f32; 3] {
    let length = planar_length(position);
    if length > 900. {
        add(
            position,
            scale(
                distance::planar_direction([0.; 3], position, [1., 0., 0.]),
                length - 900.,
            ),
        )
    } else {
        position
    }
}
fn approach(value: &mut f32, target: f32, step: f32) -> bool {
    if (*value - target).abs() <= step {
        *value = target;
        true
    } else {
        *value += if target > *value { step } else { -step };
        false
    }
}
fn approach_position(value: &mut [f32; 3], target: [f32; 3], step: f32) -> bool {
    let difference = sub(target, *value);
    let length = distance::length(difference);
    if length <= step {
        *value = target;
        true
    } else {
        let direction = if length >= 0.1 {
            distance::normalize(difference)
        } else {
            difference
        };
        *value = add(*value, scale(direction, step));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_entry_uses_wide_pose_and_fixed_focus_motion_before_tracking() -> Result<()> {
        let mut actors = [
            crate::tests::actor(crate::Side::Party),
            crate::tests::actor(crate::Side::Enemy),
        ];
        actors[0].position = [-300., 0., 0.];
        actors[1].position = [300., 0., 0.];
        let mut camera = Camera::new(
            CameraDefinition {
                leader: ActorId(0),
                target: ActorId(1),
                stage_pitch: -1.,
                adaptive: true,
                initial: CameraPose {
                    eye: [0., 120., 1750.],
                    focus: [0., 120., 0.],
                    pitch: 0.,
                    yaw: 85.,
                    radius: 4200.,
                },
            },
            &actors,
        )?;
        camera.initialize_entry(
            &actors,
            EntryCamera {
                initial_yaw: 85.,
                radius: 4200.,
                focus_x: 700.,
                focus_speed_scale: 1. / 59.,
            },
        )?;
        assert_eq!(camera.pose.focus, [700., 120., 0.]);
        assert!((camera.pose.yaw - 85.).abs() < 0.0001);
        assert_eq!((camera.pose.pitch, camera.pose.radius), (7., 4200.));
        let initial_eye_y = camera.pose.eye[1];
        let mut visits = 0;
        while camera.entry_pending() && visits < 65 {
            camera.step(&actors, None)?;
            visits += 1;
            assert_eq!(camera.pose.radius, 4200.);
        }
        // Completion comes from the SDK vector approach, including its rounding,
        // rather than a host-side counter or guessed animation duration.
        assert!((59..=60).contains(&visits));
        assert_eq!(camera.pose.focus, [0., 120., 0.]);
        assert!(camera.pose.eye[1] > initial_eye_y);
        camera.step(&actors, None)?;
        assert!(camera.pose.radius < 4200.);
        Ok(())
    }

    #[test]
    fn preparation_rejects_stale_handles_and_nonfinite_camera_values() -> Result<()> {
        let actors = [
            crate::tests::actor(crate::Side::Party),
            crate::tests::actor(crate::Side::Enemy),
        ];
        let definition = CameraDefinition {
            leader: ActorId(0),
            target: ActorId(1),
            stage_pitch: -1.,
            adaptive: true,
            initial: CameraPose {
                eye: [0., 490., 2050.],
                focus: [0., 120., 0.],
                pitch: 7.,
                yaw: 90.,
                radius: 2050.,
            },
        };
        Camera::new(definition.clone(), &actors)?;
        let mut invalid = definition.clone();
        invalid.target = ActorId(2);
        assert!(Camera::new(invalid, &actors).is_err());
        let mut invalid = definition.clone();
        invalid.leader = ActorId(255);
        assert!(Camera::new(invalid, &actors).is_err());
        for value in [f32::NAN, f32::INFINITY, -1., 0.] {
            let mut invalid = definition.clone();
            invalid.initial.radius = value;
            assert!(Camera::new(invalid, &actors).is_err());
        }
        let mut invalid = definition;
        invalid.initial.focus[0] = f32::NAN;
        assert!(Camera::new(invalid, &actors).is_err());
        Ok(())
    }

    #[test]
    fn fe9c_target_highlight_is_sampled_before_ordinary_yaw_and_focus_rates() -> Result<()> {
        let make = |target_highlight, control| -> Result<(Camera, [Actor; 2])> {
            let mut actors = [
                crate::tests::actor(crate::Side::Party),
                crate::tests::actor(crate::Side::Enemy),
            ];
            actors[0].position = [0., 0., 0.];
            actors[1].position = [100., 0., 0.];
            actors[0].control = control;
            actors[0].hud.target_highlight = target_highlight;
            let camera = Camera::new(
                CameraDefinition {
                    leader: ActorId(0),
                    target: ActorId(1),
                    stage_pitch: 0.,
                    adaptive: true,
                    initial: CameraPose {
                        eye: [0., 120., 100.],
                        focus: [0., 120., 0.],
                        pitch: 0.,
                        yaw: 0.,
                        radius: 100.,
                    },
                },
                &actors,
            )?;
            Ok((camera, actors))
        };

        let (mut highlighted, highlighted_actors) = make(1, Control::Manual)?;
        highlighted.step(&highlighted_actors, None)?;
        // +296 is nonzero: 0.1 + 85/120, before the ordinary turning rate.
        assert!(
            (f64::from(highlighted.pose.yaw) - (FE9C_TARGET_YAW_BASE + 85. / 120.)).abs() < 1e-5
        );
        // Focus is 50 units away: +7AC*50 + +794 = 6.5.
        assert!((highlighted.pose.focus[0] - 6.5).abs() < 1e-5);

        let (mut nurse, nurse_actors) = make(0, Control::Manual)?;
        nurse.step(&nurse_actors, None)?;
        // The maintained Nurse fixture has the default zero countdown and
        // retains the ordinary prepared turning/focus coefficients.
        assert!((f64::from(nurse.pose.yaw) - (f64::from(0.1_f32) + 85. / 80.)).abs() < 1e-5);
        assert!((nurse.pose.focus[0] - 14.75).abs() < 1e-5);

        let (mut action, mut action_actors) = make(1, Control::SemiAuto)?;
        action_actors[0].activity = Activity::Hurt;
        action.step(&action_actors, None)?;
        // FE9C checks action/hurt (the 5/9 branch) before +296; it therefore
        // keeps the existing action rate even while a highlight is active.
        assert!((f64::from(action.pose.yaw) - (f64::from(0.1_f32) + 85. / 80.)).abs() < 1e-5);

        let (mut dead, mut dead_actors) = make(1, Control::Manual)?;
        dead_actors[0].availability = crate::ActorAvailability::Dead;
        dead.step(&dead_actors, None)?;
        // State <= 2 takes FE9C's other coefficient family; this host keeps
        // that family in its ordinary fallback, but must not use +296.
        assert!((f64::from(dead.pose.yaw) - (FE9C_TARGET_YAW_BASE + 85. / 120.)).abs() > 1e-5);
        assert!((dead.pose.focus[0] - 6.5).abs() > 1e-5);
        Ok(())
    }

    #[test]
    fn selector_camera_uses_bit4_operands_without_reversing_or_consuming_timers() -> Result<()> {
        assert_eq!(FE9C_SELECTOR_RADIUS_SLOPE.to_bits(), 0x3fc3333340000000);
        for adaptive in [false, true] {
            let mut actors = [
                crate::tests::actor(crate::Side::Party),
                crate::tests::actor(crate::Side::Enemy),
            ];
            actors[1].position = [100., 0., 0.];
            let mut camera = Camera::new(
                CameraDefinition {
                    leader: ActorId(0),
                    target: ActorId(1),
                    stage_pitch: 0.,
                    adaptive,
                    initial: CameraPose {
                        eye: [0., 120., 1000.],
                        focus: [65., 120., 0.],
                        pitch: 0.,
                        yaw: -100.,
                        radius: 1000.,
                    },
                },
                &actors,
            )?;
            camera.remaining = 7;
            camera.turning = Some(9);
            camera.step_target_selector(&actors)?;
            assert_eq!(camera.remaining, 7);
            assert!(
                !camera.reversed,
                "F1CC bit4 bypasses the185-degree target reversal"
            );
            assert_eq!(camera.turning, None, "FE9C still approaches the held yaw");
            assert_eq!(camera.pose.yaw, -100.);
            assert_eq!(camera.pose.pitch, 0.75);
            // E9B4: midpoint50 in adaptive mode; target100 in fixed mode.
            // FE9C: source selector focus15 or15*1.5, without the25 cap.
            assert!((camera.pose.focus[0] - if adaptive { 50. } else { 87.5 }).abs() < 0.0001);
            assert_eq!(camera.pose.radius, if adaptive { 1172.5 } else { 1225. });
            for _ in 0..30 {
                camera.step_target_selector(&actors)?;
            }
            assert_eq!(camera.pose.pitch, 15.5);
            assert_eq!(camera.remaining, 7);
        }
        Ok(())
    }

    #[test]
    fn command_visit_uses_source_radius_operands_and_holds_countdown() -> Result<()> {
        assert!(
            (FE9C_TRACKING_RADIUS_INTERCEPT + FE9C_TRACKING_RADIUS_SLOPE * 100.
                - 11.500000298023224)
                .abs()
                < 1e-9
        );
        assert_eq!(
            FE9C_TRANSITION_RADIUS_INTERCEPT + FE9C_TRANSITION_RADIUS_SLOPE * 100.,
            30.
        );
        let mut actors = [
            crate::tests::actor(crate::Side::Party),
            crate::tests::actor(crate::Side::Enemy),
        ];
        actors[1].position = [100., 0., 0.];
        let mut camera = Camera::new(
            CameraDefinition {
                leader: ActorId(0),
                target: ActorId(1),
                stage_pitch: 0.,
                adaptive: true,
                initial: CameraPose {
                    eye: [0., 120., 1000.],
                    focus: [0.; 3],
                    pitch: 0.,
                    yaw: 0.,
                    radius: 1000.,
                },
            },
            &actors,
        )?;
        camera.remaining = 7;
        camera.step_command_pause(&actors)?;
        assert_eq!(camera.remaining, 7);
        assert_eq!(camera.pose.yaw, 0.);
        Ok(())
    }
}

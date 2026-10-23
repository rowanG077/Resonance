mod result;
mod spell;
use crate::{Actor, ActorId, Control, SpellCamera, distance};
use anyhow::{Result, ensure};

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
    pub stage_pitch: f32,
    /// Adjust distance to the leader and target; otherwise use fixed zoom.
    pub auto_zoom: bool,
    /// Restrict ordinary tracking to an unobstructed side of the arena.
    pub yaw_limits: Option<[f32; 2]>,
}

/// Camera updates are independent of the actor clock during entry and selection.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Update {
    Tracking,
    Held,
    Selecting(ActorId),
    Spell(ActorId),
}

// Camera framing uses the same lens as drawing and screen-space controls.
pub const VERTICAL_FOV_DEGREES: f32 = 13.33;
const ASPECT_RATIO: f32 = 4. / 3.;
const FRAMING_MARGIN: f32 = 24.;
// Keep subjects inside the central 75% of the viewport, leaving room for HUD and wide poses.
const FRAME_FRACTION: f32 = 0.75;
const MINIMUM_RADIUS: f32 = 1950.;
const RESPONSE: f32 = 0.12;
const FOCUS_HEIGHT: f32 = 120.;
const BASE_PITCH: f32 = 8.;
const SCENE_PITCH_SPEED: f32 = 0.375;
const EYE_HEIGHT: f32 = 240.;
const EYE_SPEED: f32 = 12.;
const EYE_RESPONSE: f32 = 0.2;
const VIEW_ANGLE: f32 = 85.;
const MINIMUM_TURN_SPEED: f32 = 0.1;
const FINE_TURN_ANGLE: f32 = 15.;
const WIDE_TURN_ANGLE: f32 = 40.;
const TURN_RESPONSE: f32 = 1. / 80.;
const WIDE_TURN_RESPONSE: f32 = 1. / 30.;
const FOCUS_SPEED: f32 = 6.;
const FOCUS_RESPONSE: f32 = 0.175;
const TARGET_SETTLE_RESPONSE: f32 = 0.01;
const MAXIMUM_FOCUS_SPEED: f32 = 25.;
const SELECTION_RADIUS: f32 = 1900.;
const SELECTION_PITCH: f32 = 7.5;
const DISTANCE_SCALE: f32 = 3.5;
const MINIMUM_SEPARATION: f32 = 300.;
const ZOOM_SPEED: f32 = 4.;
const ZOOM_RESPONSE: f32 = 0.075;
const FIXED_ZOOM_RADIUS: f32 = 2400.;
const SINGLE_ENEMY_RADIUS: f32 = 1000.;
const MULTIPLE_ENEMY_RADIUS: f32 = 1400.;
const FIXED_ZOOM_LOOK_AHEAD: f32 = 150.;
const ENTRY_RADIUS: f32 = 4200.;
const ENTRY_FOCUS_X: f32 = 700.;

#[derive(Debug, Clone)]
pub(crate) struct Camera {
    pub definition: CameraDefinition,
    pub pose: CameraPose,
    base_radius: f32,
    pending_radius: f32,
    result_tick: u32,
    spell: Option<spell::Shot>,
    scene: Option<SpellCamera>,
    distance_override: Option<DistanceOverride>,
    summon_focus: Option<SummonFocus>,
}

#[derive(Debug, Clone, Copy)]
struct DistanceOverride {
    distance: f32,
    remaining: u16,
}

#[derive(Debug, Clone, Copy)]
struct SummonFocus {
    target: [f32; 3],
    distance: f32,
    remaining: u16,
}

#[cfg(feature = "test-support")]
impl crate::Battle {
    /// Restore an ordinary camera at the recorded idle input boundary.
    pub fn restore_fixture_camera(
        &mut self,
        pose: CameraPose,
        pending_radius: f32,
        target_settle_remaining: u16,
    ) -> Result<CameraPose> {
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "fixture camera requires a combat input boundary"
        );
        ensure!(
            pose.eye
                .into_iter()
                .chain(pose.focus)
                .chain([pose.pitch, pose.yaw, pose.radius, pending_radius])
                .all(f32::is_finite)
                && pose.radius > 0.
                && pending_radius > 0.,
            "invalid recorded camera"
        );
        let camera = self
            .camera
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture has no ordinary camera"))?;
        ensure!(
            camera.spell.is_none()
                && camera.scene.is_none()
                && camera.distance_override.is_none()
                && camera.summon_focus.is_none()
                && camera.result_tick == 0,
            "fixture camera overlaps an active shot"
        );
        ensure!(
            target_settle_remaining <= crate::state::TARGET_SETTLE_UPDATES,
            "invalid recorded target settling interval"
        );
        self.runtime[camera.definition.leader.index()].target_settle_remaining =
            target_settle_remaining;
        camera.pose = pose;
        camera.pending_radius = pending_radius;
        Ok(camera.frame())
    }
}

impl CameraPose {
    /// Rebuild all Cartesian coordinates from one current spherical pose.
    fn place_eye(&mut self) {
        let (pitch_sin, pitch_cos) = self.pitch.to_radians().sin_cos();
        let (yaw_sin, yaw_cos) = self.yaw.to_radians().sin_cos();
        self.eye = add(
            self.focus,
            [
                self.radius * pitch_cos * yaw_cos,
                self.radius * pitch_sin,
                self.radius * pitch_cos * yaw_sin,
            ],
        );
    }
}

impl Camera {
    fn constrain_yaw(&mut self) {
        if let Some([minimum, maximum]) = self.definition.yaw_limits {
            let center = (minimum + maximum) * 0.5;
            let yaw = center + (self.pose.yaw - center + 180.).rem_euclid(360.) - 180.;
            self.pose.yaw = yaw.clamp(minimum, maximum);
        }
    }

    pub(crate) fn frame(&self) -> CameraPose {
        self.spell.as_ref().map_or(self.pose, spell::Shot::frame)
    }

    pub(crate) fn frame_entry(&mut self, update: u8) {
        self.spell = None;
        self.scene = None;
        self.distance_override = None;
        let progress = f32::from(update) / f32::from(crate::prepare::ENTRY_PAN_UPDATES);
        self.pose.focus = [ENTRY_FOCUS_X * (1. - progress), FOCUS_HEIGHT, 0.];
        self.pose.pitch = BASE_PITCH + self.definition.stage_pitch;
        self.pose.yaw = VIEW_ANGLE;
        self.constrain_yaw();
        self.pose.radius = ENTRY_RADIUS;
        self.pending_radius = ENTRY_RADIUS;
        let (sin, cos) = self.pose.yaw.to_radians().sin_cos();
        let eye_height = if update == 0 {
            FOCUS_HEIGHT
        } else {
            EYE_HEIGHT
        };
        self.pose.eye = [
            self.pose.focus[0] + ENTRY_RADIUS * cos,
            eye_height + ENTRY_RADIUS * self.pose.pitch.to_radians().sin(),
            ENTRY_RADIUS * sin,
        ];
    }

    pub fn new(definition: CameraDefinition, actors: &[Actor], target: ActorId) -> Result<Self> {
        ensure!(
            definition.leader.index() < actors.len() && target.index() < actors.len(),
            "invalid prepared camera actor"
        );
        ensure!(
            actors[definition.leader.index()].side == crate::Side::Party,
            "battle camera requires a party leader"
        );
        ensure!(
            definition.stage_pitch.is_finite(),
            "invalid battle camera pitch"
        );
        ensure!(
            definition.yaw_limits.is_none_or(|[minimum, maximum]| {
                minimum.is_finite()
                    && maximum.is_finite()
                    && minimum <= maximum
                    && maximum - minimum < 360.
            }),
            "invalid battle camera yaw limits"
        );
        let direction = sub(
            actors[target.index()].position,
            actors[definition.leader.index()].position,
        );
        let base_radius = if actors
            .iter()
            .filter(|a| a.side == crate::Side::Enemy)
            .count()
            > 1
        {
            MULTIPLE_ENEMY_RADIUS
        } else {
            SINGLE_ENEMY_RADIUS
        };
        let radius = if definition.auto_zoom {
            base_radius + DISTANCE_SCALE * direction[0].hypot(direction[2]).max(MINIMUM_SEPARATION)
        } else {
            FIXED_ZOOM_RADIUS
        };
        let mut camera = Self {
            pose: CameraPose {
                eye: [0.; 3],
                focus: [0.; 3],
                pitch: (BASE_PITCH + definition.stage_pitch).clamp(-80., 80.),
                yaw: direction[2].atan2(direction[0]).to_degrees() + VIEW_ANGLE,
                radius,
            },
            definition,
            base_radius,
            pending_radius: radius,
            result_tick: 0,
            spell: None,
            scene: None,
            distance_override: None,
            summon_focus: None,
        };
        let leader = &actors[camera.definition.leader.index()];
        camera.pose.focus =
            controlled_focus(leader, &actors[target.index()], camera.definition.auto_zoom);
        let automatic = leader.control == Control::Auto || !leader.available();
        if automatic {
            let subjects: Vec<_> = actors
                .iter()
                .filter(|actor| actor.available())
                .map(Bounds::actor)
                .collect();
            camera.pose.yaw = direction[2].atan2(direction[0]).to_degrees() + 90.;
            camera.pose.focus = bounds_center(&subjects);
            camera.pose.radius = camera
                .fitted_radius(&subjects, camera.pose.focus)
                .max(MINIMUM_RADIUS);
            camera.constrain_yaw();
            camera.pose.place_eye();
        } else {
            camera.constrain_yaw();
            // Start at focus height; tracking begins on the next visit.
            camera.pose.eye = controlled_eye(camera.pose);
            camera.pose.eye[1] -= EYE_HEIGHT - FOCUS_HEIGHT;
        }
        camera.pending_radius = camera.pose.radius;
        camera.validate_frame()?;
        Ok(camera)
    }

    pub(crate) fn request_scene(&mut self, request: SpellCamera) {
        self.scene = Some(match self.scene.filter(|current| current.duration != 0) {
            Some(current) => SpellCamera {
                bounds: resonance_content::battle_scene::CameraBounds {
                    minimum_distance: current
                        .bounds
                        .minimum_distance
                        .max(request.bounds.minimum_distance),
                    minimum_elevation: current
                        .bounds
                        .minimum_elevation
                        .max(request.bounds.minimum_elevation),
                },
                duration: current.duration.max(request.duration),
            },
            None => request,
        });
    }

    pub(crate) fn request_distance(&mut self, distance: f32, duration: u16) {
        self.distance_override = Some(DistanceOverride {
            distance,
            remaining: duration,
        });
    }

    pub(crate) fn request_summon_focus(&mut self, target: [f32; 3], distance: f32, duration: u16) {
        self.summon_focus = Some(SummonFocus {
            target: [target[0], FOCUS_HEIGHT, target[2]],
            distance,
            remaining: duration,
        });
    }

    pub(crate) fn clear_scene(&mut self) {
        self.spell = None;
        self.scene = None;
        self.distance_override = None;
        self.summon_focus = None;
    }

    pub(crate) fn step(
        &mut self,
        actors: &[Actor],
        target: ActorId,
        target_settling: bool,
        update: Update,
    ) -> Result<()> {
        if matches!(update, Update::Held) {
            return Ok(());
        }
        if let Update::Spell(actor) = update {
            let shot = self
                .spell
                .get_or_insert_with(|| spell::Shot::new(self.pose));
            shot.cast(actors[actor.index()].position, self.definition.stage_pitch);
            return self.validate_frame();
        }
        if matches!(update, Update::Selecting(_))
            && let Some(shot) = self.spell.take()
        {
            self.pose = shot.frame();
            self.pending_radius = self.pose.radius;
        }
        let bounds = self
            .scene
            .filter(|scene| scene.duration != 0)
            .map(|scene| scene.bounds);
        let minimum_distance = bounds.map_or(0., |bounds| bounds.minimum_distance);
        let distance_override = matches!(update, Update::Tracking)
            .then_some(self.distance_override)
            .flatten()
            .map(|request| request.distance);
        let summon_focus = matches!(update, Update::Tracking)
            .then_some(self.summon_focus)
            .flatten();
        let selected = match update {
            Update::Selecting(actor) => Some(actor),
            _ => None,
        };
        let base_pitch = BASE_PITCH + self.definition.stage_pitch;
        let scene_pitch = bounds.map_or(base_pitch, |bounds| {
            base_pitch.max(bounds.minimum_elevation)
        });
        let leader = &actors[self.definition.leader.index()];
        let eye = if matches!(update, Update::Tracking | Update::Selecting(_))
            && leader.control != Control::Auto
            && leader.available()
        {
            let direction = sub(actors[target.index()].position, leader.position);
            let separation = direction[0].hypot(direction[2]);
            let bearing = direction[2].atan2(direction[0]);
            if selected.is_none() && separation > 0.1 {
                // Either end of the combat line can face screen-right. Keep the
                // nearer view when actors cross instead of orbiting behind them.
                let desired_yaw = bearing.to_degrees() + VIEW_ANGLE;
                let yaw_delta = (desired_yaw - self.pose.yaw + 90.).rem_euclid(180.) - 90.;
                let error = yaw_delta.abs();
                let speed = if error < FINE_TURN_ANGLE {
                    MINIMUM_TURN_SPEED
                } else if error < WIDE_TURN_ANGLE {
                    MINIMUM_TURN_SPEED + error * TURN_RESPONSE
                } else {
                    2. * MINIMUM_TURN_SPEED + error * WIDE_TURN_RESPONSE
                };
                self.pose.yaw += yaw_delta.clamp(-speed, speed);
            }
            let scene_pitch = if selected.is_some() {
                scene_pitch.max(base_pitch + SELECTION_PITCH)
            } else {
                scene_pitch
            };
            self.pose.pitch +=
                (scene_pitch - self.pose.pitch).clamp(-SCENE_PITCH_SPEED, SCENE_PITCH_SPEED);
            let focus = if let Some(selected) = selected {
                let position = actors[selected.index()].position;
                [position[0], FOCUS_HEIGHT, position[2]]
            } else if let Some(focus) = summon_focus {
                focus.target
            } else {
                controlled_focus(leader, &actors[target.index()], self.definition.auto_zoom)
            };
            let focus_distance = distance::length(sub(focus, self.pose.focus));
            // A newly selected opponent gets a gentler pan while its actor-owned
            // settling interval remains active. Selection holds that interval.
            let response = if target_settling && selected.is_none() {
                TARGET_SETTLE_RESPONSE
            } else {
                FOCUS_RESPONSE
            };
            let focus_speed = (FOCUS_SPEED + response * focus_distance).min(MAXIMUM_FOCUS_SPEED);
            approach_position(&mut self.pose.focus, focus, focus_speed);
            let desired_radius = distance_override
                .unwrap_or_else(|| {
                    if selected.is_some() && self.definition.auto_zoom {
                        SELECTION_RADIUS
                    } else if self.definition.auto_zoom {
                        self.base_radius + DISTANCE_SCALE * separation.max(MINIMUM_SEPARATION)
                    } else {
                        FIXED_ZOOM_RADIUS
                    }
                })
                .max(minimum_distance);
            let desired_radius = summon_focus.map_or(desired_radius, |focus| focus.distance);
            if selected.is_some() {
                // Do not spend the selector's first update zooming toward the
                // previous combat target before adopting the selected target.
                self.pending_radius = desired_radius;
            }
            let delta = self.pending_radius - self.pose.radius;
            let speed = ZOOM_SPEED + ZOOM_RESPONSE * delta.abs();
            self.pose.radius += delta.clamp(-speed, speed);
            // Combat distance framing follows the preceding position sample.
            self.pending_radius = desired_radius;
            self.constrain_yaw();
            controlled_eye(self.pose)
        } else {
            if matches!(update, Update::Tracking) {
                let direction = sub(actors[target.index()].position, leader.position);
                if direction[0].hypot(direction[2]) > 0.1 {
                    let desired = direction[2].atan2(direction[0]).to_degrees() + 90.;
                    let delta = (desired - self.pose.yaw + 90.).rem_euclid(180.) - 90.;
                    self.pose.yaw += delta * RESPONSE;
                }
            }
            let subjects: Vec<_> = actors
                .iter()
                .enumerate()
                .filter(|(index, actor)| {
                    actor.available()
                        && match update {
                            Update::Selecting(id) => *index == id.index(),
                            _ => true,
                        }
                })
                .map(|(_, actor)| Bounds::actor(actor))
                .collect();
            if subjects.is_empty() {
                return Ok(());
            }
            let focus = summon_focus.map_or_else(|| bounds_center(&subjects), |focus| focus.target);
            let focus_step = (distance::length(sub(focus, self.pose.focus)) * RESPONSE)
                .clamp(1., MAXIMUM_FOCUS_SPEED);
            approach_position(&mut self.pose.focus, focus, focus_step);
            let pitch = (base_pitch
                + if matches!(update, Update::Selecting(_)) {
                    SELECTION_PITCH
                } else {
                    0.
                })
            .max(scene_pitch)
            .clamp(-80., 80.);
            let pitch_delta = pitch - self.pose.pitch;
            self.pose.pitch += if self.scene.is_some() {
                pitch_delta.clamp(-SCENE_PITCH_SPEED, SCENE_PITCH_SPEED)
            } else {
                pitch_delta * RESPONSE
            };
            let radius = summon_focus.map_or_else(
                || {
                    distance_override
                        .unwrap_or_else(|| self.fitted_radius(&subjects, focus).max(MINIMUM_RADIUS))
                        .max(minimum_distance)
                },
                |focus| focus.distance,
            );
            self.pose.radius += (radius - self.pose.radius) * RESPONSE;
            self.pending_radius = self.pose.radius;
            self.constrain_yaw();
            let mut desired = self.pose;
            desired.place_eye();
            desired.eye
        };
        let eye_speed = EYE_SPEED + EYE_RESPONSE * distance::length(sub(eye, self.pose.eye));
        approach_position(&mut self.pose.eye, eye, eye_speed);
        if let Some(shot) = &mut self.spell
            && shot.return_to(self.pose)
        {
            self.spell = None;
        }
        // Selection and the caster close-up can move the camera without spending
        // the active scene's framing interval. Retain expiry only for pitch return.
        if matches!(update, Update::Tracking)
            && let Some(scene) = &mut self.scene
        {
            scene.duration = scene.duration.saturating_sub(1);
            if scene.duration == 0 && self.pose.pitch == base_pitch {
                self.scene = None;
            }
        }
        if matches!(update, Update::Tracking)
            && let Some(request) = &mut self.distance_override
        {
            request.remaining = request.remaining.saturating_sub(1);
            if request.remaining == 0 {
                self.distance_override = None;
            }
        }
        if matches!(update, Update::Tracking)
            && let Some(focus) = &mut self.summon_focus
        {
            focus.remaining = focus.remaining.saturating_sub(1);
            if focus.remaining == 0 {
                self.summon_focus = None;
            }
        }
        self.validate_frame()
    }

    fn validate_frame(&self) -> Result<()> {
        let frame = self.frame();
        ensure!(
            frame
                .eye
                .into_iter()
                .chain(frame.focus)
                .chain([frame.radius])
                .all(f32::is_finite),
            "battle camera overflow"
        );
        Ok(())
    }

    fn fitted_radius(&self, subjects: &[Bounds], focus: [f32; 3]) -> f32 {
        let (sin, cos) = self.pose.yaw.to_radians().sin_cos();
        let (pitch_sin, pitch_cos) = self.pose.pitch.to_radians().sin_cos();
        let right = [sin, 0., -cos];
        let forward = [cos * pitch_cos, pitch_sin, sin * pitch_cos];
        let up = [-cos * pitch_sin, pitch_cos, -sin * pitch_sin];
        let vertical_tangent = (VERTICAL_FOV_DEGREES * 0.5).to_radians().tan() * FRAME_FRACTION;
        let horizontal_tangent = vertical_tangent * ASPECT_RATIO;
        let mut radius: f32 = 0.;
        for bounds in subjects {
            for corner in 0..8 {
                let point = std::array::from_fn(|axis| {
                    if corner & (1 << axis) == 0 {
                        bounds.minimum[axis]
                    } else {
                        bounds.maximum[axis]
                    }
                });
                let offset = sub(point, focus);
                let width = distance::dot(offset, right).abs();
                let height = distance::dot(offset, up).abs();
                let depth = distance::dot(offset, forward);
                radius =
                    radius.max(depth + (width / horizontal_tangent).max(height / vertical_tangent));
            }
        }
        radius
    }
}

/// Current world-space envelope of native actor geometry, independent of artwork.
struct Bounds {
    minimum: [f32; 3],
    maximum: [f32; 3],
}

impl Bounds {
    fn actor(actor: &Actor) -> Self {
        let mut bounds = Self {
            minimum: actor.position,
            maximum: actor.position,
        };
        for height in [f32::NEG_INFINITY, f32::INFINITY] {
            if let Some(point) = actor.hurt_point(height) {
                bounds.include(point.center, actor.body_radius());
            }
        }
        // The effect center also supplies a framing height for actors without a collider.
        bounds.include(
            add(
                actor.position,
                actor.body.center_offset.map(|v| 2. * v * actor.body.scale),
            ),
            0.,
        );
        let margin = FRAMING_MARGIN * actor.body.scale;
        bounds.minimum = bounds.minimum.map(|v| v - margin);
        bounds.maximum = bounds.maximum.map(|v| v + margin);
        bounds
    }

    fn include(&mut self, point: [f32; 3], radius: f32) {
        for (axis, value) in point.into_iter().enumerate() {
            self.minimum[axis] = self.minimum[axis].min(value - radius);
            self.maximum[axis] = self.maximum[axis].max(value + radius);
        }
    }
}

fn bounds_center(subjects: &[Bounds]) -> [f32; 3] {
    let Some(first) = subjects.first() else {
        return [0.; 3];
    };
    let mut minimum = first.minimum;
    let mut maximum = first.maximum;
    for bounds in subjects.iter().skip(1) {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(bounds.minimum[axis]);
            maximum[axis] = maximum[axis].max(bounds.maximum[axis]);
        }
    }
    scale(add(minimum, maximum), 0.5)
}

fn controlled_eye(pose: CameraPose) -> [f32; 3] {
    let (sin, cos) = pose.yaw.to_radians().sin_cos();
    [
        pose.focus[0] + pose.radius * cos,
        EYE_HEIGHT + pose.radius * pose.pitch.to_radians().sin(),
        pose.focus[2] + pose.radius * sin,
    ]
}

fn controlled_focus(leader: &Actor, target: &Actor, auto_zoom: bool) -> [f32; 3] {
    let direction = sub(target.position, leader.position);
    let separation = direction[0].hypot(direction[2]);
    let fraction = if auto_zoom || separation <= 2. * FIXED_ZOOM_LOOK_AHEAD {
        0.5
    } else {
        FIXED_ZOOM_LOOK_AHEAD / separation
    };
    [
        leader.position[0] + direction[0] * fraction,
        FOCUS_HEIGHT,
        leader.position[2] + direction[2] * fraction,
    ]
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
fn approach_position(value: &mut [f32; 3], target: [f32; 3], step: f32) -> bool {
    let difference = sub(target, *value);
    let length = distance::length(difference);
    if length <= step {
        *value = target;
        true
    } else {
        *value = add(*value, scale(difference, step / length));
        false
    }
}

#[cfg(test)]
mod tests;

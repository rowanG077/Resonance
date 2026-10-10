mod result;
use crate::{Actor, ActorId, Control, distance};
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
    /// Include the active roster; otherwise frame the leader and target.
    pub adaptive: bool,
}

/// Only input selection may move the camera while gameplay is held.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Update {
    Tracking,
    Held,
    Selecting(ActorId),
}

// Camera framing uses the same lens as drawing and screen-space controls.
pub const VERTICAL_FOV_DEGREES: f32 = 13.33;
const ASPECT_RATIO: f32 = 4. / 3.;
const FRAMING_MARGIN: f32 = 24.;
// Keep subjects inside the central 75% of the viewport, leaving room for HUD and wide poses.
const FRAME_FRACTION: f32 = 0.75;
const MINIMUM_RADIUS: f32 = 1950.;
const RESPONSE: f32 = 0.12;

#[derive(Debug, Clone)]
pub(crate) struct Camera {
    pub definition: CameraDefinition,
    pub pose: CameraPose,
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
        let direction = sub(
            actors[target.index()].position,
            actors[definition.leader.index()].position,
        );
        let mut camera = Self {
            pose: CameraPose {
                eye: [0.; 3],
                focus: [0.; 3],
                pitch: (8. + definition.stage_pitch).clamp(-80., 80.),
                yaw: direction[2].atan2(direction[0]).to_degrees() + 90.,
                radius: MINIMUM_RADIUS,
            },
            definition,
        };
        camera.pose.focus = bounds_center(&camera.subjects(actors, target, None));
        camera.pose.place_eye();
        camera.step(actors, target, Update::Tracking)?;
        Ok(camera)
    }

    fn subjects(&self, actors: &[Actor], target: ActorId, focus: Option<ActorId>) -> Vec<Bounds> {
        let leader = &actors[self.definition.leader.index()];
        actors
            .iter()
            .enumerate()
            .filter(|(index, actor)| {
                actor.available()
                    && match focus {
                        Some(id) => *index == id.index(),
                        None => {
                            self.definition.adaptive
                                || leader.control == Control::Auto
                                || !leader.available()
                                || *index == self.definition.leader.index()
                                || *index == target.index()
                        }
                    }
            })
            .map(|(_, actor)| Bounds::actor(actor))
            .collect()
    }

    pub(crate) fn step(&mut self, actors: &[Actor], target: ActorId, update: Update) -> Result<()> {
        if matches!(update, Update::Held) {
            return Ok(());
        }
        let focus_actor = match update {
            Update::Tracking => None,
            Update::Selecting(target) => Some(target),
            Update::Held => unreachable!(),
        };
        let subjects = self.subjects(actors, target, focus_actor);
        if subjects.is_empty() {
            return Ok(());
        }
        let focus = bounds_center(&subjects);
        if matches!(update, Update::Tracking) {
            let target = &actors[target.index()];
            let direction = sub(
                target.position,
                actors[self.definition.leader.index()].position,
            );
            if direction[0].hypot(direction[2]) > 0.1 {
                let desired = direction[2].atan2(direction[0]).to_degrees() + 90.;
                let mut delta = (desired - self.pose.yaw + 180.).rem_euclid(360.) - 180.;
                // A battle line has two equivalent viewing sides. Keep the nearer side.
                if delta > 90. {
                    delta -= 180.;
                }
                if delta < -90. {
                    delta += 180.;
                }
                self.pose.yaw += delta * RESPONSE;
            }
        }
        let pitch = 8.
            + self.definition.stage_pitch
            + if matches!(update, Update::Selecting(_)) {
                7.5
            } else {
                0.
            };
        self.pose.pitch += (pitch.clamp(-80., 80.) - self.pose.pitch) * RESPONSE;
        let focus_step = (distance::length(sub(focus, self.pose.focus)) * RESPONSE).max(1.);
        approach_position(&mut self.pose.focus, focus, focus_step);
        let radius = self.fitted_radius(&subjects).max(MINIMUM_RADIUS);
        // Expansion keeps the current pose visible; contraction remains smooth.
        self.pose.radius = radius.max(self.pose.radius + (radius - self.pose.radius) * RESPONSE);
        self.pose.place_eye();
        ensure!(
            self.pose
                .eye
                .into_iter()
                .chain(self.pose.focus)
                .chain([self.pose.radius])
                .all(f32::is_finite),
            "battle camera overflow"
        );
        Ok(())
    }

    fn fitted_radius(&self, subjects: &[Bounds]) -> f32 {
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
                let offset = sub(point, self.pose.focus);
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

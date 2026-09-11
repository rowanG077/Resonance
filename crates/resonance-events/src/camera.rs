//! An ordinary orbit/follow camera, driven by field event commands.
use crate::Actor;
use std::collections::BTreeMap;
pub mod motion;
use motion::{FovTween, MotionCamera, Tween};

/// Script-addressable camera target, present for the lifetime of a field.
pub const ANCHOR_ACTOR: i32 = 90_020;
pub const ANCHOR_RESOURCE: u32 = 24;

pub fn anchor() -> Actor {
    let position = [1., 0., 0.];
    Actor {
        visible: false,
        interaction_anchor: true,
        grounded: false,
        collidable: false,
        casts_shadow: false,
        autonomy: Some(crate::Autonomy::new(
            crate::Behavior::Stationary,
            0.,
            position,
        )),
        ..Actor::new(ANCHOR_RESOURCE, position)
    }
}

/// Persistent orbit settings; eye/target positions and easing history are rebuilt.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSettings {
    pub axes: [bool; 3],
    pub fixed_position: [f32; 3],
    pub angles: [f32; 3],
    pub offset: [f32; 3],
    pub distance: f32,
    pub fov_degrees: f32,
    pub position_bounds: [[f32; 2]; 3],
    pub target_bounds: [[f32; 2]; 3],
    pub position_rate: f32,
    pub target_rate: f32,
}
impl CameraSettings {
    pub fn entry(self, actor: i32) -> Result<EntryCamera, String> {
        if !self.fixed_position.iter().all(|v| v.is_finite())
            || !self
                .angles
                .iter()
                .chain(&self.offset)
                .all(|v| v.is_finite())
            || !self.distance.is_finite()
            || !(1. ..=100_000.).contains(&self.distance)
            || !(1. ..179.).contains(&self.fov_degrees)
            || ![self.position_rate, self.target_rate]
                .iter()
                .all(|v| (1. ..=1000.).contains(v))
            || !self
                .position_bounds
                .iter()
                .chain(&self.target_bounds)
                .all(|b| b.iter().all(|v| v.is_finite()) && b[0] <= b[1])
        {
            return Err("invalid saved camera settings".into());
        }
        Ok(EntryCamera {
            camera: FieldCamera {
                actor,
                axes: self.axes,
                position: self.fixed_position,
                follow: true,
                anchor_to_actor: true,
                angles: self.angles,
                offset: self.offset,
                distance: self.distance,
                fov_degrees: self.fov_degrees,
                position_bounds: self.position_bounds,
                target_bounds: self.target_bounds,
                ..Default::default()
            },
            position_rate: self.position_rate,
            target_rate: self.target_rate,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FieldCamera {
    pub actor: i32,
    pub offset: [f32; 3],
    pub follow: bool,
    pub anchor_to_actor: bool,
    pub axes: [bool; 3],
    pub angles: [f32; 3],
    pub distance: f32,
    pub fov_degrees: f32,
    pub position_bounds: [[f32; 2]; 3],
    pub target_bounds: [[f32; 2]; 3],
    pub position: [f32; 3],
    pub target: [f32; 3],
    anchor: [f32; 3],
    look_offset: [f32; 3],
}
impl Default for FieldCamera {
    fn default() -> Self {
        Self {
            actor: 1,
            offset: [0.; 3],
            follow: false,
            anchor_to_actor: false,
            axes: [true; 3],
            angles: [0.; 3],
            distance: 1000.,
            fov_degrees: 27.,
            position_bounds: [[-100000., 100000.]; 3],
            target_bounds: [[-100000., 100000.]; 3],
            position: [0.; 3],
            target: [0.; 3],
            anchor: [0.; 3],
            look_offset: [0., 1000., 0.],
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntryCamera {
    pub camera: FieldCamera,
    pub position_rate: f32,
    pub target_rate: f32,
}
impl EntryCamera {
    pub fn following(actor: i32) -> Self {
        Self {
            camera: FieldCamera {
                actor,
                follow: true,
                anchor_to_actor: true,
                angles: [322., 0., 42.],
                distance: 1890.,
                offset: [0., 0., 87.],
                ..Default::default()
            },
            position_rate: 8.,
            target_rate: 8.,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraRig {
    pub motion: Option<MotionCamera>,
    /// Native selector -1 edits the next field's entry camera, leaving this
    /// field's view live. This transient handoff is not part of a saved game.
    pub entry: Option<EntryCamera>,
    pub selected: usize,
    pub cameras: [FieldCamera; 4],
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub position_rate: f32,
    pub target_rate: f32,
    pub angles: [f32; 3],
    pub distance: f32,
    pub(crate) position_settled: bool,
    pub(crate) target_settled: bool,
}
impl Default for CameraRig {
    fn default() -> Self {
        Self {
            motion: None,
            entry: None,
            selected: 0,
            cameras: std::array::from_fn(|_| FieldCamera::default()),
            position: [0.; 3],
            target: [0.; 3],
            position_rate: 1.,
            target_rate: 1.,
            angles: [0.; 3],
            distance: 1000.,
            position_settled: false,
            target_settled: false,
        }
    }
}
impl CameraRig {
    /// Rebuild a follow view immediately when entering a saved field. This uses
    /// the current player position without advancing scripts or presentation.
    pub fn snap_follow_view(&mut self, actors: &BTreeMap<i32, Actor>) {
        self.angles = self.current().angles;
        self.distance = self.current().distance;
        let rates = (self.position_rate, self.target_rate);
        (self.position_rate, self.target_rate) = (1., 1.);
        self.position_settled = true;
        self.target_settled = true;
        self.step(actors);
        (self.position_rate, self.target_rate) = rates;
    }
    pub fn settings(&self, actor: i32) -> Result<CameraSettings, String> {
        let camera = self.current();
        if self.motion.is_some()
            || !camera.follow
            || !camera.anchor_to_actor
            || camera.actor != actor
        {
            return Err("quicksave requires the ordinary player-follow camera".into());
        }
        Ok(CameraSettings {
            axes: camera.axes,
            fixed_position: std::array::from_fn(|i| {
                if camera.axes[i] {
                    0.
                } else {
                    camera.position[i]
                }
            }),
            angles: camera.angles,
            offset: camera.offset,
            distance: camera.distance,
            fov_degrees: camera.fov_degrees,
            position_bounds: camera.position_bounds,
            target_bounds: camera.target_bounds,
            position_rate: self.position_rate,
            target_rate: self.target_rate,
        })
    }
    pub fn current(&self) -> &FieldCamera {
        &self.cameras[self.selected]
    }
    pub fn current_mut(&mut self) -> &mut FieldCamera {
        &mut self.cameras[self.selected]
    }
    pub fn command_camera(&mut self) -> &mut FieldCamera {
        if let Some(entry) = &mut self.entry {
            &mut entry.camera
        } else {
            &mut self.cameras[self.selected]
        }
    }
    pub fn step(&mut self, actors: &BTreeMap<i32, Actor>) {
        if let Some(motion) = &mut self.motion {
            (self.position, self.target) = motion.step(actors);
            return;
        }
        let camera = &mut self.cameras[self.selected];
        let rate = self.position_rate.max(1.);
        for i in 0..3 {
            let delta = (camera.angles[i] - self.angles[i] + 180.).rem_euclid(360.) - 180.;
            self.angles[i] = (self.angles[i] + delta.trunc() / rate).rem_euclid(360.);
        }
        self.distance += (camera.distance - self.distance) / rate;
        if camera.follow
            && let Some(actor) = actors.get(&camera.actor)
        {
            camera.target = std::array::from_fn(|i| actor.position[i] + camera.offset[i]);
            camera.look_offset = std::array::from_fn(|i| camera.target[i] - camera.position[i]);
            if camera.anchor_to_actor {
                camera.anchor = camera.target;
            }
        } else {
            // The source saves a direction and distance while following.
            // Keeping that vector directly preserves the authored view when
            // follow is disabled, without converting through Euler angles.
            camera.target = std::array::from_fn(|i| camera.position[i] + camera.look_offset[i]);
        }
        // Original camera order is Rz * Rx * Ry, applied to (0, -distance, 0).
        let [x, _, z] = self.angles.map(f32::to_radians);
        let orbit = [
            z.sin() * x.cos() * self.distance,
            -z.cos() * x.cos() * self.distance,
            -x.sin() * self.distance,
        ];
        for (i, amount) in orbit.into_iter().enumerate() {
            if camera.axes[i] {
                camera.position[i] = camera.anchor[i] + amount;
            }
            // Bounds can be configured one side at a time during initialization.
            camera.position[i] = camera.position[i]
                .max(camera.position_bounds[i][0])
                .min(camera.position_bounds[i][1]);
            camera.target[i] = camera.target[i]
                .max(camera.target_bounds[i][0])
                .min(camera.target_bounds[i][1]);
        }
        approach(
            &mut self.position,
            camera.position,
            self.position_rate,
            &mut self.position_settled,
        );
        approach(
            &mut self.target,
            camera.target,
            self.target_rate,
            &mut self.target_settled,
        );
    }
    pub fn settled(&self) -> bool {
        self.position_settled && self.target_settled
    }
    pub fn fov_degrees(&self) -> f32 {
        self.motion
            .as_ref()
            .map_or(self.current().fov_degrees, |m| m.fov.value as f32)
    }
    pub fn start_path(&mut self) {
        self.motion = Some(MotionCamera {
            mode: 0,
            actor: self.current().actor,
            position: Tween::new(self.position),
            offset: Tween::new(self.current().offset),
            angles: Tween::new([1., 0., 0.]),
            fov: FovTween::new(self.current().fov_degrees),
            target: self.target,
        });
    }
}
fn approach(current: &mut [f32; 3], target: [f32; 3], rate: f32, settled: &mut bool) {
    let delta: [f32; 3] = std::array::from_fn(|i| target[i] - current[i]);
    let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
    // Once a transition reaches its target, follow subsequent actor movement
    // directly until another camera command starts a transition.
    *settled |= distance < 1.;
    if distance == 0. {
        return;
    }
    let mut step = distance / if *settled { 1. } else { rate.max(1.) };
    if !*settled && distance > 0.5 {
        step = step.max(0.5);
    }
    for i in 0..3 {
        current[i] += delta[i] / distance * step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_rebuilds_follow_view_with_locked_axes() {
        let player = Actor::new(1, [1100., 567., 0.]);
        let mut camera = FieldCamera {
            follow: true,
            anchor_to_actor: true,
            axes: [true, true, false],
            position: [999., 888., 186.],
            angles: [354., 0., 359.],
            offset: [0., 0., 87.],
            distance: 954.,
            ..Default::default()
        };
        camera.target_bounds[1] = [-329.; 2];
        let mut original = CameraRig {
            angles: camera.angles,
            distance: camera.distance,
            position_rate: 8.,
            target_rate: 8.,
            ..Default::default()
        };
        *original.current_mut() = camera;
        let actors = [(1, player)].into();
        for _ in 0..240 {
            original.step(&actors);
        }
        let settings = original.settings(1).unwrap();
        assert_eq!(settings.fixed_position, [0., 0., 186.]);
        let entry = settings.clone().entry(1).unwrap();
        let mut restored = CameraRig {
            angles: entry.camera.angles,
            distance: entry.camera.distance,
            position_rate: entry.position_rate,
            target_rate: entry.target_rate,
            ..Default::default()
        };
        *restored.current_mut() = entry.camera;
        assert_ne!(restored.position, original.position);
        restored.snap_follow_view(&actors);
        assert_eq!(restored.position, original.position);
        assert_eq!(restored.target, original.target);
        assert_eq!(restored.settings(1).unwrap(), settings);
        restored.start_path();
        assert!(restored.settings(1).is_err());
        let mut invalid = settings;
        invalid.target_bounds[0] = [10., -10.];
        assert!(invalid.entry(1).is_err());
    }
}

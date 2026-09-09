//! An ordinary orbit/follow camera, driven by field event commands.
use crate::Actor;
use std::collections::BTreeMap;
pub mod motion;
use motion::{FovTween, MotionCamera, Tween};

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
pub struct CameraRig {
    pub motion: Option<MotionCamera>,
    pub selected: usize,
    pub cameras: [FieldCamera; 4],
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub position_rate: f32,
    pub target_rate: f32,
    pub angles: [f32; 3],
    pub distance: f32,
}
impl Default for CameraRig {
    fn default() -> Self {
        Self {
            motion: None,
            selected: 0,
            cameras: std::array::from_fn(|_| FieldCamera::default()),
            position: [0.; 3],
            target: [0.; 3],
            position_rate: 1.,
            target_rate: 1.,
            angles: [0.; 3],
            distance: 1000.,
        }
    }
}
impl CameraRig {
    pub fn current(&self) -> &FieldCamera {
        &self.cameras[self.selected]
    }
    pub fn current_mut(&mut self) -> &mut FieldCamera {
        &mut self.cameras[self.selected]
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
            approach(
                &mut self.position[i],
                camera.position[i],
                self.position_rate,
            );
            approach(&mut self.target[i], camera.target[i], self.target_rate);
        }
    }
    pub fn settled(&self) -> bool {
        (0..3).all(|i| {
            (self.position[i] - self.current().position[i]).abs() < 1.
                && (self.target[i] - self.current().target[i]).abs() < 1.
        })
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
fn approach(current: &mut f32, target: f32, rate: f32) {
    let delta = target - *current;
    *current = if delta.abs() < 1. {
        target
    } else {
        *current + delta / rate.max(1.)
    };
}

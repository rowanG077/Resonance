//! Scripted camera paths expressed as world-space motion and ordinary angles.
use crate::Actor;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Tween<const N: usize> {
    pub value: [f64; N],
    target: [f64; N],
    speed: f64,
    remaining: Option<u32>,
}
impl<const N: usize> Tween<N> {
    pub fn new(value: [f32; N]) -> Self {
        let value = value.map(f64::from);
        Self {
            value,
            target: value,
            speed: 0.,
            remaining: None,
        }
    }
    pub fn request(&mut self, target: [f32; N], timing: i32) -> Result<(), String> {
        self.target = target.map(f64::from);
        if timing < 0 {
            self.remaining = Some((timing as u32 & 0x7fff_ffff).max(1));
            self.speed = 0.;
        } else {
            if timing == 0 && self.target != self.value {
                return Err("camera path has zero speed".into());
            }
            self.remaining = None;
            self.speed = f64::from(timing) / 100.;
        }
        Ok(())
    }
    pub fn settled(&self) -> bool {
        self.remaining.is_none() && self.speed == 0.
    }
    fn step(&mut self) {
        let delta: [f64; N] = std::array::from_fn(|i| self.target[i] - self.value[i]);
        let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        let amount = if let Some(remaining) = self.remaining {
            // Camera motion clears its active flag one update after reaching the target.
            if remaining == 0 {
                self.remaining = None;
                self.speed = 0.;
                return;
            }
            self.remaining = Some(remaining - 1);
            1. / f64::from(remaining)
        } else if self.speed > 0. && distance >= 1. {
            (self.speed / distance).min(1.)
        } else {
            self.speed = 0.;
            return;
        };
        for (i, delta) in delta.into_iter().enumerate() {
            self.value[i] += delta * amount;
        }
        if amount == 1. {
            self.speed = 0.;
        }
    }
}

/// The original zoom controller uses a fixed signed increment, including for a
/// timed request, and stops within one degree. Position/angle paths have a
/// separate frame counter and must still reach their timed endpoints.
#[derive(Debug, Clone)]
pub struct FovTween {
    pub value: f64,
    target: f64,
    speed: f64,
}
impl FovTween {
    pub fn new(value: f32) -> Self {
        Self {
            value: f64::from(value),
            target: f64::from(value),
            speed: 0.,
        }
    }
    pub fn request(&mut self, target: f32, timing: i32) -> Result<(), String> {
        if timing == 0 && f64::from(target) != self.value {
            return Err("camera zoom has zero speed".into());
        }
        self.target = f64::from(target);
        // Timed zoom fixes its increment at the start.
        self.speed = if timing < 0 {
            (self.target - self.value) / f64::from((timing as u32 & 0x7fff_ffff).max(1))
        } else {
            f64::from(timing) / 100.
        };
        Ok(())
    }
    pub fn settled(&self) -> bool {
        self.speed == 0.
    }
    fn step(&mut self) {
        if self.settled() {
            return;
        }
        // Round the residual to single precision before the integer stop test.
        let delta = f64::from((self.target - self.value) as f32);
        if delta.trunc() == 0. {
            self.speed = 0.;
        } else {
            if delta.abs() < self.speed.abs() {
                self.speed = delta;
            }
            self.value += self.speed;
        }
    }
}

#[derive(Debug, Clone)]
pub struct MotionCamera {
    pub mode: u8,
    pub actor: i32,
    pub position: Tween<3>,
    pub offset: Tween<3>,
    pub angles: Tween<3>,
    pub fov: FovTween,
    pub target: [f32; 3],
}
impl MotionCamera {
    pub fn settled(&self, channel: u8) -> bool {
        match channel {
            9 => {
                self.position.settled()
                    && self.offset.settled()
                    && self.angles.settled()
                    && self.fov.settled()
            }
            10 => self.position.settled(),
            11 => self.offset.settled(),
            12 => self.angles.settled(),
            13 => self.fov.settled(),
            _ => false,
        }
    }
    pub(super) fn step(&mut self, actors: &BTreeMap<i32, Actor>) -> ([f32; 3], [f32; 3]) {
        self.position.step();
        self.offset.step();
        self.angles.step();
        self.fov.step();
        let position = self.position.value.map(|v| v as f32);
        if self.mode == 0 {
            if let Some(actor) = actors.get(&self.actor) {
                self.target =
                    std::array::from_fn(|i| actor.position[i] + self.offset.value[i] as f32);
            }
        } else {
            let [x, y, z] = self.angles.value.map(f64::to_radians);
            // Apply X, then Y, then Z rotation, looking along local negative Z.
            let direction = [
                -z.cos() * y.sin() * x.cos() - z.sin() * x.sin(),
                -z.sin() * y.sin() * x.cos() + z.cos() * x.sin(),
                -y.cos() * x.cos(),
            ];
            self.target = std::array::from_fn(|i| position[i] + (direction[i] * 1000.) as f32);
        }
        (position, self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_path_arrives_then_completes_without_overshooting() {
        let mut path = Tween::new([0., 0., 0.]);
        path.request([30., 40., 0.], (0x80000000u32 | 5) as i32)
            .unwrap();
        for _ in 0..4 {
            path.step();
        }
        assert_eq!(path.value, [24., 32., 0.]);
        assert!(!path.settled());
        path.step();
        assert_eq!(path.value, [30., 40., 0.]);
        assert!(!path.settled());
        path.step();
        assert_eq!(path.value, [30., 40., 0.]);
        assert!(path.settled());
    }
    #[test]
    fn first_classroom_camera_matches_the_independent_dolphin_direction() {
        let mut camera = MotionCamera {
            mode: 1,
            actor: 1,
            position: Tween::new([-81., -315., 138.]),
            offset: Tween::new([0.; 3]),
            angles: Tween::new([90., 0., 208.]),
            fov: FovTween::new(23.),
            target: [0.; 3],
        };
        let (_, target) = camera.step(&BTreeMap::new());
        for (a, b) in target.into_iter().zip([388.47153, -1197.9476, 138.]) {
            assert!((a - b).abs() < 0.001);
        }
    }
    #[test]
    fn classroom_bucket_zoom_matches_the_observed_settled_fov() {
        let mut zoom = FovTween::new(23.);
        zoom.request(35., (0x8000_0000u32 | 90) as i32).unwrap();
        for _ in 0..83 {
            zoom.step();
        }
        assert!(!zoom.settled());
        zoom.step();
        assert!(zoom.settled());
        // Independent Dolphin state, VI 9570; this is an observable camera
        // value, not a relaxed image tolerance or a classroom-only override.
        assert_eq!(zoom.value as f32, 34.066_666);
    }
    #[test]
    fn descending_zoom_keeps_its_signed_increment_and_completes_after_arrival() {
        let mut zoom = FovTween::new(35.);
        zoom.request(27., i32::MIN | 1).unwrap();
        zoom.step();
        assert_eq!(zoom.value, 27.);
        assert!(!zoom.settled());
        zoom.step();
        assert!(zoom.settled());
    }
}

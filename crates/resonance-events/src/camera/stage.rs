//! Camera channels used by prepared battle events, in battle world coordinates.
use super::motion::Tween;

const LOOK_DISTANCE: f32 = 500.;
// Original battle REL rodata+296C; this differs from rounding PI/180 today.
const DEGREES_TO_RADIANS: f32 = f32::from_bits(0x3c8e_fa33);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub vertical_fov_degrees: f32,
}

#[derive(Debug, Clone)]
pub struct Camera {
    eye: Tween<3>,
    vertical: Angle,
    heading: Angle,
    frame: Frame,
}

enum Command {
    Place([f32; 3], f32, f32),
    Move([f32; 3], i32),
    Vertical(f32, i32, bool),
    Heading(f32, i32, bool),
    Lens(f32),
}

impl TryFrom<[i32; 7]> for Command {
    type Error = String;

    fn try_from(arguments: [i32; 7]) -> Result<Self, String> {
        let timed = |value, counts: &[i32]| counts.iter().any(|count| value == (i32::MIN | *count));
        Ok(match arguments {
            [2, x, y, z, pitch, heading, _] => {
                Self::Place([x as f32, y as f32, z as f32], pitch as f32, heading as f32)
            }
            [3, x, y, z, timing, 0, 0] if timed(timing, &[40, 60, 120]) => {
                Self::Move([x as f32, y as f32, z as f32], timing)
            }
            [command @ (5 | 6), angle, timing, ..] => {
                Self::Vertical(angle as f32, timing, command == 6)
            }
            [command @ (7 | 8), angle, timing, ..] => {
                Self::Heading(angle as f32, timing, command == 8)
            }
            [10, lens, ..] if (1..1800).contains(&lens) => Self::Lens(lens as f32 * 0.1),
            _ => return Err(format!("unsupported battle camera command {arguments:?}")),
        })
    }
}

/// The original battle camera fixes its angular speed when a command arrives.
/// Targets are ordinary, unwrapped angles, and another command may replace an
/// active target using the current angle as its relative origin.
#[derive(Debug, Clone)]
struct Angle {
    value: f32,
    target: f32,
    speed: f32,
}

impl Angle {
    fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            speed: 1.,
        }
    }

    fn request(&mut self, angle: f32, timing: i32, relative: bool) {
        self.target = if relative { self.value + angle } else { angle };
        self.speed = if timing < 0 {
            (self.value - self.target).abs() / (timing as u32 & 0x7fff_ffff) as f32
        } else {
            timing as f32 * 0.01
        };
    }

    fn step(&mut self) {
        if (self.value - self.target).abs() <= self.speed {
            self.value = self.target;
            self.speed = 0.;
        } else if self.target > self.value {
            self.value += self.speed;
        } else {
            self.value -= self.speed;
        }
    }
}

impl Camera {
    pub fn new(vertical_fov_degrees: f32) -> Result<Self, String> {
        if !vertical_fov_degrees.is_finite()
            || vertical_fov_degrees <= 0.
            || vertical_fov_degrees >= 180.
        {
            return Err("invalid battle event lens".into());
        }
        Ok(Self {
            eye: Tween::new([0.; 3]),
            vertical: Angle::new(0.),
            heading: Angle::new(0.),
            frame: Frame {
                eye: [0.; 3],
                target: [0., 0., LOOK_DISTANCE],
                vertical_fov_degrees,
            },
        })
    }

    pub fn frame(&self) -> Frame {
        self.frame
    }

    pub(crate) fn command(&mut self, arguments: [i32; 7]) -> Result<(), String> {
        match Command::try_from(arguments)? {
            Command::Place(eye, pitch, heading) => {
                self.eye = Tween::new(eye);
                self.vertical = Angle::new(pitch);
                self.heading = Angle::new(heading);
            }
            Command::Move(eye, timing) => request(&mut self.eye, eye, timing)?,
            Command::Vertical(angle, timing, relative) => {
                self.vertical.request(angle, timing, relative);
            }
            Command::Heading(angle, timing, relative) => {
                self.heading.request(angle, timing, relative);
            }
            // Lens timing is ignored by the original service.
            Command::Lens(lens) => self.frame.vertical_fov_degrees = lens,
        }
        Ok(())
    }

    pub(crate) fn step(&mut self) {
        self.eye.step();
        self.vertical.step();
        self.heading.step();
        let eye = self.eye.value.map(|value| value as f32);
        let vertical = f64::from(self.vertical.value * DEGREES_TO_RADIANS);
        let heading = f64::from(self.heading.value * DEGREES_TO_RADIANS);
        // Horizontal aim is independent of vertical aim, rather than spherical pitch.
        let direction = [heading.sin(), vertical.sin(), heading.cos()];
        self.frame.eye = eye;
        self.frame.target =
            std::array::from_fn(|axis| eye[axis] + LOOK_DISTANCE * direction[axis] as f32);
    }
}

fn request<const N: usize>(
    channel: &mut Tween<N>,
    target: [f32; N],
    timing: i32,
) -> Result<(), String> {
    if !channel.settled() {
        return Err("battle camera channel is still moving".into());
    }
    channel.request(target, timing)
}

#[cfg(test)]
#[path = "stage/tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_view_publishes_camera_and_fade_on_the_presentation_clock() {
        let one = i32::MIN | 1;
        let mut camera = Camera::new(13.33).unwrap();
        camera.command([2, 76, 150, -102, 0, 0, 0]).unwrap();
        camera.command([5, -7, one, 0, 0, 0, 0]).unwrap();
        camera.command([7, 360, one, 0, 0, 0, 0]).unwrap();
        camera.step();
        camera
            .command([3, -4, 150, -102, i32::MIN | 120, 0, 0])
            .unwrap();
        for _ in 0..60 {
            camera.step();
        }
        assert_eq!(camera.frame().eye, [36., 150., -102.]);
        for _ in 0..61 {
            camera.step();
        }
        assert_eq!(camera.frame().eye, [-4., 150., -102.]);
        assert!((camera.frame().target[1] - 89.06533).abs() < 0.001);

        camera.command([2, 0, 390, -1022, 0, 0, 0]).unwrap();
        camera.command([5, -13, one, 0, 0, 0, 0]).unwrap();
        camera.command([10, 180, one, 0, 0, 0, 0]).unwrap();
        assert_eq!(camera.frame().vertical_fov_degrees, 18.);
        assert_eq!(camera.frame().eye, [-4., 150., -102.]);
        camera.step();
        assert_eq!(camera.frame().eye, [0., 390., -1022.]);
        assert!((camera.frame().target[1] - 277.52448).abs() < 0.001);
        assert!(camera.command([4, 0, 0, 0, one, 0, 0]).is_err());
        camera
            .command([10, 270, i32::MIN | 60, 0, 0, 0, 0])
            .unwrap();
        assert_eq!(camera.frame().vertical_fov_degrees, 27.);
        let mut fade = crate::battle_event::Fade::default();
        fade.command([2, 1, 50, 0, 0, 0, 0]).unwrap();
        for _ in 0..5 {
            fade.step();
        }
        assert_eq!(fade.opacity, 250);
        assert!(fade.command([1, 1, 50, 0, 0, 0, 0]).is_err());
        fade.step();
        assert_eq!(fade.opacity, 255);
        fade.command([1, 1, 50, 0, 0, 0, 0]).unwrap();
        fade.step();
        assert_eq!(fade.opacity, 205);
        for _ in 0..5 {
            fade.step();
        }
        assert_eq!(fade.opacity, 0);
        fade.command([2, 0, 26, 0, 0, 0, 0]).unwrap();
        for _ in 0..10 {
            fade.step();
        }
        assert_eq!((fade.color, fade.opacity), ([255; 3], 255));
        assert!(fade.command([4, 4, 0, 0, 0, 0, 0]).is_err());
    }
}

//! Camera channels used by prepared battle events, in battle world coordinates.
use super::motion::Tween;

const LOOK_DISTANCE: f32 = 500.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub vertical_fov_degrees: f32,
}

#[derive(Debug, Clone)]
pub struct Camera {
    eye: Tween<3>,
    vertical: Tween<1>,
    heading: Tween<1>,
    frame: Frame,
}

enum Command {
    Place([f32; 3]),
    Move([f32; 3], i32),
    Vertical(f32, i32),
    Heading(f32, i32),
    Lens(f32),
}

impl TryFrom<[i32; 7]> for Command {
    type Error = String;

    fn try_from(arguments: [i32; 7]) -> Result<Self, String> {
        let timed = |value, counts: &[i32]| counts.iter().any(|count| value == (i32::MIN | *count));
        Ok(match arguments {
            [2, x, y, z, 0, 0, 0] => Self::Place([x as f32, y as f32, z as f32]),
            [3, x, y, z, timing, 0, 0] if timed(timing, &[40, 60, 120]) => {
                Self::Move([x as f32, y as f32, z as f32], timing)
            }
            [5, angle, timing, 0, 0, 0, 0] if timed(timing, &[1, 60]) => {
                Self::Vertical(angle as f32, timing)
            }
            [7, angle, timing, 0, 0, 0, 0] if timed(timing, &[1, 60]) => {
                Self::Heading(angle as f32, timing)
            }
            [10, lens, timing, 0, 0, 0, 0] if timed(timing, &[1]) && (1..1800).contains(&lens) => {
                Self::Lens(lens as f32 / 10.)
            }
            _ => return Err(format!("unsupported battle camera command {arguments:?}")),
        })
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
            vertical: Tween::new([0.]),
            heading: Tween::new([0.]),
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
            Command::Place(eye) => {
                if !self.eye.settled() {
                    return Err("battle camera eye is still moving".into());
                }
                self.eye = Tween::new(eye);
            }
            Command::Move(eye, timing) => request(&mut self.eye, eye, timing)?,
            Command::Vertical(angle, timing) => request(&mut self.vertical, [angle], timing)?,
            Command::Heading(angle, timing) => request(&mut self.heading, [angle], timing)?,
            // The selected one-update lens takes effect before the next pose visit.
            Command::Lens(lens) => self.frame.vertical_fov_degrees = lens,
        }
        Ok(())
    }

    pub(crate) fn step(&mut self) {
        self.eye.step();
        self.vertical.step();
        self.heading.step();
        let eye = self.eye.value.map(|value| value as f32);
        let vertical = self.vertical.value[0].to_radians();
        let heading = self.heading.value[0].to_radians();
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
        assert!(
            camera
                .command([10, 270, i32::MIN | 60, 0, 0, 0, 0])
                .is_err()
        );
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

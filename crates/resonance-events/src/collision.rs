//! Convex half-space queries shared by field movement and projectiles.
#[derive(Clone, Copy)]
pub struct Plane {
    normal: [f32; 3],
    distance: f32,
}
impl Plane {
    pub fn triangle([a, b, c]: [[f32; 3]; 3]) -> Self {
        let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        Self {
            normal,
            distance: (0..3).map(|i| normal[i] * a[i]).sum(),
        }
    }
    pub fn expanded(mut self, half_size: [f32; 3]) -> Self {
        self.distance += (0..3)
            .map(|i| self.normal[i].abs() * half_size[i])
            .sum::<f32>();
        self
    }
    fn side(self, point: [f32; 3]) -> f32 {
        (0..3).map(|i| point[i] * self.normal[i]).sum::<f32>() - self.distance
    }
}

/// Return the first contact within a movement segment. Empty solids never block.
pub fn segment_contact(
    planes: impl IntoIterator<Item = Plane>,
    start: [f32; 3],
    end: [f32; 3],
) -> Option<f32> {
    contact_interval(planes, start, end).map(|(enter, _)| enter)
}

/// Sweep an axis-aligned body; touching a surface without entering it is allowed.
pub fn body_contact(
    planes: impl IntoIterator<Item = Plane>,
    start: [f32; 3],
    end: [f32; 3],
    half_size: [f32; 3],
) -> bool {
    contact_interval(
        planes.into_iter().map(|p| p.expanded(half_size)),
        start,
        end,
    )
    .is_some_and(|(enter, exit)| enter < exit)
}

fn contact_interval(
    planes: impl IntoIterator<Item = Plane>,
    start: [f32; 3],
    end: [f32; 3],
) -> Option<(f32, f32)> {
    let mut planes = planes.into_iter().peekable();
    planes.peek()?;
    let (mut enter, mut exit) = (0_f32, 1_f32);
    for plane in planes {
        let from = plane.side(start);
        let speed = plane.side(end) - from;
        if speed == 0. {
            if from >= 0. {
                return None;
            }
        } else if speed < 0. {
            enter = enter.max(-from / speed);
        } else {
            exit = exit.min(-from / speed);
        }
        if enter > exit {
            return None;
        }
    }
    Some((enter, exit))
}

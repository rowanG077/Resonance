use crate::{Trigger, TriggerShape};

impl Trigger {
    /// Lines/circles activate once until contact ends; polygons count contacts.
    pub fn activation_context(&self) -> Option<i16> {
        match self.shape {
            TriggerShape::Triangle(_) | TriggerShape::Quad(_) => Some(self.activations as i16),
            _ => (self.activations == 0).then_some(0),
        }
    }

    pub fn record_activation(&mut self, started: bool) {
        const MAX_CONTACT_COUNT: u16 = 9_999;
        match self.shape {
            TriggerShape::Triangle(_) | TriggerShape::Quad(_) => {
                self.activations = self.activations.saturating_add(1).min(MAX_CONTACT_COUNT);
            }
            _ if started => self.activations = 1,
            _ => {}
        }
    }

    /// Polygons test the center; lines/circles include the querying radius.
    pub fn touches(&self, p: [f32; 3], radius: f32) -> bool {
        let points: &[[f32; 3]] = match &self.shape {
            TriggerShape::Circle {
                center,
                radius: size,
            } => {
                return p[2] + radius >= center[2]
                    && p[2] <= center[2] + self.height
                    && (p[0] - center[0]).hypot(p[1] - center[1]) < radius + size;
            }
            TriggerShape::Line(points) => points,
            TriggerShape::Triangle(points) => points,
            TriggerShape::Quad(points) => points,
        };
        let low = points.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min);
        let high = points
            .iter()
            .map(|v| v[2])
            .fold(f32::NEG_INFINITY, f32::max);
        if p[2] + radius < low || p[2] > high + self.height {
            return false;
        }
        if points.len() > 2 {
            let sides = || {
                (0..points.len()).map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % points.len()]);
                    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
                })
            };
            return sides().all(|s| s >= 0.) || sides().all(|s| s <= 0.);
        }
        let (a, b) = (points[0], points[1]);
        let delta = [b[0] - a[0], b[1] - a[1]];
        let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
        let t = if length_squared > 0. {
            ((p[0] - a[0]) * delta[0] + (p[1] - a[1]) * delta[1]) / length_squared
        } else {
            0.
        }
        .clamp(0., 1.);
        (p[0] - a[0] - t * delta[0]).hypot(p[1] - a[1] - t * delta[1]) <= radius
    }
}

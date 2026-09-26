//! Persistent weapon ribbons. REL 1327C/12C38 retain eight endpoint samples;
//! 4D25C/4CB18 resample their natural cubic splines into sixteen draw columns.
use crate::{ActorId, Anchor, ModelFrame, WeaponFrame};
use anyhow::{Result, ensure};
use resonance_content::animation::transform_point;

type Point = [f32; 3];

/// Preparation resolves ordered KI endpoints to body or weapon bones.
/// A material resource belongs to the host; combat keeps only its ready ID.
#[derive(Debug, Clone)]
pub struct TrailDefinition {
    pub slot: u8,
    pub resource: u32,
    pub source: TrailSource,
}

#[derive(Debug, Clone)]
pub enum TrailSource {
    Body(Vec<Anchor>),
    Weapon { slot: u8, bones: Vec<u16> },
}

impl TrailDefinition {
    pub fn sample(&self, model: &ModelFrame, weapons: &[WeaponFrame]) -> Result<Vec<Point>> {
        let (world, pose, anchors) = match &self.source {
            TrailSource::Body(anchors) => (model.world, &model.bones, anchors.clone()),
            TrailSource::Weapon { slot, bones } => {
                let weapon = weapons
                    .iter()
                    .find(|weapon| weapon.owner == model.actor && weapon.slot == *slot);
                let weapon =
                    weapon.ok_or_else(|| anyhow::anyhow!("unprepared weapon trail slot {slot}"))?;
                (
                    weapon.world,
                    &weapon.bones,
                    bones
                        .iter()
                        .map(|&bone| Anchor {
                            bone,
                            offset: [0.; 3],
                        })
                        .collect(),
                )
            }
        };
        ensure!(
            self.slot < 8 && (2..=3).contains(&anchors.len()),
            "invalid weapon trail slot or endpoint count"
        );
        anchors
            .iter()
            .map(|anchor| {
                ensure!(
                    usize::from(anchor.bone) < pose.len()
                        && anchor.offset.iter().all(|v| v.is_finite()),
                    "invalid weapon trail anchor"
                );
                let point = transform_point(
                    world,
                    transform_point(pose[usize::from(anchor.bone)], anchor.offset),
                );
                ensure!(point.iter().all(|v| v.is_finite()), "invalid trail pose");
                Ok(point)
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailVertex {
    pub position: Point,
    pub alpha: u8,
}

/// Adjacent endpoint rows form fifteen quads, with source strip order
/// [row[i], row[i+1], next_row[i], next_row[i+1]]. The host applies atlas UVs.
#[derive(Debug, Clone, PartialEq)]
pub struct TrailFrame {
    pub actor: ActorId,
    pub slot: u8,
    pub resource: u32,
    pub rows: Vec<[TrailVertex; 16]>,
}

#[derive(Debug, Clone)]
pub(crate) struct Trail {
    definition: TrailDefinition,
    history: Vec<[Point; 8]>,
    count: usize,
    alpha: [u8; 16],
}

impl Trail {
    pub fn slot(&self) -> u8 {
        self.definition.slot
    }

    pub fn new(
        definition: TrailDefinition,
        model: &ModelFrame,
        weapons: &[WeaponFrame],
    ) -> Result<Self> {
        let points = definition.sample(model, weapons)?;
        Ok(Self {
            definition,
            history: points.into_iter().map(|point| [point; 8]).collect(),
            count: 0,
            alpha: [0; 16],
        })
    }

    /// Call at group two, before contact reactions. None suppresses collection
    /// during owner hitstop; an entirely paused battle must skip this call.
    /// Expiring the command timer never destroys or resets this object.
    pub fn tick(&mut self, timer: u8, model: Option<(&ModelFrame, &[WeaponFrame])>) -> Result<()> {
        let points = model
            .map(|(model, weapons)| self.definition.sample(model, weapons))
            .transpose()?;
        self.advance(timer != 0, points.as_deref());
        Ok(())
    }

    fn advance(&mut self, active: bool, points: Option<&[Point]>) {
        if let Some(points) = points {
            // The third endpoint does not participate in the native movement test.
            let stationary = self
                .history
                .iter()
                .zip(points)
                .take(2)
                .all(|(row, point)| row[0].iter().zip(point).all(|(a, b)| (a - b).abs() < 0.1));
            if stationary {
                self.count = self.count.saturating_sub(1);
            } else {
                for (row, &point) in self.history.iter_mut().zip(points) {
                    row.copy_within(0..self.count.min(7), 1);
                    row[0] = point;
                }
                if active {
                    self.count = (self.count + 1).min(8);
                }
            }
        }
        if active {
            self.alpha = std::array::from_fn(|i| 240 - 16 * i as u8);
        } else {
            self.count = self.count.saturating_sub(1);
            for alpha in &mut self.alpha {
                *alpha = alpha.saturating_sub(16);
            }
        }
    }

    pub fn frame(&self, actor: ActorId) -> Option<TrailFrame> {
        (self.count >= 3 && self.alpha[0] > 0).then(|| TrailFrame {
            actor,
            slot: self.definition.slot,
            resource: self.definition.resource,
            rows: self
                .history
                .iter()
                .map(|row| {
                    let samples = spline(&row[..self.count]);
                    std::array::from_fn(|i| TrailVertex {
                        position: samples[i],
                        alpha: self.alpha[i],
                    })
                })
                .collect(),
        })
    }
}

fn spline(points: &[Point]) -> [Point; 16] {
    let count = points.len();
    let mut knots = [0.; 8];
    for i in 1..count {
        let distance = (0..3)
            .map(|axis| (points[i][axis] - points[i - 1][axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        knots[i] = knots[i - 1] + distance;
    }
    let length = knots[count - 1];
    // One blade endpoint can remain fixed while another moves. Its spline is a
    // point; avoid the original zero-length normalization's non-finite output.
    if length == 0. {
        return [points[0]; 16];
    }
    for knot in &mut knots[1..count] {
        *knot /= length;
    }
    let mut widths = [0.; 8];
    let mut slopes = [[0.; 3]; 8];
    for i in 0..count - 1 {
        widths[i] = knots[i + 1] - knots[i];
        if widths[i] == 0. {
            widths[i] = 0.000001;
        }
        for axis in 0..3 {
            slopes[i + 1][axis] = (points[i + 1][axis] - points[i][axis]) / widths[i];
        }
    }
    // Solve the tridiagonal natural spline, whose endpoint curvature is zero.
    // Native coefficients are one sixth of the second derivatives.
    let mut curve = [[0.; 3]; 8];
    let mut diagonal = [0.; 8];
    diagonal[1] = 2. * (knots[2] - knots[0]);
    for axis in 0..3 {
        curve[1][axis] = slopes[2][axis] - slopes[1][axis];
    }
    for i in 1..count - 2 {
        let ratio = widths[i] / diagonal[i];
        diagonal[i + 1] = 2. * (knots[i + 2] - knots[i]) - ratio * widths[i];
        for axis in 0..3 {
            curve[i + 1][axis] = slopes[i + 2][axis] - slopes[i + 1][axis] - ratio * curve[i][axis];
        }
    }
    for i in (1..count - 1).rev() {
        let next = curve[i + 1];
        for (value, next) in curve[i].iter_mut().zip(next) {
            *value = (*value - widths[i] * next) / diagonal[i];
        }
    }
    std::array::from_fn(|i| {
        // The source excludes t=1; the final column is t=15/16.
        let t = i as f32 / 16.;
        let segment = knots[..count]
            .partition_point(|&knot| knot < t)
            .saturating_sub(1);
        let u = t - knots[segment];
        let width = widths[segment];
        std::array::from_fn(|axis| {
            let c0 = curve[segment][axis];
            let c1 = curve[segment + 1][axis];
            let p0 = points[segment][axis];
            let p1 = points[segment + 1][axis];
            let slope = (p1 - p0) / width - width * (2. * c0 + c1);
            u * (u * (3. * c0 + u * (c1 - c0) / width) + slope) + p0
        })
    })
}

#[cfg(test)]
mod tests;

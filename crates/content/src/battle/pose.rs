//! Sparse animation curves and deterministic, renderer-independent bone poses.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Conversion from native clip-frame units to the cooked glTF timeline.
pub const FRAME_HZ: f32 = 30.;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bone {
    /// An authored label; repeated labels do not merge indexed bones or tracks.
    pub name: String,
    pub parent: Option<u16>,
    pub bind_channels: TransformChannels,
    pub bind: Transform,
}

/// Authored transform presence, independent of its numerical defaults.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransformChannels(pub u8);

impl TransformChannels {
    pub fn scale(self) -> bool {
        self.0 & 1 != 0
    }

    pub fn any(self) -> bool {
        self.0 != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: [f32; 3],
    /// Quaternion components are x, y, z, w.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Motion {
    /// Native frame units. Callers control looping, playback rate and pauses.
    pub duration_frames: f32,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub bone: u16,
    pub period_frames: f32,
    pub times: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<VectorCurve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<VectorCurve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<QuaternionCurve>,
}

impl Track {
    pub fn channels(&self) -> TransformChannels {
        TransformChannels(
            u8::from(self.scale.is_some())
                | (u8::from(self.rotation.is_some()) << 2)
                | (u8::from(self.translation.is_some()) << 3),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorInterpolation {
    Step,
    Linear,
    Bezier,
    Hermite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorCurve {
    pub interpolation: VectorInterpolation,
    pub values: Vec<[f32; 3]>,
    /// Bezier control offsets or Hermite tangents, without time rescaling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming: Vec<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outgoing: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuaternionInterpolation {
    Step,
    ShortestSlerp,
    Squad,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuaternionCurve {
    pub interpolation: QuaternionInterpolation,
    pub values: Vec<[f32; 4]>,
    /// Spherical cubic controls retain their authored quaternion signs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming: Vec<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outgoing: Vec<[f32; 4]>,
}

/// Column-major affine matrix, in the model's authored coordinate system.
pub type Matrix = [[f32; 4]; 4];

#[derive(Debug, Clone)]
pub struct Pose {
    pub local: Vec<Transform>,
    pub global: Vec<Matrix>,
}

impl Skeleton {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.bones.is_empty() && self.bones.len() <= u16::MAX as usize,
            "invalid bone count"
        );
        for (index, bone) in self.bones.iter().enumerate() {
            ensure!(!bone.name.is_empty(), "empty name for bone {index}");
            bone.bind.validate()?;
            let mut cursor = bone.parent;
            let mut depth = 0;
            while let Some(parent) = cursor {
                ensure!(
                    usize::from(parent) != index && depth < self.bones.len(),
                    "cyclic bone hierarchy"
                );
                cursor = self
                    .bones
                    .get(usize::from(parent))
                    .context("invalid bone parent")?
                    .parent;
                depth += 1;
            }
        }
        Ok(())
    }

    pub fn bone(&self, name: &str) -> Option<u16> {
        self.bones
            .iter()
            .position(|bone| bone.name == name)
            .map(|i| i as u16)
    }

    pub fn bind_pose(&self) -> Result<Pose> {
        self.pose(self.bones.iter().map(|b| b.bind).collect())
    }

    pub fn sample(&self, motion: &Motion, frame: f32) -> Result<Pose> {
        ensure!(
            frame.is_finite() && frame >= 0. && frame <= motion.duration_frames,
            "motion frame outside clip"
        );
        let mut local = self.bones.iter().map(|bone| bone.bind).collect::<Vec<_>>();
        for track in &motion.tracks {
            let bind = local
                .get_mut(usize::from(track.bone))
                .context("motion bone outside skeleton")?;
            *bind = track.sample(frame, *bind)?;
        }
        self.pose(local)
    }

    /// Blend local transforms, then compose the hierarchy. The combat clock
    /// supplies the transition weight; this function does not advance time.
    pub fn blend(&self, from: &Pose, to: &Pose, weight: f32) -> Result<Pose> {
        ensure!(
            from.local.len() == self.bones.len()
                && to.local.len() == self.bones.len()
                && weight.is_finite()
                && (0. ..=1.).contains(&weight),
            "invalid pose blend"
        );
        self.pose(
            from.local
                .iter()
                .zip(&to.local)
                .map(|(a, b)| a.blend(*b, weight))
                .collect(),
        )
    }

    pub fn pose(&self, local: Vec<Transform>) -> Result<Pose> {
        let matrices = local
            .iter()
            .map(|transform| transform.matrix())
            .collect::<Vec<_>>();
        self.pose_with_matrices(local, &matrices)
    }

    /// Explicit local matrices retain model-controller products that contain shear.
    pub fn pose_with_matrices(&self, local: Vec<Transform>, matrices: &[Matrix]) -> Result<Pose> {
        ensure!(
            local.len() == self.bones.len() && matrices.len() == local.len(),
            "pose bone count differs from skeleton"
        );
        let mut global = vec![None; local.len()];
        for index in 0..local.len() {
            self.compose(index, matrices, &mut global, 0)?;
        }
        Ok(Pose {
            local,
            global: global.into_iter().map(Option::unwrap).collect(),
        })
    }

    fn compose(
        &self,
        index: usize,
        local: &[Matrix],
        global: &mut [Option<Matrix>],
        depth: usize,
    ) -> Result<Matrix> {
        ensure!(depth < self.bones.len(), "cyclic bone hierarchy");
        if let Some(matrix) = *global.get(index).context("invalid bone parent")? {
            return Ok(matrix);
        }
        let matrix = local[index];
        let matrix = if let Some(parent) = self.bones[index].parent {
            multiply(
                self.compose(usize::from(parent), local, global, depth + 1)?,
                matrix,
            )
        } else {
            matrix
        };
        global[index] = Some(matrix);
        Ok(matrix)
    }
}

impl Pose {
    pub fn point(&self, bone: u16, point: [f32; 3]) -> Result<[f32; 3]> {
        let matrix = self
            .global
            .get(usize::from(bone))
            .context("missing posed bone")?;
        Ok(transform_point(*matrix, point))
    }
}

impl Transform {
    /// Pose transitions use a shortest arc with a linear near-angle fallback.
    /// This differs from the authored spherical cubic curves inside a motion.
    pub fn blend(self, to: Self, weight: f32) -> Self {
        let rotation = if dot(self.rotation, to.rotation) < 0. {
            to.rotation.map(|v| -v)
        } else {
            to.rotation
        };
        let dot = dot(self.rotation, rotation).clamp(-1., 1.);
        let (a, b) = if 1. - dot > 0.001 {
            let angle = dot.acos();
            (
                ((1. - weight) * angle).sin() / angle.sin(),
                (weight * angle).sin() / angle.sin(),
            )
        } else {
            (1. - weight, weight)
        };
        Self {
            translation: std::array::from_fn(|i| {
                (1. - weight) * self.translation[i] + weight * to.translation[i]
            }),
            rotation: std::array::from_fn(|i| a * self.rotation[i] + b * rotation[i]),
            scale: std::array::from_fn(|i| (1. - weight) * self.scale[i] + weight * to.scale[i]),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.translation
                .iter()
                .chain(&self.scale)
                .chain(&self.rotation)
                .all(|v| v.is_finite())
                && dot(self.rotation, self.rotation) > 0.5,
            "invalid bone transform"
        );
        Ok(())
    }

    pub fn matrix(self) -> Matrix {
        let [x, y, z, w] = self.rotation;
        let [sx, sy, sz] = self.scale;
        [
            [
                (1. - 2. * (y * y + z * z)) * sx,
                2. * (x * y + z * w) * sx,
                2. * (x * z - y * w) * sx,
                0.,
            ],
            [
                2. * (x * y - z * w) * sy,
                (1. - 2. * (x * x + z * z)) * sy,
                2. * (y * z + x * w) * sy,
                0.,
            ],
            [
                2. * (x * z + y * w) * sz,
                2. * (y * z - x * w) * sz,
                (1. - 2. * (x * x + y * y)) * sz,
                0.,
            ],
            [
                self.translation[0],
                self.translation[1],
                self.translation[2],
                1.,
            ],
        ]
    }
}

pub fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|column| {
        std::array::from_fn(|row| (0..4).map(|i| a[i][row] * b[column][i]).sum())
    })
}

pub fn transform_point(matrix: Matrix, point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        matrix[3][row] + (0..3).map(|i| matrix[i][row] * point[i]).sum::<f32>()
    })
}

impl Motion {
    pub fn validate(&self, skeleton: &Skeleton) -> Result<()> {
        ensure!(
            self.duration_frames.is_finite() && self.duration_frames > 0.,
            "invalid motion duration"
        );
        let mut bones = std::collections::BTreeSet::new();
        for track in &self.tracks {
            ensure!(
                usize::from(track.bone) < skeleton.bones.len() && bones.insert(track.bone),
                "duplicate or missing motion bone"
            );
            let n = track.times.len();
            ensure!(
                track.period_frames.is_finite()
                    && track.period_frames > 0.
                    && track.period_frames <= self.duration_frames
                    && n > 0
                    && track.times[0] == 0.
                    && track
                        .times
                        .iter()
                        .all(|t| t.is_finite() && *t >= 0. && *t <= track.period_frames)
                    && track.times.windows(2).all(|t| t[0] <= t[1]),
                "invalid motion key times"
            );
            for curve in [&track.translation, &track.scale].into_iter().flatten() {
                ensure!(
                    curve.values.len() == n && finite(&curve.values),
                    "invalid vector curve values"
                );
                let controls = matches!(
                    curve.interpolation,
                    VectorInterpolation::Bezier | VectorInterpolation::Hermite
                );
                ensure!(
                    valid_controls(&curve.incoming, &curve.outgoing, n, controls),
                    "invalid vector curve controls"
                );
            }
            if let Some(curve) = &track.rotation {
                ensure!(
                    curve.values.len() == n
                        && finite(&curve.values)
                        && curve.values.iter().all(|&q| dot(q, q) > 0.5),
                    "invalid quaternion curve values"
                );
                ensure!(
                    valid_controls(
                        &curve.incoming,
                        &curve.outgoing,
                        n,
                        curve.interpolation == QuaternionInterpolation::Squad
                    ),
                    "invalid quaternion curve controls"
                );
            }
        }
        Ok(())
    }
}

fn finite<const N: usize>(values: &[[f32; N]]) -> bool {
    values.iter().flatten().all(|v| v.is_finite())
}

fn valid_controls<const N: usize>(
    incoming: &[[f32; N]],
    outgoing: &[[f32; N]],
    count: usize,
    needed: bool,
) -> bool {
    incoming.len() == if needed { count } else { 0 }
        && outgoing.len() == incoming.len()
        && finite(incoming)
        && finite(outgoing)
}

impl Track {
    pub fn sample(&self, time: f32, mut bind: Transform) -> Result<Transform> {
        ensure!(!self.times.is_empty(), "empty motion track");
        let time = if time > self.period_frames {
            time.rem_euclid(self.period_frames)
        } else {
            time
        };
        // Equal-time keys represent a discontinuity. The last key at that time
        // wins, while interpolation before it still approaches the first key.
        let a = self
            .times
            .partition_point(|key| *key <= time)
            .saturating_sub(1);
        let b = (a + 1) % self.times.len();
        let delta = if self.times[b] > self.times[a] {
            self.times[b]
        } else {
            self.period_frames
        } - self.times[a];
        let t = if delta > 0. {
            (time - self.times[a]) / delta
        } else {
            0.
        };
        if let Some(curve) = &self.translation {
            bind.translation = curve.sample(a, b, t)?;
        }
        if let Some(curve) = &self.scale {
            bind.scale = curve.sample(a, b, t)?;
        }
        if let Some(curve) = &self.rotation {
            bind.rotation = curve.sample(a, b, t)?;
        }
        Ok(bind)
    }
}

impl VectorCurve {
    fn sample(&self, a: usize, b: usize, t: f32) -> Result<[f32; 3]> {
        let av = *self.values.get(a).context("missing vector key")?;
        let bv = *self.values.get(b).context("missing vector key")?;
        Ok(match self.interpolation {
            VectorInterpolation::Step => av,
            VectorInterpolation::Linear => lerp(av, bv, t),
            VectorInterpolation::Bezier | VectorInterpolation::Hermite => {
                let outgoing = self.outgoing.get(a).context("missing outgoing tangent")?;
                let incoming = self.incoming.get(b).context("missing incoming tangent")?;
                std::array::from_fn(|i| {
                    if self.interpolation == VectorInterpolation::Bezier {
                        let u = 1. - t;
                        av[i] * (u * u * u)
                            + (av[i] + outgoing[i]) * (3. * u * u * t)
                            + (bv[i] + incoming[i]) * (3. * u * t * t)
                            + bv[i] * (t * t * t)
                    } else {
                        let t2 = t * t;
                        let t3 = t2 * t;
                        av[i] * (2. * t3 - 3. * t2 + 1.)
                            + bv[i] * (3. * t2 - 2. * t3)
                            + outgoing[i] * (t3 - 2. * t2 + t)
                            + incoming[i] * (t3 - t2)
                    }
                })
            }
        })
    }
}

impl QuaternionCurve {
    fn sample(&self, a: usize, b: usize, t: f32) -> Result<[f32; 4]> {
        let av = *self.values.get(a).context("missing quaternion key")?;
        let bv = *self.values.get(b).context("missing quaternion key")?;
        let value = match self.interpolation {
            QuaternionInterpolation::Step => av,
            QuaternionInterpolation::ShortestSlerp => shortest_slerp(av, bv, t),
            QuaternionInterpolation::Squad => {
                let ac = *self
                    .outgoing
                    .get(a)
                    .context("missing outgoing quaternion control")?;
                let bc = *self
                    .incoming
                    .get(b)
                    .context("missing incoming quaternion control")?;
                let t = t.clamp(0., 1.);
                spherical(
                    spherical(av, bv, t),
                    spherical(ac, bc, t),
                    2. * t * (1. - t),
                )
            }
        };
        ensure!(
            finite(&[value]) && dot(value, value) > 0.5,
            "invalid sampled quaternion"
        );
        Ok(normalize(value))
    }
}

fn lerp<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..4).map(|i| a[i] * b[i]).sum()
}
fn normalize(q: [f32; 4]) -> [f32; 4] {
    let length = dot(q, q).sqrt();
    q.map(|v| v / length)
}

fn shortest_slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    spherical(a, if dot(a, b) < 0. { b.map(|v| -v) } else { b }, t)
}

fn spherical(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot = dot(a, b).clamp(-1., 1.);
    if 1. + dot <= 0.000001 {
        let orthogonal = [-b[1], b[0], -b[3], b[2]];
        let angle = std::f32::consts::FRAC_PI_2 * t;
        return std::array::from_fn(|i| a[i] * angle.cos() + orthogonal[i] * angle.sin());
    }
    if 1. - dot <= 0.000001 {
        return lerp(a, b, t);
    }
    let angle = dot.acos();
    std::array::from_fn(|i| {
        (a[i] * ((1. - t) * angle).sin() + b[i] * (t * angle).sin()) / angle.sin()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_preserve_bezier_offsets_and_authored_quaternion_arcs() {
        let curve = VectorCurve {
            interpolation: VectorInterpolation::Bezier,
            values: vec![[0.; 3], [8., 0., 0.]],
            incoming: vec![[0.; 3]; 2],
            outgoing: vec![[0., 4., 0.], [0.; 3]],
        };
        assert_eq!(curve.sample(0, 1, 0.5).unwrap(), [4., 1.5, 0.]);
        assert_eq!(curve.sample(0, 1, 0.).unwrap(), curve.values[0]);
        assert_eq!(curve.sample(0, 1, 1.).unwrap(), curve.values[1]);
        let mid = spherical([0., 0., 0., 1.], [0., 0., 0.8660254, -0.5], 0.5);
        assert!(mid[2] > 0.86 && mid[3] > 0.49);
    }

    #[test]
    fn fractional_pose_blends_before_composing_scaled_parents() {
        let skeleton = Skeleton {
            bones: vec![
                Bone {
                    name: "root".into(),
                    parent: None,
                    bind_channels: TransformChannels(1),
                    bind: Transform {
                        scale: [2.; 3],
                        ..Transform::default()
                    },
                },
                Bone {
                    // Source weapon rigs can repeat a label on distinct nodes.
                    name: "root".into(),
                    parent: Some(0),
                    bind_channels: TransformChannels(8),
                    bind: Transform {
                        translation: [3., 0., 0.],
                        ..Transform::default()
                    },
                },
            ],
        };
        let mut motion = Motion {
            duration_frames: 2.,
            tracks: vec![Track {
                bone: 0,
                period_frames: 2.,
                times: vec![0., 2.],
                translation: Some(VectorCurve {
                    interpolation: VectorInterpolation::Linear,
                    values: vec![[0.; 3], [8., 0., 0.]],
                    incoming: vec![],
                    outgoing: vec![],
                }),
                scale: None,
                rotation: None,
            }],
        };
        skeleton.validate().unwrap();
        motion.validate(&skeleton).unwrap();
        let pose = skeleton.sample(&motion, 0.25).unwrap();
        assert_eq!(pose.point(1, [1., 0., 0.]).unwrap(), [9., 0., 0.]);
        let blend = skeleton
            .blend(&skeleton.bind_pose().unwrap(), &pose, 0.5)
            .unwrap();
        assert_eq!(blend.point(1, [1., 0., 0.]).unwrap(), [8.5, 0., 0.]);
        // Some battle clips return to their first pose at a duplicate endpoint.
        let track = &mut motion.tracks[0];
        track.times.push(2.);
        track.translation.as_mut().unwrap().values.push([0.; 3]);
        motion.validate(&skeleton).unwrap();
        assert_eq!(
            motion.tracks[0]
                .sample(1.75, Transform::default())
                .unwrap()
                .translation[0],
            7.
        );
        assert_eq!(
            motion.tracks[0]
                .sample(2., Transform::default())
                .unwrap()
                .translation[0],
            0.
        );
        motion.tracks[0].times[2] = 1.5;
        assert!(motion.validate(&skeleton).is_err());
    }
}

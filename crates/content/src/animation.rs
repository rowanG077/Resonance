//! Sparse animation curves and deterministic, renderer-independent bone poses.
mod codec;
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
    /// Presence of channels in the model's rest pose, including authored identity values.
    pub bind_channels: TransformChannels,
    pub period_frames: f32,
    pub times: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<VectorCurve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<VectorCurve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<QuaternionCurve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub euler_degrees: Option<VectorCurve>,
    /// Row-major 3x4 affine matrices, retained without TRS decomposition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrices: Option<Vec<[f32; 12]>>,
}

impl Track {
    pub fn channels(&self) -> TransformChannels {
        TransformChannels(
            u8::from(self.scale.is_some())
                | (u8::from(self.euler_degrees.is_some()) << 1)
                | (u8::from(self.rotation.is_some()) << 2)
                | (u8::from(self.matrices.is_some()) << 4)
                | (u8::from(self.translation.is_some()) << 3),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorInterpolation {
    Step,
    Linear,
    ShortestAngleLinear,
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
    /// Incoming/outgoing time controls, independently authored per key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ease: Vec<[f32; 2]>,
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
    /// Incoming/outgoing time controls, independently authored per key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ease: Vec<[f32; 2]>,
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

    /// Attachment queries evaluate the original curves, including affine keys,
    /// without materializing a TRS pose or sampling unrelated branches.
    pub fn sample_point(
        &self,
        motion: &Motion,
        frame: f32,
        bone: u16,
        point: [f32; 3],
    ) -> Result<[f32; 3]> {
        ensure!(
            frame.is_finite() && frame >= 0. && frame <= motion.duration_frames,
            "motion frame outside clip"
        );
        let matrix = self.sampled_global(motion, frame, bone, 0)?;
        let point = transform_point(matrix, point);
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "invalid attachment position"
        );
        Ok(point)
    }

    fn sampled_global(
        &self,
        motion: &Motion,
        frame: f32,
        index: u16,
        depth: usize,
    ) -> Result<Matrix> {
        ensure!(depth < self.bones.len(), "cyclic attachment hierarchy");
        let bone = self
            .bones
            .get(usize::from(index))
            .context("missing attachment bone")?;
        let local = motion
            .tracks
            .iter()
            .find(|track| track.bone == index)
            .map_or_else(
                || Ok(bone.bind.matrix()),
                |track| track.sample_matrix(frame, bone.bind),
            )?;
        match bone.parent {
            Some(parent) => Ok(multiply(
                self.sampled_global(motion, frame, parent, depth + 1)?,
                local,
            )),
            None => Ok(local),
        }
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

/// Native quaternion extraction from the complete basis, including scale/shear.
pub fn matrix_rotation(matrix: Matrix) -> Result<[f32; 4]> {
    let columns = matrix;
    let m = |row: usize, col: usize| columns[col][row];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let mut q = [0.; 4];
    if trace > 0. {
        let scale = (1. + trace).sqrt();
        q[3] = 0.5 * scale;
        let scale = 0.5 / scale;
        q[0] = (m(2, 1) - m(1, 2)) * scale;
        q[1] = (m(0, 2) - m(2, 0)) * scale;
        q[2] = (m(1, 0) - m(0, 1)) * scale;
    } else {
        let mut i = usize::from(m(1, 1) > m(0, 0));
        if m(2, 2) > m(i, i) {
            i = 2;
        }
        let j = (i + 1) % 3;
        let k = (j + 1) % 3;
        let mut scale = ((m(i, i) - (m(j, j) + m(k, k))) + 1.).sqrt();
        q[i] = 0.5 * scale;
        if scale != 0. {
            scale = 0.5 / scale;
        }
        q[3] = (m(k, j) - m(j, k)) * scale;
        q[j] = (m(i, j) + m(j, i)) * scale;
        q[k] = (m(i, k) + m(k, i)) * scale;
    }
    let q = glam::Quat::from_array(q);
    ensure!(
        q.is_finite() && q.length_squared().is_finite() && q.length_squared() > 0.,
        "invalid native matrix rotation"
    );
    Ok(q.normalize().to_array())
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
        self.validate_bones(skeleton.bones.len())
    }

    pub fn validate_bones(&self, bone_count: usize) -> Result<()> {
        ensure!(
            self.duration_frames.is_finite() && self.duration_frames > 0.,
            "invalid motion duration"
        );
        let mut bones = std::collections::BTreeSet::new();
        for track in &self.tracks {
            ensure!(
                track.bind_channels.0 & !31 == 0,
                "invalid rest transform channels"
            );
            ensure!(
                track.rotation.is_none() || track.euler_degrees.is_none(),
                "conflicting rotation curves"
            );
            if let Some(matrices) = &track.matrices {
                ensure!(
                    track.translation.is_none()
                        && track.scale.is_none()
                        && track.rotation.is_none()
                        && track.euler_degrees.is_none()
                        && matrices.len() == track.times.len()
                        && finite(matrices),
                    "invalid affine animation channel"
                );
            }
            ensure!(
                usize::from(track.bone) < bone_count && bones.insert(track.bone),
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
            for curve in [&track.translation, &track.scale, &track.euler_degrees]
                .into_iter()
                .flatten()
            {
                ensure!(
                    curve.values.len() == n
                        && finite(&curve.values)
                        && valid_ease(&curve.ease, n)
                        && (curve.ease.is_empty()
                            || curve.interpolation == VectorInterpolation::Hermite),
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
                        && valid_ease(&curve.ease, n)
                        && (curve.ease.is_empty()
                            || curve.interpolation == QuaternionInterpolation::Squad)
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

fn valid_ease(values: &[[f32; 2]], count: usize) -> bool {
    (values.is_empty() || values.len() == count) && finite(values)
}

/// Preserve the source evaluator's time warp, including signed controls.
fn eased_time(ease: &[[f32; 2]], a: usize, b: usize, t: f32) -> Result<f32> {
    if ease.is_empty() || t == 0. || t == 1. {
        return Ok(t);
    }
    let mut first = ease.get(a).context("missing outgoing time control")?[1];
    let mut second = ease.get(b).context("missing incoming time control")?[0];
    let total = first + second;
    if total == 0. {
        return Ok(t);
    }
    let inverse = 1. / total;
    if total > 1. {
        first *= inverse;
        second *= inverse;
    }
    let shape = 0.5 - inverse;
    let value = if t < first {
        t * (t * (shape / first))
    } else if t < 1. - second {
        shape * (2. * t - first)
    } else {
        let rest = 1. - t;
        1. - rest * (rest * (shape / second))
    };
    ensure!(value.is_finite(), "invalid eased animation time");
    Ok(value)
}

fn euler_rotation(degrees: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = degrees.map(|value| (value.to_radians() * 0.5).sin_cos());
    [
        x.0 * y.1 * z.1 - x.1 * y.0 * z.0,
        x.1 * y.0 * z.1 + x.0 * y.1 * z.0,
        x.1 * y.1 * z.0 - x.0 * y.0 * z.1,
        x.1 * y.1 * z.1 + x.0 * y.0 * z.0,
    ]
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
    fn interval(&self, time: f32) -> Result<(usize, usize, f32)> {
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
        Ok((a, b, t))
    }

    /// Affine consumers can evaluate matrix tracks without discarding shear.
    pub fn sample_matrix(&self, time: f32, bind: Transform) -> Result<Matrix> {
        if let Some(matrices) = &self.matrices {
            let (a, _, _) = self.interval(time)?;
            let m = matrices.get(a).context("missing affine animation key")?;
            return Ok([
                [m[0], m[4], m[8], 0.],
                [m[1], m[5], m[9], 0.],
                [m[2], m[6], m[10], 0.],
                [m[3], m[7], m[11], 1.],
            ]);
        }
        Ok(self.sample(time, bind)?.matrix())
    }

    pub fn sample(&self, time: f32, mut bind: Transform) -> Result<Transform> {
        ensure!(
            self.matrices.is_none(),
            "affine animation requires an affine pose consumer; refusing lossy TRS conversion"
        );
        ensure!(!self.times.is_empty(), "empty motion track");
        let (a, b, t) = self.interval(time)?;
        if let Some(curve) = &self.translation {
            bind.translation = curve.sample(a, b, t)?;
        }
        if let Some(curve) = &self.scale {
            bind.scale = curve.sample(a, b, t)?;
        }
        if let Some(curve) = &self.rotation {
            bind.rotation = curve.sample(a, b, t)?;
        }
        if let Some(curve) = &self.euler_degrees {
            bind.rotation = euler_rotation(curve.sample(a, b, t)?);
        }
        Ok(bind)
    }
}

impl VectorCurve {
    fn sample(&self, a: usize, b: usize, t: f32) -> Result<[f32; 3]> {
        let t = eased_time(&self.ease, a, b, t)?;
        let av = *self.values.get(a).context("missing vector key")?;
        let bv = *self.values.get(b).context("missing vector key")?;
        Ok(match self.interpolation {
            VectorInterpolation::Step => av,
            VectorInterpolation::Linear => lerp(av, bv, t),
            VectorInterpolation::ShortestAngleLinear => std::array::from_fn(|i| {
                let (mut a, mut b) = (av[i], bv[i]);
                if a - b > 180. {
                    a -= 360.;
                } else if a - b < -180. {
                    b -= 360.;
                }
                (1. - t) * a + t * b
            }),
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
        let t = eased_time(&self.ease, a, b, t)?;
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
            ease: vec![],
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
                bind_channels: TransformChannels(0),
                period_frames: 2.,
                times: vec![0., 2.],
                translation: Some(VectorCurve {
                    interpolation: VectorInterpolation::Linear,
                    values: vec![[0.; 3], [8., 0., 0.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
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
    #[test]
    fn authored_time_euler_and_affine_channels_remain_distinct() {
        assert_eq!(
            eased_time(&[[0., 0.25], [0.25, 0.]], 0, 1, 0.125).unwrap(),
            -0.09375
        );
        assert_eq!(eased_time(&[[0., 0.], [0., 0.]], 0, 1, 0.37).unwrap(), 0.37);
        let curve = VectorCurve {
            interpolation: VectorInterpolation::ShortestAngleLinear,
            values: vec![[0., 0., 350.], [0., 0., 10.]],
            incoming: vec![],
            outgoing: vec![],
            ease: vec![],
        };
        assert_eq!(curve.sample(0, 1, 0.5).unwrap(), [0.; 3]);
        let rotated = Transform {
            rotation: euler_rotation([0., 0., 90.]),
            ..Transform::default()
        }
        .matrix();
        let point = transform_point(rotated, [1., 0., 0.]);
        assert!(point[0].abs() < 0.00001 && (point[1] - 1.).abs() < 0.00001);
        let track = Track {
            bone: 0,
            bind_channels: TransformChannels(0),
            period_frames: 2.,
            times: vec![0.],
            translation: None,
            scale: None,
            rotation: None,
            euler_degrees: None,
            matrices: Some(vec![[1., 0.5, 0., 3., 0., 1., 0., 4., 0., 0., 1., 5.]]),
        };
        let matrix = track.sample_matrix(0.5, Transform::default()).unwrap();
        assert_eq!(transform_point(matrix, [0., 2., 0.]), [4., 6., 5.]);
        assert!(
            track.sample(0.5, Transform::default()).is_err(),
            "shear must not be approximated as TRS"
        );
    }
}

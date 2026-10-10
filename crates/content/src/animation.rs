//! Sparse animation curves and deterministic, renderer-independent bone poses.
mod codec;
pub mod pose;
mod skeleton;
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
    /// Incoming/outgoing easing durations as fractions of a key interval.
    /// Negative controls disable easing; values above one use the full interval.
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
    /// Incoming/outgoing easing durations, with the same rules as vector curves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ease: Vec<[f32; 2]>,
}

/// Column-major affine matrix, in the model's authored coordinate system.
pub type Matrix = [[f32; 4]; 4];

#[derive(Debug, Clone)]
pub struct Pose {
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
        }
        self.parent_order(0..self.bones.len())?;
        Ok(())
    }

    fn parent_order(&self, starts: impl IntoIterator<Item = usize>) -> Result<Vec<usize>> {
        #[derive(Clone, Copy)]
        enum Visit {
            New,
            Active,
            Complete,
        }
        let mut visits = vec![Visit::New; self.bones.len()];
        let mut path = Vec::new();
        let mut order = Vec::new();
        for start in starts {
            let mut cursor = Some(start);
            while let Some(index) = cursor {
                let visit = visits.get_mut(index).context("invalid bone parent")?;
                match visit {
                    Visit::Complete => break,
                    Visit::Active => anyhow::bail!("cyclic bone hierarchy"),
                    Visit::New => *visit = Visit::Active,
                }
                path.push(index);
                cursor = self.bones[index].parent.map(usize::from);
            }
            while let Some(index) = path.pop() {
                visits[index] = Visit::Complete;
                order.push(index);
            }
        }
        Ok(order)
    }

    pub fn bone(&self, name: &str) -> Option<u16> {
        self.bones
            .iter()
            .position(|bone| bone.name == name)
            .map(|i| i as u16)
    }

    pub fn bind_pose(&self) -> Result<Pose> {
        self.pose(self.bones.iter().map(|b| b.bind.matrix()).collect())
    }

    pub fn sample(&self, motion: &Motion, frame: f32) -> Result<Pose> {
        ensure!(
            frame.is_finite() && frame >= 0. && frame <= motion.duration_frames,
            "motion frame outside clip"
        );
        let mut local = self
            .bones
            .iter()
            .map(|bone| bone.bind.matrix())
            .collect::<Vec<_>>();
        for track in &motion.tracks {
            let index = usize::from(track.bone);
            let matrix = local
                .get_mut(index)
                .context("motion bone outside skeleton")?;
            *matrix = track.sample_matrix(frame, self.bones[index].bind)?;
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
        let point = transform_point(self.sample_matrix(motion, frame, bone)?, point);
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "invalid attachment position"
        );
        Ok(point)
    }

    pub fn sample_matrix(&self, motion: &Motion, frame: f32, bone: u16) -> Result<Matrix> {
        ensure!(
            frame.is_finite() && frame >= 0. && frame <= motion.duration_frames,
            "motion frame outside clip"
        );
        self.sampled_global(motion, frame, bone)
    }

    fn sampled_global(&self, motion: &Motion, frame: f32, index: u16) -> Result<Matrix> {
        self.bones
            .get(usize::from(index))
            .context("missing attachment bone")?;
        let mut tracks = vec![None; self.bones.len()];
        for track in &motion.tracks {
            if let Some(slot) = tracks.get_mut(usize::from(track.bone)) {
                slot.get_or_insert(track);
            }
        }
        let mut global = None;
        for index in self.parent_order([usize::from(index)])? {
            let bone = &self.bones[index];
            let local = tracks[index].map_or_else(
                || Ok(bone.bind.matrix()),
                |track| track.sample_matrix(frame, bone.bind),
            )?;
            global = Some(match global {
                Some(parent) => multiply(parent, local),
                None => local,
            });
        }
        global.context("missing attachment bone")
    }

    /// Compose local matrices in place, retaining affine scale and shear.
    pub fn pose(&self, mut global: Vec<Matrix>) -> Result<Pose> {
        ensure!(
            global.len() == self.bones.len(),
            "pose bone count differs from skeleton"
        );
        for index in self.parent_order(0..global.len())? {
            global[index] = match self.bones[index].parent {
                Some(parent) => multiply(global[usize::from(parent)], global[index]),
                None => global[index],
            };
        }
        Ok(Pose { global })
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
        let from_rotation = normalize(self.rotation);
        let to_rotation = normalize(to.rotation);
        let rotation = if dot(from_rotation, to_rotation) < 0. {
            to_rotation.map(|v| -v)
        } else {
            to_rotation
        };
        let dot = dot(from_rotation, rotation).clamp(-1., 1.);
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
            rotation: std::array::from_fn(|i| a * from_rotation[i] + b * rotation[i]),
            scale: std::array::from_fn(|i| (1. - weight) * self.scale[i] + weight * to.scale[i]),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.translation
                .iter()
                .chain(&self.scale)
                .all(|v| v.is_finite())
                && valid_quaternions(&[self.rotation]),
            "invalid bone transform"
        );
        Ok(())
    }

    pub fn matrix(self) -> Matrix {
        let [x, y, z, w] = normalize(self.rotation);
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

/// Extract a normalized quaternion from the 3x3 basis; translation is ignored.
pub fn matrix_rotation(matrix: Matrix) -> Result<[f32; 4]> {
    let m = |row: usize, col: usize| matrix[col][row];
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
    let length = q.iter().map(|v| v * v).sum::<f32>();
    ensure!(
        q.iter().all(|v| v.is_finite()) && length.is_finite() && length > 0.,
        "invalid native matrix rotation"
    );
    Ok(q.map(|v| v / length.sqrt()))
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
                        && valid_quaternions(&curve.values)
                        && valid_ease(&curve.ease, n)
                        && (curve.ease.is_empty()
                            || curve.interpolation == QuaternionInterpolation::Squad),
                    "invalid quaternion curve values"
                );
                ensure!(
                    valid_controls(
                        &curve.incoming,
                        &curve.outgoing,
                        n,
                        curve.interpolation == QuaternionInterpolation::Squad
                    ) && valid_quaternions(&curve.incoming)
                        && valid_quaternions(&curve.outgoing),
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

/// Accelerate, cruise, then decelerate while traversing exactly one key interval.
fn eased_time(ease: &[[f32; 2]], a: usize, b: usize, t: f32) -> Result<f32> {
    if ease.is_empty() || t == 0. || t == 1. {
        return Ok(t);
    }
    let mut first = ease.get(a).context("missing outgoing time control")?[1].clamp(0., 1.);
    let mut second = ease.get(b).context("missing incoming time control")?[0].clamp(0., 1.);
    let total = first + second;
    if total > 1. {
        first /= total;
        second /= total;
    }
    let speed = 1. / (1. - (first + second) * 0.5);
    let value = if t < first {
        speed * t * t / (2. * first)
    } else if t > 1. - second {
        let rest = 1. - t;
        1. - speed * rest * rest / (2. * second)
    } else {
        speed * (t - first * 0.5)
    };
    ensure!(value.is_finite(), "invalid eased animation time");
    Ok(value.clamp(0., 1.))
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
        let av = normalize(*self.values.get(a).context("missing quaternion key")?);
        let bv = normalize(*self.values.get(b).context("missing quaternion key")?);
        let value = match self.interpolation {
            QuaternionInterpolation::Step => av,
            QuaternionInterpolation::ShortestSlerp => shortest_slerp(av, bv, t),
            QuaternionInterpolation::Squad => {
                let ac = normalize(
                    *self
                        .outgoing
                        .get(a)
                        .context("missing outgoing quaternion control")?,
                );
                let bc = normalize(
                    *self
                        .incoming
                        .get(b)
                        .context("missing incoming quaternion control")?,
                );
                let t = t.clamp(0., 1.);
                spherical(
                    spherical(av, bv, t),
                    spherical(ac, bc, t),
                    2. * t * (1. - t),
                )
            }
        };
        let value = normalize(value);
        ensure!(finite(&[value]), "invalid sampled quaternion");
        Ok(value)
    }
}

fn lerp<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..4).map(|i| a[i] * b[i]).sum()
}
fn normalize(q: [f32; 4]) -> [f32; 4] {
    // Rescale before squaring: every finite nonzero quaternion has a length in
    // [1, 2] here, even when its authored components are subnormal or near MAX.
    let largest = q.iter().fold(0_f32, |largest, v| largest.max(v.abs()));
    let q = q.map(|v| v / largest);
    let length = dot(q, q).sqrt();
    q.map(|v| v / length)
}

fn valid_quaternions(values: &[[f32; 4]]) -> bool {
    values.iter().all(|&q| finite(&[normalize(q)]))
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
    fn deep_unordered_hierarchy_composes_and_rejects_cycles() -> Result<()> {
        let count = usize::from(u16::MAX);
        let mut skeleton = Skeleton {
            bones: (0..count)
                .map(|index| Bone {
                    name: "joint".into(),
                    parent: (index + 1 < count).then_some((index + 1) as u16),
                    bind_channels: TransformChannels(8),
                    bind: Transform {
                        translation: [1., 0., 0.],
                        ..Transform::default()
                    },
                })
                .collect(),
        };
        let motion = Motion {
            duration_frames: 1.,
            tracks: (0..count)
                .map(|bone| Track {
                    bone: bone as u16,
                    bind_channels: TransformChannels(0),
                    period_frames: 1.,
                    times: vec![0.],
                    translation: None,
                    scale: None,
                    rotation: None,
                    euler_degrees: None,
                    matrices: Some(vec![[1., 0., 0., 2., 0., 1., 0., 0., 0., 0., 1., 0.]]),
                })
                .collect(),
        };
        skeleton.validate()?;
        motion.validate(&skeleton)?;
        let pose = skeleton.bind_pose()?;
        assert_eq!(pose.point(0, [0.; 3])?, [count as f32, 0., 0.]);
        assert_eq!(pose.point((count - 1) as u16, [0.; 3])?, [1., 0., 0.]);
        assert_eq!(
            skeleton.sample_point(&motion, 0.5, 0, [2., 3., 4.])?,
            [2. * count as f32 + 2., 3., 4.]
        );
        for parent in [(count / 2) as u16, u16::MAX] {
            skeleton.bones[count - 1].parent = Some(parent);
            assert!(skeleton.validate().is_err());
            assert!(skeleton.bind_pose().is_err());
            assert!(skeleton.sample_point(&motion, 0.5, 0, [0.; 3]).is_err());
        }
        skeleton.bones[count - 1].parent = None;
        skeleton.bones[0].bind.translation[0] = f32::NAN;
        assert!(skeleton.validate().is_err());
        Ok(())
    }

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

        let mut motion = Motion {
            duration_frames: 2.,
            tracks: vec![Track {
                bone: 0,
                bind_channels: TransformChannels(0),
                period_frames: 2.,
                times: vec![0., 2.],
                translation: None,
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            }],
        };
        for interpolation in [
            QuaternionInterpolation::Step,
            QuaternionInterpolation::ShortestSlerp,
            QuaternionInterpolation::Squad,
        ] {
            let controls = if interpolation == QuaternionInterpolation::Squad {
                vec![[0., 0., 1., -1.]; 2]
            } else {
                vec![]
            };
            let reference = QuaternionCurve {
                interpolation,
                values: vec![[0., 0., 0., 1.], [0., 0., 1., 1.]],
                incoming: controls.clone(),
                outgoing: controls,
                ease: vec![],
            };
            for magnitude in [f32::MAX, f32::MIN_POSITIVE, f32::from_bits(1)] {
                let mut scaled = reference.clone();
                for q in scaled
                    .values
                    .iter_mut()
                    .chain(&mut scaled.incoming)
                    .chain(&mut scaled.outgoing)
                {
                    *q = q.map(|v| v * magnitude);
                }
                motion.tracks[0].rotation = Some(scaled.clone());
                motion.validate_bones(1).unwrap();
                for t in [0., 0.25, 0.5, 1.] {
                    let expected = reference.sample(0, 1, t).unwrap();
                    let actual = scaled.sample(0, 1, t).unwrap();
                    assert!(
                        actual
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| (a - b).abs() < 0.00001)
                    );
                }
            }
            for invalid in [[0.; 4], [f32::NAN; 4], [f32::INFINITY; 4]] {
                let mut broken = reference.clone();
                broken.values[0] = invalid;
                assert!(broken.sample(0, 1, 0.5).is_err());
                motion.tracks[0].rotation = Some(broken);
                assert!(motion.validate_bones(1).is_err());
                if interpolation == QuaternionInterpolation::Squad {
                    let mut broken = reference.clone();
                    broken.outgoing[0] = invalid;
                    assert!(broken.sample(0, 1, 0.5).is_err());
                    motion.tracks[0].rotation = Some(broken);
                    assert!(motion.validate_bones(1).is_err());
                }
            }
        }
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
    fn native_time_easing_is_continuous_bounded_and_monotonic() {
        for (incoming, outgoing) in [
            (0., 0.),
            (0.25, 0.25),
            (0.8, 0.8),
            (-0.5, 0.25),
            (2., 0.1),
            (0., 1.),
            (1., 0.),
        ] {
            let ease = [[0., incoming], [outgoing, 0.]];
            assert_eq!(eased_time(&ease, 0, 1, 0.).unwrap(), 0.);
            assert_eq!(eased_time(&ease, 0, 1, 1.).unwrap(), 1.);
            let mut previous = 0.;
            for step in 1..=1000 {
                let value = eased_time(&ease, 0, 1, step as f32 / 1000.).unwrap();
                assert!((previous..=1.).contains(&value));
                previous = value;
            }
            for boundary in [0.25, 0.5, 0.75] {
                let before = eased_time(&ease, 0, 1, boundary - 0.00001).unwrap();
                let after = eased_time(&ease, 0, 1, boundary + 0.00001).unwrap();
                assert!((after - before).abs() < 0.0001);
            }
        }
        assert_eq!(eased_time(&[[0.; 2]; 2], 0, 1, 0.37).unwrap(), 0.37);
        assert_eq!(
            eased_time(&[[0., -1.], [-1., 0.]], 0, 1, 0.37).unwrap(),
            0.37
        );
    }

    #[test]
    fn authored_time_euler_and_affine_channels_remain_distinct() {
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

        let mut skeleton = Skeleton {
            bones: [Some(2), None, Some(1), None]
                .into_iter()
                .map(|parent| Bone {
                    name: "joint".into(),
                    parent,
                    bind_channels: TransformChannels(0),
                    bind: Transform::default(),
                })
                .collect(),
        };
        skeleton.bones[0].bind.translation = [3., 0., 0.];
        skeleton.bones[0].bind.rotation = [0., 0., 0., f32::from_bits(1)];
        skeleton.bones[1].bind.scale = [2., 3., 1.];
        skeleton.bones[1].bind.rotation = [0., 0., 0., f32::MAX];
        let root_track = Track {
            bone: 1,
            times: vec![0., 2.],
            translation: Some(VectorCurve {
                interpolation: VectorInterpolation::Linear,
                values: vec![[0.; 3], [4., 0., 0.]],
                incoming: vec![],
                outgoing: vec![],
                ease: vec![],
            }),
            matrices: None,
            ..track.clone()
        };
        let mut motion = Motion {
            duration_frames: 2.,
            tracks: vec![root_track, Track { bone: 2, ..track }],
        };
        skeleton.validate().unwrap();
        motion.validate(&skeleton).unwrap();
        let point = [0., 2., 0.];
        for frame in [0., 0.25, 1., 2.] {
            let pose = skeleton.sample(&motion, frame).unwrap();
            let sparse = skeleton.sample_point(&motion, frame, 0, point).unwrap();
            assert_eq!(sparse, [14. + 2. * frame, 18., 5.]);
            assert_eq!(sparse, pose.point(0, point).unwrap());
        }
        skeleton.bones[3].parent = Some(3);
        let mut unused = motion.tracks[0].clone();
        unused.bone = 3;
        unused.times.clear();
        motion.tracks.push(unused);
        assert!(skeleton.bind_pose().is_err());
        assert_eq!(
            skeleton.sample_point(&motion, 0.5, 0, point).unwrap(),
            [15., 18., 5.]
        );
        assert!(skeleton.sample_point(&motion, 0.5, 3, point).is_err());
    }

    #[test]
    fn near_angle_blends_keep_orthogonal_basis_and_authored_scale() {
        let from = Transform {
            rotation: [0., 0., 0.5, 0.8660254],
            ..Transform::default()
        };
        let to = Transform {
            rotation: [0., 0., 0.5150381, 0.8571673],
            ..from
        };
        let mut middle = from.blend(to, 0.4);
        middle.scale = [2., 3., 4.];
        middle.translation = [5., 6., 7.];
        let matrix = middle.matrix();
        for (column, scale) in matrix[..3].iter().zip(middle.scale) {
            let length_squared: f32 = column[..3].iter().map(|v| v * v).sum();
            assert!((length_squared - scale * scale).abs() < 0.00001);
        }
        assert_eq!(matrix[3], [5., 6., 7., 1.]);
        assert_eq!(
            Transform::default().matrix(),
            [
                [1., 0., 0., 0.],
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
                [0., 0., 0., 1.],
            ]
        );
        let turn = Transform {
            rotation: [0., 0., 1., 1.],
            translation: middle.translation,
            scale: middle.scale,
        };
        for magnitude in [f32::MAX, 1e-30, f32::from_bits(1)] {
            let scaled = Transform {
                rotation: [0., 0., magnitude, magnitude],
                ..turn
            };
            scaled.validate().unwrap();
            for (actual, expected) in scaled
                .matrix()
                .into_iter()
                .flatten()
                .zip(turn.matrix().into_iter().flatten())
            {
                assert!(actual.is_finite() && (actual - expected).abs() < 0.00001);
            }
            let to = Transform {
                rotation: [0., 0., 0., -magnitude],
                ..Transform::default()
            };
            for weight in [0., 0.25, 0.75, 1.] {
                let actual = scaled.blend(to, weight).matrix();
                let expected = turn.blend(Transform::default(), weight).matrix();
                assert!(
                    actual
                        .into_iter()
                        .flatten()
                        .zip(expected.into_iter().flatten())
                        .all(|(a, b)| a.is_finite() && (a - b).abs() < 0.00001)
                );
            }
        }
        for rotation in [[0.; 4], [f32::NAN, 0., 0., 1.], [0., f32::INFINITY, 0., 1.]] {
            assert!(
                Transform {
                    rotation,
                    ..Transform::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}

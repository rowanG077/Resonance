//! Standard glTF animation tracks and renderer-independent skeletal poses.
mod codec;
mod named;
pub use named::NamedMotion;
mod tracks;
pub use tracks::{Channel, Interpolation, Motion, NodeTrack, Track};
pub mod pose;
mod skeleton;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
pub use skeleton::joint_names_from_glb;
pub(crate) use skeleton::scene_nodes_from_glb;
pub(crate) use skeleton::{node_parents, rest_transform};

/// The game animation clock. glTF timestamps are stored in seconds.
pub const FRAME_HZ: f32 = 30.;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bone {
    /// An authored label; repeated labels do not merge indexed bones or tracks.
    pub name: String,
    /// Include this joint in the animated body footprint. Read from GLB extras;
    /// renaming a joint must not change its contribution.
    #[serde(
        default = "crate::defaults::enabled",
        skip_serializing_if = "crate::defaults::is_enabled"
    )]
    pub shadow_bounds: bool,
    pub parent: Option<u16>,
    /// Static GLB nodes between this joint and its indexed parent. Kept apart
    /// from the local rest pose so animation cannot overwrite an editor's group
    /// transform. This is prepared from the model, never a cooked sidecar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_transform: Option<Matrix>,
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
#[serde(default)]
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

/// Column-major affine matrix, in the model's authored coordinate system.
pub type Matrix = [[f32; 4]; 4];

#[derive(Debug, Clone)]
pub struct Pose {
    pub local: Vec<Transform>,
    pub global: Vec<Matrix>,
}

impl Skeleton {
    pub fn single(name: impl Into<String>) -> Self {
        Self {
            bones: vec![Bone {
                name: name.into(),
                parent: None,
                parent_transform: None,
                shadow_bounds: true,
                bind: Transform::default(),
            }],
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.bones.is_empty() && self.bones.len() <= u16::MAX as usize,
            "invalid bone count"
        );
        for (index, bone) in self.bones.iter().enumerate() {
            ensure!(!bone.name.is_empty(), "empty name for bone {index}");
            bone.bind.validate()?;
            if let Some(matrix) = bone.parent_transform {
                ensure!(
                    matrix.iter().flatten().all(|value| value.is_finite()),
                    "invalid parent transform for bone {index}"
                );
            }
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
        let point = transform_point(self.sample_matrix(motion, frame, bone)?, point);
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "invalid attachment position"
        );
        Ok(point)
    }

    pub fn sample_matrix(&self, motion: &Motion, frame: f32, bone: u16) -> Result<Matrix> {
        self.sample_layered_matrix(&[(motion, frame)], bone)
    }

    /// Later layers replace only the bones they animate, retaining other branches.
    pub fn sample_layered_matrix(&self, layers: &[(&Motion, f32)], bone: u16) -> Result<Matrix> {
        self.sample_adjusted_matrix(layers, bone, &|_, _, local| local)
    }

    /// Apply local joint adjustments before composing the attachment's ancestor chain.
    pub fn sample_adjusted_matrix(
        &self,
        layers: &[(&Motion, f32)],
        bone: u16,
        adjust: &impl Fn(u16, &Bone, pose::LocalPose) -> pose::LocalPose,
    ) -> Result<Matrix> {
        for &(motion, frame) in layers {
            ensure!(
                frame.is_finite() && frame >= 0. && frame <= motion.duration_frames,
                "motion frame outside clip"
            );
        }
        self.sampled_global(layers, bone, 0, adjust)
    }

    fn sampled_global(
        &self,
        layers: &[(&Motion, f32)],
        index: u16,
        depth: usize,
        adjust: &impl Fn(u16, &Bone, pose::LocalPose) -> pose::LocalPose,
    ) -> Result<Matrix> {
        ensure!(depth < self.bones.len(), "cyclic attachment hierarchy");
        let bone = self
            .bones
            .get(usize::from(index))
            .context("missing attachment bone")?;
        let local = layers
            .iter()
            .rev()
            .find_map(|&(motion, frame)| {
                motion
                    .tracks
                    .iter()
                    .find(|track| track.bone == index)
                    .map(|track| (track, frame))
            })
            .map_or_else(
                || pose::sample_bone(bone.bind, None, 0.),
                |(track, frame)| pose::sample_bone(bone.bind, Some(track), frame),
            )?;
        let local = pose::BonePose::complete(adjust(index, bone, local.value())).matrix();
        let local = bone
            .parent_transform
            .map_or(local, |parent| multiply(parent, local));
        match bone.parent {
            Some(parent) => Ok(multiply(
                self.sampled_global(layers, parent, depth + 1, adjust)?,
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
        let matrix = self.bones[index]
            .parent_transform
            .map_or(local[index], |parent| multiply(parent, local[index]));
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

pub fn euler_rotation(degrees: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = degrees.map(|value| (value.to_radians() * 0.5).sin_cos());
    [
        x.0 * y.1 * z.1 - x.1 * y.0 * z.0,
        x.1 * y.0 * z.1 + x.0 * y.1 * z.0,
        x.1 * y.1 * z.0 - x.0 * y.0 * z.1,
        x.1 * y.1 * z.1 + x.0 * y.0 * z.0,
    ]
}

fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..4).map(|i| a[i] * b[i]).sum()
}
fn normalize(q: [f32; 4]) -> [f32; 4] {
    let length = dot(q, q).sqrt();
    q.map(|v| v / length)
}

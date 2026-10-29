//! glTF's three interpolation modes; each property owns its own timeline.
use super::{Matrix, Skeleton, Transform, TransformChannels, multiply};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    Step,
    Linear,
    CubicSpline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel<const N: usize> {
    pub interpolation: Interpolation,
    /// Sampling units; the glTF decoder supplies seconds. Values clamp outside
    /// this range. Camera preparation converts these to its resource-frame clock.
    pub times: Vec<f32>,
    #[serde(bound(
        serialize = "[f32; N]: Serialize",
        deserialize = "[f32; N]: Deserialize<'de>"
    ))]
    pub values: Vec<[f32; N]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeTrack {
    pub bind: Transform,
    pub translation: Option<Channel<3>>,
    pub rotation: Option<Channel<4>>,
    pub scale: Option<Channel<3>>,
}

impl NodeTrack {
    pub fn new(bind: Transform) -> Self {
        Self {
            bind,
            translation: None,
            rotation: None,
            scale: None,
        }
    }

    pub fn channels(&self) -> TransformChannels {
        TransformChannels(
            u8::from(self.scale.is_some())
                | (u8::from(self.rotation.is_some()) << 2)
                | (u8::from(self.translation.is_some()) << 3),
        )
    }

    pub fn sample(&self, seconds: f32, mut bind: Transform) -> Result<Transform> {
        if let Some(channel) = &self.translation {
            bind.translation = channel.sample(seconds, false)?;
        }
        if let Some(channel) = &self.rotation {
            bind.rotation = channel.sample(seconds, true)?;
        }
        if let Some(channel) = &self.scale {
            bind.scale = channel.sample(seconds, false)?;
        }
        Ok(bind)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        self.bind.validate()?;
        for channel in [&self.translation, &self.scale].into_iter().flatten() {
            channel.validate(false)?;
        }
        if let Some(channel) = &self.rotation {
            channel.validate(true)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub bone: u16,
    /// Standard node transforms between the model's parent joint and this joint.
    /// Intermediate nodes allow ordinary glTF TRS animation to retain affine shear.
    pub nodes: Vec<NodeTrack>,
}

impl Track {
    pub fn channels(&self) -> TransformChannels {
        if self.is_affine() {
            TransformChannels(16)
        } else {
            self.nodes[0].channels()
        }
    }

    pub fn is_affine(&self) -> bool {
        self.nodes.len() > 1
    }

    pub fn sample(&self, frame: f32, bind: Transform) -> Result<Transform> {
        ensure!(
            self.nodes.len() == 1,
            "a transform chain requires affine sampling"
        );
        self.nodes[0].sample(frame / super::FRAME_HZ, bind)
    }

    pub fn sample_matrix(&self, frame: f32, bind: Transform) -> Result<Matrix> {
        self.sample_matrix_at(frame / super::FRAME_HZ, bind)
    }

    /// Sample in the channels' units, including prepared frame-domain curves.
    pub fn sample_matrix_at(&self, time: f32, bind: Transform) -> Result<Matrix> {
        if !self.is_affine() {
            return Ok(self.nodes[0].sample(time, bind)?.matrix());
        }
        let mut matrix = Transform::default().matrix();
        for node in &self.nodes {
            matrix = multiply(matrix, node.sample(time, node.bind)?.matrix());
        }
        Ok(matrix)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// Game clock units; file timestamps and channel sampling use seconds.
    pub duration_frames: f32,
    pub tracks: Vec<Track>,
}

impl Motion {
    pub fn validate(&self, skeleton: &Skeleton) -> Result<()> {
        self.validate_bones(skeleton.bones.len())
    }

    pub fn validate_bones(&self, count: usize) -> Result<()> {
        ensure!(
            self.duration_frames.is_finite() && self.duration_frames >= 0.,
            "invalid animation duration"
        );
        let mut bones = std::collections::BTreeSet::new();
        for track in &self.tracks {
            ensure!(
                usize::from(track.bone) < count && bones.insert(track.bone),
                "duplicate or missing animation joint"
            );
            ensure!(!track.nodes.is_empty(), "empty animation transform chain");
            for node in &track.nodes {
                node.validate()?;
            }
        }
        Ok(())
    }
}

impl<const N: usize> Channel<N> {
    pub fn validate(&self, rotation: bool) -> Result<()> {
        ensure!(
            !self.times.is_empty()
                && self.times.iter().all(|t| t.is_finite() && *t >= 0.)
                && self.times.windows(2).all(|t| t[0] < t[1]),
            "invalid glTF animation times"
        );
        let stride = if self.interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
        ensure!(
            self.values.len() == self.times.len() * stride
                && self.values.iter().flatten().all(|v| v.is_finite()),
            "invalid glTF animation values"
        );
        ensure!(
            stride == 1 || self.times.len() >= 2,
            "cubic animation requires two keys"
        );
        if rotation {
            ensure!(N == 4, "rotation needs four components");
            for value in self.values.iter().skip(stride / 2).step_by(stride) {
                let length: f32 = value.iter().map(|v| v * v).sum();
                ensure!(
                    (length - 1.).abs() < 0.001,
                    "animation quaternion must have unit length"
                );
            }
        }
        Ok(())
    }

    pub fn sample(&self, seconds: f32, rotation: bool) -> Result<[f32; N]> {
        ensure!(seconds.is_finite(), "invalid animation time");
        let last = self
            .times
            .len()
            .checked_sub(1)
            .context("empty animation channel")?;
        let a = self
            .times
            .partition_point(|&t| t <= seconds)
            .saturating_sub(1);
        let b = (a + 1).min(last);
        let stride = if self.interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
        let av = *self
            .values
            .get(a * stride + stride / 2)
            .context("missing animation key")?;
        if a == b || seconds <= self.times[0] || self.interpolation == Interpolation::Step {
            return Ok(av);
        }
        let bv = self.values[b * stride + stride / 2];
        let dt = self.times[b] - self.times[a];
        let t = (seconds - self.times[a]) / dt;
        if self.interpolation == Interpolation::Linear && rotation {
            ensure!(N == 4, "rotation needs four components");
            let q = glam::Quat::from_xyzw(av[0], av[1], av[2], av[3])
                .slerp(glam::Quat::from_xyzw(bv[0], bv[1], bv[2], bv[3]), t)
                .normalize()
                .to_array();
            return Ok(std::array::from_fn(|i| q[i]));
        }
        let mut value = match self.interpolation {
            Interpolation::Step => av,
            Interpolation::Linear => std::array::from_fn(|i| av[i] + t * (bv[i] - av[i])),
            Interpolation::CubicSpline => {
                let outgoing = self.values[a * 3 + 2];
                let incoming = self.values[b * 3];
                let t2 = t * t;
                let t3 = t2 * t;
                std::array::from_fn(|i| {
                    (2. * t3 - 3. * t2 + 1.) * av[i]
                        + (t3 - 2. * t2 + t) * dt * outgoing[i]
                        + (-2. * t3 + 3. * t2) * bv[i]
                        + (t3 - t2) * dt * incoming[i]
                })
            }
        };
        if rotation {
            let length = value.iter().map(|v| v * v).sum::<f32>().sqrt();
            ensure!(
                length.is_finite() && length > 0.,
                "invalid interpolated quaternion"
            );
            value.iter_mut().for_each(|v| *v /= length);
        }
        Ok(value)
    }
}

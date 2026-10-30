//! Editable camera position, orientation and aim in ordinary glTF animations.
//! GLB uses Y-up and seconds; gameplay uses Z-up and 30-Hz resource frames.
use crate::{
    CameraKey,
    animation::{self, Bone, Channel, Interpolation, NodeTrack, Skeleton, Track, Transform},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

const NAMES: [&str; 2] = ["Camera position", "Camera target"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraMotion {
    duration_frames: f32,
    /// Curves use resource frames after preparation; GLBs use seconds.
    tracks: Vec<Track>,
}

fn to_gltf([x, y, height]: [f32; 3]) -> [f32; 3] {
    [x, height, -y]
}
fn from_gltf([x, height, z]: [f32; 3]) -> [f32; 3] {
    [x, -z, height]
}
fn basis() -> glam::Quat {
    glam::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

// Resource-frame boundaries must survive float32 glTF timestamps. Snap only
// errors within float32 conversion precision; fractional editor times remain.
fn seconds_to_frames(seconds: f32) -> f32 {
    let frames = seconds * animation::FRAME_HZ;
    let rounded = frames.round();
    if (rounded - frames).abs() <= 4. * f32::EPSILON * frames.max(1.) {
        rounded
    } else {
        frames
    }
}
fn convert_times(tracks: &mut [Track], frames: bool) {
    fn channel<const N: usize>(channel: &mut Option<Channel<N>>, frames: bool) {
        if let Some(channel) = channel {
            for time in &mut channel.times {
                *time = if frames {
                    seconds_to_frames(*time)
                } else {
                    *time / animation::FRAME_HZ
                };
            }
            if channel.interpolation == Interpolation::CubicSpline {
                for (i, value) in channel.values.iter_mut().enumerate() {
                    if i % 3 != 1 {
                        for tangent in value {
                            *tangent = if frames {
                                *tangent / animation::FRAME_HZ
                            } else {
                                *tangent * animation::FRAME_HZ
                            };
                        }
                    }
                }
            }
        }
    }
    for track in tracks {
        for node in &mut track.nodes {
            channel(&mut node.translation, frames);
            channel(&mut node.rotation, frames);
            channel(&mut node.scale, frames);
        }
    }
}

impl CameraMotion {
    pub fn from_keys(keys: &[CameraKey]) -> Result<Self> {
        ensure!(
            !keys.is_empty()
                && keys.len() <= 100_000
                && keys.iter().all(|key| key.time.is_finite()
                    && key.time >= 0.
                    && key
                        .position
                        .iter()
                        .chain(&key.target)
                        .chain(&key.orientation)
                        .all(|v| v.is_finite())
                    && (glam::Quat::from_array(key.orientation).length_squared() - 1.).abs()
                        < 0.001)
                && keys.windows(2).all(|pair| pair[0].time < pair[1].time),
            "invalid source camera trajectory"
        );
        let times = keys.iter().map(|key| key.time).collect::<Vec<_>>();
        let tracks = (0..2)
            .map(|bone| Track {
                bone,
                nodes: vec![NodeTrack {
                    bind: Transform::default(),
                    translation: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: times.clone(),
                        values: keys
                            .iter()
                            .map(|key| to_gltf(if bone == 0 { key.position } else { key.target }))
                            .collect(),
                    }),
                    rotation: (bone == 0).then(|| Channel {
                        interpolation: Interpolation::Linear,
                        times: times.clone(),
                        values: keys
                            .iter()
                            .map(|key| {
                                (basis() * glam::Quat::from_array(key.orientation))
                                    .normalize()
                                    .to_array()
                            })
                            .collect(),
                    }),
                    scale: None,
                }],
            })
            .collect();
        let value = Self {
            duration_frames: keys.last().unwrap().time,
            tracks,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut tracks = self.tracks.clone();
        convert_times(&mut tracks, false);
        animation::Motion {
            duration_frames: self.duration_frames,
            tracks,
        }
        .encode_nodes(&Skeleton {
            bones: NAMES
                .into_iter()
                .map(|name| Bone {
                    name: name.into(),
                    parent: None,
                    parent_transform: None,
                    shadow_bounds: true,
                    bind: Transform::default(),
                })
                .collect(),
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut motion = animation::Motion::decode_world_nodes(bytes, &NAMES.map(String::from))?;
        let duration_frames = seconds_to_frames(motion.duration_frames / animation::FRAME_HZ);
        convert_times(&mut motion.tracks, true);
        let value = Self {
            duration_frames,
            tracks: motion.tracks,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn duration_frames(&self) -> f32 {
        self.duration_frames
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.duration_frames.is_finite() && self.duration_frames >= 0.,
            "invalid camera duration"
        );
        ensure!(
            self.tracks.len() == 2 && self.tracks.iter().map(|track| track.bone).eq(0..2),
            "camera needs position and target trajectories"
        );
        for track in &self.tracks {
            ensure!(!track.nodes.is_empty(), "empty camera transform chain");
            for node in &track.nodes {
                node.validate()?;
            }
        }
        Ok(())
    }

    pub fn sample(&self, frame: f32) -> Result<CameraKey> {
        ensure!(frame.is_finite(), "invalid camera time");
        let matrix = |track: &Track| -> Result<glam::Mat4> {
            let matrix = track
                .sample_matrix_at(frame.clamp(0., self.duration_frames()), track.nodes[0].bind)?;
            Ok(glam::Mat4::from_cols_array_2d(&matrix))
        };
        let point = |matrix: glam::Mat4| -> Result<[f32; 3]> {
            let result = from_gltf(matrix.w_axis.truncate().to_array());
            ensure!(
                result.iter().all(|v| v.is_finite()),
                "invalid camera position"
            );
            Ok(result)
        };
        let eye = matrix(&self.tracks[0])?;
        let (_, rotation, _) = eye.to_scale_rotation_translation();
        let orientation = (basis().conjugate() * rotation).normalize();
        ensure!(orientation.is_finite(), "invalid camera orientation");
        Ok(CameraKey {
            time: frame,
            position: point(eye)?,
            target: point(matrix(&self.tracks[1])?)?,
            orientation: orientation.to_array(),
        })
    }
}

#[cfg(test)]
mod tests;

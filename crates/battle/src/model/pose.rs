//! Sparse channels and affine keys keep their native mode through a cross-fade.
use anyhow::Result;
use resonance_content::animation::{Bone, Matrix, Motion, Skeleton, Transform};

#[derive(Debug, Clone, Copy)]
enum Value {
    Trs(Transform),
    Affine(Matrix),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BonePose {
    value: Value,
    flags: u8,
    authored: u8,
    animated: bool,
}

pub(super) fn sample(skeleton: &Skeleton, motion: &Motion, frame: f32) -> Result<Vec<BonePose>> {
    skeleton
        .bones
        .iter()
        .enumerate()
        .map(|(index, bone)| {
            let track = motion.tracks.iter().find(|t| usize::from(t.bone) == index);
            let authored = track.map_or(0, |t| t.channels().0);
            if let Some(track) = track.filter(|t| t.matrices.is_some()) {
                Ok(BonePose {
                    value: Value::Affine(track.sample_matrix(frame, bone.bind)?),
                    flags: 16,
                    authored,
                    animated: true,
                })
            } else {
                let value = match track {
                    Some(t) => t.sample(frame, bone.bind)?,
                    None => bone.bind,
                };
                let rest = bone.bind_channels.0;
                let flags = (authored | rest) & !6
                    | if authored & 6 != 0 {
                        authored & 6
                    } else {
                        rest & 6
                    };
                Ok(BonePose {
                    value: Value::Trs(value),
                    flags,
                    authored,
                    animated: track.is_some(),
                })
            }
        })
        .collect()
}

impl BonePose {
    fn set(&mut self, channel: u8, rest: &Bone) {
        let Value::Trs(mut value) = self.value else {
            self.value = Value::Trs(Transform::default());
            self.flags = 0;
            return self.set(channel, rest);
        };
        let present = rest.bind_channels.0 & channel != 0;
        match channel {
            1 => value.scale = if present { rest.bind.scale } else { [1.; 3] },
            8 => {
                value.translation = if present {
                    rest.bind.translation
                } else {
                    [0.; 3]
                }
            }
            4 => {
                value.rotation = if present {
                    rest.bind.rotation
                } else {
                    [0., 0., 0., 1.]
                };
                self.flags &= !6;
            }
            _ => unreachable!(),
        }
        self.flags |= channel;
        self.value = Value::Trs(value);
    }

    pub fn mix(mut self, mut to: Self, rest: &Bone, weight: f32) -> Self {
        // An absent track frees its channel controller and does not enter the
        // native per-bone cross-fade branch (part.initialized != 2).
        if !to.animated {
            return to;
        }
        // 8006D2E0: setters supply missing channels and clear affine mode. Euler
        // rotations are sampled normally but are not a quaternion blend channel.
        for channel in [1, 8, 4] {
            if self.flags & channel != 0 && to.authored & channel == 0 {
                to.set(channel, rest);
            }
            if to.authored & channel != 0 && self.flags & channel == 0 {
                self.set(channel, rest);
            }
        }
        if let (Value::Trs(from), Value::Trs(mut value)) = (self.value, to.value) {
            let mixed = from.blend(value, weight);
            let shared = self.flags & to.flags;
            if shared & 1 != 0 {
                value.scale = mixed.scale;
            }
            if shared & 8 != 0 {
                value.translation = mixed.translation;
            }
            if shared & 4 != 0 {
                value.rotation = mixed.rotation;
            }
            to.value = Value::Trs(value);
        }
        to
    }

    pub fn matrix(&self) -> Matrix {
        match self.value {
            Value::Trs(t) => t.matrix(),
            Value::Affine(m) => m,
        }
    }
    pub fn transform(&self) -> Transform {
        // Hierarchy composition uses the explicit matrix; no affine decomposition.
        match self.value {
            Value::Trs(t) => t,
            Value::Affine(_) => Transform::default(),
        }
    }
    pub fn translation(&self) -> [f32; 3] {
        match self.value {
            Value::Trs(t) if self.flags & 8 != 0 => t.translation,
            _ => [0.; 3],
        }
    }
    pub fn suppress_translation(&mut self, axes: [bool; 3]) {
        if let Value::Trs(t) = &mut self.value {
            for (value, suppress) in t.translation.iter_mut().zip(axes) {
                if suppress {
                    *value = 0.;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_keys_keep_shear_and_only_channel_setters_leave_matrix_mode() {
        let rest = Bone {
            name: "root".into(),
            parent: None,
            bind_channels: resonance_content::animation::TransformChannels(9),
            bind: Transform {
                translation: [2., 4., 6.],
                scale: [2.; 3],
                ..Default::default()
            },
        };
        let matrix = [
            [1., 0., 0., 0.],
            [0.5, 1., 0., 0.],
            [0., 0., 1., 0.],
            [99., 99., 99., 1.],
        ];
        let affine = BonePose {
            value: Value::Affine(matrix),
            flags: 16,
            authored: 16,
            animated: true,
        };
        assert_eq!(affine.mix(affine, &rest, 0.25).matrix(), matrix);
        let trs = BonePose {
            value: Value::Trs(Transform {
                translation: [10., 20., 30.],
                scale: [4.; 3],
                ..Default::default()
            }),
            flags: 13,
            authored: 13,
            animated: true,
        };
        let mixed = trs.mix(affine, &rest, 0.25).transform();
        assert_eq!(mixed.translation, [8., 16., 24.]);
        assert_eq!(mixed.scale, [3.5; 3]);
        let missing = BonePose {
            animated: false,
            ..trs
        };
        assert_eq!(affine.mix(missing, &rest, 0.25).matrix(), trs.matrix());
    }
}

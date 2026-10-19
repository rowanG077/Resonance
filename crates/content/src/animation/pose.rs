//! Sparse channels and affine keys keep their native mode through a cross-fade.
use super::{Matrix, Motion, Skeleton, Track, Transform};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalPose {
    Trs(Transform),
    Affine(Matrix),
}

#[derive(Debug, Clone, Copy)]
pub struct BonePose {
    value: LocalPose,
    authored: Option<Channels>,
}

#[derive(Debug, Clone, Copy)]
struct Channels {
    scale: bool,
    rotation: bool,
    translation: bool,
}

pub fn sample(skeleton: &Skeleton, motion: &Motion, frame: f32) -> Result<Vec<BonePose>> {
    skeleton
        .bones
        .iter()
        .enumerate()
        .map(|(index, bone)| {
            sample_bone(
                bone.bind,
                motion.tracks.iter().find(|t| usize::from(t.bone) == index),
                frame,
            )
        })
        .collect()
}

/// Sample only authored channels. Callers may retain the unwritten channels
/// from their last displayed pose before starting a new cross-fade.
pub fn sample_bone(bind: Transform, track: Option<&Track>, frame: f32) -> Result<BonePose> {
    let authored = track.map(|track| Channels {
        scale: track.scale.is_some(),
        rotation: track.rotation.is_some() || track.euler_degrees.is_some(),
        translation: track.translation.is_some(),
    });
    let value = match track {
        Some(track) if track.matrices.is_some() => {
            LocalPose::Affine(track.sample_matrix(frame, bind)?)
        }
        Some(track) => LocalPose::Trs(track.sample(frame, bind)?),
        None => LocalPose::Trs(bind),
    };
    Ok(BonePose { value, authored })
}

impl LocalPose {
    /// Rotate locally without changing translation or decomposing an affine basis.
    pub fn rotate(self, rotation: [f32; 4]) -> Self {
        match self {
            Self::Trs(mut value) => {
                value.rotation = (glam::Quat::from_array(value.rotation)
                    * glam::Quat::from_array(rotation))
                .to_array();
                Self::Trs(value)
            }
            Self::Affine(matrix) => Self::Affine(super::multiply(
                matrix,
                Transform {
                    rotation,
                    ..Transform::default()
                }
                .matrix(),
            )),
        }
    }

    /// Set axis lengths, retaining affine axis directions (including shear).
    /// A collapsed axis has no direction until a new authored pose supplies one.
    pub fn with_scale(self, scale: [f32; 3]) -> Self {
        match self {
            Self::Trs(mut value) => {
                value.scale = scale;
                Self::Trs(value)
            }
            Self::Affine(mut matrix) => {
                for (column, scale) in matrix[..3].iter_mut().zip(scale) {
                    let axis = glam::Vec3::from_slice(&column[..3]).normalize_or_zero() * scale;
                    column[..3].copy_from_slice(&axis.to_array());
                }
                Self::Affine(matrix)
            }
        }
    }

    /// TRS transforms blend smoothly; arbitrary affine bases step at the final
    /// endpoint so valid rotations and shear never pass through a singular mix.
    pub fn mix(self, to: Self, weight: f32) -> Self {
        if weight <= 0. {
            return self;
        }
        if weight >= 1. {
            return to;
        }
        match (self, to) {
            (Self::Trs(from), Self::Trs(to)) => Self::Trs(from.blend(to, weight)),
            _ => self,
        }
    }
}

impl BonePose {
    pub fn complete(value: LocalPose) -> Self {
        Self {
            value,
            authored: Some(Channels {
                scale: true,
                rotation: true,
                translation: true,
            }),
        }
    }

    pub fn value(self) -> LocalPose {
        self.value
    }

    pub fn retain_unwritten(&mut self, previous: Self) {
        let Some(authored) = self.authored else {
            self.value = previous.value;
            return;
        };
        match (&mut self.value, previous.value) {
            (LocalPose::Trs(value), LocalPose::Trs(from)) => {
                if !authored.scale {
                    value.scale = from.scale;
                }
                if !authored.translation {
                    value.translation = from.translation;
                }
                if !authored.rotation {
                    value.rotation = from.rotation;
                }
            }
            (LocalPose::Trs(value), LocalPose::Affine(mut from)) => {
                if !authored.scale && !authored.rotation {
                    if authored.translation {
                        from[3][..3].copy_from_slice(&value.translation);
                    }
                    self.value = LocalPose::Affine(from);
                } else if !authored.translation {
                    // Explicit basis channels select TRS without decomposing shear;
                    // translation is independent and can still be retained exactly.
                    value.translation.copy_from_slice(&from[3][..3]);
                }
            }
            // Authored affine keys supply the complete matrix.
            _ => {}
        }
    }

    pub fn mix(self, mut to: Self, weight: f32) -> Self {
        to.value = self.value.mix(to.value, weight);
        to.retain_unwritten(self);
        to
    }

    pub fn matrix(&self) -> Matrix {
        match self.value {
            LocalPose::Trs(t) => t.matrix(),
            LocalPose::Affine(m) => m,
        }
    }
    pub fn translation(&self) -> [f32; 3] {
        match self.value {
            LocalPose::Trs(t) => t.translation,
            LocalPose::Affine(matrix) => matrix[3][..3].try_into().unwrap(),
        }
    }
    pub fn suppress_translation(&mut self, axes: [bool; 3]) {
        let translation = match &mut self.value {
            LocalPose::Trs(transform) => transform.translation.as_mut_slice(),
            LocalPose::Affine(matrix) => &mut matrix[3][..3],
        };
        for (value, suppress) in translation.iter_mut().zip(axes) {
            if suppress {
                *value = 0.;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{
        Bone, QuaternionCurve, QuaternionInterpolation, Track, TransformChannels, VectorCurve,
        VectorInterpolation, transform_point,
    };

    fn rest() -> Bone {
        Bone {
            name: "root".into(),
            parent: None,
            bind_channels: TransformChannels(4),
            bind: Transform::default(),
        }
    }

    fn rotation_pose(euler: bool, degrees: f32) -> BonePose {
        let mut track = Track {
            bone: 0,
            bind_channels: TransformChannels(4),
            period_frames: 1.,
            times: vec![0.],
            translation: None,
            scale: None,
            rotation: None,
            euler_degrees: None,
            matrices: None,
        };
        if euler {
            track.euler_degrees = Some(VectorCurve {
                interpolation: VectorInterpolation::Step,
                values: vec![[0., 0., degrees]],
                incoming: vec![],
                outgoing: vec![],
                ease: vec![],
            });
        } else {
            track.rotation = Some(QuaternionCurve {
                interpolation: QuaternionInterpolation::Step,
                values: vec![glam::Quat::from_rotation_z(degrees.to_radians()).to_array()],
                incoming: vec![],
                outgoing: vec![],
                ease: vec![],
            });
        }
        sample(
            &Skeleton {
                bones: vec![rest()],
            },
            &Motion {
                duration_frames: 1.,
                tracks: vec![track],
            },
            0.,
        )
        .unwrap()[0]
    }

    #[test]
    fn rotation_cross_fades_are_independent_of_curve_encoding() {
        for from_euler in [false, true] {
            for to_euler in [false, true] {
                for (start, end, middle) in [(30., 150., 90.), (170., -170., 180.)] {
                    let from = rotation_pose(from_euler, start);
                    let to = rotation_pose(to_euler, end);
                    for (weight, degrees) in [(0., start), (0.5, middle), (1., end)] {
                        let point = transform_point(from.mix(to, weight).matrix(), [1., 0., 0.]);
                        let (sin, cos) = degrees.to_radians().sin_cos();
                        assert!(
                            point
                                .into_iter()
                                .zip([cos, sin, 0.])
                                .all(|(a, b)| (a - b).abs() < 0.00001)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn affine_root_suppression_and_stepped_transitions_preserve_the_basis() {
        let matrix = [
            [1., 0., 0., 0.],
            [0.5, 1., 0., 0.],
            [0., 0., 1., 0.],
            [10., 20., 30., 1.],
        ];
        let mut affine = BonePose {
            value: LocalPose::Affine(matrix),
            authored: Some(Channels {
                scale: false,
                rotation: false,
                translation: false,
            }),
        };
        assert_eq!(affine.translation(), [10., 20., 30.]);
        affine.suppress_translation([false, true, false]);
        assert_eq!(affine.translation(), [10., 0., 30.]);
        assert_eq!(&affine.matrix()[..3], &matrix[..3]);
        let from = rotation_pose(false, 0.);
        assert_eq!(from.mix(affine, 0.).matrix(), from.matrix());
        let mixed = from.mix(affine, 0.5);
        assert_eq!(mixed.matrix(), from.matrix());
        assert_eq!(from.mix(affine, 1.).matrix(), affine.matrix());

        let mut rotated = rotation_pose(false, 45.);
        rotated.retain_unwritten(affine);
        assert_eq!(rotated.translation(), affine.translation());
        assert!(matches!(rotated.value(), LocalPose::Trs(_)));
        let mut untracked = sample_bone(Transform::default(), None, 0.).unwrap();
        untracked.retain_unwritten(affine);
        assert_eq!(untracked.matrix(), affine.matrix());
    }

    #[test]
    fn affine_half_turn_transition_never_collapses_the_model() {
        let from = rotation_pose(false, 0.);
        let to = BonePose {
            value: LocalPose::Affine([
                [-1., 0., 0., 0.],
                [0., -1., 0., 0.],
                [0., 0., 1., 0.],
                [0., 0., 0., 1.],
            ]),
            authored: Some(Channels {
                scale: false,
                rotation: false,
                translation: false,
            }),
        };
        for weight in [0., 0.25, 0.5, 0.75, 1.] {
            let matrix = from.mix(to, weight).matrix();
            assert_eq!(
                matrix,
                if weight < 1. {
                    from.matrix()
                } else {
                    to.matrix()
                }
            );
            assert!(glam::Mat4::from_cols_array_2d(&matrix).determinant().abs() > 0.99);
        }
    }
}

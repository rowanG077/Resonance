//! Cross-fades retain channel presence; matrix mode is cleared by channel setters.
use super::Pose;
use bevy::prelude::*;

const SCALE: u8 = 1;
const ROTATION: u8 = 6;
const QUATERNION: u8 = 4;
const TRANSLATION: u8 = 8;
pub(super) const TRS: u8 = SCALE | QUATERNION | TRANSLATION;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    pub pose: Pose,
    flags: u8,
    authored: u8,
}

impl From<Transform> for Frame {
    fn from(transform: Transform) -> Self {
        Self::sample(transform.into(), TRS, TRS)
    }
}

impl Frame {
    pub fn sample(pose: Pose, authored: u8, bind: u8) -> Self {
        let flags = if matches!(pose, Pose::Affine(_)) {
            16
        } else {
            (authored | bind) & !ROTATION
                | if authored & ROTATION != 0 {
                    authored & ROTATION
                } else {
                    bind & ROTATION
                }
        };
        Self {
            pose,
            flags,
            authored,
        }
    }

    fn set(&mut self, channel: u8, rest: Transform, rest_channels: u8) {
        let Pose::Trs(mut value) = self.pose else {
            self.pose = Transform::IDENTITY.into();
            self.flags = 0;
            return self.set(channel, rest, rest_channels);
        };
        match channel {
            SCALE => {
                value.scale = if rest_channels & SCALE != 0 {
                    rest.scale
                } else {
                    Vec3::ONE
                }
            }
            QUATERNION => {
                value.rotation = if rest_channels & QUATERNION != 0 {
                    rest.rotation
                } else {
                    Quat::IDENTITY
                };
                self.flags &= !ROTATION;
            }
            TRANSLATION => {
                value.translation = if rest_channels & TRANSLATION != 0 {
                    rest.translation
                } else {
                    Vec3::ZERO
                }
            }
            _ => unreachable!(),
        }
        self.flags |= channel;
        self.pose = value.into();
    }

    pub fn mix(mut self, mut to: Self, rest: Transform, rest_channels: u8, weight: f32) -> Self {
        if matches!((self.pose, to.pose), (Pose::Trs(_), Pose::Trs(_))) {
            to.pose = self.pose.mix(to.pose, weight);
            return to;
        }
        for channel in [SCALE, TRANSLATION, QUATERNION] {
            if self.flags & channel != 0 && to.authored & channel == 0 {
                to.set(channel, rest, rest_channels);
            }
            if to.authored & channel != 0 && self.flags & channel == 0 {
                self.set(channel, rest, rest_channels);
            }
        }
        if let (Pose::Trs(from), Pose::Trs(mut value)) = (self.pose, to.pose) {
            let shared = self.flags & to.flags;
            if shared & SCALE != 0 {
                value.scale = from.scale.lerp(value.scale, weight);
            }
            if shared & TRANSLATION != 0 {
                value.translation = from.translation.lerp(value.translation, weight);
            }
            if shared & QUATERNION != 0 {
                value.rotation = from.rotation.slerp(value.rotation, weight);
            }
            to.pose = value.into();
        }
        to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Affine3A;

    #[test]
    fn mixed_modes_prefill_missing_channels_before_blending() {
        let matrix = Pose::Affine(Affine3A::from_cols(
            Vec3::X.into(),
            Vec3::new(0.5, 1., 0.).into(),
            Vec3::Z.into(),
            Vec3::splat(99.).into(),
        ));
        let rest = Transform::from_xyz(2., 4., 6.).with_scale(Vec3::splat(2.));
        let old = Transform::from_xyz(10., 20., 30.).with_scale(Vec3::splat(4.));
        let from = Frame::sample(old.into(), TRS, 0);
        let to = Frame::sample(matrix, 16, 0);
        let Pose::Trs(mixed) = from.mix(to, rest, TRS, 0.25).pose else {
            panic!("TRS setters clear matrix mode")
        };
        assert_eq!(mixed.translation, Vec3::new(8., 16., 24.));
        assert_eq!(mixed.scale, Vec3::splat(3.5));
        let translation_only =
            Frame::sample(Transform::from_xyz(10., 20., 30.).into(), TRANSLATION, 0);
        let Pose::Trs(mixed) = to.mix(translation_only, rest, TRS, 0.25).pose else {
            panic!()
        };
        assert_eq!(mixed.translation, Vec3::new(4., 8., 12.));
        assert_eq!(
            mixed.scale,
            Vec3::ONE,
            "unrequested scale never prefills from a sheared matrix"
        );
        assert_eq!(to.mix(to, rest, TRS, 0.25), to);
        // A rest Euler channel is not a quaternion channel, even though both
        // have already become quaternions in the renderer's Transform.
        let rest = rest.with_rotation(Quat::from_rotation_z(1.));
        let old = Frame::sample(
            Transform::from_rotation(Quat::from_rotation_z(2.)).into(),
            QUATERNION,
            0,
        );
        let Pose::Trs(mixed) = old.mix(to, rest, 2, 0.5).pose else {
            panic!()
        };
        assert!(mixed.rotation.angle_between(Quat::from_rotation_z(1.)) < 0.001);
        assert_eq!(mixed.scale, Vec3::ONE);
        assert_eq!(mixed.translation, Vec3::ZERO);
        let euler = Frame::sample(rest.into(), 2, 0);
        assert_eq!(euler.mix(to, rest, 2, 0.5), to);
        assert_eq!(to.mix(euler, rest, 2, 0.5), euler);
    }
}

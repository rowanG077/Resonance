//! Cross-fades use the model rest pose when switching between TRS and affine keys.
use super::Pose;
use bevy::prelude::*;

const SCALE: u8 = 1;
const QUATERNION: u8 = 4;
const TRANSLATION: u8 = 8;
const TRS: u8 = SCALE | QUATERNION | TRANSLATION;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    pub pose: Pose,
    flags: u8,
    authored: u8,
}

impl From<Transform> for Frame {
    fn from(transform: Transform) -> Self {
        Self::sample(transform.into(), TRS)
    }
}

impl Frame {
    pub fn sample(pose: Pose, authored: u8) -> Self {
        Self {
            pose,
            flags: if matches!(pose, Pose::Affine(_)) {
                16
            } else {
                authored | TRS
            },
            authored,
        }
    }

    fn set(&mut self, channel: u8, rest: Transform) {
        let mut value = match self.pose {
            Pose::Trs(value) => value,
            Pose::Affine(_) => {
                self.flags = 0;
                Transform::IDENTITY
            }
        };
        match channel {
            SCALE => value.scale = rest.scale,
            QUATERNION => value.rotation = rest.rotation,
            TRANSLATION => value.translation = rest.translation,
            _ => unreachable!(),
        }
        self.flags |= channel;
        self.pose = value.into();
    }

    pub fn mix(mut self, mut to: Self, rest: Transform, weight: f32) -> Self {
        if matches!((self.pose, to.pose), (Pose::Trs(_), Pose::Trs(_))) {
            to.pose = self.pose.mix(to.pose, weight);
            return to;
        }
        // A matrix has no individual TRS channels. Seed only the channels that
        // participate in this transition; several controllers may have sampled
        // this joint, so authored presence cannot be inferred from its last pose.
        for channel in [SCALE, TRANSLATION, QUATERNION] {
            if self.flags & channel != 0 && to.authored & channel == 0 {
                to.set(channel, rest);
            }
            if to.authored & channel != 0 && self.flags & channel == 0 {
                self.set(channel, rest);
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
        let from = Frame::sample(old.into(), TRS);
        let to = Frame::sample(matrix, 16);
        let Pose::Trs(mixed) = from.mix(to, rest, 0.25).pose else {
            panic!("TRS setters clear matrix mode")
        };
        assert_eq!(mixed.translation, Vec3::new(8., 16., 24.));
        assert_eq!(mixed.scale, Vec3::splat(3.5));
        let translation_only =
            Frame::sample(Transform::from_xyz(10., 20., 30.).into(), TRANSLATION);
        let Pose::Trs(mixed) = to.mix(translation_only, rest, 0.25).pose else {
            panic!()
        };
        assert_eq!(mixed.translation, Vec3::new(4., 8., 12.));
        assert_eq!(
            mixed.scale,
            Vec3::ONE,
            "unrequested scale never prefills from a sheared matrix"
        );
        assert_eq!(to.mix(to, rest, 0.25), to);
        // GLB models provide a complete rest transform, including rotation.
        let rest = rest.with_rotation(Quat::from_rotation_z(1.));
        let old = Frame::sample(
            Transform::from_rotation(Quat::from_rotation_z(2.)).into(),
            QUATERNION,
        );
        let Pose::Trs(mixed) = old.mix(to, rest, 0.5).pose else {
            panic!()
        };
        assert!(mixed.rotation.angle_between(Quat::from_rotation_z(1.5)) < 0.001);
        assert_eq!(mixed.scale, Vec3::splat(1.5));
        assert_eq!(mixed.translation, Vec3::new(1., 2., 3.));
    }

    #[test]
    fn overlapping_controllers_keep_channel_presence_after_an_affine_transition() {
        let rest = Transform::from_xyz(2., 4., 6.).with_scale(Vec3::splat(2.));
        let from = Frame::from(Transform::from_xyz(10., 20., 30.).with_scale(Vec3::splat(4.)));
        let affine = Frame::sample(
            Pose::Affine(Affine3A::from_scale(Vec3::splat(7.))),
            16 | TRANSLATION,
        );
        let partial = from.mix(affine, rest, 0.25);
        let Pose::Trs(value) = partial.pose else {
            panic!("unwritten channels leave matrix mode")
        };
        assert_eq!(value.scale, Vec3::splat(3.5));
        assert_eq!(value.translation, Vec3::ZERO);

        let next = Frame::sample(Transform::from_xyz(10., 20., 30.).into(), TRANSLATION);
        let Pose::Trs(value) = affine.mix(next, rest, 0.25).pose else {
            panic!()
        };
        assert_eq!(value.translation, Vec3::new(4., 8., 12.));
        assert_eq!(value.scale, Vec3::ONE);

        // When every TRS channel was also authored, no setter should replace
        // the final affine sample with a rest transform.
        let complete = Frame::sample(affine.pose, 16 | TRS);
        assert_eq!(from.mix(complete, rest, 0.25), complete);
        assert_eq!(partial.mix(complete, rest, 0.25), complete);
    }
}

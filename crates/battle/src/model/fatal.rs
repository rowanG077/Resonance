//! Equipment-only choreography sampled from the same published scene clock as
//! the body. The chakrams travel independently and meet at the gameplay contact;
//! no extra damaging projectile or second contact is created by this artwork.
use super::*;
use glam::{Mat4, Quat, Vec3};

impl Model {
    pub(super) fn fatal_equipment(&mut self, frame: &crate::BattleFrame) -> Result<()> {
        let Some(scene) = frame
            .fatal_strikes
            .as_ref()
            .and_then(|f| f.scene.as_ref())
            .filter(|s| s.performer == self.shown.actor && s.style == crate::FatalStyle::Chakram)
        else {
            return Ok(());
        };
        let launch = scene.impact_at.saturating_sub(12);
        let catch = scene.impact_at + 34;
        if scene.age < launch || scene.age >= catch {
            return Ok(());
        }
        let motion = self.definition.motion(self.shown.clip)?;
        let pose = self
            .definition
            .skeleton
            .sample(motion, f32::from(launch) * 0.5)?;
        let world = Mat4::from_cols_array_2d(&self.shown.world);
        let target = Vec3::from_array(frame.actors[scene.target.index()].target_center());
        for weapon in self
            .weapons
            .iter_mut()
            .filter(|w| w.slot().is_some_and(|s| s.is_primary()))
        {
            let attachment = usize::from(
                weapon
                    .definition
                    .attachment
                    .context("unbound finisher chakram")?,
            );
            let layer = weapon.definition.primary_layer();
            let bind = layer.skeleton.bind_pose()?;
            let markers = layer
                .skeleton
                .bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.name.to_ascii_lowercase().starts_with("at"))
                .map(|(i, _)| {
                    Mat4::from_cols_array_2d(&bind.global[i]).transform_point3(Vec3::ZERO)
                })
                .collect::<Vec<_>>();
            ensure!(
                !markers.is_empty(),
                "chakram has no prepared contact marker"
            );
            let marker = markers.iter().copied().sum::<Vec3>() / markers.len() as f32;
            let from = world * Mat4::from_cols_array_2d(&pose.global[attachment]);
            let to = world * Mat4::from_cols_array_2d(&self.shown.bones[attachment]);
            let start = from.transform_point3(marker);
            let end = to.transform_point3(marker);
            let (scale, initial, _) = from.to_scale_rotation_translation();
            let (_, caught, _) = to.to_scale_rotation_translation();
            let axis = (target - start).normalize_or_zero();
            let side = axis.cross(Vec3::Y).normalize_or(Vec3::Z);
            let sign = if weapon.slot() == Some(WeaponSlot(0)) {
                1.
            } else {
                -1.
            };
            let return_t =
                ((f32::from(scene.age) - f32::from(scene.impact_at) - 4.) / 30.).clamp(0., 1.);
            let ease = |t: f32| t * t * t * (t * (t * 6. - 15.) + 10.);
            let (position, rotation) = if scene.age <= scene.impact_at {
                let t = f32::from(scene.age - launch) / 12.;
                let p = t * t;
                let arc = (p * std::f32::consts::PI).sin();
                let offset = match scene.color {
                    crate::FatalColor::Blue => Vec3::Y * (-65. * arc) + side * (sign * 35. * arc),
                    crate::FatalColor::Red => Vec3::Y * (100. * arc) + side * (sign * 30. * arc),
                    crate::FatalColor::Green => side * (sign * 95. * arc),
                };
                (start.lerp(target, p) + offset, initial)
            } else {
                let p = ease(return_t);
                let arc = (p * std::f32::consts::PI).sin();
                (
                    target.lerp(end, p) + side * (sign * 75. * arc) + Vec3::Y * (22. * arc),
                    initial.slerp(caught, p),
                )
            };
            let spin = f32::from(scene.age - launch) * 0.7 * (1. - ease(return_t));
            let rotation = rotation * Quat::from_rotation_z(spin);
            let matrix = Mat4::from_scale_rotation_translation(
                scale,
                rotation,
                position - rotation * (marker * scale),
            );
            weapon.detach(Some(matrix.to_cols_array_2d()));
            weapon.step_with_clocks(&self.shown, false, false)?;
        }
        Ok(())
    }
}

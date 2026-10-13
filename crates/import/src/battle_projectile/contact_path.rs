//! Bake stationary model contacts independently of their visual effect origin.
use super::*;
use glam::{EulerRot, Mat4, Quat, Vec3};
use resonance_content::battle_effect::{
    EffectOperation, ParticleBirth, ParticleGeometry, declaration::Declaration,
};

const FIXED_MODEL_CONTACT: u32 = 0x809;
const GROUNDED_MODEL_CONTACT: u32 = FIXED_MODEL_CONTACT | 0x40;
const TENTACLE_ACTIVE: [u32; 2] = [10, 24];
const TENTACLE_LIFETIME: u32 = 46;

pub(super) fn prepare(
    row: &[u8],
    projectile: &Projectile,
    members: &crate::model_preview::PointerMembers<'_>,
) -> Result<Vec<Vec<[f32; 3]>>> {
    let grounded_model =
        u32::read(row, 8)? == GROUNDED_MODEL_CONTACT && row[0x16] == 1 && projectile.clamp_ground;
    let early_ground_contact = grounded_model && projectile.active == Some([0, 50]);
    let tentacle_contact = grounded_model
        && projectile.active == Some(TENTACLE_ACTIVE)
        && projectile.lifetime == Some(TENTACLE_LIFETIME);
    ensure!(
        (u32::read(row, 8)? == FIXED_MODEL_CONTACT || early_ground_contact || tentacle_contact)
            && row[0x16] <= 1,
        "model contact selection is not prepared"
    );
    ensure!(
        projectile.velocity == [0.; 3]
            && projectile.acceleration == [0.; 3]
            && projectile.motion == ProjectileMotion::default()
            && projectile.contact.offset == [0.; 3]
            && projectile.birth_effect.bank == 2,
        "moving model contact is not prepared"
    );
    let lifetime = projectile
        .lifetime
        .context("model contact needs a lifetime")?;
    let bank = crate::battle_effect::read(
        members
            .model(0x1cc)?
            .context("missing projectile effect bank")?,
    )?;
    let mut model = None;
    for event in bank.program(usize::from(projectile.birth_effect.member))? {
        ensure!(
            !matches!(event.operation, EffectOperation::SetMotion { .. }),
            "scheduled model motion is not prepared"
        );
        let EffectOperation::Spawn {
            particle, birth, ..
        } = &event.operation
        else {
            continue;
        };
        let Some(Declaration::ModelParticle {
            template, binding, ..
        }) = bank.actors.get(usize::from(*particle))
        else {
            continue;
        };
        ensure!(
            model.is_none() && event.at == 0 && **birth == ParticleBirth::default(),
            "model contact needs one immediate unmodified model"
        );
        model = Some((template, binding));
    }
    let (template, binding) = model.context("missing projectile contact model")?;
    ensure!(
        binding.slot < 6 && binding.animation == Some(0) && !binding.repeat,
        "model contact animation binding is not prepared"
    );
    ensure!(
        !tentacle_contact || matches!(binding.slot, 4 | 5),
        "tentacle contact needs its projectile-owned model"
    );
    let ParticleGeometry::Size {
        value,
        velocity,
        acceleration,
    } = &template.state.geometry
    else {
        anyhow::bail!("model contact needs a fixed scale");
    };
    ensure!(
        template.follow_origin
            && !template.follow_orientation
            && template.lifetime.is_none()
            && template.model_elevation.is_none()
            && template.ground_restitution.is_none()
            && [
                template.state.offset,
                template.state.velocity,
                template.state.acceleration,
                template.state.angular_velocity,
                template.state.orbit,
                template.jerk,
                template.orbit_velocity,
                *velocity,
                *acceleration,
            ]
            .into_iter()
            .all(|value| value == [0.; 3]),
        "animated particle transforms in model contacts are not prepared"
    );
    let slot = usize::from(binding.slot);
    let model = members
        .model(0x180 + slot * 4)?
        .context("missing contact model")?;
    let rig = crate::battle_model::rig(model, crate::battle_model::RigKind::Effect)?;
    let (_, model) = crate::geometry::model_resource(model)?;
    let model = crate::model::Model::parse(&model[crate::geometry::skeleton_range(model)?])?;
    let animation = members
        .model(0x1b0 + slot * 4)?
        .context("missing contact motion")?;
    let motion = crate::animation::read_member(animation)?
        .motion(&crate::animation::ModelBindings::from_model(&model))?;
    motion.validate(&rig.skeleton)?;
    let bones: Vec<_> = rig
        .skeleton
        .bones
        .iter()
        .enumerate()
        .filter(|(_, bone)| bone.name.to_ascii_lowercase().starts_with("at00_"))
        .map(|(index, _)| index as u16)
        .collect();
    ensure!(!bones.is_empty(), "missing model contact attachments");
    ensure!(
        !tentacle_contact || bones.len() == 1,
        "tentacle contact needs one attachment"
    );
    ensure!(
        row[0x16] == 1 || bones.len() == 1,
        "ambiguous model contact selection"
    );
    ensure!(
        projectile.active.map_or(bones.len() == 1, |[start, _]| {
            start >= 2 || (early_ground_contact && bones.len() == 1)
        }),
        "early model contact is not prepared"
    );
    let [x, y, z] = template.state.angles.map(f32::to_radians);
    let transform = Mat4::from_scale_rotation_translation(
        Vec3::from_array(*value),
        Quat::from_euler(EulerRot::ZYX, z, y, x),
        Vec3::ZERO,
    );
    let final_age = lifetime.min((motion.duration_frames * 2.).ceil() as u32);
    (0..=final_age)
        .map(|age| {
            let frame = (age as f32 * 0.5).min(motion.duration_frames);
            bones
                .iter()
                .map(|&bone| {
                    let point = rig.skeleton.sample_point(&motion, frame, bone, [0.; 3])?;
                    Ok(transform
                        .transform_point3(Vec3::from_array(point))
                        .to_array())
                })
                .collect()
        })
        .collect()
}

//! Enemy-local 400-byte shots and authored emission edits.
use crate::{battle_enemy_rules, battle_projectiles, dol};
use anyhow::{Context, Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec3};
use resonance_content::{
    animation::{Motion, Skeleton},
    battle_effect::{
        EffectOperation, ParticleBirth, ParticleGeometry, declaration::Declaration,
        document::Definition,
    },
    battle_enemy::rules::Shot,
    battle_martial::ProjectileFeedback,
    battle_model::document::Model,
    battle_projectile::{
        DamageKind, Projectile, ProjectileBoneDirection, ProjectileMotion, ProjectileOrigin,
    },
};
use std::{fs, path::Path};

fn member(data: &[u8], field: usize) -> Result<&[u8]> {
    let start = dol::u32(data, field)? as usize;
    ensure!(
        start >= 0x1e8 && start < data.len(),
        "invalid enemy member {field:#x}"
    );
    let mut end = data.len();
    for at in (0x18..0x1e8).step_by(4) {
        let other = dol::u32(data, at)? as usize;
        if other > start {
            end = end.min(other);
        }
    }
    data.get(start..end).context("enemy member exceeds package")
}

fn finite(row: &[u8], at: usize) -> Result<f32> {
    let value = f32::from_bits(dol::u32(row, at)?);
    ensure!(value.is_finite(), "non-finite enemy projectile at {at:#x}");
    Ok(value)
}

/// Bake the same bound animation that the shared model player will draw.
fn contact_points(
    skeleton: &Skeleton,
    motion: &Motion,
    bones: &[u16],
    lifetime: u32,
) -> Result<Vec<Vec<[f32; 3]>>> {
    let end = lifetime.min((motion.duration_frames * 2.).ceil() as u32);
    (0..=end)
        .map(|age| {
            bones
                .iter()
                .map(|&bone| {
                    skeleton.sample_point(
                        motion,
                        (age as f32 * 0.5).min(motion.duration_frames),
                        bone,
                        [0.; 3],
                    )
                })
                .collect()
        })
        .collect()
}

fn damage_kind(value: u8) -> Result<DamageKind> {
    Ok(match value {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        _ => anyhow::bail!("unknown enemy projectile damage kind {value}"),
    })
}

fn model_missile(row: &[u8; 400]) -> Result<bool> {
    let flags = dol::u32(row, 8)? & !0x10;
    Ok([
        0..8,
        0x10..0x11,
        0x18..0x24,
        0x58..0x5c,
        0x93..0x94,
        0x9d..400,
    ]
    .into_iter()
    .all(|range| row[range].iter().all(|&byte| byte == 0))
        && row[0x4c..0x5c] == [0; 16]
        && row[0x6c..0x88] == [0; 28]
        && row[0x90..0x94] == [0; 4]
        && row[0x9a..0x9c] == [0; 2]
        && match (flags, row[0x9c]) {
            (0x4201, 2) => {
                row[0x14..0x18] == [0; 4] && row[0x60..0x6c] == [0; 12] && row[0x94..0x9a] == [0; 6]
            }
            (0x16005, 6) => row[0x14..0x18] == [0, 0, 1, 0] && row[0x30..0x3c] == [0; 12],
            _ => false,
        })
}

/// The emission's modifier stream changes a private copy of its template. No
/// authored template or another shot is rewritten in the cooked library.
fn modified(data: &[u8], contact: &[u8]) -> Result<([u8; 400], u8, bool)> {
    ensure!(
        contact.len() >= 32
            && contact[2] == 254
            && contact[3] == 1
            && contact[6..18].iter().all(|&v| v == 0)
            && contact[19..22].iter().all(|&v| v == 0)
            && contact[24..32].iter().all(|&v| v == 0),
        "unsupported enemy projectile emission"
    );
    let table = member(data, 0x1c8)?;
    let at = usize::from(contact[4]) * 400;
    let mut row: [u8; 400] = table
        .get(at..at + 400)
        .context("missing enemy projectile template")?
        .try_into()?;
    let missile = model_missile(&row)?;
    let mut recoil = row[0x13];
    let offset = usize::from(dol::u16(contact, 22)?);
    if offset == 0 {
        return Ok((row, recoil, missile));
    }
    let source = member(data, 0x1dc)?;
    let mut cursor = offset;
    for _ in 0..64 {
        let operation = dol::u16(source, cursor)?;
        if operation == 0 {
            return Ok((row, recoil, missile));
        }
        let target = usize::from(dol::u16(source, cursor + 2)?);
        match (operation, target) {
            (1, 0x24 | 0x30 | 0x60 | 0x6c) => {
                let bytes = source
                    .get(cursor + 4..cursor + 16)
                    .context("truncated enemy projectile vector edit")?;
                for component in (0..12).step_by(4) {
                    finite(bytes, component)?;
                }
                row[target..target + 12].copy_from_slice(bytes);
                cursor += 16;
            }
            (2, 0x13) => {
                recoil = u8::try_from(dol::u32(source, cursor + 4)?)
                    .context("projectile recoil exceeds byte")?;
                cursor += 8;
            }
            (4, 0x24 | 0x2c | 0x30 | 0x60 | 0x68) => {
                finite(source, cursor + 4)?;
                row[target..target + 4].copy_from_slice(
                    source
                        .get(cursor + 4..cursor + 8)
                        .context("truncated enemy projectile scalar edit")?,
                );
                cursor += 8;
            }
            (5, 0x5d) => {
                row[0x5d] = u8::try_from(dol::u32(source, cursor + 4)?)
                    .context("projectile effect member exceeds byte")?;
                cursor += 8;
            }
            _ => anyhow::bail!("unsupported projectile modifier {operation} at {target:#x}"),
        }
    }
    anyhow::bail!("unterminated enemy projectile modifiers")
}

fn record(row: &[u8; 400], missile: bool, library: &Path, name: &str) -> Result<Projectile> {
    let flags = dol::u32(row, 8)? & !0x10;
    let model_contact = flags & 0x800 != 0;
    let point_history = flags & 0x4000 != 0;
    let bone_launch = flags & 0x20000 != 0 && row[0x9a] != row[0x9b];
    let plain_layout = [
        0..8,
        0x10..0x11,
        0x18..0x24,
        0x58..0x5c,
        0x93..0x94,
        0x9d..400,
    ]
    .into_iter()
    .all(|range| row[range].iter().all(|&byte| byte == 0));
    let ballistic_weapon =
        flags == 0x7209 && row[0x9c] == 4 && row[0x9a] != 0 && row[0x9b] != 0 && plain_layout;
    let target_floor = flags == 0x489
        && matches!(row[0x9c], 0 | 2)
        && row[0x24..0x3c].iter().all(|&v| v == 0)
        && row[0x6c..0x78].iter().all(|&v| v == 0);
    let ground_ricochet = flags == 0xf209 && row[0x9c] == 2 && row[0x8c..0x90] == [0; 4];
    // Point-history is an allocated draw buffer, not a submitted emitter. It
    // is safe to discard only for the native ordinary-flight recipes.
    if point_history {
        ensure!(
            matches!(
                (flags, row[0x9c]),
                (0x4209 | 0x7209, 0 | 2)
                    | (0x6209 | 0x7229, 2)
                    | (0x620a, 2)
                    | (0x214209, 4)
                    | (0x4211, 2)
                    | (0x16015, 6)
            ) || bone_launch
                || ground_ricochet
                || missile
                || ballistic_weapon,
            "unsupported projectile point-history controller {flags:#x}"
        );
        // The ordinary 0x7209/0 flight allocates a colored history buffer
        // but submits only its authored birth and trail effects.
        ensure!(
            missile
                || ballistic_weapon
                || (flags == 0x7209 && row[0x9c] == 0)
                || row[0x8c..0x90] == [0; 4]
                || row[0x8c..0x90] == [48, 48, 96, 255],
            "projectile point-history drawing is not prepared"
        );
    }
    ensure!(
        !model_contact || matches!(flags, 0x809 | 0x849),
        "unsupported model-contact controller {flags:#x}"
    );
    ensure!(
        matches!(row[0x9c], 0 | 2 | 4)
            || matches!(
                (flags, row[0x9c]),
                (1, 5) | (0x16005 | 0x16015, 6) | (0x409, 10)
            ),
        "unsupported enemy projectile update mode"
    );
    let mut plain = *row;
    let normalized = flags
        & !(0x4000
            | 0x800
            | if bone_launch { 0x20000 } else { 0 }
            | if target_floor { 0x80 } else { 0 }
            | if ground_ricochet { 0x8000 } else { 0 });
    plain[8..12].copy_from_slice(&normalized.to_be_bytes());
    if bone_launch || ballistic_weapon {
        plain[0x9a..0x9c].fill(0);
    }
    if matches!(row[0x9c], 5 | 6 | 10) {
        plain[0x9c] = 4;
    }
    let mut shot = battle_projectiles::scene_record(&plain, 1.)?;
    if target_floor {
        shot.motion.origin = ProjectileOrigin::TargetFloor;
    }
    if bone_launch {
        shot.motion.bone_direction = Some(ProjectileBoneDirection {
            from: row[0x9b],
            to: row[0x9a],
        });
    }
    if ground_ricochet {
        shot.motion
            .response
            .as_mut()
            .context("ground ricochet has no response")?
            .ground_ricochet = true;
    }
    if model_contact {
        shot.motion.contact_path = model_contact_path(row, &shot, library, name)?;
    }
    Ok(shot)
}

fn model_contact_path(
    row: &[u8; 400],
    shot: &Projectile,
    library: &Path,
    name: &str,
) -> Result<Vec<Vec<[f32; 3]>>> {
    let flags = dol::u32(row, 8)?;
    let grounded = flags == 0x849 && row[0x16] == 1 && shot.clamp_ground;
    let early = grounded && shot.active == Some([0, 50]);
    let tentacle = grounded && shot.active == Some([10, 24]) && shot.lifetime == Some(46);
    ensure!(
        flags == 0x809 || early || tentacle,
        "unsupported model-contact selection {flags:#x}"
    );
    ensure!(
        shot.velocity == [0.; 3]
            && shot.acceleration == [0.; 3]
            && shot.motion == ProjectileMotion::default()
            && shot.contact.offset == [0.; 3]
            && shot.birth_effect.bank == 2,
        "moving model contact is not prepared"
    );
    let lifetime = shot.lifetime.context("model contact needs a lifetime")?;
    let effect_path = format!("battle/enemies/{name}/effects/effects.json");
    let effect: Definition = serde_json::from_slice(
        &crate::authoring::read_document(library, &effect_path)
            .with_context(|| format!("missing native-cooked {effect_path}"))?,
    )?;
    let program = effect
        .programs
        .get(usize::from(shot.birth_effect.member))
        .context("missing projectile effect program")?;
    let mut selected = None;
    for event in program {
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
        }) = effect.templates.get(usize::from(*particle))
        else {
            continue;
        };
        ensure!(
            selected.is_none() && event.at == 0 && **birth == ParticleBirth::default(),
            "model contact needs one immediate unmodified model"
        );
        selected = Some((template, binding));
    }
    let (template, binding) = selected.context("missing projectile contact model")?;
    ensure!(
        binding.slot < 6 && binding.animation == Some(0) && !binding.repeat,
        "unsupported model-contact animation binding"
    );
    ensure!(
        !tentacle || matches!(binding.slot, 4 | 5),
        "tentacle contact needs its projectile-owned model"
    );
    let ParticleGeometry::Size {
        value,
        velocity,
        acceleration,
    } = &template.state.geometry
    else {
        anyhow::bail!("model contact needs a fixed size")
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
                *acceleration
            ]
            .into_iter()
            .all(|v| v == [0.; 3]),
        "animated particle transforms in model contact"
    );
    let model: Model = serde_json::from_value(
        effect
            .artwork
            .as_ref()
            .context("effect bank has no artwork")?
            .models
            .get(&binding.slot)
            .context("missing contact model slot")?
            .clone(),
    )?;
    let scene = &model
        .layers
        .first()
        .context("contact model has no layers")?
        .scene;
    resonance_content::validate_asset_path(&scene.mesh)?;
    let glb = fs::read(library.join(&scene.mesh))?;
    let skeleton = Skeleton::from_glb(&glb)?;
    let bones = skeleton
        .bones
        .iter()
        .enumerate()
        .filter(|(_, bone)| bone.name.to_ascii_lowercase().starts_with("at00_"))
        .map(|(index, _)| index as u16)
        .collect::<Vec<_>>();
    ensure!(
        !bones.is_empty(),
        "contact model has no at00_ attachment joints"
    );
    ensure!(
        !tentacle || bones.len() == 1,
        "tentacle contact needs one attachment"
    );
    ensure!(
        row[0x16] == 1 || bones.len() == 1,
        "ambiguous model-contact joint selection"
    );
    ensure!(
        shot.active.map_or(bones.len() == 1, |[start, _]| {
            start >= 2 || (early && bones.len() == 1)
        }),
        "early model contact is not prepared"
    );
    let clip = scene
        .clips
        .iter()
        .find(|clip| clip.resource_slot == 0)
        .context("contact model has no animation slot 0")?;
    ensure!(
        scene
            .clips
            .iter()
            .filter(|other| other.resource_slot == 0)
            .count()
            == 1,
        "ambiguous model-contact animation"
    );
    resonance_content::validate_asset_path(&clip.motion)?;
    ensure!(
        library.join(&clip.motion).is_file(),
        "missing model-contact clip"
    );
    let motion = Motion::decode(&fs::read(library.join(&clip.motion))?)?;
    let positions = contact_points(&skeleton, &motion, &bones, lifetime)?;
    let [x, y, z] = template.state.angles.map(f32::to_radians);
    let transform = Mat4::from_scale_rotation_translation(
        Vec3::from_array(*value),
        Quat::from_euler(EulerRot::ZYX, z, y, x),
        Vec3::ZERO,
    );
    Ok(positions
        .into_iter()
        .map(|points| {
            points
                .into_iter()
                .map(|point| {
                    transform
                        .transform_point3(Vec3::from_array(point))
                        .to_array()
                })
                .collect()
        })
        .collect::<Vec<_>>())
}

/// A policy may select a second local effect program for the same stationary
/// model contact (Plantix's paired ground eruptions).
pub(crate) fn with_model_effect(
    data: &[u8],
    index: u8,
    member_id: u8,
    library: &Path,
    name: &str,
) -> Result<Projectile> {
    let table = member(data, 0x1c8)?;
    let at = usize::from(index) * 400;
    let mut row: [u8; 400] = table
        .get(at..at + 400)
        .context("missing enemy model-contact template")?
        .try_into()?;
    ensure!(
        dol::u32(&row, 8)? & 0x800 != 0,
        "model effect variant needs a model contact"
    );
    row[0x5d] = member_id;
    record(&row, model_missile(&row)?, library, name)
}

/// Convert one emitted shot. The action owner interns equal returned recipes.
pub(crate) fn projectile(
    data: &[u8],
    contact: &[u8],
    hits: &[u8],
    library: &Path,
    name: &str,
) -> Result<Shot> {
    let (row, recoil, missile) = modified(data, contact)?;
    let mut definition = record(&row, missile, library, name)?;
    if definition.motion.origin != ProjectileOrigin::TargetFloor {
        definition.motion.origin = if contact[5] == 0 {
            ProjectileOrigin::Actor
        } else {
            ProjectileOrigin::Bone(contact[5])
        };
    }
    let kind = damage_kind(row[0x11])?;
    let at = usize::from(contact[18]) * 28;
    let mut hit: [u8; 28] = hits
        .get(at..at + 28)
        .context("missing enemy projectile hit")?
        .try_into()?;
    // Bank two explicitly names this enemy's own effect bank, which is the
    // ordinary owner-relative bank in the cooked hit definition.
    if hit[22] != 0 && hit[24] == 2 {
        hit[24] = 0;
    }
    let flags = dol::u16(&hit, 0)?;
    let sound = dol::u16(&hit, 16)?;
    let feedback = if hit[24] == 1 {
        ensure!(
            hit[22] == 0 || hit[24] <= 1,
            "unsupported martial projectile effect bank"
        );
        ensure!(
            hit[22] != 0 || sound == 0 || flags & 0x800 == 0,
            "martial projectile guard sound without effect"
        );
        let feedback = (hit[22] != 0).then_some(ProjectileFeedback {
            effect: u16::from(hit[22]),
            sound,
            on_guard: flags & 0x800 != 0,
        });
        hit[..2].copy_from_slice(&(flags & !0x800).to_be_bytes());
        hit[16..18].fill(0);
        hit[22] = 0;
        hit[24] = 0;
        feedback
    } else {
        None
    };
    let mut hit = battle_enemy_rules::hit_rule(&hit, 0, recoil, kind)?;
    ensure!(!hit.capture, "projectile cannot capture a melee body");
    if feedback.is_none() && sound != 0 {
        hit.sound = Some(sound);
    }
    Ok(Shot {
        definition,
        hit,
        feedback,
    })
}

#[cfg(test)]
mod cooked_contact_tests {
    use super::*;
    use crate::{battle_enemy_effects, enemy_package::EnemyPackages, world_gpl, world_motion};

    #[test]
    #[ignore = "requires RESONANCE_TEST_EXTRACTED with GC enemy packages"]
    fn cooked_gc_contact_motion_matches_native_reference_points() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_EXTRACTED").context("set RESONANCE_TEST_EXTRACTED")?,
        );
        let library = tempfile::tempdir()?;
        battle_enemy_effects::cook_battle_enemy_effects(&root, library.path(), 0)?;
        let path = library
            .path()
            .join("battle/enemies/torent-0000/effects/effects.json");
        let definition: Definition = serde_json::from_slice(&fs::read(path)?)?;
        let mut selected = None;
        for (slot, value) in definition
            .artwork
            .context("missing GC effect models")?
            .models
        {
            let model: Model = serde_json::from_value(value)?;
            let scene = &model.layers[0].scene;
            let Some(clip) = scene.clips.iter().find(|clip| clip.resource_slot == 0) else {
                continue;
            };
            let skeleton = Skeleton::from_glb(&fs::read(library.path().join(&scene.mesh))?)?;
            let bones = skeleton
                .bones
                .iter()
                .enumerate()
                .filter(|(_, bone)| bone.name.to_ascii_lowercase().starts_with("at00_"))
                .map(|(index, _)| index as u16)
                .collect::<Vec<_>>();
            if bones.is_empty() {
                continue;
            }
            let motion = Motion::decode(&fs::read(library.path().join(&clip.motion))?)?;
            selected = Some((slot, skeleton, motion, bones));
            break;
        }
        let (slot, skeleton, motion, bones) =
            selected.context("GC effect owner has no animated contact rig")?;
        let mut packages = EnemyPackages::open(&root.join("disc1"))?;
        let source = packages.read(0)?;
        let bindings =
            world_gpl::animation_bindings(member(&source, 0x180 + usize::from(slot) * 4)?)?;
        let native = world_motion::sample_bound_points(
            member(&source, 0x1b0 + usize::from(slot) * 4)?,
            &skeleton,
            &bindings,
            &bones,
            120,
        )?;
        let cooked = contact_points(&skeleton, &motion, &bones, 120)?;
        assert_eq!(native.len(), cooked.len());
        let error = native
            .iter()
            .flatten()
            .flatten()
            .zip(cooked.iter().flatten().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        ensure!(
            error < 1.,
            "cooked contact trajectory differs from native reference by {error}"
        );
        eprintln!(
            "GC cooked contact trajectory matches {} native samples; maximum component error {error}",
            native.len()
        );
        Ok(())
    }
}

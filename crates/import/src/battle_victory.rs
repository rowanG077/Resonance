//! B78 loads one fixed BTLwin package. 564D4 binds its sparse motion to slot23;
//! group performance hooks 80A78/80A7C are empty in the supported USA revision.
use crate::{
    animation::ModelBindings,
    read::{u16 as half, u32 as word},
    rel::Rel,
    resource::PartyResource,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_victory::{Group, OPENING_GROUPS, Ordinary, Performances, Posture},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, fs, path::Path};

pub fn publish(extracted: &Path, output: &Path) -> Result<String> {
    publish_source(extracted, output, "battle")
}

/// Every source affecting sparse binding or selection descriptors, for the
/// ordinary full publication DAG and the selected development publisher.
pub fn inputs(extracted: &Path) -> Result<BTreeMap<String, String>> {
    let files = extracted.join("files");
    let module_path = crate::field_resources::resolve_path(&files, "US_r_Top2Btl.rel")?;
    let module = Rel::read(&files.join(&module_path))?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let catalogue = crate::resource::read(&executable)?;
    let mut paths = vec![module_path];
    for at in [0x1d0, 0x1e4] {
        paths.push(crate::all_assets::roles::declared_path(
            &files,
            &module.text((4, at))?,
        )?);
    }
    for character in 1..=3 {
        paths.push(crate::field_resources::resolve_path(
            &files,
            catalogue.party(PartyResource::Body, character, 0)?,
        )?);
    }
    let mut result = paths
        .into_iter()
        .map(|path| {
            Ok((
                format!("files/{path}"),
                crate::media::hash_file(&files.join(path))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    result.insert("sys/main.dol".into(), crate::digest(&executable));
    Ok(result)
}

pub(crate) fn publish_source(extracted: &Path, output: &Path, prefix: &str) -> Result<String> {
    let paths = extracted.join("files");
    let module = Rel::read(&paths.join(crate::field_resources::resolve_path(
        &paths,
        "US_r_Top2Btl.rel",
    )?))?;
    // Reject a different regional runner rather than silently discarding its program.
    for at in [0x80a78, 0x80a7c] {
        ensure!(
            word(module.at((1, at))?, 0)? == 0x4e800020,
            "unsupported group victory runner"
        );
    }
    let archive_path = crate::all_assets::roles::declared_path(&paths, &module.text((4, 0x1e4))?)?;
    let groups_path = crate::all_assets::roles::declared_path(&paths, &module.text((4, 0x1d0))?)?;
    let archive = fs::read(paths.join(archive_path))?;
    let group_archive = fs::read(paths.join(groups_path))?;
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let mut ordinary = Vec::new();
    let mut files = BTreeMap::new();
    for character in 1..=3 {
        let path = crate::field_resources::resolve_path(
            &paths,
            catalogue.party(PartyResource::Body, character, 0)?,
        )?;
        let original = fs::read(paths.join(path))?;
        let body_sha256 = crate::digest(&original);
        let body = crate::compression::payload(original)?;
        let primary = crate::field::sections(&body)?
            .into_iter()
            .next()
            .flatten()
            .context("missing victory body")?;
        let (_, resource) = crate::geometry::model_resource(&body[primary])?;
        let model =
            crate::model::Model::parse(&resource[crate::geometry::skeleton_range(resource)?])?;
        let bindings = ModelBindings::from_model(&model);
        for selector in 0..5 {
            let index = usize::from((character - 1) * 5 + selector);
            let bytes = archive
                .get(index * 0x38000..(index + 1) * 0x38000)
                .context("missing victory package")?;
            ensure!(word(bytes, 0)? == 2, "invalid victory package directory");
            let start = word(bytes, 4)? as usize;
            let motion = word(bytes, 8)? as usize;
            ensure!(
                start >= 12 && start < motion && motion < bytes.len(),
                "invalid victory package members"
            );
            let rows = bytes[start..motion]
                .chunks_exact(12)
                .map(crate::battle_action::animation)
                .collect::<Result<Vec<_>>>()?;
            let terminal = rows
                .iter()
                .position(|row| row.time == -2)
                .context("unterminated victory animation stream")?;
            let animations = rows[..=terminal].to_vec();
            let motion = crate::animation::read_member(&bytes[motion..])?
                .motion(&bindings)?
                .encode()?;
            let path = format!("clips/{}.motion", crate::digest(&motion));
            crate::write_atomic(&output.join(&path), &motion)?;
            files.insert(
                path.clone(),
                File {
                    sha256: crate::digest(&motion),
                    bytes: motion.len() as u64,
                    roles: [Role::Data].into(),
                },
            );
            ordinary.push(Ordinary {
                character,
                selector,
                source_sha256: crate::digest(bytes),
                body_sha256: body_sha256.clone(),
                motion: path,
                animations,
            });
        }
    }
    let groups = OPENING_GROUPS
        .into_iter()
        .map(|id| {
            let index = usize::from(id);
            let bytes = group_archive
                .get(index * 0x800..(index + 1) * 0x800)
                .context("missing group victory package")?;
            let row = module
                .at((5, 0x5ea8 + (index - 1) * 8))?
                .get(..8)
                .context("truncated group victory descriptor")?;
            Ok(Group {
                id,
                character: row[4],
                voice_command: half(row, 0)?,
                source_sha256: crate::digest(bytes),
                descriptor: row.try_into()?,
            })
        })
        .collect::<Result<_>>()?;
    let postures = (1..=3)
        .map(|character| {
            let profile = module.at((
                5,
                0x3d30 + (usize::from(character) - 1) * crate::battle_profile::BYTES,
            ))?;
            let count = usize::from(profile[0xce]);
            ensure!(count <= 4, "too many result expression channels");
            let mut healthy_expression = [0; 4];
            let healthy = module.at((4, 0x2cc0 + (usize::from(character) - 1) * 2))?;
            healthy_expression[..2].copy_from_slice(&healthy[..2]);
            let mut weak_expression = [0; 4];
            weak_expression[..count].copy_from_slice(&profile[0xdf..0xdf + count]);
            Ok(Posture {
                character,
                rate: f32::from_bits(word(module.at((4, 0x3004))?, 0)?),
                healthy_expression,
                weak_expression,
            })
        })
        .collect::<Result<_>>()?;
    let record = Performances {
        camera: camera(&module)?,
        module_sha256: crate::digest(&module.bytes),
        archive_sha256: crate::digest(&archive),
        group_archive_sha256: crate::digest(&group_archive),
        ordinary,
        postures,
        groups,
        files,
    };
    let path = format!("{prefix}/victory.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&record)?)?;
    Ok(path)
}

fn camera(module: &Rel) -> Result<resonance_content::battle_victory::Camera> {
    let read = |offset| {
        let value = f32::from_bits(word(module.at((4, offset))?, 0)?);
        ensure!(value.is_finite(), "invalid result camera operand");
        Ok::<_, anyhow::Error>(value)
    };
    let mut placement = [[[0.; 3]; 4]; 3];
    for (row, actors) in placement.iter_mut().enumerate() {
        for (actor, position) in actors.iter_mut().enumerate() {
            for (axis, value) in position.iter_mut().enumerate() {
                *value = read(0x2cd4 + row * 48 + actor * 12 + axis * 4)?;
            }
        }
    }
    Ok(resonance_content::battle_victory::Camera {
        initial_radius: read(0x2f2c)?,
        minimum_radius: read(0x2fe4)?,
        additional_radius: read(0x2ffc)?,
        pitch: read(0x2fe8)?,
        group_angle: read(0x3000)?,
        angular_step: read(0x2fec)?,
        contraction: read(0x2f28)?,
        degrees_to_radians: read(0x2e44)?,
        focus_height: read(0x2e40)?,
        placement,
        circle_radius: read(0x2ff8)?,
        circle_extra_degrees: read(0x2ff0)?,
        circle_full_degrees: read(0x2ff4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires original discs; binds all opening victory sparse curves"]
    fn original_victory_packages_share_bodies_and_keep_selector_rows() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let output = tempfile::tempdir()?;
            let path = publish(&extracted, output.path())?;
            let bytes = fs::read(output.path().join(path))?;
            let value: Performances = serde_json::from_slice(&bytes)?;
            assert_eq!(value.camera.initial_radius, 2500.);
            assert_eq!(value.camera.minimum_radius, 1400.);
            assert_eq!(value.camera.additional_radius, 40.);
            assert_eq!(value.camera.pitch, 4.5);
            assert_eq!(value.camera.group_angle, -3.);
            assert_eq!(value.camera.angular_step, 0.025);
            assert_eq!(value.camera.contraction, 50.);
            assert_eq!(value.camera.focus_height, 112.5);
            assert_eq!(value.camera.degrees_to_radians.to_bits(), 0x3c8efa33);
            assert_eq!(value.camera.circle_radius, 200.);
            assert_eq!(value.camera.circle_extra_degrees, 10.);
            assert_eq!(value.camera.circle_full_degrees, 360.);
            assert_eq!(
                value.camera.placement,
                [
                    [
                        [70., 0., 0.],
                        [-70., 0., 0.],
                        [0., 0., -180.],
                        [-140., 0., -180.]
                    ],
                    [
                        [0., 0., 0.],
                        [-140., 0., 0.],
                        [140., 0., 0.],
                        [-70., 0., -180.]
                    ],
                    [
                        [55., 0., 0.],
                        [-55., 0., -90.],
                        [165., 0., -90.],
                        [-165., 0., 0.]
                    ],
                ]
            );
            assert_eq!(value.ordinary.len(), 15);
            assert_eq!(
                value
                    .groups
                    .iter()
                    .map(|group| group.id)
                    .collect::<Vec<_>>(),
                OPENING_GROUPS
            );
            for performance in &value.ordinary {
                assert_eq!(performance.animations[0].clip, 23);
                assert_eq!(performance.animations[0].time, 0);
                assert_eq!(performance.animations[1].clip, 255);
                assert_eq!(performance.animations.last().unwrap().time, -2);
                let motion = fs::read(output.path().join(&performance.motion))?;
                let declared = &value.files[&performance.motion];
                assert_eq!(declared.sha256, crate::digest(&motion));
                assert_eq!(declared.bytes, motion.len() as u64);
                assert!(
                    resonance_content::animation::Motion::decode(&motion)?.duration_frames > 0.
                );
            }
            // Module identity may differ by disc, but all supported packages,
            // bindings, selection operands and group descriptors must agree.
            let semantic = serde_json::to_vec(&(
                value.camera,
                value.postures,
                value.ordinary,
                value.groups,
                value.files,
            ))?;
            if let Some(previous) = previous {
                assert_eq!(semantic, previous);
            }
            previous = Some(semantic);
        }
        Ok(())
    }
}

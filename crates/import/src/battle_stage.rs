//! Fixed arena resources selected by 46C34 and installed by 46724.
use crate::{
    field::MapArchive,
    model_preview::{self, Layer},
    read::{f32 as float, u16 as half, u32 as word},
    rel::Rel,
    scene::decoded::Package,
    source_assets::{Sources, read_range},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_scene::Texture,
    battle_stage::Stage,
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, ops::Range, path::Path};

/// Publish a selected static arena using the ordinary shared model conversion.
pub fn publish(extracted: &Path, arena: u16, output: &Path) -> Result<String> {
    publish_source(
        extracted,
        &Sources::read(extracted)?,
        arena,
        output,
        "battle",
    )
}

pub(crate) fn publish_source(
    extracted: &Path,
    sources: &Sources,
    arena: u16,
    output: &Path,
    prefix: &str,
) -> Result<String> {
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let table = module
        .at((5, 0x3b90))?
        .get(..0x188)
        .context("truncated stage directory")?;
    let bytes = read_range(
        &extracted.join("files").join(&sources.stages),
        range(table, arena)?,
    )?;
    let archive = MapArchive::decode(&bytes)?;
    let path = format!("{prefix}/stages/{arena:03}.json");
    let stage = read(
        &archive,
        &format!("{prefix}/stages/{arena:03}.effects.json"),
        output,
    )?;
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&stage)?)?;
    Ok(path)
}

fn range(table: &[u8], arena: u16) -> Result<Range<usize>> {
    ensure!(arena <= i8::MAX as u16, "invalid stage index {arena}");
    let index = usize::from(arena) * 4;
    let start = word(table, index)? as usize;
    ensure!(arena == 0 || start != 0, "stage {arena} has no package");
    let end = table
        .get(index + 4..)
        .context("stage directory has no range end")?
        .chunks_exact(4)
        .map(|row| word(row, 0))
        .find(|value| !matches!(value, Ok(0)))
        .context("stage directory has no range end")?? as usize;
    let size = end
        .checked_sub(start)
        .filter(|&n| n != 0)
        .context("invalid stage range")?;
    let size = size.checked_add(31).context("stage range overflow")? & !31;
    Ok(start..start.checked_add(size).context("stage range overflow")?)
}

fn metadata(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() >= 0x318, "truncated stage metadata");
    ensure!(
        bytes[0x304] == 0,
        "stage scenery animation is not supported"
    );
    ensure!(
        half(bytes, 0x2d4)? == 0,
        "recurring stage effects are not supported"
    );
    ensure!(half(bytes, 0x302)? == 0, "stage fog is not supported");
    Ok(())
}

fn read(archive: &MapArchive, effects_path: &str, output: &Path) -> Result<Stage> {
    ensure!(
        archive.sections.len() == 11,
        "stage scenery is not supported"
    );
    let data = archive.section(0)?;
    metadata(data)?;
    let vector = |offset| -> Result<[f32; 3]> {
        Ok([
            float(data, offset)?,
            float(data, offset + 4)?,
            float(data, offset + 8)?,
        ])
    };
    let color = |offset| -> [u8; 4] { data[offset..offset + 4].try_into().unwrap() };
    let mut decoded = Package::default();
    let mut layers = BTreeMap::new();
    for slot in 0..4 {
        ensure!(
            archive.optional_section(slot + 5).is_none(),
            "stage model animation is not supported"
        );
        let Some(model) = archive.optional_section(slot + 1) else {
            continue;
        };
        let settings = 0x2c + slot * 0xa8;
        ensure!(
            data[settings + 0xa5] == 0,
            "stage UV animation is not supported"
        );
        let flags = data[settings + 0xa4];
        ensure!(flags & !2 == 0, "unsupported stage layer flags {flags}");
        let additive = flags & 2 != 0;
        let mut parts = Vec::new();
        model_preview::layers(
            Layer {
                model,
                outline: None,
                animation: None,
                attached_to: None,
                // Stage blend does not change the model's culling policy.
                additive: false,
            },
            &mut parts,
            &format!("battle/stage/{slot}"),
            output,
            &mut decoded,
        )?;
        let mut layer = parts.pop().context("missing stage model")?;
        ensure!(parts.is_empty(), "unexpected stage model layers");
        layer.additive = additive;
        layer.scene.resource = slot as u16;
        for material in &mut layer.scene.materials {
            // 459C0 overrides the fixed model's depth mask and blend function.
            material.depth_write = slot == 0 || slot == 3;
            material.blend = true;
        }
        layers.insert(slot as u8, layer);
    }
    let mut files = crate::battle_model::files(layers.values().map(|p| &p.scene), output)?;
    let effects = archive
        .optional_section(9)
        .map(|bytes| -> Result<_> {
            let bank = crate::battle_effect::read(bytes)?;
            let bytes = serde_json::to_vec(&bank)?;
            crate::write_atomic(&output.join(effects_path), &bytes)?;
            files.insert(
                effects_path.into(),
                File {
                    sha256: crate::digest(&bytes),
                    bytes: bytes.len() as u64,
                    roles: [Role::Data].into(),
                },
            );
            Ok(effects_path.to_owned())
        })
        .transpose()?;
    let textures = archive
        .optional_section(10)
        .map(|bytes| crate::texture::decode_source(bytes)?.write(output))
        .transpose()?
        .map(|catalogue| catalogue.textures)
        .unwrap_or_default()
        .into_iter()
        .map(|texture| -> Result<_> {
            let texture = texture.context("invalid stage texture")?;
            let images = (0..texture.images.len())
                .map(|i| texture.image(i))
                .collect::<Result<Vec<_>>>()?;
            for image in &images {
                files.insert(
                    image.path.clone(),
                    File {
                        sha256: crate::media::hash_file(&output.join(&image.path))?,
                        bytes: std::fs::metadata(output.join(&image.path))?.len(),
                        roles: [Role::Texture].into(),
                    },
                );
            }
            Ok(Texture {
                images,
                sampler: texture.sampler,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let stage = Stage {
        source_sha256: archive.source_sha256.clone(),
        light_position: vector(0)?,
        chain_acceleration: vector(12)?,
        actor_color: color(24),
        translation: vector(28)?,
        yaw_degrees: float(data, 40)?,
        color: color(0x2f8),
        camera_pitch_offset: float(data, 0x314)?,
        layers,
        effects,
        textures,
        files,
    };
    stage.validate()?;
    Ok(stage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_range_skips_holes_and_preserves_read_alignment() {
        let table = [0_u32, 64, 0, 0, 97, 160]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        assert_eq!(range(&table, 0).unwrap(), 0..64);
        assert_eq!(range(&table, 1).unwrap(), 64..128);
        assert!(range(&table, 2).is_err());
        assert!(range(&table, 5).is_err());
        assert!(range(&table, 128).is_err());
        let reversed = [0_u32, 64, 32]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        assert!(range(&reversed, 1).is_err());
    }

    #[test]
    fn unsupported_metadata_is_rejected_instead_of_silently_dropped() {
        let data = vec![0; 0x320];
        assert!(metadata(&data).is_ok());
        assert!(metadata(&data[..0x314]).is_err());
        for offset in [0x304, 0x2d4, 0x302] {
            let mut data = data.clone();
            data[offset] = 1;
            assert!(metadata(&data).is_err());
        }
    }

    #[test]
    #[ignore = "requires both extracted original discs; publishes only arena 13"]
    fn original_school_arena_preserves_models_metadata_and_dependencies() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let path = publish(&local.join(format!("disc{disc}")), 13, output.path())?;
            let bytes = std::fs::read(output.path().join(path))?;
            let stage: Stage = serde_json::from_slice(&bytes)?;
            stage.validate()?;
            assert_eq!(
                stage.source_sha256,
                "783d4bc91d71c2e6caff80f0980fea9d8e1cbf4b850024ba365a973d311bcb3c"
            );
            assert_eq!(stage.layers.keys().copied().collect::<Vec<_>>(), [0]);
            assert_eq!(stage.light_position, [300., 800., -200.]);
            assert_eq!(stage.chain_acceleration, [0.; 3]);
            assert_eq!(stage.translation, [0.; 3]);
            assert_eq!(stage.yaw_degrees, -15.);
            assert_eq!(stage.camera_pitch_offset, -1.);
            assert_eq!(stage.actor_color, [64, 64, 64, 255]);
            assert_eq!(stage.color, [64, 64, 64, 255]);
            assert_eq!(stage.model_colors(), [Some(stage.color), None, None, None]);
            assert!(!stage.layers[&0].scene.materials.is_empty());
            assert!(
                stage.layers[&0]
                    .scene
                    .materials
                    .iter()
                    .all(|m| m.depth_write && m.blend)
            );
            assert!(stage.effects.is_some());
            assert!(!stage.textures.is_empty());
            for (path, file) in &stage.files {
                let bytes = std::fs::read(output.path().join(path))?;
                assert_eq!(file.sha256, crate::digest(&bytes));
                assert_eq!(file.bytes, bytes.len() as u64);
            }
            if let Some(previous) = &previous {
                assert_eq!(&bytes, previous);
            }
            previous = Some(bytes);
        }
        Ok(())
    }
}

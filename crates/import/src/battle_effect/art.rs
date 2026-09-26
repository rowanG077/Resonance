//! Shared ordinary artwork registered by 34C; bank lookups are 4AEFC/4262C.
use anyhow::{Context, Result};
use resonance_content::{
    battle_effect::{Art, EffectTexture, SourceBank},
    battle_model::ModelPart,
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, path::Path};

pub(super) fn publish(usual: &[u8], banks: &[SourceBank], output: &Path) -> Result<Art> {
    let directory = crate::source_assets::section(usual, 4)
        .context("BTLusual.dat effect texture directory, member 4")?;
    let mut textures = BTreeMap::new();
    let mut strides: BTreeMap<u8, u16> = BTreeMap::new();
    for declaration in banks.iter().flat_map(|bank| &bank.actors) {
        let p = &declaration.prefix;
        if (4..=18).contains(&p.kind) && p.resource_slot != 255 && p.palette_stride != 0 {
            strides
                .entry(p.resource_slot)
                .and_modify(|stride| {
                    *stride = crate::texture::palette_page_step(
                        usize::from(*stride),
                        p.palette_stride.into(),
                    ) as u16;
                })
                .or_insert(p.palette_stride.into());
        }
    }
    // 34C reads these five slots from REL rodata+D8; texture indices remain intact.
    for (member, slot) in [10, 0, 11, 12, 1].into_iter().enumerate() {
        let entries = (|| {
            textures_with_stride(
                crate::source_assets::section(directory, member)?,
                strides.get(&slot).copied().unwrap_or(0),
                output,
            )
        })()
        .with_context(|| {
            format!("BTLusual.dat member 4 texture member {member}, native slot {slot}")
        })?;
        textures.insert(slot, entries);
    }
    let directory = crate::source_assets::section(usual, 6)
        .context("BTLusual.dat effect model directory, member 6")?;
    let mut decoded = crate::scene::decoded::Package::default();
    let mut models = BTreeMap::new();
    for (index, range) in crate::field::sections(directory)?.into_iter().enumerate() {
        let model = &directory[range.context("missing ordinary effect model")?];
        let mut layers = Vec::new();
        crate::model_preview::layers(
            crate::model_preview::Layer {
                model,
                outline: None,
                animation: None,
                attached_to: None,
                additive: false,
            },
            &mut layers,
            &format!("battle/effects/models/{}", index + 1),
            output,
            &mut decoded,
        )
        .with_context(|| format!("BTLusual.dat member 6 effect model {index}"))?;
        models.insert(
            u8::try_from(index + 1)?,
            ModelPart {
                rig: crate::battle_model::rig(model, crate::battle_model::RigKind::Effect)?,
                layers,
            },
        );
    }
    let mut files = crate::battle_model::files(
        models
            .values()
            .flat_map(|part| part.layers.iter().map(|layer| &layer.scene)),
        output,
    )?;
    for image in textures
        .values()
        .flatten()
        .flat_map(|texture: &EffectTexture| &texture.images)
    {
        files.insert(
            image.path.clone(),
            File {
                sha256: crate::media::hash_file(&output.join(&image.path))?,
                bytes: std::fs::metadata(output.join(&image.path))?.len(),
                roles: [Role::Texture].into(),
            },
        );
    }
    Ok(Art {
        source_sha256: crate::digest(usual),
        textures,
        models,
        files,
    })
}

/// Preserve original texture indices and palette pages for any ordinary bank.
pub(crate) fn textures_from_source(bytes: &[u8], output: &Path) -> Result<Vec<EffectTexture>> {
    textures_with_stride(bytes, 0, output)
}

fn textures_with_stride(bytes: &[u8], stride: u16, output: &Path) -> Result<Vec<EffectTexture>> {
    // 34C calls 4B390 before registering usual member 4/1. That member is
    // method-3 resource compression; its neighbors are ordinary TPL banks.
    let expanded =
        crate::compression::payload(bytes.to_vec()).context("expand effect texture bank")?;
    crate::texture::decode_source_with_palette_step(&expanded, stride)?
        .write(output)?
        .textures
        .into_iter()
        .enumerate()
        .map(|(index, texture)| {
            let texture = texture.with_context(|| format!("missing effect texture {index}"))?;
            let palette_step = match texture.format {
                crate::tpl::Format::Ci4 => 16,
                crate::tpl::Format::Ci8 => 256,
                crate::tpl::Format::Ci14 => 16384,
                _ => 0,
            };
            Ok(EffectTexture {
                images: (0..texture.images.len())
                    .map(|page| {
                        texture
                            .image(page)
                            .with_context(|| format!("effect texture {index}, palette {page}"))
                    })
                    .collect::<Result<_>>()?,
                sampler: texture.sampler,
                palette_step,
                page_step: crate::texture::palette_page_step(palette_step.into(), stride) as u16,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_ci8_publication_preserves_full_index_width_and_default_stride() -> Result<()> {
        let palette_at = 96;
        let pixels_at = palette_at + 512 * 2;
        let mut tpl = vec![0; pixels_at + 32];
        for (offset, value) in [
            (0, 0x0020_af30_u32),
            (4, 1),
            (8, 12),
            (12, 20),
            (16, 56),
            (20, 0x0001_0002),
            (24, 9),
            (28, pixels_at as u32),
            (60, 1),
            (64, palette_at as u32),
        ] {
            tpl[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        tpl[56..58].copy_from_slice(&512_u16.to_be_bytes());
        tpl[pixels_at..pixels_at + 2].copy_from_slice(&[0, 255]);
        for (index, rgb565) in [
            (32, 0xf800_u16),
            (287, 0x07e0),
            (256, 0x001f),
            (511, 0xffff),
        ] {
            tpl[palette_at + index * 2..palette_at + index * 2 + 2]
                .copy_from_slice(&rgb565.to_be_bytes());
        }
        let output = tempfile::tempdir()?;
        let textures = textures_with_stride(&tpl, 32, output.path())?;
        let texture = &textures[0];
        assert_eq!(texture.palette_step, 256);
        assert_eq!(texture.page_step, 32);
        assert_eq!(texture.images.len(), 9);
        assert_eq!(texture.palette_page(0, 32)?, 0);
        let shifted = texture.palette_page(1, 32)?;
        let image = crate::texture::pixels(&output.path().join(&texture.images[shifted].path))?;
        assert_eq!(image.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 255]);
        let native_default = texture.palette_page(1, 0)?;
        assert_eq!(native_default, 8);
        let image =
            crate::texture::pixels(&output.path().join(&texture.images[native_default].path))?;
        assert_eq!(image.as_raw(), &[0, 0, 255, 255, 255, 255, 255, 255]);
        assert!(texture.palette_page(9, 32).is_err());
        assert!(texture.palette_page(1, 16).is_err());
        // Ordinary texture publication retains its existing paths and spacing.
        let ordinary = textures_from_source(&tpl, output.path())?;
        assert_eq!(ordinary[0].page_step, 256);
        assert_eq!(ordinary[0].images.len(), 2);
        assert!(ordinary[0].palette_page(1, 32).is_err());
        assert_ne!(ordinary[0].images[1].path, texture.images[1].path);
        Ok(())
    }

    #[test]
    fn method_three_texture_bank_keeps_plain_tpl_identity_and_pixels() -> Result<()> {
        let mut tpl = vec![0; 96];
        for (offset, value) in [
            (0, 0x0020_af30_u32),
            (4, 1),
            (8, 12),
            (12, 20),
            (20, 0x0008_0008),
            (28, 64),
        ] {
            tpl[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        tpl[64..].fill(0xa5);
        let mut packed = vec![];
        for literals in tpl.chunks(8) {
            packed.push(0xff);
            packed.extend_from_slice(literals);
        }
        let mut source = vec![3];
        source.extend((packed.len() as u32).to_le_bytes());
        source.extend((tpl.len() as u32).to_le_bytes());
        source.extend(packed);
        source.resize(source.len().next_multiple_of(32), 0);
        let output = tempfile::tempdir()?;
        let plain = textures_from_source(&tpl, output.path())?;
        let expanded = textures_from_source(&source, output.path())?;
        assert_eq!(
            serde_json::to_value(&expanded)?,
            serde_json::to_value(&plain)?
        );
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].palette_step, 0);
        let image = crate::texture::pixels(&output.path().join(&expanded[0].images[0].path))?;
        assert_eq!(image.dimensions(), (8, 8));
        for pair in image.as_raw().chunks_exact(8) {
            assert_eq!(pair, [170, 170, 170, 170, 85, 85, 85, 85]);
        }
        Ok(())
    }
}

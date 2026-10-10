//! Shared battle effect artwork.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{Art, EffectTexture, SourceBank},
    battle_model::ModelPart,
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, path::Path};

pub(super) fn publish(
    usual: &[u8],
    banks: &mut [SourceBank],
    palette_strides: &[Vec<u8>],
    output: &Path,
) -> Result<Art> {
    let directory = crate::source_assets::section(usual, 4)
        .context("BTLusual.dat effect texture directory, member 4")?;
    let mut textures = BTreeMap::new();
    let mut strides: BTreeMap<u8, u16> = BTreeMap::new();
    for (declaration, &palette_stride) in banks
        .iter()
        .zip(palette_strides)
        .flat_map(|(bank, strides)| bank.actors.iter().zip(strides))
    {
        let Ok(visual) = declaration.particle_visual() else {
            continue;
        };
        if let Some(slot) = visual.texture.slot()
            && palette_stride != 0
        {
            strides
                .entry(slot)
                .and_modify(|stride| {
                    *stride = crate::texture::palette_page_step(
                        usize::from(*stride),
                        palette_stride.into(),
                    ) as u16;
                })
                .or_insert(palette_stride.into());
        }
    }
    // These five declared texture slots retain their texture indices.
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
    for (bank, strides) in banks.iter_mut().zip(palette_strides) {
        bind_palettes(bank, strides, &mut textures)?;
    }
    let textures: BTreeMap<_, _> = textures
        .into_iter()
        .map(|(slot, textures)| {
            (
                slot,
                textures
                    .into_iter()
                    .map(|texture| texture.effect)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
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
                layer_skeletons: Default::default(),
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

/// Palette storage details are kept only while importing a texture bank.
pub(crate) struct PublishedTexture {
    pub effect: EffectTexture,
    capacity: u16,
    page_step: u16,
}

impl PublishedTexture {
    fn selectors(&self, stride: u8) -> Vec<Option<usize>> {
        if self.capacity == 0 {
            return vec![Some(0); 256];
        }
        let stride = if stride != 0 {
            usize::from(stride)
        } else if self.capacity == 256 {
            256
        } else {
            16
        };
        let page_step = usize::from(self.page_step);
        (0..=255)
            .map(|selector| {
                let offset = selector * stride;
                (offset.is_multiple_of(page_step) && offset / page_step < self.effect.images.len())
                    .then_some(offset / page_step)
            })
            .collect()
    }
}

/// Bind each declaration to a dense selector table shared by its color and alpha textures.
pub(crate) fn bind_palettes(
    source: &mut SourceBank,
    strides: &[u8],
    textures: &mut BTreeMap<u8, Vec<PublishedTexture>>,
) -> Result<()> {
    use resonance_content::battle_effect::{declaration::Declaration, visual::ParticleTexture};
    ensure!(
        source.actors.len() == strides.len(),
        "effect palette binding count changed"
    );
    for (declaration, &stride) in source.actors.iter_mut().zip(strides) {
        let Declaration::Particle { visual, .. } = declaration else {
            continue;
        };
        let Some(slot) = visual.texture.slot() else {
            continue;
        };
        let Some(textures) = textures
            .get_mut(&slot)
            .filter(|textures| !textures.is_empty())
        else {
            continue;
        };
        let maps: Vec<_> = textures
            .iter()
            .map(|texture| texture.selectors(stride))
            .collect();
        let existing = (0..textures[0].effect.palette_sets.len()).find(|&set| {
            textures
                .iter()
                .zip(&maps)
                .all(|(texture, map)| texture.effect.palette_sets.get(set) == Some(map))
        });
        let set = if let Some(set) = existing {
            set
        } else {
            let set = textures[0].effect.palette_sets.len();
            for (texture, map) in textures.iter_mut().zip(maps) {
                texture.effect.palette_sets.push(map);
            }
            set
        };
        match &mut visual.texture {
            ParticleTexture::Atlas { palette_set, .. } => {
                *palette_set = set.try_into().context("too many effect palette sets")?;
            }
            ParticleTexture::Untextured => {}
        }
    }
    Ok(())
}

pub(crate) fn textures_from_source(bytes: &[u8], output: &Path) -> Result<Vec<PublishedTexture>> {
    textures_with_stride(bytes, 0, output)
}

fn textures_with_stride(bytes: &[u8], stride: u16, output: &Path) -> Result<Vec<PublishedTexture>> {
    let expanded =
        crate::compression::payload(bytes.to_vec()).context("expand effect texture bank")?;
    crate::texture::decode_source_with_palette_step(&expanded, stride)?
        .write(output)?
        .textures
        .into_iter()
        .enumerate()
        .map(|(index, texture)| {
            let texture = texture.with_context(|| format!("missing effect texture {index}"))?;
            let capacity = match texture.format {
                crate::tpl::Format::Ci4 => 16,
                crate::tpl::Format::Ci8 => 256,
                crate::tpl::Format::Ci14 => 16384,
                _ => 0,
            };
            let mut texture = PublishedTexture {
                effect: EffectTexture {
                    images: (0..texture.images.len())
                        .map(|page| {
                            texture
                                .image(page)
                                .with_context(|| format!("effect texture {index}, palette {page}"))
                        })
                        .collect::<Result<_>>()?,
                    sampler: texture.sampler,
                    palette_sets: vec![],
                },
                capacity,
                page_step: crate::texture::palette_page_step(capacity.into(), stride) as u16,
            };
            texture.effect.palette_sets.push(texture.selectors(0));
            Ok(texture)
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
        let mut textures = textures_with_stride(&tpl, 32, output.path())?;
        let texture = &mut textures[0];
        texture.effect.palette_sets.push(texture.selectors(32));
        assert_eq!(texture.effect.images.len(), 9);
        assert_eq!(texture.effect.palette_image(1, 0)?, 0);
        let shifted = texture.effect.palette_image(1, 1)?;
        let image =
            crate::texture::pixels(&output.path().join(&texture.effect.images[shifted].path))?;
        assert_eq!(image.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 255]);
        let default = texture.effect.palette_image(0, 1)?;
        assert_eq!(default, 8);
        let image =
            crate::texture::pixels(&output.path().join(&texture.effect.images[default].path))?;
        assert_eq!(image.as_raw(), &[0, 0, 255, 255, 255, 255, 255, 255]);
        assert!(texture.effect.palette_image(1, 9).is_err());
        let ordinary = textures_from_source(&tpl, output.path())?;
        assert_eq!(ordinary[0].effect.images.len(), 2);
        assert!(ordinary[0].selectors(32)[1].is_none());
        assert_ne!(
            ordinary[0].effect.images[1].path,
            texture.effect.images[1].path
        );
        // A dual texture binding shares its ID while keeping distinct image maps.
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../battle/tests/fixtures/stun-particle-source.json"
        ))?;
        let mut declaration: resonance_content::battle_effect::declaration::Declaration =
            serde_json::from_value(fixture["declaration"].clone())?;
        if let resonance_content::battle_effect::declaration::Declaration::Particle {
            visual, ..
        } = &mut declaration
        {
            visual.texture = resonance_content::battle_effect::visual::ParticleTexture::Atlas {
                slot: 0,
                dual: true,
                palette_set: 0,
            };
        }
        let mut bank = SourceBank {
            source_sha256: String::new(),
            art: None,
            programs: vec![],
            actors: vec![declaration; 3],
        };
        let mut alpha = PublishedTexture {
            effect: textures[0].effect.clone(),
            capacity: 0,
            page_step: 0,
        };
        alpha.effect.images.truncate(1);
        alpha.effect.palette_sets = vec![alpha.selectors(0)];
        textures[0].effect.palette_sets.truncate(1);
        textures.push(alpha);
        let mut textures = BTreeMap::from([(0, textures)]);
        bind_palettes(&mut bank, &[0, 32, 32], &mut textures)?;
        let sets: Vec<_> = bank
            .actors
            .iter()
            .map(|declaration| declaration.particle_visual().unwrap().texture.palette_set())
            .collect();
        assert_eq!(sets, [0, 1, 1]);
        assert_eq!(textures[&0][0].effect.palette_image(1, 1)?, 1);
        assert_eq!(textures[&0][1].effect.palette_image(1, 1)?, 0);
        assert_eq!(textures[&0][0].effect.palette_sets.len(), 2);
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
            serde_json::to_value(&expanded[0].effect)?,
            serde_json::to_value(&plain[0].effect)?
        );
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].effect.palette_image(0, 255)?, 0);
        let image =
            crate::texture::pixels(&output.path().join(&expanded[0].effect.images[0].path))?;
        assert_eq!(image.dimensions(), (8, 8));
        for pair in image.as_raw().chunks_exact(8) {
            assert_eq!(pair, [170, 170, 170, 170, 85, 85, 85, 85]);
        }
        Ok(())
    }
}

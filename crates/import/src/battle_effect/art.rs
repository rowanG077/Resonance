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
    palettes: BTreeMap<usize, usize>,
}

impl PublishedTexture {
    fn selector_stride(&self, stride: u8) -> usize {
        if stride != 0 {
            usize::from(stride)
        } else if self.capacity == 256 {
            256
        } else {
            16
        }
    }

    fn selectors(&self, stride: u8) -> Vec<Option<usize>> {
        if self.capacity == 0 {
            return vec![Some(0); 256];
        }
        let stride = self.selector_stride(stride);
        (0..=255)
            .map(|selector| self.palettes.get(&(selector * stride)).copied())
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

/// Effect banks can bind small subpalettes inside a full-width indexed atlas.
pub(crate) fn bank_textures(
    bytes: &[u8],
    bank: &SourceBank,
    strides: &[u8],
    slot: u8,
    output: &Path,
) -> Result<Vec<PublishedTexture>> {
    let bytes = crate::compression::payload(bytes.to_vec())?;
    let mut textures = textures_from_source(&bytes, output)?;
    complete_palettes(&bytes, bank, strides, slot, &mut textures, output)?;
    Ok(textures)
}

/// Retain prepared images and publish only newly required palette windows.
pub(crate) fn cached_bank_textures(
    bytes: &[u8],
    bank: &SourceBank,
    strides: &[u8],
    slot: u8,
    cached: &[EffectTexture],
    output: &Path,
) -> Result<Vec<PublishedTexture>> {
    let bytes = crate::compression::payload(bytes.to_vec())?;
    let descriptors = crate::tpl::parse_tpl(&bytes)?;
    ensure!(
        descriptors.len() == cached.len(),
        "cached effect texture count differs from source"
    );
    let identity = crate::digest(&bytes);
    let mut textures = descriptors
        .iter()
        .zip(cached)
        .enumerate()
        .map(|(index, (descriptor, effect))| {
            let capacity = match descriptor.format {
                8 => 16,
                9 => 256,
                10 => 16384,
                _ => 0,
            };
            let prefix = format!("textures/{identity}/texture-{index}");
            let mut palettes = BTreeMap::new();
            for (image, prepared) in effect.images.iter().enumerate() {
                ensure!(
                    prepared.width == u32::from(descriptor.width)
                        && prepared.height == u32::from(descriptor.height),
                    "cached effect texture dimensions differ from source"
                );
                let suffix = prepared
                    .path
                    .strip_prefix(&prefix)
                    .and_then(|path| path.strip_suffix(".ktx2"))
                    .context("cached effect texture identity differs from source")?;
                let offset = if capacity == 0 && suffix.is_empty() {
                    0
                } else if let Some(offset) = suffix.strip_prefix("-palette-offset-") {
                    offset.parse::<usize>()?
                } else {
                    suffix
                        .strip_prefix("-palette-")
                        .context("unrecognized cached effect palette image")?
                        .parse::<usize>()?
                        .checked_mul(usize::from(capacity))
                        .context("cached palette offset overflow")?
                };
                ensure!(
                    palettes.insert(offset, image).is_none(),
                    "duplicate cached effect palette window"
                );
            }
            ensure!(!palettes.is_empty(), "cached effect texture has no images");
            Ok(PublishedTexture {
                effect: effect.clone(),
                capacity,
                palettes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    complete_palettes(&bytes, bank, strides, slot, &mut textures, output)?;
    Ok(textures)
}

fn complete_palettes(
    bytes: &[u8],
    bank: &SourceBank,
    strides: &[u8],
    slot: u8,
    textures: &mut [PublishedTexture],
    output: &Path,
) -> Result<()> {
    use resonance_content::battle_effect::{EffectOperation, UvAnimation, UvChange};
    let declared_strides: std::collections::BTreeSet<_> = bank
        .actors
        .iter()
        .zip(strides)
        .filter_map(|(actor, &stride)| {
            (actor.particle_visual().ok()?.texture.slot() == Some(slot)).then_some(stride)
        })
        .collect();
    for (index, (texture, descriptor)) in textures
        .iter_mut()
        .zip(crate::tpl::parse_tpl(bytes)?)
        .enumerate()
    {
        if texture.capacity == 0 {
            continue;
        }
        let window = descriptor
            .palette_entries
            .min(usize::from(texture.capacity));
        for &stride in &declared_strides {
            let stride = texture.selector_stride(stride);
            for selector in 0..=u8::MAX {
                let offset = usize::from(selector) * stride;
                if offset + window > descriptor.palette_entries {
                    break;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    texture.palettes.entry(offset)
                {
                    let image =
                        crate::texture::publish_palette_window(bytes, index, offset, output)?;
                    entry.insert(texture.effect.images.len());
                    texture.effect.images.push(image);
                }
            }
        }
    }
    // Keep explicitly referenced tail windows beyond the declared palette, but
    // never infer palette storage from unrelated bytes that follow it.
    for (member, (actor, &stride)) in bank.actors.iter().zip(strides).enumerate() {
        let Ok(visual) = actor.particle_visual() else {
            continue;
        };
        if visual.texture.slot() != Some(slot) || stride == 0 {
            continue;
        }
        let template = actor.template()?;
        let mut selectors = std::collections::BTreeSet::from(template.state.palettes);
        if let Some(UvAnimation::Frames { frames, .. }) = &template.uv_animation {
            for frame in frames {
                if let UvChange::Palette { index } = &frame.change {
                    selectors.insert(*index);
                }
            }
        }
        for event in bank.programs.iter().flatten() {
            if let EffectOperation::Spawn {
                particle,
                palette,
                birth,
                ..
            } = &event.operation
                && usize::from(*particle) == member
            {
                selectors.extend(palette);
                for range in [birth.palette, birth.alpha_palette].into_iter().flatten() {
                    selectors.extend(
                        range.min.round().clamp(0., 255.) as u8
                            ..=range.max.round().clamp(0., 255.) as u8,
                    );
                }
            }
        }
        for (index, texture) in textures.iter_mut().enumerate() {
            if texture.capacity == 0 {
                continue;
            }
            for &selector in &selectors {
                let offset = usize::from(selector) * usize::from(stride);
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    texture.palettes.entry(offset)
                {
                    let image =
                        crate::texture::publish_palette_window(bytes, index, offset, output)?;
                    entry.insert(texture.effect.images.len());
                    texture.effect.images.push(image);
                }
            }
        }
    }
    Ok(())
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
            let step = crate::texture::palette_page_step(capacity.into(), stride);
            let palettes = (0..texture.images.len())
                .map(|page| (page * step, page))
                .collect();
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
                palettes,
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
            palettes: BTreeMap::new(),
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

        // A rejected program cannot hide declared-stride windows. An explicit
        // tail selector still retains the full eight-bit texture indices.
        tpl.resize(palette_at + (352 + 256) * 2, 0);
        tpl[pixels_at..pixels_at + 2].copy_from_slice(&[0, 31]);
        for (index, rgb565) in [(63, 0x07e0), (352, 0xf800_u16), (383, 0x07e0)] {
            tpl[palette_at + index * 2..palette_at + index * 2 + 2]
                .copy_from_slice(&rgb565.to_be_bytes());
        }
        bank.actors.truncate(1);
        if let resonance_content::battle_effect::declaration::Declaration::Particle {
            template,
            visual,
        } = &mut bank.actors[0]
        {
            template.state.palettes = [11, 11];
            template.uv_animation = None;
            visual.texture = resonance_content::battle_effect::visual::ParticleTexture::Atlas {
                slot: 6,
                dual: false,
                palette_set: 0,
            };
        }
        bank.programs = vec![vec![resonance_content::battle_effect::ScheduledEvent {
            at: 0,
            operation: resonance_content::battle_effect::EffectOperation::Unsupported {
                reason: "unprepared controller".into(),
            },
        }]];
        let mut textures =
            BTreeMap::from([(6, bank_textures(&tpl, &bank, &[32], 6, output.path())?)]);
        bind_palettes(&mut bank, &[32], &mut textures)?;
        let texture = &textures[&6][0].effect;
        assert_eq!(texture.images.len(), 10);
        let set = bank.actors[0].particle_visual()?.texture.palette_set();
        let declared = texture.palette_image(set, 1)?;
        let pixels = crate::texture::pixels(&output.path().join(&texture.images[declared].path))?;
        assert_eq!(pixels.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert!(texture.palette_image(set, 9).is_err());
        let image = texture.palette_image(set, 11)?;
        assert_eq!(image, 9);
        assert!(texture.palette_image(0, 2).is_err());
        let pixels = crate::texture::pixels(&output.path().join(&texture.images[image].path))?;
        assert_eq!(pixels.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 255]);

        // A metadata refresh retains the ordinary pages and fills only the
        // missing overlapping windows, including the explicit tail selector.
        let cached = textures_from_source(&tpl, output.path())?
            .into_iter()
            .map(|texture| texture.effect)
            .collect::<Vec<_>>();
        let supplement = tempfile::tempdir()?;
        let refreshed = cached_bank_textures(&tpl, &bank, &[32], 6, &cached, supplement.path())?;
        assert_eq!(refreshed[0].effect.images.len(), 10);
        for (before, after) in cached[0].images.iter().zip(&refreshed[0].effect.images) {
            assert_eq!(before.path, after.path);
            assert!(!supplement.path().join(&before.path).exists());
        }
        let index = refreshed[0].selectors(32)[11].unwrap();
        let pixels = crate::texture::pixels(
            &supplement
                .path()
                .join(&refreshed[0].effect.images[index].path),
        )?;
        assert_eq!(pixels.as_raw(), &[255, 0, 0, 255, 0, 255, 0, 255]);

        // For CI4, explicit stride16 is already the ordinary default layout.
        tpl[24..28].copy_from_slice(&8_u32.to_be_bytes());
        let cached = textures_from_source(&tpl, output.path())?
            .into_iter()
            .map(|texture| texture.effect)
            .collect::<Vec<_>>();
        let supplement = tempfile::tempdir()?;
        let refreshed = cached_bank_textures(&tpl, &bank, &[16], 6, &cached, supplement.path())?;
        assert_eq!(refreshed[0].selectors(16), cached[0].palette_sets[0]);
        assert_eq!(std::fs::read_dir(supplement.path())?.count(), 0);
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

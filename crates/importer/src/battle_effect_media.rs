//! Resident battle visual-effect media from the native battle archive.
use crate::{compression, dol, world_gpl, world_texture};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::{fs, path::Path};

pub(crate) const ATLAS_SLOTS: [usize; 5] = [10, 0, 11, 12, 1];

pub(crate) fn member(bytes: &[u8], index: usize) -> Result<&[u8]> {
    let count = dol::u32(bytes, 0)? as usize;
    ensure!(
        (1..=64).contains(&count) && index < count,
        "invalid effect member table"
    );
    let start = dol::u32(bytes, 4 + index * 4)? as usize;
    let end = (0..count)
        .map(|other| dol::u32(bytes, 4 + other * 4).map(|offset| offset as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&offset| offset > start)
        .min()
        .unwrap_or(bytes.len());
    ensure!(
        start >= 4 + count * 4 && end > start,
        "invalid effect member {index}"
    );
    bytes
        .get(start..end)
        .context("effect member exceeds archive")
}

/// Convert one selected resident effect atlas page. Both the common and
/// technique banks use these atlas sources; the image has one owner.
pub fn cook_battle_effect_texture(
    root: &Path,
    library: &Path,
    source_member: usize,
    texture_index: usize,
) -> Result<()> {
    let slot = *ATLAS_SLOTS
        .get(source_member)
        .context("invalid resident effect texture source")?;
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let atlas = member(member(&usual, 4)?, source_member)?;
    let decoded = if atlas.starts_with(&[0, 0x20, 0xaf, 0x30]) {
        atlas.to_vec()
    } else {
        compression::decode(atlas)?
    };
    let (ktx, _) = world_texture::tpl_texture(&decoded, texture_index)?;
    let target = library.join(format!(
        "battle/effects/common/textures/atlas-{slot}-{texture_index}-palette-0.ktx2"
    ));
    ensure!(!target.exists(), "battle effect atlas already exists");
    crate::output::write(&target, &ktx)?;
    Ok(())
}

pub(crate) fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Resident effect declarations specify palette strides. They are used only
/// while converting the source TPL; the cooked images contain ordinary colors.
pub(crate) fn palette_strides(usual: &[u8]) -> Result<[usize; 5]> {
    let mut strides = [0; 5];
    for bank in [2, 3] {
        let bytes = member(usual, bank)?;
        ensure!(bytes.starts_with(b"ef1\0"), "invalid resident effect bank");
        let start = usize::from(dol::u16(bytes, 8)?);
        let end = (10..20)
            .step_by(2)
            .map(|at| dol::u16(bytes, at).map(usize::from))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|&offset| offset > start)
            .min()
            .unwrap_or(bytes.len());
        ensure!(
            end >= start && (end - start).is_multiple_of(352),
            "invalid effect actor pool"
        );
        for actor in bytes
            .get(start..end)
            .context("resident effect actors exceed bank")?
            .chunks_exact(352)
        {
            let Some(slot) = ATLAS_SLOTS
                .iter()
                .position(|&slot| slot == usize::from(actor[2]))
            else {
                continue;
            };
            let stride = usize::from(actor[6]);
            if stride != 0 {
                strides[slot] = if strides[slot] == 0 {
                    stride
                } else {
                    gcd(strides[slot], stride)
                };
            }
        }
    }
    Ok(strides)
}

/// Convert the five shared, resident TPL atlases directly to editable KTX2.
/// The native effect banks determine overlapping palette windows. Repeated
/// windows are emitted once, with their first page number retained as its name.
pub fn cook_battle_effect_atlases(root: &Path, library: &Path) -> Result<usize> {
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let strides = palette_strides(&usual)?;
    let directory = member(&usual, 4)?;
    let mut published = 0;
    let mut seen = Vec::<Vec<u8>>::new();
    for (source_member, &slot) in ATLAS_SLOTS.iter().enumerate() {
        let source = member(directory, source_member)?;
        let tpl = if source.starts_with(&[0, 0x20, 0xaf, 0x30]) {
            source.to_vec()
        } else {
            compression::decode(source)?
        };
        let count = dol::u32(&tpl, 4)? as usize;
        for index in 0..count {
            let capacity = world_texture::tpl_palette_capacity(&tpl, index)?;
            let entries = world_texture::tpl_palette_entries(&tpl, index)?;
            let step = if capacity == 0 {
                0
            } else {
                gcd(capacity, strides[source_member]).max(1)
            };
            let pages = if capacity == 0 {
                1
            } else if step == capacity {
                entries.div_ceil(capacity).max(1)
            } else {
                entries.saturating_sub(capacity) / step + 1
            };
            for page in 0..pages {
                let offset = page * step;
                let (image, _) = world_texture::tpl_texture_palette(&tpl, index, offset)?;
                if seen.iter().any(|other| *other == image) {
                    continue;
                }
                seen.push(image.clone());
                let target = library.join(format!(
                    "battle/effects/common/textures/atlas-{slot}-{index}-palette-{page}.ktx2"
                ));
                ensure!(!target.exists(), "battle effect atlas already exists");
                crate::output::write(&target, &image)?;
                published += 1;
            }
        }
    }
    Ok(published)
}

/// Export the thirteen shared effect meshes and their embedded textures.
/// The technique bank references these same meshes; it does not own a copy.
pub fn cook_battle_effect_models(root: &Path, library: &Path) -> Result<usize> {
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let directory = member(&usual, 6)?;
    let count = dol::u32(directory, 0)? as usize;
    ensure!(count == 13, "unexpected resident effect model count");
    let mut images = Vec::<(Vec<u8>, String)>::new();
    for index in 0..count {
        let model = member(directory, index)?;
        let start = dol::u32(model, 0)? as usize;
        let end = dol::u32(model, 4)? as usize;
        let palette = model
            .get(start..end)
            .context("effect model TPL exceeds model")?;
        ensure!(
            dol::u32(palette, 0)? == 0x0020_af30,
            "effect model lacks TPL"
        );
        let stem = format!("battle/effects/common/models/model-{}-layer-0", index + 1);
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for texture_index in 0..dol::u32(palette, 4)? as usize {
            let (image, transparent) = world_texture::tpl_texture(palette, texture_index)?;
            let path = if let Some((_, path)) = images.iter().find(|(old, _)| *old == image) {
                path.clone()
            } else {
                let path = format!("{stem}-texture-{texture_index}.ktx2");
                let target = library.join(&path);
                ensure!(!target.exists(), "effect model image already exists");
                crate::output::write(&target, &image)?;
                images.push((image, path.clone()));
                path
            };
            paths.push(path);
            alpha.push(transparent);
        }
        let (glb, _) = world_gpl::static_model(model, paths, alpha)?;
        let target = library.join(format!("{stem}.glb"));
        ensure!(!target.exists(), "effect model already exists");
        crate::output::write(&target, &glb)?;
    }
    Ok(count)
}

pub(crate) fn model_artwork(usual: &[u8]) -> Result<BTreeMap<u8, serde_json::Value>> {
    let directory = member(usual, 6)?;
    let count = dol::u32(directory, 0)? as usize;
    ensure!(count == 13, "unexpected resident effect model count");
    let mut seen = Vec::<(Vec<u8>, String)>::new();
    let mut artwork = BTreeMap::new();
    for index in 0..count {
        let model = member(directory, index)?;
        let start = dol::u32(model, 0)? as usize;
        let end = dol::u32(model, 4)? as usize;
        let palette = model
            .get(start..end)
            .context("effect model TPL exceeds model")?;
        let stem = format!("battle/effects/common/models/model-{}-layer-0", index + 1);
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for texture_index in 0..dol::u32(palette, 4)? as usize {
            let (image, transparent) = world_texture::tpl_texture(palette, texture_index)?;
            let path = if let Some((_, path)) = seen.iter().find(|(old, _)| *old == image) {
                path.clone()
            } else {
                let path = format!("{stem}-texture-{texture_index}.ktx2");
                seen.push((image, path.clone()));
                path
            };
            paths.push(path);
            alpha.push(transparent);
        }
        let (_, mut scene) = world_gpl::static_model(model, paths, alpha)?;
        scene.mesh = format!("{stem}.glb");
        let document = resonance_content::battle_model::document::Model {
            attachment_nodes: Default::default(),
            independent_layers: Default::default(),
            layers: vec![resonance_content::model_preview::PreviewPart {
                scene,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            }],
        };
        artwork.insert(u8::try_from(index + 1)?, serde_json::to_value(document)?);
    }
    Ok(artwork)
}

struct AtlasEntry {
    effect: resonance_content::battle_effect::EffectTexture,
    capacity: usize,
    pages: BTreeMap<usize, usize>,
}

impl AtlasEntry {
    fn selectors(&self, stride: u8) -> Vec<Option<usize>> {
        if self.capacity == 0 {
            return vec![Some(0); 256];
        }
        let step = if stride == 0 {
            self.capacity
        } else {
            usize::from(stride)
        };
        (0..256)
            .map(|selector| self.pages.get(&(selector * step)).copied())
            .collect()
    }
}

/// Reconstruct editable image bindings directly from the resident TPL pages.
/// `bindings` are the native (slot, palette stride) pairs in actor order.
pub(crate) fn atlas_artwork(
    usual: &[u8],
    bindings: &[(u8, u8)],
) -> Result<(
    BTreeMap<u8, Vec<resonance_content::battle_effect::EffectTexture>>,
    BTreeMap<(u8, u8), u8>,
)> {
    use resonance_content::{battle_effect::EffectTexture, font::UiTexture};
    let strides = palette_strides(usual)?;
    let directory = member(usual, 4)?;
    let mut seen = Vec::<(Vec<u8>, String)>::new();
    let mut atlas = BTreeMap::<u8, Vec<AtlasEntry>>::new();
    for (source_member, &slot) in ATLAS_SLOTS.iter().enumerate() {
        let source = member(directory, source_member)?;
        let tpl = if source.starts_with(&[0, 0x20, 0xaf, 0x30]) {
            source.to_vec()
        } else {
            compression::decode(source)?
        };
        let count = dol::u32(&tpl, 4)? as usize;
        let table = dol::u32(&tpl, 8)? as usize;
        let mut entries = Vec::new();
        for index in 0..count {
            let descriptor = dol::u32(&tpl, table + index * 8)? as usize;
            let width = u32::from(dol::u16(&tpl, descriptor + 2)?);
            let height = u32::from(dol::u16(&tpl, descriptor)?);
            let capacity = world_texture::tpl_palette_capacity(&tpl, index)?;
            let colors = world_texture::tpl_palette_entries(&tpl, index)?;
            let step = if capacity == 0 {
                0
            } else {
                gcd(capacity, strides[source_member]).max(1)
            };
            let pages = if capacity == 0 {
                1
            } else if step == capacity {
                colors.div_ceil(capacity).max(1)
            } else {
                colors.saturating_sub(capacity) / step + 1
            };
            let mut images = Vec::<UiTexture>::new();
            let mut offsets = BTreeMap::new();
            for page in 0..pages {
                let (pixels, _) = world_texture::tpl_texture_palette(&tpl, index, page * step)?;
                let path = if let Some((_, path)) = seen.iter().find(|(old, _)| *old == pixels) {
                    path.clone()
                } else {
                    let path = format!(
                        "battle/effects/common/textures/atlas-{slot}-{index}-palette-{page}.ktx2"
                    );
                    seen.push((pixels, path.clone()));
                    path
                };
                let image = images
                    .iter()
                    .position(|item| item.path == path)
                    .unwrap_or_else(|| {
                        images.push(UiTexture {
                            path,
                            width,
                            height,
                        });
                        images.len() - 1
                    });
                offsets.insert(page * step, image);
            }
            let mut entry = AtlasEntry {
                effect: EffectTexture {
                    images,
                    sampler: world_texture::sampler(&tpl, descriptor)?,
                    palette_sets: Vec::new(),
                },
                capacity,
                pages: offsets,
            };
            entry.effect.palette_sets.push(entry.selectors(0));
            entries.push(entry);
        }
        atlas.insert(slot as u8, entries);
    }
    let mut sets = BTreeMap::new();
    for &(slot, stride) in bindings {
        let Some(textures) = atlas.get_mut(&slot) else {
            continue;
        };
        let maps = textures
            .iter()
            .map(|texture| texture.selectors(stride))
            .collect::<Vec<_>>();
        let existing = (0..textures[0].effect.palette_sets.len()).find(|&set| {
            textures
                .iter()
                .zip(&maps)
                .all(|(texture, map)| texture.effect.palette_sets[set] == *map)
        });
        let set = existing.unwrap_or(textures[0].effect.palette_sets.len());
        if existing.is_none() {
            for (texture, map) in textures.iter_mut().zip(maps) {
                texture.effect.palette_sets.push(map);
            }
        }
        sets.insert((slot, stride), u8::try_from(set)?);
    }
    Ok((
        atlas
            .into_iter()
            .map(|(slot, textures)| {
                (
                    slot,
                    textures.into_iter().map(|texture| texture.effect).collect(),
                )
            })
            .collect(),
        sets,
    ))
}

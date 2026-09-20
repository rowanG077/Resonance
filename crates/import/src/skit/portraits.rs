use super::*;
use crate::read::u32 as word;
use resonance_content::skit::{PortraitAsset, PortraitImage};

/// Bind the complete physical image set; animation never creates new textures.
pub(super) fn cook(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
) -> Result<BTreeMap<u32, PortraitAsset>> {
    let source = portrait_path(extracted, executable)?;
    let archive = fs::read(extracted.join("files").join(&source))?;
    let sources: BTreeMap<String, Vec<String>> = serde_json::from_slice(
        &fs::read(output.join("sources.json")).context("portrait images require cook-all")?,
    )?;
    let source = format!("disc{}/{source}", crate::disc_number(extracted)?);
    let directory = format!("assets/{}", crate::digest(&archive));
    ensure!(
        sources
            .get(&source)
            .is_some_and(|paths| paths.contains(&directory)),
        "missing or stale cooked portrait archive; rerun cook-all"
    );
    bind(&archive, output, &directory)
}

pub(crate) fn members(archive: &[u8]) -> Result<Vec<&[u8]>> {
    let count = word(archive, 0)? as usize;
    let end = count
        .checked_mul(8)
        .and_then(|n| n.checked_add(4))
        .context("portrait directory size overflow")?;
    let table = archive
        .get(4..end)
        .context("truncated portrait directory")?;
    table
        .chunks_exact(8)
        .map(|row| {
            let start = word(row, 0)? as usize;
            let size = word(row, 4)? as usize;
            if size == 0 {
                return Ok(&archive[..0]);
            }
            ensure!(start >= end, "portrait overlaps directory");
            archive
                .get(start..start.checked_add(size).context("portrait range overflow")?)
                .context("portrait outside archive")
        })
        .collect()
}

fn bind(archive: &[u8], output: &Path, directory: &str) -> Result<BTreeMap<u32, PortraitAsset>> {
    let mut result = BTreeMap::new();
    for (index, source) in members(archive)?.into_iter().enumerate() {
        if source.is_empty() {
            continue;
        }
        let id =
            0xd0000 | u32::from(u16::try_from(index).context("portrait exceeds native member ID")?);
        let original = crate::tpl::parse_tpl(source)?;
        let textures = crate::texture::bind(output, &format!("{directory}/{index}"))?;
        ensure!(
            original.len() == textures.len(),
            "portrait {index} image inventory changed"
        );
        let images = original
            .iter()
            .zip(textures)
            .map(|(original, texture)| {
                ensure!(
                    texture.dimensions == [original.width, original.height]
                        && texture.images.len() == 1,
                    "portrait {index} image binding differs from source"
                );
                Ok(PortraitImage {
                    texture: texture.images.into_iter().next().unwrap(),
                    size: texture.dimensions.map(u32::from),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let size = images.first().context("empty portrait")?.size;
        result.insert(id, PortraitAsset { size, images });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_members_are_independent_of_recipe_count_and_names() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-skit-images"));
        let result = (|| -> Result<()> {
            let count = recipe::COUNT + 1;
            let start = 4 + count * 8;
            let mut archive = vec![0; start + 96];
            archive[..4].copy_from_slice(&(count as u32).to_be_bytes());
            let row = 4 + (count - 1) * 8;
            archive[row..row + 4].copy_from_slice(&(start as u32).to_be_bytes());
            archive[row + 4..row + 8].copy_from_slice(&96u32.to_be_bytes());
            let tpl = &mut archive[start..];
            for (at, value) in [
                (0, 0x0020af30u32),
                (4, 1),
                (8, 12),
                (12, 20),
                (20, 0x00080008),
                (24, 14),
                (28, 64),
            ] {
                tpl[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            let directory = format!("assets/renamed/{last}", last = count - 1);
            let image = format!("{directory}/image.ktx2");
            crate::write_atomic(&root.join(&image), b"cooked image")?;
            let texture = crate::texture::Texture {
                dimensions: [8, 8],
                format: crate::tpl::Format::Cmpr,
                sampler: crate::tpl::parse_tpl(tpl)?[0].sampler()?,
                palette: None,
                images: vec![image.clone()],
            };
            crate::write_atomic(
                &root.join(directory).join("textures.json"),
                &serde_json::to_vec(&crate::texture::Catalogue {
                    textures: vec![Some(texture)],
                })?,
            )?;
            let members = members(&archive)?;
            assert_eq!(members.len(), count);
            assert!(members[..count - 1].iter().all(|row| row.is_empty()));
            let portraits = bind(&archive, &root, "assets/renamed")?;
            assert_eq!(portraits.len(), 1);
            assert_eq!(
                portraits[&(0xd0000 + (count - 1) as u32)].images[0].texture,
                image
            );
            assert!(self::members(&archive[..start - 1]).is_err());
            archive[row..row + 4].copy_from_slice(&4u32.to_be_bytes());
            assert!(self::members(&archive).is_err());
            archive[row..row + 4].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(self::members(&archive).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both extracted discs and cook-all textures; no conversion or playback"]
    fn original_portrait_images_and_all_recipes_bind_without_atlas_conversion() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let portraits = cook(&extracted, &local.join("all-assets"), &executable)?;
            assert_eq!(portraits.len(), 108);
            let recipes = recipe::read(&executable)?;
            assert_eq!(recipes.len(), 230);
            let archive = fs::read(
                extracted
                    .join("files")
                    .join(portrait_path(&extracted, &executable)?),
            )?;
            for (&id, portrait) in &portraits {
                let index = (id & 0xffff) as usize;
                let start = word(&archive, 4 + index * 8)? as usize;
                let size = word(&archive, 8 + index * 8)? as usize;
                let textures = crate::tpl::parse_tpl(&archive[start..start + size])?;
                assert_eq!(textures.len(), portrait.images.len());
                for (original, image) in textures.iter().zip(&portrait.images) {
                    assert_eq!(
                        [u32::from(original.width), u32::from(original.height)],
                        image.size
                    );
                    assert_eq!(original.format, 14);
                    assert!(image.size.iter().all(|size| size.is_multiple_of(8)));
                    assert!(local.join("all-assets").join(&image.texture).is_file());
                }
            }
            for recipe in &recipes {
                let prepared = recipe.prepared();
                for timeline in &recipe.timelines {
                    let track = &prepared.tracks[timeline.channel.index()];
                    let rows = timeline
                        .records
                        .iter()
                        .filter_map(|row| {
                            let recipe::Timing::Frame { duration } = row.timing else {
                                return None;
                            };
                            Some((duration, row.image, row.position))
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(
                        track
                            .iter()
                            .map(|row| (row.ticks, row.image, row.position))
                            .collect::<Vec<_>>(),
                        rows
                    );
                }
            }
        }
        Ok(())
    }
}

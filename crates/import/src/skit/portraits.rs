use super::*;
use crate::all_assets::skits::Portrait;
use crate::read::u32 as word;
use resonance_content::skit::{PortraitAsset, PortraitImage};

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

/// Pixels and their bindings stay together until terminal publication.
pub(super) struct Decoded {
    pub id: u32,
    pub asset: PortraitAsset,
    directory: String,
    textures: crate::texture::Decoded,
}

pub(super) fn decode(archive: &[u8], portrait: &Portrait, directory: &str) -> Result<Decoded> {
    let index = portrait.member;
    let members = members(archive)?;
    let bytes = members.get(index).context("portrait member is absent")?;
    let directory = format!("{directory}/{index}");
    let textures = crate::texture::decode(bytes, &directory)?;
    textures.validate()?;
    ensure!(
        portrait.images.len() == textures.catalogue.textures.len(),
        "portrait {index} image inventory changed"
    );
    let images = portrait
        .images
        .iter()
        .zip(&textures.catalogue.textures)
        .map(|(original, texture)| {
            let texture = texture.as_ref().context("invalid portrait texture")?;
            ensure!(
                texture.dimensions == [original.width, original.height]
                    && texture.images.len() == 1,
                "portrait {index} image binding differs from source"
            );
            Ok(PortraitImage {
                texture: texture.images[0].clone(),
                size: texture.dimensions.map(u32::from),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let size = images.first().context("empty portrait")?.size;
    Ok(Decoded {
        id: 0xd0000 | u32::from(u16::try_from(index).context("portrait exceeds native member ID")?),
        asset: PortraitAsset { size, images },
        directory,
        textures,
    })
}

impl Decoded {
    pub fn publish(&self, output: &Path) -> Result<(u32, PortraitAsset)> {
        let mut failures = Vec::new();
        self.textures.publish(output, |name, result| {
            if let Err(error) = result {
                failures.push(format!("{name}: {error:#}"));
            }
        });
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        write_atomic(
            &output.join(&self.directory).join("textures.json"),
            &serde_json::to_vec(&self.textures.catalogue)?,
        )?;
        Ok((self.id, self.asset.clone()))
    }
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
            let image = format!("assets/renamed/{}/texture-0.ktx2", count - 1);
            let members = members(&archive)?;
            assert_eq!(members.len(), count);
            assert!(members[..count - 1].iter().all(|row| row.is_empty()));
            let mut physical = [Portrait {
                member: count - 1,
                images: vec![crate::all_assets::skits::PortraitImage {
                    width: 8,
                    height: 8,
                }],
            }];
            let portrait = decode(&archive, &physical[0], "assets/renamed")?;
            assert!(!root.exists(), "decoding must not publish intermediates");
            let (id, asset) = portrait.publish(&root)?;
            assert_eq!(id, 0xd0000 + (count - 1) as u32);
            assert_eq!(asset.images[0].texture, image);
            assert_eq!(
                crate::texture::pixels(&root.join(image))?.dimensions(),
                (8, 8)
            );
            physical[0].images[0].width = 16;
            assert!(decode(&archive, &physical[0], "assets/renamed").is_err());
            assert!(self::members(&archive[..start - 1]).is_err());
            archive[row..row + 4].copy_from_slice(&4u32.to_be_bytes());
            assert!(self::members(&archive).is_err());
            archive[row..row + 4].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(self::members(&archive).is_err());
            Ok(())
        })();
        if root.exists() {
            fs::remove_dir_all(root)?;
        }
        result
    }

    #[test]
    #[ignore = "requires both extracted discs; decodes portrait pixels without publication or playback"]
    fn original_portrait_images_and_all_recipes_decode_without_intermediate_assets() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let catalog = Catalog::read(&extracted, &executable)?;
            assert_eq!(
                catalog
                    .portraits
                    .iter()
                    .filter(|p| !p.images.is_empty())
                    .count(),
                108
            );
            let recipes = recipe::read(&executable)?;
            assert_eq!(recipes.len(), 230);
            let archive = fs::read(
                extracted
                    .join("files")
                    .join(portrait_path(&extracted, &executable)?),
            )?;
            for physical in catalog.portraits.iter().filter(|p| !p.images.is_empty()) {
                let decoded = decode(&archive, physical, "assets/portraits")?;
                let portrait = &decoded.asset;
                let index = physical.member;
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

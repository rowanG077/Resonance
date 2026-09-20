//! Enemy target icons bind the texture bank exported from their package header.
use crate::battle::{all::Sources, visual::binding::Directory};
use anyhow::{Context, Result, ensure};
use resonance_content::font::UiTexture;
use std::{collections::BTreeMap, path::Path};

pub(super) fn bind(
    root: &Path,
    disc: u8,
    sources: &Sources,
    enemies: &[u8],
) -> Result<BTreeMap<u8, UiTexture>> {
    if enemies.is_empty() {
        return Ok(BTreeMap::new());
    }
    let directory = Directory::open(root, disc, &sources.enemy)?;
    enemies
        .iter()
        .map(|&id| {
            let textures = directory.textures(&format!("battle/enemy-{id}/member-1d4"))?;
            let image = textures.first().context("empty enemy icon bank")?;
            ensure!(
                image.dimensions == [32, 32],
                "unexpected enemy {id} icon dimensions"
            );
            Ok((
                id,
                UiTexture {
                    path: image.images[0].clone(),
                    width: u32::from(image.dimensions[0]),
                    height: u32::from(image.dimensions[1]),
                },
            ))
        })
        .collect()
}

#[test]
#[ignore = "requires both extracted discs and cook-all; no conversion or devices"]
fn original_enemy_icons_bind_the_complete_shared_image_library() -> Result<()> {
    use crate::read::u32 as word;
    use std::fs;
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = project.join("local/all-assets");
    for disc in [1, 2] {
        let extracted = project.join(format!("local/extracted/disc{disc}"));
        let sources = Sources::read(&extracted)?;
        let usual = fs::read(extracted.join("files").join(&sources.usual))?;
        let archive = fs::read(extracted.join("files").join(&sources.enemy))?;
        let table = word(&usual, 0x2c)? as usize;
        let icons = bind(&root, disc, &sources, &(0..251).collect::<Vec<_>>())?;
        for (id, icon) in icons {
            let start = word(&usual, table + usize::from(id) * 4)? as usize;
            let end = word(&usual, table + (usize::from(id) + 1) * 4)? as usize;
            let package = crate::compression::decode(&archive[start..end])?;
            let offset = word(&package, 0x1d4)? as usize;
            ensure!(offset != 0, "source has no enemy icon");
            let original = crate::tpl::parse_tpl(&package[offset..])?;
            assert_eq!(
                [icon.width, icon.height],
                [u32::from(original[0].width), u32::from(original[0].height)]
            );
            assert!(icon.path.starts_with("assets/"));
            let palette = if original[0].palette_offset.is_some() {
                "-palette-0"
            } else {
                ""
            };
            assert!(icon.path.ends_with(&format!(
                "battle/enemy-{id}/member-1d4/texture-0{palette}.ktx2"
            )));
            assert_eq!(
                &fs::read(root.join(icon.path))?[..12],
                b"\xabKTX 20\xbb\r\n\x1a\n"
            );
        }
    }
    Ok(())
}

//! Original defeat artwork and system-font labels, retained as typed inputs.
use anyhow::{Context, Result};
use resonance_content::{font::UiTexture, game_over::document::Definition};
use std::{fs, path::Path};

pub(crate) fn prepare(
    extracted: &Path,
) -> Result<(Definition, Vec<resonance_asset_writer::texture::Pixels>)> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let name = crate::dol::text(&executable, 0x8017d870)?;
    let source = crate::all_assets::roles::declared_path(&extracted.join("files"), &name)?;
    let images = crate::texture::decode_source(&fs::read(extracted.join("files").join(source))?)?;
    let background = images
        .catalogue
        .textures
        .get(14)
        .and_then(Option::as_ref)
        .context("missing game-over image14")?
        .image(0)?;
    let definition = Definition {
        version: 1,
        background: UiTexture {
            path: "ui/game-over/background.png".into(),
            width: background.width,
            height: background.height,
        },
        font: "fonts/dialogue.json".into(),
        caption: crate::dol::text(&executable, 0x8017d87c)?,
        choices: [
            crate::dol::text(&executable, 0x801f9c48)?,
            crate::dol::text(&executable, 0x801f9c54)?,
        ],
    };
    definition.art()?;
    Ok((definition, images.image_levels(14, 0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted discs; compares native labels and pixels"]
    fn original_game_over_keeps_both_disc_labels_and_pixels() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let (definition, pixels) = prepare(&extracted)?;
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            assert_eq!(
                definition.caption,
                crate::dol::text(&executable, 0x8017d87c)?
            );
            assert_eq!(definition.choices, ["Load data", "Quit game"]);
            assert_eq!(
                [definition.background.width, definition.background.height],
                [640, 480]
            );
            let name = crate::dol::text(&executable, 0x8017d870)?;
            let path = crate::all_assets::roles::declared_path(&extracted.join("files"), &name)?;
            let native =
                crate::texture::decode_source(&fs::read(extracted.join("files").join(path))?)?;
            let (_, original) = native
                .base_images()
                .nth(14)
                .context("missing native game-over image")??;
            assert_eq!(pixels[0].rgba, original);
            let current = (serde_json::to_vec(&definition)?, pixels);
            if let Some(previous) = &previous {
                assert_eq!(&current, previous);
            }
            previous = Some(current);
        }
        Ok(())
    }
}

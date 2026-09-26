//! DOL 7202C: title image14 and the three literal system-font strings.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field_preload::{File, Role},
    font::{BitmapFont, UiTexture},
    game_over::{Art, PATH},
};
use std::{collections::BTreeMap, fs, path::Path};

pub fn inputs(extracted: &Path) -> Result<BTreeMap<String, String>> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let name = crate::dol::text(&executable, 0x8017d870)?;
    let path = crate::all_assets::roles::declared_path(&extracted.join("files"), &name)?;
    Ok([
        ("sys/main.dol".into(), crate::digest(&executable)),
        (
            format!("files/{path}"),
            crate::media::hash_file(&extracted.join("files").join(path))?,
        ),
    ]
    .into())
}

/// Reuse the ordinary texture publisher; the shared system font is already cooked.
pub fn publish(extracted: &Path, output: &Path) -> Result<String> {
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
    images.write(output)?;
    let font_path = "fonts/dialogue.json";
    let font: BitmapFont = serde_json::from_slice(&fs::read(output.join(font_path))?)?;
    font.validate()?;
    let caption = crate::dol::text(&executable, 0x8017d87c)?;
    let choices = [
        crate::dol::text(&executable, 0x801f9c48)?,
        crate::dol::text(&executable, 0x801f9c54)?,
    ];
    ensure!(
        std::iter::once(&caption)
            .chain(&choices)
            .flat_map(|s| s.chars())
            .all(|c| font.glyphs.contains_key(&c)),
        "uncooked game-over glyph"
    );
    let mut files = BTreeMap::new();
    for (path, role) in [
        (&background.path[..], Role::Texture),
        (font_path, Role::Data),
        (&font.texture, Role::Texture),
    ] {
        let bytes = fs::read(output.join(path))?;
        files.insert(
            path.into(),
            File {
                sha256: crate::digest(&bytes),
                bytes: bytes.len() as u64,
                roles: [role].into(),
            },
        );
    }
    let art = Art {
        version: 1,
        source_sha256: crate::digest(&executable),
        background: UiTexture {
            path: background.path.clone(),
            width: background.width,
            height: background.height,
        },
        font: font_path.into(),
        caption,
        choices,
        files,
    };
    art.validate()?;
    crate::write_atomic(&output.join(PATH), &serde_json::to_vec_pretty(&art)?)?;
    Ok(PATH.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires both extracted discs; publishes the original UI images and font"]
    fn original_game_over_keeps_both_disc_labels_image_and_verified_dependencies() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("game-over"));
        let result = (|| -> Result<()> {
            let mut previous = None;
            for disc in [1, 2] {
                let extracted = local.join(format!("disc{disc}"));
                let directory = output.join(format!("disc{disc}"));
                crate::font::cook_embedded(&extracted, &directory)?;
                assert_eq!(publish(&extracted, &directory)?, PATH);
                let art: Art = serde_json::from_slice(&fs::read(directory.join(PATH))?)?;
                art.validate()?;
                let source = fs::read(extracted.join("sys/main.dol"))?;
                assert_eq!(art.caption, crate::dol::text(&source, 0x8017d87c)?);
                assert_eq!(art.choices, ["Load data", "Quit game"]);
                assert_eq!([art.background.width, art.background.height], [640, 480]);
                for (path, record) in &art.files {
                    let bytes = fs::read(directory.join(path))?;
                    assert_eq!(record.bytes, bytes.len() as u64);
                    assert_eq!(record.sha256, crate::digest(&bytes));
                }
                let semantics = (art.background.path, art.caption, art.choices);
                if let Some(previous) = &previous {
                    assert_eq!(&semantics, previous);
                }
                previous = Some(semantics);
                assert_eq!(inputs(&extracted)?.len(), 2);
            }
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(output);
        result?;
        cleanup?;
        Ok(())
    }
}

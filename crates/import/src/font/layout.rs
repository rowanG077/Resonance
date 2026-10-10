//! Dialogue declarations assembled from font and window textures.
use super::*;
use resonance_content::font::{DialogueArt, SelectionArt, UiTexture};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Recipe {
    system: String,
    default_style: usize,
    styles: [SelectionArt; 3],
}

struct Layout {
    art: DialogueArt,
    windows: crate::texture::Decoded,
}

impl Recipe {
    fn windows(&self, extracted: &Path) -> Result<(crate::texture::Decoded, String)> {
        let system =
            crate::all_assets::roles::declared_path(&extracted.join("files"), &self.system)?;
        let bytes = fs::read(extracted.join("files").join(system))?;
        Ok((crate::texture::decode_source(&bytes)?, digest(&bytes)))
    }

    fn assemble(&self, extracted: &Path) -> Result<Layout> {
        let style = self
            .styles
            .get(self.default_style)
            .context("unsupported default dialogue style")?;
        let (windows, source_sha256) = self.windows(extracted)?;
        let art = DialogueArt {
            version: 3,
            font: "fonts/dialogue.json".into(),
            textures: windows
                .catalogue
                .textures
                .iter()
                .map(|texture| {
                    texture
                        .as_ref()
                        .context("missing dialogue window texture")?
                        .image(0)
                })
                .collect::<Result<_>>()?,
            selection: style.clone(),
            source_sha256,
        };
        art.validate()?;
        Ok(Layout { art, windows })
    }

    pub(super) fn read(executable: &[u8]) -> Result<Self> {
        let style = crate::all_assets::ui_style::read(executable)?;
        let options = crate::all_assets::options_ui::read(executable)?;
        let styles = std::array::from_fn(|mode| SelectionArt {
            mode: mode as u8,
            color: options.themes[mode].selection,
            row_offsets: style.selection_rows,
        });
        Ok(Self {
            system: dol::text(executable, 0x8017a55c)?,
            default_style: usize::from(options.defaults.window),
            styles,
        })
    }
}

/// Field status sprites occupy the first image of the dialogue window bank.
pub(crate) fn system_texture(extracted: &Path, output: &Path) -> Result<UiTexture> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let (windows, _) = Recipe::read(&executable)?.windows(extracted)?;
    let texture = windows
        .catalogue
        .textures
        .first()
        .and_then(Option::as_ref)
        .context("missing system texture")?
        .image(0)?;
    windows.write(output)?;
    Ok(texture)
}

pub(crate) fn prepare(extracted: &Path, output: &Path) -> Result<PreparedDialogue> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let font = read_font(extracted, &executable)?;
    let Layout { art, windows } = Recipe::read(&executable)?.assemble(extracted)?;
    windows.write(output)?;
    let font = font.publish(output)?;
    write_atomic(
        &output.join("ui/dialogue.json"),
        &serde_json::to_vec_pretty(&art)?,
    )?;
    Ok(PreparedDialogue { font, art })
}

#[test]
#[ignore = "requires both extracted discs; compares published pixels with physical font and texture inputs"]
fn dialogue_publication_preserves_source_pixels_and_declared_resources() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let root = tempfile::tempdir()?;
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let output = root.path().join(format!("output-{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let prepared = prepare(&extracted, &output)?;
        assert!(!output.join("sources.json").exists());
        assert!(!output.join("data").exists());
        let directory = crate::font_directory::Directory::read(&executable)?;
        let source = fs::read(
            extracted
                .join("files")
                .join(crate::field_resources::resolve_path(
                    &extracted.join("files"),
                    &directory.startup,
                )?),
        )?;
        let atlas = crate::texture::pixels(&output.join(&prepared.font.texture))?;
        assert_eq!(
            atlas.dimensions(),
            (prepared.font.width, prepared.font.height)
        );
        // Native glyph decoding and halfwidth aliases have their own binary tests.
        // Here the delivered atlas must preserve the selected physical pixels.
        for character in ['A', '“', '漢'] {
            let code = directory.metrics.code(character)?;
            let glyph = &prepared.font.glyphs[&character];
            let pixels = decode_glyph(&source, code)?;
            let [x, y, width, height] = glyph.rect;
            assert_eq!((width, height), (24, 24));
            assert_eq!(glyph.advance, directory.metrics.advance(code));
            for (index, &pixel) in pixels.iter().enumerate() {
                assert_eq!(
                    atlas
                        .get_pixel(x + index as u32 % width, y + index as u32 / width)
                        .0,
                    rgb5a3(directory.palette[usize::from(pixel)]),
                    "disc {disc} glyph {character:?} pixel {index}",
                );
            }
        }
        let subtitles: MovieSubtitles =
            serde_json::from_slice(&fs::read(output.join("ui/story-subtitles.json"))?)?;
        subtitles.validate()?;
        assert!(
            subtitles
                .cues
                .iter()
                .flat_map(|cue| &cue.lines)
                .flat_map(|line| line.text.chars())
                .all(|c| prepared.font.glyphs.contains_key(&c))
        );

        let compare = |actual: &UiTexture, expected: &(u32, u32, Vec<u8>)| -> Result<()> {
            assert_eq!((actual.width, actual.height), (expected.0, expected.1));
            let pixels = crate::texture::pixels(&output.join(&actual.path))?;
            assert_eq!(pixels.dimensions(), (expected.0, expected.1));
            assert_eq!(pixels.as_raw(), &expected.2);
            Ok(())
        };
        let mut recipe = Recipe::read(&executable)?;
        let system =
            crate::all_assets::roles::declared_path(&extracted.join("files"), &recipe.system)?;
        let windows = crate::tpl::decode(&crate::compression::payload(fs::read(
            extracted.join("files").join(&system),
        )?)?)?;
        assert_eq!(prepared.art.textures.len(), windows.len());
        for (actual, expected) in prepared.art.textures.iter().zip(&windows) {
            compare(actual, expected)?;
        }
        assert_eq!(
            system_texture(&extracted, &output)?.path,
            prepared.art.textures[0].path
        );
        for mode in 0..recipe.styles.len() {
            recipe.default_style = mode;
            let layout = recipe.assemble(&extracted)?;
            assert_eq!(layout.art.selection.mode, mode as u8);
            assert_eq!(
                layout.art.selection.color,
                dol::slice(&executable, 0x8019ad10 + mode as u32 * 28 + 24, 4)?
            );
        }
        let renamed = root.path().join(format!("renamed-{disc}"));
        fs::create_dir_all(renamed.join("files/Art"))?;
        let renamed_path = renamed.join("files/Art/window.bin");
        fs::copy(extracted.join("files").join(system), &renamed_path)?;
        recipe.system = "art/window.bin".into();
        let renamed_art = recipe.assemble(&renamed)?.art;
        assert_eq!(renamed_art.source_sha256, prepared.art.source_sha256);
        assert_eq!(
            serde_json::to_value(&renamed_art.textures)?,
            serde_json::to_value(&prepared.art.textures)?
        );
        fs::remove_file(renamed_path)?;
        assert!(recipe.assemble(&renamed).is_err());
        assert!(!prepared.font.glyphs.contains_key(&'🙂'));
    }
    Ok(())
}

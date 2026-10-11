//! Original dialogue declarations assembled from typed font and texture resources.
use super::*;
use resonance_content::font::{DialogueArt, SelectionArt, UiTexture};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Recipe {
    system: String,
    default_style: usize,
    styles: [Style; 3],
}

#[derive(Serialize, Deserialize)]
struct Style {
    cursor_bank: String,
    cursor_image: usize,
    selection: SelectionArt,
    #[serde(skip)]
    cursor_source: (u32, usize),
}

struct Layout {
    art: DialogueArt,
    windows: crate::texture::Decoded,
    cursor: crate::texture::Decoded,
}

impl Recipe {
    fn windows(&self, extracted: &Path) -> Result<(crate::texture::Decoded, String)> {
        let system =
            crate::all_assets::roles::declared_path(&extracted.join("files"), &self.system)?;
        let bytes = fs::read(extracted.join("files").join(system))?;
        Ok((crate::texture::decode_source(&bytes)?, digest(&bytes)))
    }

    fn assemble(&self, extracted: &Path, executable: &[u8]) -> Result<Layout> {
        let style = self
            .styles
            .get(self.default_style)
            .context("unsupported default dialogue style")?;
        let (windows, source_sha256) = self.windows(extracted)?;
        let cursor = crate::texture::decode_source(dol::slice(
            executable,
            style.cursor_source.0,
            style.cursor_source.1,
        )?)?;
        let art = DialogueArt {
            version: 2,
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
            cursor: cursor
                .catalogue
                .textures
                .get(style.cursor_image)
                .and_then(Option::as_ref)
                .context("missing dialogue cursor")?
                .image(0)?,
            selection: style.selection.clone(),
            source_sha256,
        };
        art.validate()?;
        Ok(Layout {
            art,
            windows,
            cursor,
        })
    }

    pub(super) fn read(executable: &[u8]) -> Result<Self> {
        let style = crate::all_assets::ui_style::read(executable)?;
        let options = crate::all_assets::options_ui::read(executable)?;
        let cursor = &style.cursor;
        let scale = cursor.phase_scale.finite()?;
        let divisor = cursor.phase_divisor.finite()?;
        let styles = [
            (0x80249500, 0x280, 0),
            (0x80249280, 0x280, 0),
            (0x80236e80, 0x2500, 13),
        ]
        .into_iter()
        .enumerate()
        .map(|(mode, (bank, size, cursor_image))| {
            Ok(Style {
                cursor_bank: dol::texture_bank(executable, bank)?,
                cursor_image,
                cursor_source: (bank, size),
                selection: SelectionArt {
                    mode: mode as u8,
                    color: options.themes[mode].selection,
                    row_offsets: cursor.row_offsets,
                    bob_amplitude: cursor.amplitudes[usize::from(mode != 0)].finite()?,
                    bob_step: scale * if mode == 0 { 2. } else { 1. } / divisor,
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            system: dol::text(executable, 0x8017a55c)?,
            default_style: usize::from(options.defaults.window),
            styles: styles
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid dialogue style count"))?,
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
    let Layout {
        art,
        windows,
        cursor,
    } = Recipe::read(&executable)?.assemble(extracted, &executable)?;
    windows.write(output)?;
    cursor.write(output)?;
    let font = font.publish(output)?;
    write_atomic(
        &output.join("ui/dialogue.json"),
        &serde_json::to_vec_pretty(&art)?,
    )?;
    Ok(PreparedDialogue { font, art })
}

#[test]
#[cfg(unix)]
#[ignore = "requires original discs and frozen cooked baseline; no codecs or devices"]
fn original_dialogue_reads_sources_into_fresh_output_and_matches_frozen_pixels() -> Result<()> {
    use crate::cooked::Source;
    use std::os::unix::fs::symlink;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_TEST_BASELINE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| local.join("worktrees/generic-cooking/local/all-assets"));
    let root = crate::temporary_path(&std::env::temp_dir().join("dialogue-sources"));
    let result = (|| -> Result<()> {
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let output = root.join(format!("output-{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let source = Source::open(&baseline, disc, "sys/main.dol")?;
            let prepared = prepare(&extracted, &output)?;
            assert!(
                ['“', '漢']
                    .iter()
                    .all(|c| prepared.font.glyphs.contains_key(c))
            );
            assert!(!output.join("sources.json").exists());
            assert!(!output.join("data").exists());
            assert_eq!(prepared.font.texture, "fonts/dialogue.ktx2");
            let compare = |actual: &UiTexture, expected: &UiTexture| -> Result<()> {
                assert_eq!(
                    (actual.width, actual.height),
                    (expected.width, expected.height)
                );
                assert_eq!(
                    crate::texture::pixels(&output.join(&actual.path))?,
                    crate::texture::pixels(&baseline.join(&expected.path))?
                );
                Ok(())
            };
            let font_paths: Vec<_> = source
                .publications()
                .iter()
                .filter(|path| path.ends_with("/fonts"))
                .collect();
            ensure!(
                font_paths.len() == 1,
                "expected one frozen font publication"
            );
            let directory = font_paths[0];
            let mut expected_font: BitmapFont =
                serde_json::from_slice(&fs::read(baseline.join(directory).join("dialogue.json"))?)?;
            expected_font.validate()?;
            expected_font.texture = format!(
                "{directory}/{}",
                expected_font
                    .texture
                    .strip_prefix("fonts/")
                    .context("frozen font path")?
            );
            assert_eq!(
                crate::texture::pixels(&output.join(&prepared.font.texture))?,
                crate::texture::pixels(&baseline.join(&expected_font.texture))?
            );
            expected_font.texture = prepared.font.texture.clone();
            let mut original_glyphs = prepared.font.clone();
            original_glyphs
                .glyphs
                .retain(|c, _| !('\u{ff61}'..='\u{ff9f}').contains(c));
            assert_eq!(
                serde_json::to_value(&original_glyphs)?,
                serde_json::to_value(&expected_font)?
            );
            let expected_subtitles: MovieSubtitles = source.document("ui/story-subtitles.json")?;
            assert_eq!(
                fs::read(output.join("ui/story-subtitles.json"))?,
                serde_json::to_vec_pretty(&expected_subtitles)?
            );
            let mut recipe = Recipe::read(&executable)?;
            let published: Recipe = source.document("embedded/dialogue.json")?;
            assert_eq!(
                serde_json::to_value(&recipe)?,
                serde_json::to_value(&published)?
            );
            let system =
                crate::all_assets::roles::declared_path(&extracted.join("files"), &recipe.system)?;
            let windows = Source::open(&baseline, disc, &system)?.standalone_textures()?;
            assert_eq!(prepared.art.textures.len(), windows.len());
            for (actual, expected) in prepared.art.textures.iter().zip(&windows) {
                compare(actual, &expected.image(0)?)?;
            }
            assert_eq!(
                system_texture(&extracted, &output)?.path,
                prepared.art.textures[0].path
            );
            for mode in 0..recipe.styles.len() {
                recipe.default_style = mode;
                let layout = recipe.assemble(&extracted, &executable)?;
                layout.cursor.write(&output)?;
                let style = &recipe.styles[mode];
                let expected = source.published_textures(&style.cursor_bank)?;
                compare(&layout.art.cursor, &expected[style.cursor_image].image(0)?)?;
                assert_eq!(layout.art.selection.mode, mode as u8);
                assert_eq!(
                    layout.art.selection.color,
                    dol::slice(&executable, 0x8019ad10 + mode as u32 * 28 + 24, 4)?
                );
            }
            let renamed = root.join(format!("renamed-{disc}"));
            fs::create_dir_all(renamed.join("files/Art"))?;
            symlink(
                extracted.join("files").join(&system),
                renamed.join("files/Art/window.bin"),
            )?;
            recipe.system = "art/window.bin".into();
            let renamed_art = recipe.assemble(&renamed, &executable)?.art;
            assert_eq!(renamed_art.source_sha256, prepared.art.source_sha256);
            assert_eq!(
                serde_json::to_value(&renamed_art.textures)?,
                serde_json::to_value(&prepared.art.textures)?
            );
            fs::remove_file(renamed.join("files/Art/window.bin"))?;
            assert!(recipe.assemble(&renamed, &executable).is_err());
            assert!(!prepared.font.glyphs.contains_key(&'🙂'));
        }
        Ok(())
    })();
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    result
}

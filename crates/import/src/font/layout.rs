//! Dialogue layout recipes and bindings to the shared image library.
use super::*;
use crate::cooked::Source;
use resonance_content::font::{DialogueArt, MovieSubtitles, SelectionArt, UiTexture};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
}

impl Recipe {
    fn windows(
        &self,
        extracted: &Path,
        output: &Path,
        disc: u8,
    ) -> Result<(String, Vec<UiTexture>)> {
        let system =
            crate::all_assets::roles::declared_path(&extracted.join("files"), &self.system)?;
        let textures = Source::open(output, disc, &system)?
            .standalone_textures()?
            .into_iter()
            .map(|texture| texture.image(0))
            .collect::<Result<_>>()?;
        Ok((system, textures))
    }

    pub(super) fn read(executable: &[u8]) -> Result<Self> {
        let style = crate::all_assets::ui_style::read(executable)?;
        let options = crate::all_assets::options_ui::read(executable)?;
        let cursor = &style.cursor;
        let scale = cursor.phase_scale.finite()?;
        let divisor = cursor.phase_divisor.finite()?;
        let styles = [(0x80249500, 0), (0x80249280, 0), (0x80236e80, 13)]
            .into_iter()
            .enumerate()
            .map(|(mode, (bank, cursor_image))| {
                Ok(Style {
                    cursor_bank: dol::texture_bank(executable, bank)?,
                    cursor_image,
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
    let disc = crate::disc_number(extracted)?;
    let recipe: Recipe =
        Source::open(output, disc, "sys/main.dol")?.document("embedded/dialogue.json")?;
    recipe
        .windows(extracted, output, disc)?
        .1
        .into_iter()
        .next()
        .context("missing system texture")
}

pub(super) fn prepare(extracted: &Path, output: &Path, required: &BTreeSet<char>) -> Result<()> {
    let disc = crate::disc_number(extracted)?;
    let source = Source::open(output, disc, "sys/main.dol")?;
    let font = super::bind(&source)?;
    for character in required {
        ensure!(
            matches!(character, '\n' | '\r' | '\u{c}') || font.glyphs.contains_key(character),
            "source font cannot represent {character:?}"
        );
    }
    let recipe: Recipe = source.document("embedded/dialogue.json")?;
    let style = recipe
        .styles
        .get(recipe.default_style)
        .context("unsupported default dialogue style")?;
    let (system, textures) = recipe.windows(extracted, output, disc)?;
    let cursor = source
        .published_textures(&style.cursor_bank)?
        .get(style.cursor_image)
        .context("missing dialogue cursor")?
        .image(0)?;
    let art = DialogueArt {
        version: 2,
        font: "fonts/dialogue.json".into(),
        textures,
        cursor,
        selection: style.selection.clone(),
        source_sha256: crate::media::hash_file(&extracted.join("files").join(system))?,
    };
    art.validate()?;
    let subtitles: MovieSubtitles = source.document("ui/story-subtitles.json")?;
    subtitles.validate()?;
    for (path, bytes) in [
        ("fonts/dialogue.json", serde_json::to_vec_pretty(&font)?),
        ("ui/dialogue.json", serde_json::to_vec_pretty(&art)?),
        (
            "ui/story-subtitles.json",
            serde_json::to_vec_pretty(&subtitles)?,
        ),
    ] {
        write_atomic(&output.join(path), &bytes)?;
    }
    Ok(())
}

#[test]
#[cfg(unix)]
#[ignore = "requires both extracted discs and cook-all; no codecs or devices"]
fn original_dialogue_styles_bind_shared_pixels_and_renamed_sources() -> Result<()> {
    use std::os::unix::fs::symlink;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let cooked = local.join("all-assets");
    let root = crate::temporary_path(&std::env::temp_dir().join("dialogue-bindings"));
    let output = root.join("cooked");
    let extracted = root.join("extracted");
    let result = (|| -> Result<()> {
        fs::create_dir_all(extracted.join("files/Art"))?;
        fs::create_dir_all(extracted.join("sys"))?;
        fs::create_dir_all(&output)?;
        for directory in ["assets", "data"] {
            symlink(cooked.join(directory), output.join(directory))?;
        }
        let compare = |image: &resonance_content::font::UiTexture,
                       (w, h, bytes): (u32, u32, Vec<u8>)|
         -> Result<()> {
            assert_eq!((image.width, image.height), (w, h));
            assert_eq!(
                crate::texture::pixels(&output.join(&image.path))?.as_raw(),
                &bytes
            );
            Ok(())
        };
        for disc in [1, 2] {
            let original = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(original.join("sys/main.dol"))?;
            let mut recipe = Recipe::read(&executable)?;
            let source = Source::open(&cooked, disc, "sys/main.dol")?;
            let published: Recipe = source.document("embedded/dialogue.json")?;
            assert_eq!(
                serde_json::to_value(&recipe)?,
                serde_json::to_value(published)?
            );
            let system =
                crate::all_assets::roles::declared_path(&original.join("files"), &recipe.system)?;
            let images = crate::tpl::decode(&fs::read(original.join("files").join(&system))?)?;
            fs::copy(
                original.join("sys/boot.bin"),
                extracted.join("sys/boot.bin"),
            )?;
            symlink(
                original.join("files").join(&system),
                extracted.join("files/Art/window.bin"),
            )?;
            recipe.system = "art/window.bin".into();
            let mut sources: BTreeMap<String, Vec<String>> =
                serde_json::from_slice(&fs::read(cooked.join("sources.json"))?)?;
            let system_paths = sources
                .remove(&format!("disc{disc}/{system}"))
                .context("missing original system source")?;
            sources.insert(format!("disc{disc}/Art/window.bin"), system_paths);
            let paths = sources
                .get_mut(&format!("disc{disc}/sys/main.dol"))
                .unwrap();
            paths.retain(|path| !path.ends_with("/embedded/dialogue.json"));
            paths.push("fixture/embedded/dialogue.json".into());
            write_atomic(&output.join("sources.json"), &serde_json::to_vec(&sources)?)?;
            for (mode, (address, size, index)) in [
                (0x80249500, 0x280, 0),
                (0x80249280, 0x280, 0),
                (0x80236e80, 0x2500, 13),
            ]
            .into_iter()
            .enumerate()
            {
                recipe.default_style = mode;
                write_atomic(
                    &output.join("fixture/embedded/dialogue.json"),
                    &serde_json::to_vec(&recipe)?,
                )?;
                // The fixture has no executable, original font or original artwork name.
                prepare(&extracted, &output, &BTreeSet::from(['“', '漢', '\n']))?;
                let art: DialogueArt =
                    serde_json::from_slice(&fs::read(output.join("ui/dialogue.json"))?)?;
                assert_eq!(
                    system_texture(&extracted, &output)?.path,
                    art.textures[0].path
                );
                for (actual, original) in art.textures.iter().zip(&images) {
                    assert!(actual.path.starts_with("assets/"));
                    compare(actual, original.clone())?;
                }
                let cursor =
                    crate::tpl::decode(dol::slice(&executable, address, size)?)?.remove(index);
                assert!(art.cursor.path.starts_with("data/embedded/"));
                compare(&art.cursor, cursor)?;
                assert_eq!(art.selection.mode, mode as u8);
                assert_eq!(
                    art.selection.color,
                    dol::slice(&executable, 0x8019ad10 + mode as u32 * 28 + 24, 4)?
                );
                let mut font: BitmapFont =
                    serde_json::from_slice(&fs::read(output.join(&art.font))?)?;
                assert!(font.texture.starts_with("data/fonts/"));
                assert!(output.join(&font.texture).is_file());
                assert!(!output.join("fonts/dialogue.ktx2").exists());
                font.texture = "fonts/dialogue.ktx2".into();
                let (_, original) = source
                    .published_directory("fonts")?
                    .resolve("dialogue.json")?;
                assert_eq!(serde_json::to_vec_pretty(&font)?, original);
                let subtitles: MovieSubtitles = source.document("ui/story-subtitles.json")?;
                assert_eq!(
                    fs::read(output.join("ui/story-subtitles.json"))?,
                    serde_json::to_vec_pretty(&subtitles)?
                );
            }
            assert!(prepare(&extracted, &output, &BTreeSet::from(['🙂'])).is_err());
            fs::remove_file(extracted.join("files/Art/window.bin"))?;
            assert!(prepare(&extracted, &output, &BTreeSet::new()).is_err());
        }
        eprintln!(
            "Both discs: three dialogue styles, nine window images, font and subtitles bind without source decoding or image conversion"
        );
        Ok(())
    })();
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    result
}

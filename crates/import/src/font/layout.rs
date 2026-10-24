//! Original dialogue declarations assembled from typed font and texture resources.
use super::*;
use resonance_content::font::{DialogueArt, SelectionArt, UiTexture};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Recipe {
    system: String,
    default_style: usize,
    styles: [Style; 3],
}

#[derive(Serialize)]
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
    crate::texture::cook(
        font.font.width,
        font.font.height,
        &font.pixels,
        &output.join(&font.font.texture),
    )?;
    Ok(PreparedDialogue {
        font: font.font,
        art,
        subtitles: font.subtitles,
    })
}

#[test]
#[cfg(unix)]
#[ignore = "requires original discs and current RESONANCE_TEST_ASSETS; no codecs or devices"]
fn original_dialogue_reads_sources_into_fresh_output_and_matches_current_pixels() -> Result<()> {
    use std::os::unix::fs::symlink;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let library = resonance_content::library::Library::open(&baseline, &[])?;
    let mut snapshot = library.snapshot();
    let expected_art: DialogueArt = snapshot.json("ui/dialogue.json", 64 * 1024 * 1024)?;
    let expected_font: BitmapFont = snapshot.json(&expected_art.font, 64 * 1024 * 1024)?;
    let expected_subtitles: MovieSubtitles =
        snapshot.json("ui/story-subtitles.json", 64 * 1024 * 1024)?;
    for disc in [1, 2] {
        let temporary = tempfile::tempdir()?;
        let output = temporary.path();
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let prepared = prepare(&extracted, output)?;
        assert!(
            ['“', '漢']
                .iter()
                .all(|c| prepared.font.glyphs.contains_key(c))
        );
        assert!(!prepared.font.glyphs.contains_key(&'🙂'));
        assert!(!output.join("sources.json").exists());
        assert!(!output.join("data").exists());
        crate::named::equivalent_assets(
            &prepared.font,
            &expected_font,
            output,
            &baseline,
            &mut BTreeMap::new(),
        )?;
        // The font document is published separately; compare the rest of the UI
        // with the same reference, then compare every resolved texture's pixels.
        let mut native_art = prepared.art.clone();
        native_art.font = expected_art.font.clone();
        crate::named::equivalent_assets(
            &native_art,
            &expected_art,
            output,
            &baseline,
            &mut BTreeMap::new(),
        )?;
        assert_eq!(
            serde_json::to_value(&prepared.subtitles)?,
            serde_json::to_value(&expected_subtitles)?
        );
        assert_eq!(
            system_texture(&extracted, output)?.path,
            prepared.art.textures[0].path
        );

        let mut recipe = Recipe::read(&executable)?;
        for (mode, (address, length, image)) in [
            (0x80249500, 0x280, 0),
            (0x80249280, 0x280, 0),
            (0x80236e80, 0x2500, 13),
        ]
        .into_iter()
        .enumerate()
        {
            recipe.default_style = mode;
            let layout = recipe.assemble(&extracted, &executable)?;
            layout.cursor.write(output)?;
            let native = crate::texture::decode_source(dol::slice(&executable, address, length)?)?;
            let ([width, height], pixels) = native
                .base_images()
                .nth(image)
                .context("missing native cursor")??;
            let expected = image::RgbaImage::from_raw(width, height, pixels.to_vec())
                .context("invalid cursor pixels")?;
            assert_eq!(
                crate::texture::pixels(&output.join(&layout.art.cursor.path))?,
                expected
            );
            assert_eq!(layout.art.selection.mode, mode as u8);
            assert_eq!(
                layout.art.selection.color,
                dol::slice(&executable, 0x8019ad10 + mode as u32 * 28 + 24, 4)?
            );
        }
        let system =
            crate::all_assets::roles::declared_path(&extracted.join("files"), &recipe.system)?;
        let renamed = output.join("renamed");
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
    }
    Ok(())
}

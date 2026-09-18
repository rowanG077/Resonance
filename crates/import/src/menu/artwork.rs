//! Decode original menu images; only the sprite atlas is composed.
use super::recipe::{Bank, Recipe};
use crate::{
    dol,
    texture::{Decoded, Texture},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{TextureWrap, menu::MenuTexture, texture::Filter};
use std::{fs, path::Path};

pub(super) struct Library<'a> {
    pub(super) recipe: Recipe,
    extracted: &'a Path,
    root: &'a Path,
    executable: &'a [u8],
    pictures: crate::item::Pictures<'a>,
}

impl<'a> Library<'a> {
    pub(super) fn open(
        extracted: &'a Path,
        root: &'a Path,
        executable: &'a [u8],
        recipe: Recipe,
    ) -> Result<Self> {
        crate::disc_number(extracted)?;
        Ok(Self {
            recipe,
            extracted,
            root,
            executable,
            pictures: crate::item::Pictures::read(executable)?,
        })
    }

    fn bank_bytes(&self, bank: Bank) -> Result<&[u8]> {
        let bank = self
            .recipe
            .banks
            .get(&bank)
            .context("missing menu artwork bank")?;
        dol::slice(self.executable, bank.address, bank.length)
    }

    pub(super) fn bank(&self, bank: Bank, opaque_images: usize) -> Result<Vec<MenuTexture>> {
        self.publish(self.decode(bank)?, opaque_images)
    }

    pub(super) fn decode(&self, bank: Bank) -> Result<Decoded> {
        crate::texture::decode_source(self.bank_bytes(bank)?)
    }

    pub(super) fn publish(
        &self,
        decoded: Decoded,
        opaque_images: usize,
    ) -> Result<Vec<MenuTexture>> {
        decoded
            .write(self.root)?
            .textures
            .into_iter()
            .enumerate()
            .map(|(index, texture)| {
                bind(
                    texture.context("invalid menu texture")?,
                    index < opaque_images,
                )
            })
            .collect()
    }

    pub(super) fn images(&self, bank: Bank) -> Result<Vec<image::RgbaImage>> {
        crate::tpl::decode(self.bank_bytes(bank)?)?
            .into_iter()
            .map(image)
            .collect()
    }

    pub(super) fn item_picture(&self, id: u16) -> Result<image::RgbaImage> {
        let bytes = self.pictures.decode(self.pictures.offset(id)?)?;
        image(
            crate::tpl::decode(&bytes)?
                .into_iter()
                .next()
                .context("empty item picture bank")?,
        )
    }

    pub(super) fn glyph(&self, character: char) -> Result<image::RgbaImage> {
        image(crate::font::glyph_image(
            self.extracted,
            self.executable,
            u8::try_from(character as u32)?,
        )?)
    }

    pub(super) fn portraits(&self, extracted: &Path, root: &Path) -> Result<Vec<MenuTexture>> {
        let path = crate::all_assets::roles::declared_path(
            &extracted.join("files"),
            &self.recipe.portraits,
        )?;
        let bytes = fs::read(extracted.join("files").join(path))?;
        let (_, bytes) = crate::compression::cabinet(&bytes)?;
        crate::texture::decode_source(&bytes)?
            .write(root)?
            .textures
            .into_iter()
            .map(|texture| bind(texture.context("invalid status portrait")?, false))
            .collect()
    }
}

fn image((width, height, pixels): (u32, u32, Vec<u8>)) -> Result<image::RgbaImage> {
    image::RgbaImage::from_raw(width, height, pixels).context("invalid menu image dimensions")
}

fn bind(texture: Texture, opaque: bool) -> Result<MenuTexture> {
    ensure!(
        matches!(
            texture.sampler.wrap,
            [TextureWrap::Clamp, TextureWrap::Clamp] | [TextureWrap::Repeat, TextureWrap::Repeat]
        ) && matches!(texture.sampler.min_filter, Filter::Linear)
            && matches!(texture.sampler.mag_filter, Filter::Linear),
        "unsupported menu sampler"
    );
    let image = texture.image(0)?;
    Ok(MenuTexture {
        path: image.path,
        width: image.width,
        height: image.height,
        repeat: matches!(texture.sampler.wrap[0], TextureWrap::Repeat),
        opaque,
    })
}

#[test]
#[ignore = "requires both extracted discs; fresh output, no cooked inputs"]
fn whole_menu_images_preserve_source_pixels_samplers_and_opacity() -> Result<()> {
    use std::fs;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    for disc in [1, 2] {
        let temporary = tempfile::tempdir()?;
        let output = temporary.path();
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let library = Library::open(
            &extracted,
            output,
            &executable,
            super::data::read(&executable)?.artwork,
        )?;
        let mut count = 0;
        for (bank, address, size, opaque) in [
            (Bank::Frames, 0x80234260, 0x2c20, 1),
            (Bank::Symbols, 0x80249780, 0x16a0, 0),
            (Bank::WorldMaps, 0x8024e2a0, 0x1b0e0, 0),
            (Bank::Plain, 0x80231720, 0x240, 1),
            (Bank::Alternate, 0x80236e80, 0x2500, 1),
            (Bank::Patterns, 0x80231960, 0x2900, 5),
            (Bank::Cursor, 0x80249500, 0x280, 0),
        ] {
            let bytes = crate::dol::slice(&executable, address, size)?;
            let expected = crate::tpl::parse_tpl(bytes)?;
            let actual = library.bank(bank, opaque)?;
            assert_eq!(actual.len(), expected.len());
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(
                    [actual.width, actual.height],
                    [u32::from(expected.width), u32::from(expected.height)]
                );
                assert_eq!(actual.repeat, expected.wrap[0] == 1);
                assert_eq!(actual.opaque, index < opaque);
                assert!(actual.path.starts_with("textures/"));
                assert_eq!(
                    crate::texture::pixels(&output.join(&actual.path))?.as_raw(),
                    &crate::tpl::decode_texture(bytes, &expected)?,
                    "{bank:?} image {index}"
                );
            }
            // Only the two scroll arrows from the symbol bank are whole images.
            count += if address == 0x80249780 {
                2
            } else {
                actual.len()
            };
        }
        let portraits = library.portraits(&extracted, output)?;
        let path = crate::all_assets::roles::declared_path(
            &extracted.join("files"),
            &library.recipe.portraits,
        )?;
        let bytes = fs::read(extracted.join("files").join(path))?;
        let (_, bytes) = crate::compression::cabinet(&bytes)?;
        let original = crate::tpl::decode(&bytes)?;
        assert_eq!(portraits.len(), 9);
        for (portrait, (_, _, pixels)) in portraits.iter().zip(original) {
            assert_eq!([portrait.width, portrait.height], [328, 480]);
            assert!(!portrait.repeat && !portrait.opaque);
            assert!(portrait.path.starts_with("textures/"));
            assert_eq!(
                crate::texture::pixels(&output.join(&portrait.path))?.as_raw(),
                &pixels
            );
        }
        assert_eq!(count + portraits.len(), 50);
        assert!(!output.join("sources.json").exists() && !output.join("data").exists());
    }
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs and frozen prepared menu atlas; private output"]
fn original_menu_sprite_atlas_preserves_frozen_pixels_and_layout() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_COOKED")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| local.join("worktrees/generic-cooking/local/all-assets"));
    let expected: resonance_content::menu::MenuArt =
        serde_json::from_slice(&fs::read(baseline.join("ui/menu.json"))?)?;
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let temporary = tempfile::tempdir()?;
        let output = temporary.path();
        super::cook_art(
            &extracted,
            output,
            &executable,
            super::data::read(&executable)?.artwork,
        )?;
        let actual: resonance_content::menu::MenuArt =
            serde_json::from_slice(&fs::read(output.join("ui/menu.json"))?)?;
        crate::texture::compare_images(
            &output.join("ui/menu/party.ktx2"),
            &baseline.join("ui/menu/party.ktx2"),
        )?;
        assert_eq!(
            serde_json::to_value(&actual.sprites)?,
            serde_json::to_value(&expected.sprites)?
        );
        assert!(!output.join("sources.json").exists() && !output.join("data").exists());
    }
    Ok(())
}

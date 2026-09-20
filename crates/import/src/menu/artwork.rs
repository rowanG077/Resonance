//! Bind whole menu images to the physical library; only the sprite atlas is composed.
use super::recipe::{Bank, Recipe};
use crate::{cooked::Source, texture::Texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{TextureWrap, menu::MenuTexture, texture::Filter};
use std::{collections::BTreeMap, path::Path};

pub(super) struct Library<'a> {
    pub(super) recipe: Recipe,
    root: &'a Path,
    source: Source<'a>,
    disc: u8,
    item_pictures: BTreeMap<u16, String>,
}

impl<'a> Library<'a> {
    pub(super) fn open(extracted: &Path, root: &'a Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Tables {
            artwork: Recipe,
        }
        let disc = crate::disc_number(extracted)?;
        let source = Source::open(root, disc, "sys/main.dol")?;
        let tables: Tables = source.document("embedded/menu/tables.json")?;
        Ok(Self {
            recipe: tables.artwork,
            root,
            item_pictures: source.document("embedded/item-pictures.json")?,
            source,
            disc,
        })
    }

    fn textures(&self, bank: Bank) -> Result<Vec<Texture>> {
        self.source.published_textures(
            self.recipe
                .banks
                .get(&bank)
                .context("missing menu artwork bank")?,
        )
    }

    pub(super) fn bank(&self, bank: Bank, opaque_images: usize) -> Result<Vec<MenuTexture>> {
        self.textures(bank)?
            .into_iter()
            .enumerate()
            .map(|(index, texture)| bind(texture, index < opaque_images))
            .collect()
    }

    fn pixels(&self, texture: &Texture) -> Result<image::RgbaImage> {
        let image = texture.image(0)?;
        let pixels = crate::texture::pixels(&self.root.join(image.path))?;
        ensure!(
            pixels.dimensions() == (image.width, image.height),
            "menu image dimensions disagree"
        );
        Ok(pixels)
    }

    pub(super) fn images(&self, bank: Bank) -> Result<Vec<image::RgbaImage>> {
        self.textures(bank)?
            .iter()
            .map(|texture| self.pixels(texture))
            .collect()
    }

    pub(super) fn item_picture(&self, id: u16) -> Result<image::RgbaImage> {
        let suffix = self
            .item_pictures
            .get(&id)
            .context("missing item picture")?;
        self.pixels(
            self.source
                .published_textures(suffix)?
                .first()
                .context("empty item picture bank")?,
        )
    }

    pub(super) fn glyph(&self, character: char) -> Result<image::RgbaImage> {
        let font = crate::font::bind(&self.source)?;
        let atlas = crate::texture::pixels(&self.root.join(font.texture))?;
        ensure!(
            atlas.dimensions() == (font.width, font.height),
            "font image dimensions disagree"
        );
        let [x, y, width, height] = font
            .glyphs
            .get(&character)
            .context("missing menu glyph")?
            .rect;
        Ok(image::imageops::crop_imm(&atlas, x, y, width, height).to_image())
    }

    pub(super) fn portraits(&self, extracted: &Path, root: &Path) -> Result<Vec<MenuTexture>> {
        let path = crate::all_assets::roles::declared_path(
            &extracted.join("files"),
            &self.recipe.portraits,
        )?;
        let source = Source::open(root, self.disc, &path)?;
        let (directory, bytes) = source.resolve("cabinet.json")?;
        ensure!(
            directory
                == format!(
                    "assets/{}",
                    crate::media::hash_file(&extracted.join("files").join(path))?
                ),
            "status portrait source digest mismatch"
        );
        let members: Vec<String> = serde_json::from_slice(&bytes)?;
        let [member] = members.as_slice() else {
            anyhow::bail!("invalid status portrait archive");
        };
        resonance_content::validate_asset_path(member)?;
        crate::texture::bind(root, &format!("{directory}/{member}"))?
            .into_iter()
            .map(|texture| bind(texture, false))
            .collect()
    }
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
#[ignore = "requires both extracted discs and cook-all; no codecs or devices"]
fn whole_menu_images_bind_original_banks_without_texture_cooking() -> Result<()> {
    use std::fs;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let output = local.join("all-assets");
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let library = Library::open(&extracted, &output)?;
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
            let expected = crate::tpl::parse_tpl(crate::dol::slice(&executable, address, size)?)?;
            let actual = library.bank(bank, opaque)?;
            assert_eq!(actual.len(), expected.len());
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(
                    [actual.width, actual.height],
                    [u32::from(expected.width), u32::from(expected.height)]
                );
                assert_eq!(actual.repeat, expected.wrap[0] == 1);
                assert_eq!(actual.opaque, index < opaque);
                assert!(actual.path.starts_with("data/embedded/dol/"));
                assert!(output.join(&actual.path).is_file());
            }
            // Only the two scroll arrows from the symbol bank are whole images.
            count += if address == 0x80249780 {
                2
            } else {
                actual.len()
            };
        }
        let portraits = library.portraits(&extracted, &output)?;
        assert_eq!(portraits.len(), 9);
        for portrait in &portraits {
            assert_eq!([portrait.width, portrait.height], [328, 480]);
            assert!(!portrait.repeat && !portrait.opaque);
            assert!(portrait.path.starts_with("assets/"));
        }
        assert_eq!(count + portraits.len(), 50);
    }
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs and cook-all; compares pixels without conversion"]
fn original_menu_sprite_pixels_match_shared_library() -> Result<()> {
    use crate::{dol, tpl};
    use std::fs;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let output = local.join("all-assets");
    let mut count = 0;
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let library = Library::open(&extracted, &output)?;
        let compare = |actual: image::RgbaImage, (w, h, expected): (u32, u32, Vec<u8>)| {
            assert_eq!(actual.dimensions(), (w, h));
            assert_eq!(actual.as_raw(), &expected);
        };
        for (bank, address, size) in [
            (Bank::Portraits, 0x8023d3e0, 0x49a0),
            (Bank::Technique, 0x8024ba00, 0x28a0),
            (Bank::Strategy, 0x80241d80, 0x15c0),
            (Bank::Symbols, 0x80249780, 0x16a0),
            (Bank::Elements, 0x8024ae20, 0xbe0),
            (Bank::Numbers, 0x8026a1c0, 0xcc0),
            (Bank::Items, 0x80243340, 0x4a00),
            (Bank::ItemTabs, 0x80247d40, 0x1a40),
            (Bank::Buttons, 0x80239380, 0x4060),
            (Bank::Recipes, 0x80212060, 0x3860),
            (Bank::Conditions, 0x80269380, 0xe40),
        ] {
            let original = tpl::decode(dol::slice(&executable, address, size)?)?;
            let cooked = library.images(bank)?;
            assert_eq!(original.len(), cooked.len(), "{bank:?}");
            for (actual, expected) in cooked.into_iter().zip(original) {
                compare(actual, expected);
                count += 1;
            }
        }
        assert_eq!(library.item_pictures.len(), 545);
        for id in 0..545 {
            let offset = crate::read::u32(
                dol::slice(&executable, 0x802a11d8 + u32::from(id) * 4, 4)?,
                0,
            )?;
            let address = 0x8026bdfc + offset;
            let header = dol::slice(&executable, address, 9)?;
            let size = u32::from_le_bytes(header[1..5].try_into()?) as usize + 9;
            let decoded = crate::compression::decode(dol::slice(&executable, address, size)?)?;
            let original = tpl::decode(&decoded)?
                .into_iter()
                .next()
                .context("missing original item image")?;
            compare(library.item_picture(id)?, original);
            count += 1;
        }
        compare(
            library.glyph(library.recipe.equipped)?,
            crate::font::glyph_image(&extracted, &executable, library.recipe.equipped as u8)?,
        );
        count += 1;
    }
    eprintln!("{count} shared menu images exactly match original pixels across both discs");
    Ok(())
}

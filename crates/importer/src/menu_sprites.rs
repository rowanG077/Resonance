//! Native menu pictures directly into the one editable icon atlas.
use crate::{compression, dialogue_font, dol, rel::Rel, text};
use anyhow::{Context, Result, ensure};
use resonance_content::menu::{MenuSprites, MenuTexture, Sprite};
use std::{collections::BTreeMap, fs, path::Path};

const SIZE: usize = 1024;
const PICTURE_BANK: u32 = 0x8026_bdfc;
const PICTURE_TABLE: u32 = 0x802a_11d8;

/// Decode the base image directly for packing into a menu atlas.
pub(crate) fn decode_image(tpl: &[u8], index: usize) -> Result<(u32, u32, Vec<u8>)> {
    let image = crate::world_texture::tpl_base_image(tpl, index)?;
    Ok((image.width, image.height, image.rgba))
}

fn bank(exe: &[u8], address: u32, size: usize) -> Result<Vec<(u32, u32, Vec<u8>)>> {
    let tpl = dol::slice(exe, address, size)?;
    let count = dol::u32(tpl, 4)? as usize;
    (0..count).map(|index| decode_image(tpl, index)).collect()
}

struct Atlas {
    pixels: Vec<u8>,
    x: usize,
    y: usize,
    row_height: usize,
}

impl Atlas {
    fn new() -> Self {
        Self {
            pixels: vec![0; SIZE * SIZE * 4],
            x: 1,
            y: 1,
            row_height: 0,
        }
    }

    fn add(&mut self, (width, height, pixels): (u32, u32, Vec<u8>)) -> Result<[u32; 4]> {
        let (width, height) = (width as usize, height as usize);
        ensure!(
            pixels.len() == width * height * 4,
            "invalid menu sprite buffer"
        );
        if self.x + width + 1 > SIZE {
            self.x = 1;
            self.y += self.row_height + 2;
            self.row_height = 0;
        }
        ensure!(
            self.x + width < SIZE && self.y + height < SIZE,
            "menu sprite atlas is full"
        );
        for row in 0..height {
            let to = ((self.y + row) * SIZE + self.x) * 4;
            let from = row * width * 4;
            self.pixels[to..to + width * 4].copy_from_slice(&pixels[from..from + width * 4]);
        }
        let rect = [self.x as u32, self.y as u32, width as u32, height as u32];
        self.x += width + 2;
        self.row_height = self.row_height.max(height);
        Ok(rect)
    }

    fn add_all(
        &mut self,
        images: impl IntoIterator<Item = (u32, u32, Vec<u8>)>,
    ) -> Result<Vec<[u32; 4]>> {
        images.into_iter().map(|image| self.add(image)).collect()
    }
}

fn picture(exe: &[u8], id: usize) -> Result<(u32, u32, Vec<u8>)> {
    let bank = dol::slice(exe, PICTURE_BANK, 0x353dc)?;
    let table = dol::slice(exe, PICTURE_TABLE, 0x884)?;
    let offset = dol::u32(table, id * 4)? as usize;
    let header = bank
        .get(offset..offset + 9)
        .context("truncated item picture header")?;
    let length = u32::from_le_bytes(header[1..5].try_into()?) as usize + 9;
    let compressed = bank
        .get(offset..offset + length)
        .context("truncated item picture")?;
    decode_image(&compression::decode(compressed)?, 0)
}

fn overlay(base: &mut (u32, u32, Vec<u8>), top: (u32, u32, Vec<u8>)) -> Result<()> {
    ensure!(
        base.0 == top.0 && base.1 == top.1,
        "mismatched item picture overlay"
    );
    for (below, above) in base.2.chunks_exact_mut(4).zip(top.2.chunks_exact(4)) {
        let alpha = above[3];
        if alpha == 255 {
            below.copy_from_slice(above);
        } else if alpha != 0 {
            // Match image::Rgba::blend used by the reviewed importer.
            let fg = f32::from(alpha) / 255.0;
            let bg = f32::from(below[3]) / 255.0;
            let out = bg + fg - bg * fg;
            for channel in 0..3 {
                let background = f32::from(below[channel]) / 255.0 * bg;
                let foreground = f32::from(above[channel]) / 255.0 * fg;
                below[channel] = (255.0 * (foreground + background * (1.0 - fg)) / out) as u8;
            }
            below[3] = (255.0 * out) as u8;
        }
    }
    Ok(())
}

pub(crate) fn sprites(extracted: &Path, exe: &[u8]) -> Result<(MenuTexture, MenuSprites, Vec<u8>)> {
    let mut atlas = Atlas::new();
    let mut rects = BTreeMap::new();
    let hud = Rel::read(fs::read(extracted.join("disc1/files/US_r_Top2field.rel"))?)?;
    let hud = hud.tail(6, 0x2a980)?;
    let hud_count = dol::u32(hud, 4)? as usize;
    rects.insert(
        Sprite::WorldHud,
        atlas.add_all(
            (0..hud_count)
                .map(|i| decode_image(hud, i))
                .collect::<Result<Vec<_>>>()?,
        )?,
    );

    let portraits = bank(exe, 0x8023_d3e0, 0x49a0)?;
    rects.insert(
        Sprite::Portraits,
        atlas.add_all(portraits.iter().take(9).cloned())?,
    );
    for (kind, address, length) in [
        (Sprite::Technique, 0x8024_ba00, 0x28a0),
        (Sprite::StrategyCharacters, 0x8024_1d80, 0x15c0),
    ] {
        rects.insert(kind, atlas.add_all(bank(exe, address, length)?)?);
    }
    let symbols = bank(exe, 0x8024_9780, 0x16a0)?;
    rects.insert(
        Sprite::TechRanks,
        atlas.add_all(symbols.iter().take(2).cloned())?,
    );
    rects.insert(
        Sprite::Elements,
        atlas.add_all(bank(exe, 0x8024_ae20, 0xbe0)?)?,
    );
    rects.insert(
        Sprite::Numbers,
        vec![
            atlas.add(
                bank(exe, 0x8026_a1c0, 0xcc0)?
                    .into_iter()
                    .next()
                    .context("missing menu numbers")?,
            )?,
        ],
    );
    rects.insert(
        Sprite::Leader,
        vec![atlas.add(symbols.get(13).context("missing leader icon")?.clone())?],
    );
    for (kind, address, length) in [
        (Sprite::Items, 0x8024_3340, 0x4a00),
        (Sprite::ItemTabs, 0x8024_7d40, 0x1a40),
        (Sprite::Buttons, 0x8023_9380, 0x4060),
    ] {
        rects.insert(kind, atlas.add_all(bank(exe, address, length)?)?);
    }
    let mut item_rects = Vec::with_capacity(528);
    for id in 0..528 {
        let mut image = picture(exe, id)?;
        if let Some(extra) = match id {
            158 => Some(544),
            154 => Some(236),
            _ => None,
        } {
            overlay(&mut image, picture(exe, extra)?)?;
        }
        item_rects.push(atlas.add(image)?);
    }
    rects.insert(Sprite::ItemImages, item_rects);
    rects.insert(
        Sprite::Recipes,
        atlas.add_all(bank(exe, 0x8021_2060, 0x3860)?)?,
    );
    rects.insert(
        Sprite::CookingStars,
        atlas.add_all(symbols.iter().skip(11).take(2).cloned())?,
    );
    rects.insert(
        Sprite::ConditionIcons,
        atlas.add_all(bank(exe, 0x8026_9380, 0xe40)?)?,
    );
    let stone = portraits.into_iter().take(9).map(|(w, h, mut rgba)| {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[1] = pixel[0];
            pixel[2] = pixel[0];
        }
        (w, h, rgba)
    });
    rects.insert(Sprite::PetrifiedPortraits, atlas.add_all(stone)?);
    let mut equipment = symbols.into_iter().skip(4).take(7).collect::<Vec<_>>();
    let equipped = text::string(exe, 0x8035_d898)?;
    let mut characters = equipped.chars();
    let character = characters.next().context("missing equipped marker")?;
    ensure!(
        characters.next().is_none(),
        "equipped marker is not one character"
    );
    equipment.push(dialogue_font::menu_glyph(exe, extracted, character)?);
    rects.insert(Sprite::EquipmentMarkers, atlas.add_all(equipment)?);
    let bytes =
        resonance_asset_writer::ktx2::encode_rgba8(SIZE as u32, SIZE as u32, &atlas.pixels)?;
    Ok((
        MenuTexture {
            path: "ui/menu/icons.ktx2".into(),
            width: SIZE as u32,
            height: SIZE as u32,
            repeat: false,
            opaque: false,
        },
        MenuSprites {
            rects,
            ..Default::default()
        },
        bytes,
    ))
}

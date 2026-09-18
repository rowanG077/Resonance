//! Cook the default menu's frame slices, colors and labels from the executable.
use crate::{dol, tpl, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::menu::{MenuArt, MenuSprites, MenuTexture, WindowArt};
use std::{fs, path::Path};
mod data;
mod shops;
pub use shops::{ShopInventoryCheck, ShopInventoryValidation, ShopItemCheck, validate_shops};

/// Refresh the shared player menus and their field preparation manifests.
pub fn cook_all(extracted: &Path, output: &Path) -> Result<()> {
    let session = crate::session::cook(extracted, output)?;
    cook(
        extracted,
        &fs::read(extracted.join("sys/main.dol"))?,
        output,
    )?;
    crate::field::refresh_shared(output, &[session])
}

pub(crate) fn cook(extracted: &Path, executable: &[u8], output: &Path) -> Result<()> {
    let missing: Vec<_> = (0..resonance_content::monster::MONSTER_COUNT as u8)
        .filter(|id| !output.join(format!("monsters/{id:03}.json")).is_file())
        .collect();
    if !missing.is_empty() {
        crate::monsters::cook(extracted, output, &missing)?;
    }
    let missing: Vec<_> = (0..resonance_content::figurine::FIGURINE_COUNT as u16)
        .filter(|id| !output.join(format!("figurines/{id:03}.json")).is_file())
        .collect();
    if !missing.is_empty() {
        crate::figurines::cook(extracted, output, &missing)?;
    }
    data::cook(executable, output)?;
    let settings = dol::slice(executable, 0x80219cf8, 44)?;
    ensure!(settings[7] == 0x15, "unsupported default menu theme");
    let bank = dol::slice(executable, 0x80234260, 0x2c20)?;
    let mut textures = Vec::new();
    fs::create_dir_all(output.join("ui/menu"))?;
    let arrows = dol::slice(executable, 0x80249780, 0x16a0)?;
    let images = tpl::parse_tpl(bank)?
        .into_iter()
        .map(|image| (bank, image))
        .chain(
            tpl::parse_tpl(arrows)?
                .into_iter()
                .skip(2)
                .take(2)
                .map(|image| (arrows, image)),
        );
    for (i, (bank, source)) in images.enumerate() {
        ensure!(
            source.wrap[0] == source.wrap[1] && source.wrap[0] <= 1 && source.filter == [1, 1],
            "unsupported menu sampler"
        );
        let (width, height) = (u32::from(source.width), u32::from(source.height));
        let mut rgba = tpl::decode_texture(bank, &source)?;
        if i == 0 {
            // Window color supplies opacity independently of pattern intensity.
            for pixel in rgba.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        let path = format!("ui/menu/{i}.ktx2");
        crate::texture::cook(width, height, &rgba, &output.join(&path))?;
        textures.push(MenuTexture {
            path,
            width,
            height,
            repeat: source.wrap[0] == 1,
        });
    }
    let (atlas, sprites) = cook_sprites(extracted, executable, output)?;
    textures.push(atlas);
    use std::io::{Cursor, Read};
    let mut cabinet = cab::Cabinet::new(Cursor::new(fs::read(extracted.join("files/FIELD/s.z"))?))?;
    let names: Vec<_> = cabinet
        .folder_entries()
        .flat_map(|folder| folder.file_entries())
        .map(|entry| entry.name().to_owned())
        .collect();
    ensure!(names.len() == 1, "invalid status portrait archive");
    let mut portraits = Vec::new();
    cabinet.read_file(&names[0])?.read_to_end(&mut portraits)?;
    let portraits = tpl::decode(&portraits)?;
    ensure!(portraits.len() == 9, "invalid status portrait count");
    for (index, (width, height, rgba)) in portraits.into_iter().enumerate() {
        let path = format!("ui/menu/portrait-{index}.ktx2");
        crate::texture::cook(width, height, &rgba, &output.join(&path))?;
        textures.push(MenuTexture {
            path,
            width,
            height,
            repeat: false,
        });
    }
    let maps = tpl::decode(dol::slice(executable, 0x8024e2a0, 0x1b0e0)?)?;
    ensure!(maps.len() == 2, "invalid world-map artwork");
    for (index, (width, height, rgba)) in maps.into_iter().enumerate() {
        let path = format!("ui/menu/world-{index}.ktx2");
        crate::texture::cook(width, height, &rgba, &output.join(&path))?;
        textures.push(MenuTexture {
            path,
            width,
            height,
            repeat: false,
        });
    }
    let windows = cook_windows(executable, output, &mut textures)?;
    let art = MenuArt {
        version: MenuArt::VERSION,
        windows,
        textures,
        sprites,
        fill: settings[16..20].try_into()?,
        popup_fill: settings[28..32].try_into()?,
        shade: [settings[32..36].try_into()?, settings[36..40].try_into()?],
        palette: dol::slice(executable, 0x8026bdd0, 44)?
            .chunks_exact(4)
            .map(|c| c.try_into().unwrap())
            .collect(),
        labels: [
            ("tech", 0x8035d41c),
            ("unison", 0x8019a9d4),
            ("strategy", 0x8035d424),
            ("status", 0x8035d42c),
            ("synopsis", 0x801aaab8),
            ("items", 0x8035d434),
            ("ex_skill", 0x801aaac4),
            ("equip", 0x8035d43c),
            ("cooking", 0x8035d444),
            ("system", 0x8035d44c),
            ("save", 0x8035cc70),
            ("go_in", 0x8035af14),
            ("talk", 0x8035af1c),
            ("shop", 0x8035af24),
            ("examine", 0x8035af2c),
            ("go_out", 0x8035af8c),
            ("shop_buy", 0x8035d4c0),
            ("shop_sell", 0x8035d4c4),
            ("shop_equip", 0x8035d4cc),
            ("shop_exit", 0x8035d4d4),
            ("shop_status", 0x801aad34),
            ("shop_empty", 0x801aad44),
            ("shop_confirm", 0x801aad60),
            ("shop_yes", 0x8035d4dc),
            ("shop_no", 0x8035d4e0),
            ("shop_total", 0x8035d4e4),
            ("shop_gald", 0x8035d4ec),
            ("shop_select", 0x801aad6c),
            ("shop_add", 0x801aad78),
            ("shop_reduce", 0x801aad84),
            ("shop_ok", 0x8035d4f4),
            ("shop_info", 0x801aad90),
            ("shop_slash", 0x8035d4f8),
            ("shop_thrust", 0x8035d4fc),
            ("shop_defense", 0x8035d500),
            ("shop_accuracy", 0x8035d504),
            ("shop_evasion", 0x8035d508),
            ("shop_intelligence", 0x8035d50c),
            ("shop_luck", 0x8035d510),
            ("shop_attack", 0x8035d514),
            ("shop_cannot_equip", 0x801aada0),
            ("load", 0x8035cc68),
            ("customize", 0x8019a9a0),
            ("empty", 0x8035cc58),
            ("time", 0x8035d3e8),
            ("encounter", 0x801aaa28),
            ("combo", 0x8035d3f0),
            ("next", 0x8035d3f8),
            ("gald", 0x8035cc60),
            ("play_time", 0x8019bc50),
            ("encounters", 0x8019bc5c),
            ("max_combo", 0x8019bc68),
            ("yes", 0x8035cc20),
            ("no", 0x8035cc24),
            ("confirm_save_a", 0x8019bf08),
            ("confirm_save_b", 0x8019bf38),
            ("confirm_load_a", 0x8019bf68),
            ("confirm_load_b", 0x8019bf94),
            ("confirm_overwrite_a", 0x8019bfc0),
            ("confirm_overwrite_b", 0x8019bff0),
        ]
        .into_iter()
        .map(|(key, address)| Ok((key.into(), text(executable, address)?)))
        .collect::<Result<_>>()?,
    };
    art.validate()?;
    write_atomic(
        &output.join("ui/menu.json"),
        &serde_json::to_vec_pretty(&art)?,
    )
}

fn cook_windows(
    executable: &[u8],
    output: &Path,
    textures: &mut Vec<MenuTexture>,
) -> Result<[WindowArt; 3]> {
    let mut bank = |address, size, name: &str, opaque_images: usize| -> Result<Vec<usize>> {
        let bytes = dol::slice(executable, address, size)?;
        tpl::parse_tpl(bytes)?
            .into_iter()
            .enumerate()
            .map(|(i, source)| {
                let (width, height) = (u32::from(source.width), u32::from(source.height));
                let mut rgba = tpl::decode_texture(bytes, &source)?;
                if i < opaque_images {
                    for pixel in rgba.chunks_exact_mut(4) {
                        pixel[3] = 255;
                    }
                }
                let path = format!("ui/menu/{name}-{i}.ktx2");
                crate::texture::cook(width, height, &rgba, &output.join(&path))?;
                let index = textures.len();
                textures.push(MenuTexture {
                    path,
                    width,
                    height,
                    repeat: source.wrap[0] == 1,
                });
                Ok(index)
            })
            .collect()
    };
    let plain = bank(0x80231720, 0x240, "plain", 1)?;
    let alternate = bank(0x80236e80, 0x2500, "window-c", 1)?;
    let patterns = bank(0x80231960, 0x2900, "pattern", 5)?;
    let cursor = bank(0x80249500, 0x280, "cursor-a", 0)?;
    ensure!(
        plain.len() == 1 && alternate.len() == 14 && patterns.len() == 5 && cursor.len() == 1,
        "unsupported menu window banks"
    );
    let patterns = |last| std::array::from_fn(|i| if i == 5 { last } else { patterns[i] });
    let float = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    let cursor_step = float(0x8035d8a8)? / float(0x8035d8ac)?;
    let cursor_amplitude = float(0x8035d8b4)?;
    Ok([
        WindowArt {
            patterns: patterns(plain[0]),
            heading: None,
            cursor: Some(cursor[0]),
            cursor_motion: [float(0x8035d8b0)?, cursor_step * 2.],
            slices: None,
            outset: 4,
            flourish_outset: [0; 2],
            foot_outset: 0,
            left_joins: [0; 2],
            left_strip: [0; 2],
        },
        WindowArt {
            patterns: patterns(0),
            heading: Some(12),
            cursor: None,
            cursor_motion: [cursor_amplitude, cursor_step],
            slices: Some(std::array::from_fn(|i| i)),
            outset: 3,
            flourish_outset: [7, 17],
            foot_outset: 3,
            left_joins: [49, 37],
            left_strip: [55, 37],
        },
        WindowArt {
            patterns: patterns(alternate[0]),
            heading: Some(alternate[12]),
            cursor: Some(alternate[13]),
            cursor_motion: [cursor_amplitude, cursor_step],
            slices: Some(alternate[..12].try_into()?),
            outset: 4,
            flourish_outset: [8, 4],
            foot_outset: 8,
            left_joins: [48, 32],
            left_strip: [68, 24],
        },
    ])
}

fn text(executable: &[u8], address: u32) -> Result<String> {
    let bytes = dol::slice(executable, address, 128)?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .context("unterminated menu label")?;
    Ok(std::str::from_utf8(&bytes[..end])?.into())
}

fn cook_sprites(
    extracted: &Path,
    executable: &[u8],
    output: &Path,
) -> Result<(MenuTexture, MenuSprites)> {
    let mut atlas = image::RgbaImage::new(1024, 1024);
    let (mut x, mut y, mut row_height) = (1, 1, 0);
    let mut add = |(w, h, pixels): (u32, u32, Vec<u8>)| -> Result<[u32; 4]> {
        if x + w + 1 > atlas.width() {
            x = 1;
            y += row_height + 2;
            row_height = 0;
        }
        ensure!(
            x + w < atlas.width() && y + h < atlas.height(),
            "menu atlas is full"
        );
        let source = image::RgbaImage::from_raw(w, h, pixels).context("invalid menu sprite")?;
        image::imageops::replace(&mut atlas, &source, i64::from(x), i64::from(y));
        let rect = [x, y, w, h];
        x += w + 2;
        row_height = row_height.max(h);
        Ok(rect)
    };
    let bank =
        |address, size| -> Result<_> { Ok(tpl::decode(dol::slice(executable, address, size)?)?) };
    let portrait_images = bank(0x8023d3e0, 0x49a0)?;
    let portraits = portrait_images
        .iter()
        .take(9)
        .cloned()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing party portraits"))?;
    let technique = bank(0x8024ba00, 0x28a0)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("unexpected technique gauge atlas"))?;
    let strategy_characters = bank(0x80241d80, 0x15c0)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing strategy characters"))?;
    let symbols = bank(0x80249780, 0x16a0)?;
    let tech_ranks = symbols
        .iter()
        .take(2)
        .cloned()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let elements = bank(0x8024ae20, 0xbe0)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let numbers = add(bank(0x8026a1c0, 0xcc0)?
        .into_iter()
        .next()
        .context("missing number atlas")?)?;
    let leader = add(symbols.get(13).context("missing leader marker")?.clone())?;
    let items = bank(0x80243340, 0x4a00)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let item_tabs = bank(0x80247d40, 0x1a40)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let mut item_images = Vec::new();
    let buttons = bank(0x80239380, 0x4060)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    for id in 0..528u32 {
        let decode = |id| -> Result<image::RgbaImage> {
            let offset =
                u32::from_be_bytes(dol::slice(executable, 0x802a11d8 + id * 4, 4)?.try_into()?);
            let address = 0x8026bdfc + offset;
            let header = dol::slice(executable, address, 9)?;
            let size = u32::from_le_bytes(header[1..5].try_into()?) as usize + 9;
            let bytes = crate::compression::decode(dol::slice(executable, address, size)?)?;
            let (w, h, pixels) = tpl::decode(&bytes)?
                .into_iter()
                .next()
                .context("missing item picture")?;
            image::RgbaImage::from_raw(w, h, pixels).context("invalid item picture")
        };
        let mut picture = decode(id)?;
        if let Some(overlay) = match id {
            158 => Some(544),
            154 => Some(236),
            _ => None,
        } {
            image::imageops::overlay(&mut picture, &decode(overlay)?, 0, 0);
        }
        item_images.push(add((
            picture.width(),
            picture.height(),
            picture.into_raw(),
        ))?);
    }
    let recipes = bank(0x80212060, 0x3860)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("unexpected recipe artwork count"))?;
    let cooking_stars = symbols
        .iter()
        .skip(11)
        .take(2)
        .cloned()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let number_colors = dol::slice(executable, 0x801abf74, 0x90)?
        .chunks_exact(8)
        .map(|c| [c[..4].try_into().unwrap(), c[4..].try_into().unwrap()])
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let bar_colors = dol::slice(executable, 0x801abf44, 0x30)?
        .chunks_exact(16)
        .map(|c| std::array::from_fn(|i| c[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let names = (0..9)
        .map(|i| text(executable, 0x801f9fc8 + i * 0x118))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let condition_icons = bank(0x80269380, 0xe40)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("unexpected portrait condition icons"))?;
    let petrified_portraits = portrait_images
        .into_iter()
        .take(9)
        .map(|(w, h, mut pixels)| {
            // Stone portraits retain the original red-channel brightness and alpha.
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[1] = pixel[0];
                pixel[2] = pixel[0];
            }
            add((w, h, pixels))
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing petrified portraits"))?;
    let equipped = text(executable, 0x8035d898)?;
    ensure!(equipped.len() == 1, "equipped marker is not a single glyph");
    let equipment_markers = symbols
        .into_iter()
        .skip(4)
        .take(7)
        .chain([crate::font::glyph_image(
            extracted,
            executable,
            equipped.as_bytes()[0],
        )?])
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing equipment comparison markers"))?;
    let path = "ui/menu/party.ktx2";
    crate::texture::cook(
        atlas.width(),
        atlas.height(),
        atlas.as_raw(),
        &output.join(path),
    )?;
    Ok((
        MenuTexture {
            path: path.into(),
            width: atlas.width(),
            height: atlas.height(),
            repeat: false,
        },
        MenuSprites {
            strategy_characters,
            tech_ranks,
            elements,
            buttons,
            item_images,
            recipes,
            cooking_stars,
            item_tabs,
            items,
            portraits,
            petrified_portraits,
            condition_icons,
            equipment_markers,
            technique,
            numbers,
            leader,
            number_colors,
            bar_colors,
            names,
        },
    ))
}

//! Assemble player menus from the shared cooked library.
use crate::write_atomic;
use anyhow::{Context, Result, ensure};
use recipe::Bank;
use resonance_content::menu::{MenuArt, MenuSprites, MenuTexture, WindowArt};
use std::{fs, path::Path};
mod data;
pub(crate) use data::monster_catalogue;
pub(crate) use data::text::source as source_text;
#[cfg(test)]
pub(crate) use data::titles;
pub(crate) use data::world_map::cook as world_map;
pub(crate) use data::{BoneRule, FigurineModel, FigurineRow, figurine_catalogue};
mod artwork;
mod recipe;
mod shops;
pub use shops::{ShopInventoryCheck, ShopInventoryValidation, ShopItemCheck, validate_shops};

/// Refresh the shared player menus and their field preparation manifests.
pub fn cook_all(extracted: &Path, output: &Path) -> Result<()> {
    let session = crate::session::cook(extracted, output)?;
    cook(extracted, output)?;
    crate::field::refresh_shared(output, &[session])
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<()> {
    crate::monsters::cook(extracted, output, &[])?;
    crate::figurines::cook(extracted, output, &[])?;
    data::cook(output, crate::disc_number(extracted)?)?;
    cook_art(extracted, output)
}

/// Preserve menu tables independently of playable menu preparation.
pub(crate) fn cook_embedded(executable: &[u8], output: &Path) -> Result<()> {
    data::cook_source(executable, output)
}

fn cook_art(extracted: &Path, output: &Path) -> Result<()> {
    let library = artwork::Library::open(extracted, output)?;
    let mut textures = library.bank(Bank::Frames, 1)?;
    fs::create_dir_all(output.join("intermediate/ui/menu"))?;
    fs::create_dir_all(output.join("ui/menu"))?;
    textures.extend(library.bank(Bank::Symbols, 0)?.into_iter().skip(2).take(2));
    ensure!(textures.len() == 18, "invalid menu frame image count");
    let (atlas, sprites) = cook_sprites(&library, output)?;
    textures.push(atlas);
    let portraits = library.portraits(extracted, output)?;
    ensure!(portraits.len() == 9, "invalid status portrait count");
    textures.extend(portraits);
    let maps = library.bank(Bank::WorldMaps, 0)?;
    ensure!(maps.len() == 2, "invalid world-map artwork");
    textures.extend(maps.into_iter().map(|mut image| {
        image.repeat = false;
        image
    }));
    let windows = cook_windows(&library, &mut textures)?;
    let art = MenuArt {
        version: MenuArt::VERSION,
        windows,
        textures,
        sprites,
        fill: library.recipe.fill,
        popup_fill: library.recipe.popup_fill,
        shade: library.recipe.shade,
        palette: library.recipe.palette,
        labels: library.recipe.labels,
    };
    art.validate()?;
    write_atomic(
        &output.join("ui/menu.json"),
        &serde_json::to_vec_pretty(&art)?,
    )
}

fn cook_windows(
    library: &artwork::Library<'_>,
    textures: &mut Vec<MenuTexture>,
) -> Result<[WindowArt; 3]> {
    let mut bank = |address, opaque_images: usize| -> Result<Vec<usize>> {
        Ok(library
            .bank(address, opaque_images)?
            .into_iter()
            .map(|texture| {
                let index = textures.len();
                textures.push(texture);
                index
            })
            .collect())
    };
    let plain = bank(Bank::Plain, 1)?;
    let alternate = bank(Bank::Alternate, 1)?;
    let patterns = bank(Bank::Patterns, 5)?;
    let cursor = bank(Bank::Cursor, 0)?;
    ensure!(
        plain.len() == 1 && alternate.len() == 14 && patterns.len() == 5 && cursor.len() == 1,
        "unsupported menu window banks"
    );
    let patterns = |last| std::array::from_fn(|i| if i == 5 { last } else { patterns[i] });
    Ok([
        WindowArt {
            patterns: patterns(plain[0]),
            heading: None,
            cursor: Some(cursor[0]),
            cursor_motion: library.recipe.cursor_motion[0],
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
            cursor_motion: library.recipe.cursor_motion[1],
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
            cursor_motion: library.recipe.cursor_motion[1],
            slices: Some(alternate[..12].try_into()?),
            outset: 4,
            flourish_outset: [8, 4],
            foot_outset: 8,
            left_joins: [48, 32],
            left_strip: [68, 24],
        },
    ])
}

fn cook_sprites(
    library: &artwork::Library<'_>,
    output: &Path,
) -> Result<(MenuTexture, MenuSprites)> {
    let mut atlas = image::RgbaImage::new(1024, 1024);
    let (mut x, mut y, mut row_height) = (1, 1, 0);
    let mut add = |source: image::RgbaImage| -> Result<[u32; 4]> {
        let (w, h) = source.dimensions();
        if x + w + 1 > atlas.width() {
            x = 1;
            y += row_height + 2;
            row_height = 0;
        }
        ensure!(
            x + w < atlas.width() && y + h < atlas.height(),
            "menu atlas is full"
        );
        image::imageops::replace(&mut atlas, &source, i64::from(x), i64::from(y));
        let rect = [x, y, w, h];
        x += w + 2;
        row_height = row_height.max(h);
        Ok(rect)
    };
    let portrait_images = library.images(Bank::Portraits)?;
    let portraits = portrait_images
        .iter()
        .take(9)
        .cloned()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing party portraits"))?;
    let technique = library
        .images(Bank::Technique)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("unexpected technique gauge atlas"))?;
    let strategy_characters = library
        .images(Bank::Strategy)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing strategy characters"))?;
    let symbols = library.images(Bank::Symbols)?;
    let tech_ranks = symbols
        .iter()
        .take(2)
        .cloned()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let elements = library
        .images(Bank::Elements)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let numbers = add(library
        .images(Bank::Numbers)?
        .into_iter()
        .next()
        .context("missing number atlas")?)?;
    let leader = add(symbols.get(13).context("missing leader marker")?.clone())?;
    let items = library
        .images(Bank::Items)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let item_tabs = library
        .images(Bank::ItemTabs)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let mut item_images = Vec::new();
    let buttons = library
        .images(Bank::Buttons)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    for id in 0..528 {
        let mut picture = library.item_picture(id)?;
        if let Some(overlay) = match id {
            158 => Some(544),
            154 => Some(236),
            _ => None,
        } {
            image::imageops::overlay(&mut picture, &library.item_picture(overlay)?, 0, 0);
        }
        item_images.push(add(picture)?);
    }
    let recipes = library
        .images(Bank::Recipes)?
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
    let condition_icons = library
        .images(Bank::Conditions)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("unexpected portrait condition icons"))?;
    let petrified_portraits = portrait_images
        .into_iter()
        .take(9)
        .map(|mut pixels| {
            // Stone portraits retain the original red-channel brightness and alpha.
            for pixel in pixels.pixels_mut() {
                pixel[1] = pixel[0];
                pixel[2] = pixel[0];
            }
            add(pixels)
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing petrified portraits"))?;
    let equipment_markers = symbols
        .into_iter()
        .skip(4)
        .take(7)
        .chain([library.glyph(library.recipe.equipped)?])
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing equipment comparison markers"))?;
    let png = output.join("intermediate/ui/menu/party.png");
    atlas.save(&png)?;
    let path = "ui/menu/party.ktx2";
    crate::texture::cook_png(&png, &output.join(path))?;
    Ok((
        MenuTexture {
            path: path.into(),
            width: atlas.width(),
            height: atlas.height(),
            repeat: false,
            opaque: false,
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
            number_colors: library.recipe.number_colors,
            bar_colors: library.recipe.bar_colors,
            names: library.recipe.names.clone(),
        },
    ))
}

//! Prepare player menus from original tables and shared model readers.
use crate::write_atomic;
use anyhow::{Context, Result, ensure};
use recipe::Bank;
use resonance_content::menu::{MenuArt, MenuSprites, MenuTexture, Sprite, WindowArt};
use std::path::Path;
mod data;
pub use data::items;
pub(crate) use data::text::source as source_text;
pub(crate) fn world_map(
    source: &crate::all_assets::world_map::Catalogue,
    phases: &crate::field_catalogue::Phases,
    ui: &crate::all_assets::inventory_ui::Catalogue,
) -> anyhow::Result<resonance_content::menu_data::WorldMapData> {
    Ok(data::world_map::cook(source, phases, ui)?.0)
}
pub(crate) use data::{Inputs, Source, Tables, assemble};
mod artwork;
mod recipe;
mod shops;
pub use shops::{ShopInventoryCheck, ShopInventoryValidation, ShopItemCheck, validate_shops};

/// Republish menu artwork, rules and labels while reusing converted monster previews.
/// Every gameplay record is rebuilt from the source tables; no old rule schema is read.
pub fn publish_metadata(
    extracted: &Path,
    prepared: &Path,
    output: &Path,
) -> Result<(resonance_content::menu_data::MenuData, Vec<String>)> {
    let executable = std::fs::read(extracted.join("sys/main.dol"))?;
    let catalogues = crate::all_assets::Catalogues::read(&executable)?;
    let mut tables = catalogues.menu()?;
    tables.data.presentation.monsters = Some(crate::monsters::publish_metadata(
        extracted, prepared, output,
    )?);
    tables.data.validate()?;
    let path = "game/menu-data.json";
    write_atomic(
        &output.join(path),
        &serde_json::to_vec_pretty(&tables.data)?,
    )?;
    let mut paths = vec![
        path.into(),
        crate::session::cook(&executable, &catalogues.menu, output)?,
        crate::session::cook_text(&executable, &catalogues.menu, output)?,
        crate::arte::publish(&catalogues.menu.arte, output)?,
    ];
    let crate::font::PreparedDialogue { font, art } = crate::font::prepare(extracted, output)?;
    paths.extend([
        "ui/dialogue.json".into(),
        "ui/story-subtitles.json".into(),
        art.font,
        font.texture,
    ]);
    paths.extend(art.textures.into_iter().map(|texture| texture.path));
    paths.extend(cook_art(
        extracted,
        output,
        &executable,
        tables.artwork,
        tables.data.items.len(),
    )?);
    Ok((tables.data, paths))
}

pub(crate) fn cook(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
    catalogues: &crate::all_assets::Catalogues,
    battle_sources: &crate::source_assets::Sources,
    usual: &[u8],
) -> Result<()> {
    let mut tables = catalogues.menu()?;
    tables.data.presentation.monsters = Some(crate::monsters::prepare(
        extracted,
        output,
        battle_sources,
        usual,
        &catalogues.monsters,
        &catalogues.menu.inventory,
    )?);
    tables.figurines = crate::figurines::prepare(extracted, output, &catalogues.figurines)?;
    tables.data.validate()?;
    tables.manual.validate()?;
    tables.figurines.validate()?;
    tables.synopsis.validate()?;
    tables.customize.validate()?;
    tables.rename.validate()?;
    let sources: std::collections::BTreeMap<_, _> = resonance_script_content::MODULES
        .iter()
        .map(|&(module, source)| (module.to_owned(), source.to_owned()))
        .collect();
    let mut preparation = symphonia_script_tools::PreparationCache::default();
    for preview in tables
        .data
        .monsters()?
        .records
        .iter()
        .map(|row| &row.preview)
        .chain(tables.figurines.records.iter().map(|row| &row.preview))
    {
        if let Some(binding) = &preview.behavior {
            resonance_model_behavior::PreparedBehavior::prepare(
                &mut preparation,
                &sources,
                binding,
                preview,
            )?;
        }
    }
    crate::model_behavior::publish(output)?;
    write_atomic(
        &output.join("game/menu-data.json"),
        &serde_json::to_vec_pretty(&tables.data)?,
    )?;
    for (path, payload) in [
        (
            resonance_content::menu_data::MANUAL_PATH,
            serde_json::to_vec_pretty(&tables.manual)?,
        ),
        (
            resonance_content::menu_data::FIGURINES_PATH,
            serde_json::to_vec_pretty(&tables.figurines)?,
        ),
        (
            resonance_content::menu_data::SYNOPSIS_PATH,
            serde_json::to_vec_pretty(&tables.synopsis)?,
        ),
        (
            resonance_content::menu_data::CUSTOMIZE_PATH,
            serde_json::to_vec_pretty(&tables.customize)?,
        ),
        (
            resonance_content::menu_data::RENAME_PATH,
            serde_json::to_vec_pretty(&tables.rename)?,
        ),
    ] {
        write_atomic(&output.join(path), &payload)?;
    }
    cook_art(
        extracted,
        output,
        executable,
        tables.artwork,
        tables.data.items.len(),
    )?;
    Ok(())
}

fn cook_art(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
    recipe: recipe::Recipe,
    item_count: usize,
) -> Result<Vec<String>> {
    let library = artwork::Library::open(extracted, output, executable, recipe)?;
    let mut textures = library.bank(Bank::Frames, 1)?;
    let symbols = library.decode(Bank::Symbols)?;
    let symbol_images = symbols
        .base_images()
        .map(|image| {
            let ([width, height], pixels) = image?;
            image::RgbaImage::from_raw(width, height, pixels.to_vec())
                .context("invalid menu symbol")
        })
        .collect::<Result<Vec<_>>>()?;
    textures.extend(library.publish(symbols, 0)?.into_iter().skip(2).take(2));
    ensure!(textures.len() == 18, "invalid menu frame image count");
    let (atlas, sprites) = cook_sprites(&library, output, symbol_images)?;
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
        windows: windows.into_iter().enumerate().collect(),
        textures: textures.into_iter().enumerate().collect(),
        sprites,
        fill: library.recipe.fill,
        popup_fill: library.recipe.popup_fill,
        shade: library.recipe.shade,
        palette: library.recipe.palette,
        labels: library.recipe.labels,
    };
    art.validate(item_count)?;
    write_atomic(
        &output.join("ui/menu.json"),
        &serde_json::to_vec_pretty(&art)?,
    )?;
    Ok(std::iter::once("ui/menu.json".to_owned())
        .chain(art.textures.values().map(|texture| texture.path.clone()))
        .collect())
}

fn cook_windows(
    library: &artwork::Library<'_>,
    textures: &mut Vec<MenuTexture>,
) -> Result<[WindowArt; 3]> {
    let mut bank = |address, opaque_images: usize| -> Result<Vec<usize>> {
        Ok(library
            .bank(address, opaque_images)?
            .into_iter()
            .take(if address == Bank::Alternate {
                13
            } else {
                usize::MAX
            })
            .map(|texture| {
                let index = textures.len();
                textures.push(texture);
                index
            })
            .collect())
    };
    let plain = bank(Bank::Plain, 1)?;
    // The last alternate-window image is an unused selection sprite.
    let alternate = bank(Bank::Alternate, 1)?;
    let patterns = bank(Bank::Patterns, 5)?;
    ensure!(
        plain.len() == 1 && alternate.len() == 13 && patterns.len() == 5,
        "unsupported menu window banks"
    );
    let patterns = |last| std::array::from_fn(|i| if i == 5 { last } else { patterns[i] });
    Ok([
        WindowArt {
            patterns: patterns(plain[0]),
            heading: None,
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
    symbols: Vec<image::RgbaImage>,
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
        .collect::<Result<Vec<_>>>()?;
    let technique = library
        .images(Bank::Technique)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let strategy_characters = library
        .images(Bank::Strategy)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
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
        .collect::<Result<Vec<_>>>()?;
    let cooking_stars = symbols
        .iter()
        .skip(11)
        .take(2)
        .cloned()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
    let condition_icons = library
        .images(Bank::Conditions)?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
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
        .collect::<Result<Vec<_>>>()?;
    let equipment_markers = symbols
        .into_iter()
        .skip(4)
        .take(7)
        .chain([library.glyph(library.recipe.equipped)?])
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
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
            opaque: false,
        },
        MenuSprites {
            rects: [
                (Sprite::StrategyCharacters, strategy_characters),
                (Sprite::TechRanks, tech_ranks),
                (Sprite::Elements, elements),
                (Sprite::Buttons, buttons),
                (Sprite::ItemImages, item_images),
                (Sprite::Recipes, recipes),
                (Sprite::CookingStars, cooking_stars),
                (Sprite::ItemTabs, item_tabs),
                (Sprite::Items, items),
                (Sprite::Portraits, portraits),
                (Sprite::PetrifiedPortraits, petrified_portraits),
                (Sprite::ConditionIcons, condition_icons),
                (Sprite::EquipmentMarkers, equipment_markers),
                (Sprite::Technique, technique),
                (Sprite::Numbers, vec![numbers]),
                (Sprite::Leader, vec![leader]),
            ]
            .into(),
            number_colors: library.recipe.number_colors.to_vec(),
            bar_colors: library.recipe.bar_colors.to_vec(),
            names: library.recipe.names.to_vec(),
        },
    ))
}

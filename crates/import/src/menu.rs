//! Prepare player menus from original tables and shared model readers.
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

pub(crate) struct Publication {
    pub paths: Vec<String>,
    pub enemies: Vec<String>,
}

pub(crate) fn publish(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
    catalogues: &crate::all_assets::Catalogues,
    battle_sources: &crate::source_assets::Sources,
    usual: &[u8],
    shared_effects: &crate::battle_effect::SharedPalettes,
) -> Result<Publication> {
    let mut tables = catalogues.menu()?;
    let monsters = crate::monsters::prepare(
        extracted,
        output,
        battle_sources,
        usual,
        &catalogues.monsters,
        &catalogues.menu.inventory,
        shared_effects,
    )?;
    let enemy_paths =
        crate::named::Publisher::new(output, output)?.battle_enemies(monsters.enemies)?;
    tables.data.presentation.monsters = Some(monsters.book);
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
    let mut publisher = crate::named::Publisher::new(output, output)?;
    let mut paths = publisher.menu_tables(&tables)?;
    let art = prepare_art(
        extracted,
        output,
        executable,
        tables.artwork,
        tables.data.items.len(),
    )?;
    paths.push(publisher.menu_art(art)?);
    Ok(Publication {
        paths,
        enemies: enemy_paths,
    })
}

fn prepare_art(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
    recipe: recipe::Recipe,
    item_count: usize,
) -> Result<MenuArt> {
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
    Ok(art)
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
    let world_hud = library
        .world_hud()?
        .into_iter()
        .map(&mut add)
        .collect::<Result<Vec<_>>>()?;
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
                (Sprite::WorldHud, world_hud),
            ]
            .into(),
            number_colors: library.recipe.number_colors.to_vec(),
            bar_colors: library.recipe.bar_colors.to_vec(),
            names: library.recipe.names.to_vec(),
        },
    ))
}

//! Native images packed into the same menu drawing roles as the GC importer.
use crate::{menu_labels, output, package::Package, pe, source::Source, text, ui_images};
use anyhow::{Context, Result, ensure};
use image::{
    RgbaImage,
    imageops::{self, FilterType},
};
use resonance_content::menu::{MenuArt, MenuSprites, MenuTexture, Sprite, WindowArt};
use std::{collections::BTreeMap, path::Path};

fn image_bank(source: &Source, name: &str) -> Result<Vec<RgbaImage>> {
    let bytes = source.require(name)?;
    let package = Package::parse(&bytes)?;
    (0..package.len())
        .map(|i| ui_images::read(source, package.reference(i)?.context("missing menu image")?))
        .collect()
}

fn sized(pixels: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if pixels.dimensions() == (width, height) {
        pixels.clone()
    } else {
        imageops::resize(pixels, width, height, FilterType::Triangle)
    }
}
fn half(pixels: &RgbaImage) -> RgbaImage {
    sized(pixels, pixels.width() / 2, pixels.height() / 2)
}
fn write_image(
    output: &Path,
    name: &str,
    pixels: &RgbaImage,
    opaque: bool,
    repeat: bool,
) -> Result<MenuTexture> {
    let path = format!("{name}.ktx2");
    ui_images::write(output, &path, pixels)?;
    Ok(MenuTexture {
        path,
        width: pixels.width(),
        height: pixels.height(),
        opaque,
        repeat,
    })
}

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    resonance_importer::cook_input_prompts(output)?;
    let executable = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = text::Bank::parse(&bytes)?;
    let slot = source.language.slot();
    let mut textures = BTreeMap::new();
    let styles = [
        image_bank(source, "MENUSTYLE1TEX.PAC")?,
        image_bank(source, "MENUSTYLE2TEX.PAC")?,
    ];
    let edges = [
        "fill",
        "top",
        "bottom",
        "left",
        "right",
        "top-left",
        "top-right",
        "bottom-left",
        "bottom-right",
        "bottom-right-flourish",
        "top-left-flourish",
        "bottom-left-flourish",
        "heading",
        "cursor",
    ];
    for (style, pixels) in styles.iter().enumerate() {
        ensure!(pixels.len() == 14, "unsupported menu frame bank");
        for (i, pixels) in pixels.iter().enumerate() {
            // The shared ornate layout uses its heading marker as the cursor.
            if style == 0 && i == 13 {
                continue;
            }
            let id = if style == 0 { i } else { i + 31 };
            let family = if style == 0 { "ornate" } else { "gold" };
            textures.insert(
                id,
                write_image(
                    output,
                    &format!("ui/menu/windows/{family}-{}", edges[i]),
                    &half(pixels),
                    i == 0,
                    i == 0,
                )?,
            );
        }
    }
    // Use the same pattern images already owned by the dialogue document.
    let system = source.require("SYSTEM.PAC")?;
    let system = Package::parse(&system)?;
    for (index, name) in [
        "cracked-stone",
        "rough-stone",
        "fine-grain",
        "speckled",
        "small-stones",
    ]
    .into_iter()
    .enumerate()
    {
        let pixels = ui_images::read(
            source,
            system
                .reference(index + 2)?
                .context("missing window pattern")?,
        )?;
        textures.insert(
            index + 45,
            MenuTexture {
                path: format!("ui/windows/{name}.ktx2"),
                width: pixels.width(),
                height: pixels.height(),
                opaque: true,
                repeat: true,
            },
        );
    }
    let white = ui_images::read(source, system.reference(8)?.context("missing plain fill")?)?;
    textures.insert(
        30,
        MenuTexture {
            path: "ui/windows/white-fill.ktx2".into(),
            width: white.width(),
            height: white.height(),
            opaque: true,
            repeat: true,
        },
    );
    let hand = ui_images::read(source, "CURSORTOETEX_PIC000.TOTEXB_D")?;
    textures.insert(
        50,
        write_image(output, "ui/cursors/hand", &half(&hand), false, false)?,
    );
    let symbols = image_bank(source, "OTHERTEX.PAC")?;
    for (index, name) in [(2, "scroll-down"), (3, "scroll-up")] {
        textures.insert(
            index + 14,
            write_image(
                output,
                &format!("ui/menu/{name}"),
                &symbols[index],
                false,
                false,
            )?,
        );
    }
    let portraits = image_bank(source, "ST.PAC")?;
    ensure!(portraits.len() == 9, "missing status portraits");
    for (index, pixels) in portraits.iter().enumerate() {
        let scale = (328. / pixels.width() as f32).min(480. / pixels.height() as f32);
        let fit = sized(
            pixels,
            (pixels.width() as f32 * scale) as u32,
            (pixels.height() as f32 * scale) as u32,
        );
        let mut canvas = RgbaImage::new(328, 480);
        imageops::replace(
            &mut canvas,
            &fit,
            i64::from((328 - fit.width()) / 2),
            i64::from(480 - fit.height()),
        );
        textures.insert(
            index + 19,
            write_image(
                output,
                &format!(
                    "ui/menu/{}-portrait",
                    [
                        "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal",
                        "kratos"
                    ][index]
                ),
                &canvas,
                false,
                false,
            )?,
        );
    }
    for (index, pixels) in image_bank(source, "WORLDMAPTEX.PAC")?.iter().enumerate() {
        textures.insert(
            index + 28,
            write_image(
                output,
                &format!("ui/menu/{}-map", ["sylvarant", "tethealla"][index]),
                pixels,
                false,
                false,
            )?,
        );
    }
    let mut groups = BTreeMap::<Sprite, Vec<RgbaImage>>::new();
    for (role, package, dimensions) in [
        (Sprite::Portraits, "FACETEX.PAC", Some((64, 64))),
        (
            Sprite::StrategyCharacters,
            "MINIFACETEX.PAC",
            Some((32, 32)),
        ),
        (Sprite::Recipes, "COOKTEX.PAC", Some((32, 32))),
        (Sprite::ItemTabs, "SORTICONTEX.PAC", Some((24, 24))),
        (Sprite::Items, "ITEMKINDICONTEX.PAC", Some((24, 24))),
        (Sprite::ConditionIcons, "STATUSMARKTEX.PAC", Some((24, 24))),
        (Sprite::Elements, "ATTRIBUTETEX.PAC", None),
        (Sprite::Technique, "TSGAGETEX.PAC", None),
        (Sprite::WorldHud, "NAVITEX.PAC", None),
    ] {
        groups.insert(
            role,
            image_bank(source, package)?
                .into_iter()
                .map(|pixels| {
                    if let Some((w, h)) = dimensions {
                        sized(&pixels, w, h)
                    } else if role == Sprite::WorldHud {
                        half(&pixels)
                    } else {
                        pixels
                    }
                })
                .collect(),
        );
    }
    let buttons = image_bank(source, "BUTTONTEX.PAC")?;
    for (id, source, name) in [
        (13, 35, "panel-left"),
        (14, 37, "panel-right"),
        (15, 36, "panel-middle"),
    ] {
        textures.insert(
            id,
            write_image(
                output,
                &format!("ui/menu/{name}"),
                &half(&buttons[source]),
                false,
                false,
            )?,
        );
    }
    // Reorder the native device atlas to the shared script glyph IDs. Active
    // button conversion is 0x52e4e0; adjacent frames retain the common blink.
    // The three caption-strip pieces occupy a separate range in the port.
    groups.insert(
        Sprite::Buttons,
        [
            0, 1, 2, 3, 4, 11, 12, 17, 18, 15, 16, 13, 14, 19, 20, 25, 26, 31, 32, 35, 36, 37, 29,
            30, 24, 28, 22, 10, 5, 6, 7, 8,
        ]
        .into_iter()
        .map(|id| half(&buttons[id]))
        .collect(),
    );
    groups.insert(
        Sprite::PetrifiedPortraits,
        groups[&Sprite::Portraits]
            .iter()
            .cloned()
            .map(|mut pixels| {
                for pixel in pixels.pixels_mut() {
                    pixel[1] = pixel[0];
                    pixel[2] = pixel[0];
                }
                pixels
            })
            .collect(),
    );
    groups.insert(Sprite::TechRanks, symbols[..2].to_vec());
    groups.insert(
        Sprite::CookingStars,
        symbols[11..13].iter().map(|p| sized(p, 24, 24)).collect(),
    );
    groups.insert(Sprite::Leader, vec![symbols[13].clone()]);
    let mut equipment = symbols[4..11].to_vec();
    let latin = ui_images::read(source, "PAL_FONTB0.TOTEXB_D")?;
    let latin = sized(&latin, 384, 864);
    let ascii = executable.slice(0x8ad8a8, 192)?;
    let at = usize::from(b'E' - 32) * 2;
    let code = u16::from_be_bytes(ascii[at..at + 2].try_into().unwrap());
    let index = u32::from((code >> 8) - 0x81) * 192 + u32::from((code & 255) - 0x40);
    equipment.push(imageops::crop_imm(&latin, index % 16 * 24, index / 16 * 24, 24, 24).to_image());
    groups.insert(Sprite::EquipmentMarkers, equipment);
    let numbers = ui_images::read(source, "NUMERALFONTTEX_PIC000.TOTEXB_D")?;
    groups.insert(Sprite::Numbers, vec![sized(&numbers, 256, 24)]);
    let mut items = Vec::new();
    for id in 0..531 {
        let mut pixels =
            ui_images::read(source, &format!("MN_ITEMTEXDAT_PIC{id:03}_PIC000.TOTEXB_D"))?;
        if let Some(overlay) = match id {
            158 => Some(544),
            154 => Some(236),
            _ => None,
        } {
            imageops::overlay(
                &mut pixels,
                &ui_images::read(
                    source,
                    &format!("MN_ITEMTEXDAT_PIC{overlay:03}_PIC000.TOTEXB_D"),
                )?,
                0,
                0,
            );
        }
        items.push(sized(&pixels, 32, 32));
    }
    groups.insert(Sprite::ItemImages, items);
    let mut atlas = RgbaImage::new(2048, 2048);
    let (mut x, mut y, mut row_height) = (1, 1, 0);
    let mut rects = BTreeMap::new();
    for (role, images) in groups {
        let mut entries = Vec::new();
        for pixels in images {
            let (w, h) = pixels.dimensions();
            if x + w + 1 > atlas.width() {
                x = 1;
                y += row_height + 2;
                row_height = 0;
            }
            ensure!(
                x + w < atlas.width() && y + h < atlas.height(),
                "menu atlas full"
            );
            imageops::replace(&mut atlas, &pixels, i64::from(x), i64::from(y));
            entries.push([x, y, w, h]);
            x += w + 2;
            row_height = row_height.max(h);
        }
        rects.insert(role, entries);
    }
    let atlas = imageops::crop_imm(&atlas, 0, 0, atlas.width(), y + row_height + 1).to_image();
    textures.insert(
        18,
        write_image(output, "ui/menu/icons", &atlas, false, false)?,
    );
    let names = menu_labels::character_names(&executable, &bank, slot)?;
    let labels = menu_labels::read(source, &executable, &bank, slot)?;
    // The common menu deliberately retains the GameCube palette, gradients and
    // 640x480 window metrics (GQSEAF main.dol UI style tables).
    let palette = vec![
        [0, 0, 0, 255],
        [128, 128, 255, 255],
        [255, 128, 128, 255],
        [255, 128, 255, 255],
        [128, 255, 128, 255],
        [128, 255, 255, 255],
        [255, 255, 128, 255],
        [128, 128, 128, 255],
        [255, 192, 128, 255],
        [255, 255, 255, 255],
        [224, 255, 224, 255],
    ];
    let number_colors = palette
        .iter()
        .copied()
        .chain([[192, 192, 192, 255]; 3])
        .chain([
            [192, 144, 144, 255],
            [192, 120, 120, 255],
            [96, 192, 96, 255],
            [0, 0, 0, 128],
        ])
        .map(|color| [color; 2])
        .collect();
    let patterns = |fill| [45, 46, 47, 48, 49, fill];
    let windows = [
        WindowArt {
            patterns: patterns(30),
            heading: None,
            cursor: Some(50),
            cursor_motion: [2., 0.2],
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
            cursor_motion: [4., 0.1],
            slices: Some(std::array::from_fn(|i| i)),
            outset: 3,
            flourish_outset: [7, 17],
            foot_outset: 3,
            left_joins: [49, 37],
            left_strip: [55, 37],
        },
        WindowArt {
            patterns: patterns(31),
            heading: Some(43),
            cursor: Some(44),
            cursor_motion: [4., 0.1],
            slices: Some(std::array::from_fn(|i| i + 31)),
            outset: 4,
            flourish_outset: [8, 4],
            foot_outset: 8,
            left_joins: [48, 32],
            left_strip: [68, 24],
        },
    ]
    .into_iter()
    .enumerate()
    .collect();
    let colors = resonance_content::menu_data::CustomizeSettings::default().colors;
    let mut art = MenuArt {
        textures,
        windows,
        fill: colors.menu,
        popup_fill: colors.popup,
        shade: [colors.shade_top, colors.shade_bottom],
        palette,
        labels,
        sprites: {
            let mut sprites = MenuSprites::default();
            sprites.rects = rects;
            sprites.names = names.clone();
            sprites.number_colors = number_colors;
            sprites.bar_colors = vec![
                [
                    [192, 48, 48, 255],
                    [192, 192, 48, 255],
                    [192, 192, 48, 255],
                    [192, 48, 48, 255],
                ],
                [
                    [48, 48, 192, 255],
                    [144, 144, 192, 255],
                    [144, 144, 192, 255],
                    [48, 48, 192, 255],
                ],
                [[0, 0, 0, 160]; 4],
            ];
            sprites
        },
    };
    let items: resonance_content::items::Document =
        serde_json::from_slice(&std::fs::read(output.join(resonance_content::items::PATH))?)?;
    art.sprites
        .name_catalogue(Sprite::ItemImages, &items.mappings)?;
    let recipes = serde_json::from_str(include_str!("../../importer/src/recipe-names.json"))?;
    art.sprites.name_catalogue(Sprite::Recipes, &recipes)?;
    art.sprites.use_character_defaults(&names)?;
    art.validate_layout(531)?;
    output::json(&output.join("ui/menu.json"), &art)
}

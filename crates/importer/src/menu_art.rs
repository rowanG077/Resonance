//! Native player-menu artwork directly into named KTX2 images and `ui/menu.json`.
use crate::{compression, dol, menu_sprites, menu_ui_labels, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Mappings,
    menu::{MenuArt, MenuTexture, Sprite, WindowArt},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::{ItemId, KnownId};

const NAMES: [&str; 51] = [
    "ui/windows/frame-fill",
    "ui/windows/ornate-top",
    "ui/windows/ornate-bottom",
    "ui/windows/ornate-left",
    "ui/windows/ornate-right",
    "ui/windows/ornate-top-left",
    "ui/windows/ornate-top-right",
    "ui/windows/ornate-bottom-left",
    "ui/windows/ornate-bottom-right",
    "ui/windows/ornate-bottom-right-flourish",
    "ui/windows/ornate-top-left-flourish",
    "ui/windows/ornate-bottom-left-flourish",
    "ui/windows/ornate-heading",
    "ui/menu/panel-left",
    "ui/menu/panel-right",
    "ui/menu/panel-middle",
    "ui/menu/scroll-down",
    "ui/menu/scroll-up",
    "ui/menu/icons",
    "ui/menu/lloyd-portrait",
    "ui/menu/colette-portrait",
    "ui/menu/genis-portrait",
    "ui/menu/raine-portrait",
    "ui/menu/sheena-portrait",
    "ui/menu/zelos-portrait",
    "ui/menu/presea-portrait",
    "ui/menu/regal-portrait",
    "ui/menu/kratos-portrait",
    "ui/menu/sylvarant-map",
    "ui/menu/tethealla-map",
    "ui/windows/white-fill",
    "ui/windows/frame-fill",
    "ui/windows/gold-top",
    "ui/windows/gold-bottom",
    "ui/windows/gold-left",
    "ui/windows/gold-right",
    "ui/windows/gold-top-left",
    "ui/windows/gold-top-right",
    "ui/windows/gold-bottom-left",
    "ui/windows/gold-bottom-right",
    "ui/windows/gold-bottom-right-flourish",
    "ui/windows/gold-top-left-flourish",
    "ui/windows/gold-bottom-left-flourish",
    "ui/windows/gold-heading",
    "ui/cursors/gold-blade",
    "ui/windows/cracked-stone",
    "ui/windows/rough-stone",
    "ui/windows/fine-grain",
    "ui/windows/speckled",
    "ui/windows/small-stones",
    "ui/cursors/hand",
];

struct ImageBank<'a> {
    bytes: &'a [u8],
}

impl<'a> ImageBank<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self> {
        ensure!(dol::u32(bytes, 0)? == 0x0020_af30, "invalid menu TPL");
        Ok(Self { bytes })
    }

    fn count(&self) -> Result<usize> {
        Ok(dol::u32(self.bytes, 4)? as usize)
    }

    fn image(&self, index: usize, name: usize, opaque: bool) -> Result<(MenuTexture, Vec<u8>)> {
        ensure!(index < self.count()?, "missing native menu image");
        let (width, height, mut rgba) = menu_sprites::decode_image(self.bytes, index)?;
        if (45..50).contains(&name) {
            for pixel in rgba.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        let table = dol::u32(self.bytes, 8)? as usize;
        let descriptor = dol::u32(self.bytes, table + index * 8)? as usize;
        let wrap = dol::u32(self.bytes, descriptor + 12)?;
        ensure!(
            wrap == dol::u32(self.bytes, descriptor + 16)?,
            "mixed menu image wrap"
        );
        ensure!(wrap <= 1, "unsupported menu image wrap");
        let path = format!("{}.ktx2", NAMES[name]);
        Ok((
            MenuTexture {
                path,
                width,
                height,
                repeat: wrap == 1,
                opaque,
            },
            resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?,
        ))
    }
}

fn append(
    exe: &[u8],
    textures: &mut BTreeMap<usize, MenuTexture>,
    images: &mut BTreeMap<String, Vec<u8>>,
    address: u32,
    length: usize,
    indices: impl IntoIterator<Item = (usize, usize, bool)>,
) -> Result<()> {
    let bank = ImageBank::new(dol::slice(exe, address, length)?)?;
    for (source, target, opaque) in indices {
        let (texture, bytes) = bank.image(source, target, opaque)?;
        insert(textures, images, target, texture, bytes)?;
    }
    Ok(())
}

fn insert(
    textures: &mut BTreeMap<usize, MenuTexture>,
    images: &mut BTreeMap<String, Vec<u8>>,
    index: usize,
    texture: MenuTexture,
    bytes: Vec<u8>,
) -> Result<()> {
    ensure!(
        textures.insert(index, texture.clone()).is_none(),
        "duplicate menu texture slot"
    );
    match images.get(&texture.path) {
        Some(existing) => ensure!(
            resonance_asset_writer::texture::ktx2(existing)?
                == resonance_asset_writer::texture::ktx2(&bytes)?,
            "conflicting shared menu image {}",
            texture.path
        ),
        None => {
            images.insert(texture.path, bytes);
        }
    }
    Ok(())
}

fn windows(exe: &[u8]) -> Result<[WindowArt; 3]> {
    let motion = dol::slice(exe, 0x8035_d8a8, 16)?;
    let float = |at| -> Result<f32> {
        let value = f32::from_bits(dol::u32(motion, at)?);
        ensure!(value.is_finite(), "invalid native cursor motion");
        Ok(value)
    };
    let scale = float(0)?;
    let divisor = float(4)?;
    ensure!(divisor != 0., "zero cursor divisor");
    let motion = |mode: usize| -> Result<[f32; 2]> {
        Ok([
            float(if mode == 0 { 8 } else { 12 })?,
            scale * if mode == 0 { 2. } else { 1. } / divisor,
        ])
    };
    let patterns = |fill| std::array::from_fn(|index| if index == 5 { fill } else { 45 + index });
    Ok([
        WindowArt {
            patterns: patterns(30),
            heading: None,
            cursor: Some(50),
            cursor_motion: motion(0)?,
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
            cursor_motion: motion(1)?,
            slices: Some(std::array::from_fn(|index| index)),
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
            cursor_motion: motion(1)?,
            slices: Some(std::array::from_fn(|index| 31 + index)),
            outset: 4,
            flourish_outset: [8, 4],
            foot_outset: 8,
            left_joins: [48, 32],
            left_strip: [68, 24],
        },
    ])
}

fn color_rows<const ROWS: usize, const COLUMNS: usize>(
    exe: &[u8],
    address: u32,
) -> Result<Vec<[[u8; 4]; COLUMNS]>> {
    let bytes = dol::slice(exe, address, ROWS * COLUMNS * 4)?;
    Ok((0..ROWS)
        .map(|row| {
            std::array::from_fn(|column| {
                let at = (row * COLUMNS + column) * 4;
                bytes[at..at + 4].try_into().unwrap()
            })
        })
        .collect())
}

fn character_names(exe: &[u8]) -> Result<Vec<String>> {
    dol::slice(exe, 0x801f_9fc8, 9 * 280)?
        .chunks_exact(280)
        .map(|row| {
            let end = row[..13]
                .iter()
                .position(|&byte| byte == 0)
                .context("unterminated native character name")?;
            let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(&row[..end]);
            ensure!(!invalid, "invalid native character name");
            Ok(name.into_owned())
        })
        .collect()
}

fn prepare(extracted: &Path) -> Result<(MenuArt, BTreeMap<String, Vec<u8>>)> {
    let disc = extracted.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let mut textures = BTreeMap::new();
    let mut images = BTreeMap::new();
    append(
        &exe,
        &mut textures,
        &mut images,
        0x8023_4260,
        0x2c20,
        (0..16).map(|i| (i, i, i == 0)),
    )?;
    append(
        &exe,
        &mut textures,
        &mut images,
        0x8024_9780,
        0x16a0,
        (2..4).map(|i| (i, i + 14, false)),
    )?;
    let (atlas, mut sprites, bytes) = menu_sprites::sprites(extracted, &exe)?;
    insert(&mut textures, &mut images, 18, atlas, bytes)?;

    let declaration = text::string(&exe, 0x8019_9dbc)?;
    let path = declaration.trim_start_matches('/');
    resonance_content::validate_asset_path(path)?;
    ensure!(
        path.eq_ignore_ascii_case("field/s.z"),
        "unknown menu portrait bank"
    );
    let portraits = compression::cabinet(&fs::read(disc.join("files/FIELD/s.z"))?)?;
    let bank = ImageBank::new(&portraits)?;
    ensure!(bank.count()? == 9, "unexpected status portrait count");
    for i in 0..9 {
        let (texture, bytes) = bank.image(i, 19 + i, false)?;
        insert(&mut textures, &mut images, 19 + i, texture, bytes)?;
    }
    append(
        &exe,
        &mut textures,
        &mut images,
        0x8024_e2a0,
        0x1b0e0,
        (0..2).map(|i| (i, 28 + i, false)),
    )?;
    for (address, length, first, count, opaque) in [
        (0x8023_1720, 0x240, 30, 1, 1),
        (0x8023_6e80, 0x2500, 31, 14, 1),
        (0x8023_1960, 0x2900, 45, 5, 5),
        (0x8024_9500, 0x280, 50, 1, 0),
    ] {
        append(
            &exe,
            &mut textures,
            &mut images,
            address,
            length,
            (0..count).map(|i| (i, first + i, i < opaque)),
        )?;
    }

    sprites.number_colors = color_rows::<18, 2>(&exe, 0x801a_bf74)?;
    sprites.bar_colors = color_rows::<3, 4>(&exe, 0x801a_bf44)?;
    sprites.names = character_names(&exe)?;
    let item_names = Mappings::new(
        ItemId::ALL
            .iter()
            .filter_map(|id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    sprites.name_catalogue(Sprite::ItemImages, &item_names)?;
    let recipes: Mappings = serde_json::from_str(include_str!("recipe-names.json"))?;
    sprites.name_catalogue(Sprite::Recipes, &recipes)?;
    sprites.use_character_defaults(&character_names(&exe)?)?;

    let original = dol::slice(&exe, 0x8021_9cf8, 48)?;
    let colors = &original[16..44];
    let rgba = |at| colors[at..at + 4].try_into().unwrap();
    let palette = dol::slice(&exe, 0x8026_bdd0, 44)?
        .chunks_exact(4)
        .map(|row| row.try_into().unwrap())
        .collect();
    let art = MenuArt {
        textures,
        windows: windows(&exe)?.into_iter().enumerate().collect(),
        fill: rgba(0),
        popup_fill: rgba(12),
        shade: [rgba(16), rgba(20)],
        palette,
        labels: menu_ui_labels::menu_labels(&exe)?,
        sprites,
    };
    art.validate(528)?;
    Ok((art, images))
}

pub fn cook_menu_art(extracted: &Path, library: &Path) -> Result<()> {
    let (art, images) = prepare(extracted)?;
    for (relative, bytes) in images {
        let path = library.join(&relative);

        if path.exists() {
            ensure!(
                resonance_asset_writer::texture::ktx2(&fs::read(&path)?)?
                    == resonance_asset_writer::texture::ktx2(&bytes)?,
                "conflicting existing menu image {relative}"
            );
        } else {
            crate::output::write(&path, &bytes)?;
        }
    }
    let document = library.join("ui/menu.json");
    crate::output::json(&document, &art)?;
    Ok(())
}

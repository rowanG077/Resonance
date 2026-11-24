//! Native battle HUD palette pages and final named artwork/document.
mod atlas;
use crate::{dol, native_archive, rel::Rel};
use anyhow::{Context, Result, ensure};
use atlas::{Atlas, Sheet};
use resonance_content::{
    battle_ui::{
        Art, ComboArt, CommandArt, CommandMotion, DamageNumbers, FeedbackArt, MeterColors,
        NoticeArt,
    },
    font::UiTexture,
};
use std::{fs, path::Path};
use symphonia_constants::PartyCharacterId;

fn hud_tpl(disc: &Path) -> Result<Vec<u8>> {
    let usual = fs::read(disc.join("files/BTL/BTLusual.dat"))?;
    let images = native_archive::member(&usual, 4, 14)?.context("battle images are missing")?;
    Ok(native_archive::member(images, 3, 5)?
        .context("battle HUD is missing")?
        .to_vec())
}

/// Shared with the battle font owner; page zero is the HUD glyph sheet.
pub(crate) fn hud_page(disc: &Path, palette_page: usize) -> Result<Vec<u8>> {
    let sheet = Sheet::read(&hud_tpl(disc)?)?;
    let mut pixels = vec![0; 512 * 512 * 4];
    sheet.copy(palette_page, [0, 0, 512, 512], &mut pixels, [0, 0])?;
    // Every palette page shares the same tile sheet. These eight face-button
    // cells are obsolete; all live prompts use the configurable input atlas.
    for row in 64..128 {
        pixels[(row * 512 + 384) * 4..(row * 512 + 512) * 4].fill(0);
    }
    resonance_asset_writer::gpu::encode_rgba8(512, 512, &pixels)
}

fn array<T, const N: usize>(mut read: impl FnMut(usize) -> Result<T>) -> Result<[T; N]> {
    (0..N)
        .map(&mut read)
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid battle UI array"))
}

fn color(module: &Rel, at: usize) -> Result<[u8; 4]> {
    Ok(module.slice(4, at, 4)?.try_into()?)
}

fn colors<const N: usize>(module: &Rel, at: usize) -> Result<[[u8; 4]; N]> {
    array(|i| color(module, at + i * 4))
}

fn number(module: &Rel, at: usize) -> Result<f32> {
    Ok(f32::from_bits(dol::u32(module.slice(4, at, 4)?, 0)?))
}

fn half(module: &Rel, at: usize) -> Result<u16> {
    dol::u16(module.slice(4, at, 2)?, 0)
}

fn commands(module: &Rel) -> Result<CommandArt> {
    Ok(CommandArt {
        texture: UiTexture {
            path: "ui/battle/commands/atlas.ktx2".into(),
            width: 512,
            height: 1024,
        },
        icons: [
            vec![[49, 161, 46, 46], [97, 161, 46, 46], [145, 161, 46, 46]],
            vec![
                [97, 321, 46, 46],
                [145, 321, 46, 46],
                [193, 321, 46, 46],
                [241, 321, 46, 46],
                [289, 321, 46, 46],
            ],
            vec![
                [193, 161, 46, 46],
                [241, 161, 46, 46],
                [289, 161, 46, 46],
                [337, 161, 46, 46],
            ],
            vec![[385, 161, 46, 46], [433, 161, 46, 46]],
            vec![[1, 209, 46, 46], [49, 209, 46, 46], [97, 209, 46, 46]],
            vec![[145, 209, 46, 46]],
        ],
        faces: std::array::from_fn(|i| [64 + i as u16 * 32, 104, 32, 32]),
        background: [0, 160, 48, 48],
        shadow: [0, 160, 48, 48],
        disabled: [192, 256, 48, 48],
        plate: [304, 0, 80, 40],
        cursor: [160, 72, 32, 32],
        cursor_shadow: [160, 72, 32, 32],
        names: array(|i| {
            let (section, offset) = module.pointer(5, 0x90 + 4 * i)?;
            module.text(section, offset)
        })?,
        cancel_escape: module.text(4, 0x3b4)?,
        player_format: module.text(4, 0x3d8)?,
        player_colors: colors(module, 0x35c)?,
        text_color: color(module, 0x2758)?,
        shadow_color: color(module, 0x384)?,
        pending_item_color: color(module, 0x388)?,
        selector_labels: [module.text(4, 0x3c4)?, module.text(4, 0x3cc)?],
        selector_colors: [colors(module, 0x364)?, colors(module, 0x36c)?],
        selector_plate_colors: colors(module, 0x374)?,
        motion: CommandMotion {
            selected_shade: [number(module, 0x38c)?, number(module, 0x390)?],
            bob: number(module, 0x394)?,
            tilt: number(module, 0x398)?,
            small_bob: number(module, 0x39c)?,
            strategy_rotation: number(module, 0x3a0)?,
            lift: number(module, 0x3a4)?,
            label_bob: number(module, 0x3a8)?,
            item_sway: number(module, 0x3ac)?,
            escape_rotation: number(module, 0x3b0)?,
            cursor_bob: number(module, 0x4554)?,
            selector_bob: number(module, 0x3d4)?,
        },
    })
}

fn command_atlas(sheet: &Sheet, art: &mut CommandArt) -> Result<Vec<u8>> {
    let mut pixels = vec![0; 512 * 1024 * 4];
    let mut tile = 0usize;
    let mut copy = |palette: usize, rect: &mut [u16; 4]| -> Result<()> {
        let [u, v, width, height] = rect.map(usize::from);
        let x = tile % 4 * 128 + 2;
        let y = tile / 4 * 64 + 2;
        sheet.copy(palette, [u, v, width, height], &mut pixels, [x, y])?;
        *rect = [x as u16, y as u16, width as u16, height as u16];
        tile += 1;
        Ok(())
    };
    for (icons, palettes) in art.icons.iter_mut().zip([
        &[3, 4, 5][..],
        &[75, 76, 77, 77, 77],
        &[6, 7, 8, 9],
        &[10, 11],
        &[12, 13, 14],
        &[15],
    ]) {
        for (rect, &palette) in icons.iter_mut().zip(palettes) {
            copy(palette, rect)?;
        }
    }
    for (palette, rect) in [
        (2, &mut art.background),
        (86, &mut art.shadow),
        (47, &mut art.disabled),
        (58, &mut art.plate),
        (21, &mut art.cursor),
        (0, &mut art.cursor_shadow),
    ] {
        copy(palette, rect)?;
    }
    for (i, rect) in art.faces.iter_mut().enumerate() {
        copy(32 + i, rect)?;
    }
    Ok(resonance_asset_writer::gpu::encode_rgba8(
        512, 1024, &pixels,
    )?)
}

fn convert(disc: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let module = Rel::read(fs::read(disc.join("files/US_r_Top2Btl.rel"))?)?;
    ensure!(
        module.slice(4, 0xd8, 5)? == [10, 0, 11, 12, 1],
        "unsupported battle texture bindings"
    );
    ensure!(
        module.slice(4, 0xe74, 4)? == [25, 25, 24, 24],
        "unsupported Unison button palettes"
    );
    let sheet = Sheet::read(&hud_tpl(disc)?)?;
    let mut atlas = Atlas::new();
    let mut region = |palette, rect| atlas.insert(&sheet, palette, rect);
    let icon_origins: [[u16; 2]; 4] =
        array(|index| array(|axis| half(&module, 0x4280 + index * 4 + axis * 2)))?;
    let portraits = std::array::from_fn(|index| {
        let name = PartyCharacterId::RESOURCE_NAMES[index];
        Some(UiTexture {
            path: format!("ui/battle/portraits/{name}.ktx2"),
            width: 64,
            height: 256,
        })
    });
    let mut art = Art::<String> {
        font: "fonts/battle.json".into(),
        portraits,
        meters: MeterColors {
            bars: array(|side| array(|corner| color(&module, 0x4190 + side * 16 + corner * 4)))?,
            numbers: array(|side| colors(&module, 0x4180 + side * 8))?,
            trail: color(&module, 0x41b0)?,
            shadow: color(&module, 0x41b4)?,
        },
        damage: DamageNumbers {
            colors: array(|side| colors(&module, 0x4350 + side * 8))?,
            sizes: array(|index| array(|axis| half(&module, 0x4368 + index * 4 + axis * 2)))?,
        },
        recovery_colors: array(|kind| colors(&module, 0x4290 + kind * 8))?,
        result_colors: [
            colors(&module, 0x2db0)?,
            colors(&module, 0x2dc8)?,
            colors(&module, 0x2de0)?,
        ],
        targets: array(|index| {
            region(
                16 + index,
                if index == 0 {
                    [0, 256, 48, 64]
                } else {
                    [48, 256, 144, 64]
                },
            )
        })?,
        stun: region(62, [256, 24, 48, 32])?,
        notices: NoticeArt {
            frame: region(80, [384, 376, 128, 80])?,
            fill: region(81, [408, 456, 104, 56])?,
            icons: array(|index| {
                let [u, v] = icon_origins[index].map(usize::from);
                region(82 + index, [u, v, 24, 24])
            })?,
            icon_origins,
            defeated: module.text(4, 0x2c3c)?,
            escaped: module.text(4, 0x220)?,
            faces: array(|index| {
                // Notices and Unison use slightly different cuts of the last three faces.
                let x = 64 + index * 32;
                region(32 + index, [x, 104, if index >= 6 { 40 } else { 32 }, 32])
            })?,
        },
        combo: ComboArt {
            anchors: [half(&module, 0x4304)?, half(&module, 0x4306)?],
            number_colors: colors(&module, 0x4308)?,
            bar_colors: array(|side| {
                array(|corner| color(&module, 0x4328 + side * 16 + corner * 4))
            })?,
        },
        stat_colors: colors(&module, 0x4374)?,
        stat_backdrops: array(|line| {
            array(|corner| color(&module, 0x437c + line * 16 + corner * 4))
        })?,
        stat_icons: [
            region(94, [288, 136, 24, 24])?,
            region(93, [352, 56, 24, 24])?,
            region(91, [304, 56, 24, 24])?,
            region(87, [328, 80, 24, 24])?,
            region(89, [256, 56, 24, 24])?,
            region(90, [280, 56, 24, 24])?,
            // Accuracy and the blinking curse overlay share this palette.
            region(42, [0, 72, 280, 32])?,
            region(43, [32, 72, 24, 24])?,
            region(88, [352, 80, 24, 24])?,
            region(92, [328, 56, 24, 24])?,
        ],
        unison_buttons: [
            UiTexture {
                path: resonance_content::input_prompts::IMAGE_PATH.into(),
                width: 0,
                height: 0,
            },
            UiTexture {
                path: resonance_content::input_prompts::IMAGE_PATH.into(),
                width: 0,
                height: 0,
            },
        ],
        commands: commands(&module)?,
        feedback: FeedbackArt {
            item_label: module.text(4, 0x44d4)?,
            item_bar: colors(&module, 0x439c)?,
            item_text: colors(&module, 0x43ac)?,
            scan_vitals: array(|side| colors(&module, 0x4c0 + side * 8))?,
            scan_icons: array(|index| {
                let rect = if index < 9 {
                    [index * 24, 136, 24, 24]
                } else {
                    [214, 136, 48, 24]
                };
                region(48 + index, rect)
            })?,
            unison_frame: region(27, [0, 368, 352, 144])?,
            unison_ready: module.text(4, 0x4560)?,
            unison_text: colors(&module, 0x4274)?,
            escape: [
                region(29, [304, 208, 144, 80])?,
                region(30, [448, 208, 16, 32])?,
                region(31, [312, 208, 128, 80])?,
            ],
            escape_positions: array(|index| {
                array(|axis| half(&module, 0x420c + index * 4 + axis * 2))
            })?,
        },
    };
    let commands = command_atlas(&sheet, &mut art.commands)?;
    Ok(vec![
        (
            atlas::PATH.into(),
            resonance_asset_writer::gpu::encode_rgba8(512, 512, &atlas.pixels)?,
        ),
        (art.commands.texture.path.clone(), commands),
        (
            "ui/battle/battle.json".into(),
            resonance_asset_writer::json::encode(&art)?,
        ),
    ])
}

/// Cook battle HUD data and artwork. Fonts and character portraits have their own owners.
pub fn cook_battle_ui_hud(root: &Path, library: &Path) -> Result<()> {
    let files = convert(&root.join("disc1"))?;
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}

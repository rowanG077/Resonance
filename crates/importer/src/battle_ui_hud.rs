//! Native battle HUD palette pages and final named artwork/document.
use crate::{dol, native_archive, rel::Rel};
use anyhow::{Context, Result, ensure};
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

fn pixel(value: u16) -> [u8; 4] {
    let x3 = |v: u16| ((v << 5) | (v << 2) | (v >> 1)) as u8;
    let x4 = |v: u16| ((v << 4) | v) as u8;
    let x5 = |v: u16| ((v << 3) | (v >> 2)) as u8;
    if value & 0x8000 != 0 {
        [
            x5((value >> 10) & 31),
            x5((value >> 5) & 31),
            x5(value & 31),
            255,
        ]
    } else {
        [
            x4((value >> 8) & 15),
            x4((value >> 4) & 15),
            x4(value & 15),
            x3(value >> 12),
        ]
    }
}

/// Decode a complete 512×512 CI4 image with the selected authored palette.
fn page(tpl: &[u8], palette_page: usize) -> Result<Vec<u8>> {
    ensure!(
        dol::u32(tpl, 0)? == 0x0020_af30 && dol::u32(tpl, 4)? == 1,
        "unexpected battle HUD TPL"
    );
    let table = dol::u32(tpl, 8)? as usize;
    let descriptor = dol::u32(tpl, table)? as usize;
    let palette = dol::u32(tpl, table + 4)? as usize;
    ensure!(
        dol::u16(tpl, descriptor)? == 512
            && dol::u16(tpl, descriptor + 2)? == 512
            && dol::u32(tpl, descriptor + 4)? == 8,
        "unsupported battle HUD image"
    );
    ensure!(
        dol::u32(tpl, palette + 4)? == 2,
        "unsupported battle HUD palette"
    );
    let entries = usize::from(dol::u16(tpl, palette)?);
    ensure!(
        entries == 96 * 16 && palette_page < 96,
        "unexpected battle HUD palette pages"
    );
    let palette_start = dol::u32(tpl, palette + 8)? as usize + palette_page * 32;
    let colors = (0..16)
        .map(|index| dol::u16(tpl, palette_start + index * 2).map(pixel))
        .collect::<Result<Vec<_>>>()?;
    let start = dol::u32(tpl, descriptor + 8)? as usize;
    let blocks = tpl
        .get(start..start + 512 * 512 / 2)
        .context("truncated battle HUD image")?;
    let mut rgba = vec![0; 512 * 512 * 4];
    for (index, block) in blocks.chunks_exact(32).enumerate() {
        let left = index % 64 * 8;
        let top = index / 64 * 8;
        for y in 0..8 {
            for x in 0..8 {
                let packed = block[(y * 8 + x) / 2];
                let color = colors[usize::from(if x % 2 == 0 { packed >> 4 } else { packed & 15 })];
                let at = ((top + y) * 512 + left + x) * 4;
                rgba[at..at + 4].copy_from_slice(&color);
            }
        }
    }
    Ok(rgba)
}

/// Shared with the battle font owner; page zero is the HUD glyph sheet.
pub(crate) fn hud_page(disc: &Path, palette_page: usize) -> Result<Vec<u8>> {
    let pixels = page(&hud_tpl(disc)?, palette_page)?;
    Ok(resonance_asset_writer::ktx2::encode_rgba8(
        512, 512, &pixels,
    )?)
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

fn image(
    tpl: &[u8],
    files: &mut Vec<(String, Vec<u8>)>,
    palette: usize,
    name: &str,
) -> Result<UiTexture> {
    let path = format!("{name}.ktx2");
    let rgba = page(tpl, palette)?;
    files.push((
        path.clone(),
        resonance_asset_writer::ktx2::encode_rgba8(512, 512, &rgba)?,
    ));
    Ok(UiTexture {
        path,
        width: 512,
        height: 512,
    })
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

fn command_atlas(tpl: &[u8], art: &mut CommandArt) -> Result<Vec<u8>> {
    let mut pixels = vec![0; 512 * 1024 * 4];
    let mut tile = 0usize;
    let mut copy = |palette: usize, rect: &mut [u16; 4]| -> Result<()> {
        let source = page(tpl, palette)?;
        let [u, v, width, height] = rect.map(usize::from);
        let x = tile % 4 * 128 + 2;
        let y = tile / 4 * 64 + 2;
        for row in 0..height {
            let from = ((v + row) * 512 + u) * 4;
            let to = ((y + row) * 512 + x) * 4;
            pixels[to..to + width * 4].copy_from_slice(&source[from..from + width * 4]);
        }
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
    Ok(resonance_asset_writer::ktx2::encode_rgba8(
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
    let tpl = hud_tpl(disc)?;
    let mut files = Vec::new();
    macro_rules! image {
        ($page:expr, $name:expr $(,)?) => {
            image(&tpl, &mut files, $page, $name)
        };
    }
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
            let names = ["frame", "player-1", "player-2", "player-3", "player-4"];
            image!(16 + index, &format!("ui/battle/target/{}", names[index]))
        })?,
        stun: image!(62, "ui/battle/stun")?,
        notices: NoticeArt {
            frame: image!(80, "ui/battle/notices/frame")?,
            fill: image!(81, "ui/battle/notices/fill")?,
            icons: array(|index| {
                let names = ["arte", "spell", "item", "outcome"];
                image!(82 + index, &format!("ui/battle/notices/{}", names[index]))
            })?,
            icon_origins: array(|index| {
                array(|axis| half(&module, 0x4280 + index * 4 + axis * 2))
            })?,
            defeated: module.text(4, 0x2c3c)?,
            faces: array(|index| {
                image!(
                    32 + index,
                    &format!(
                        "ui/battle/notices/{}",
                        PartyCharacterId::RESOURCE_NAMES[index]
                    ),
                )
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
            image!(94, "ui/battle/status/attack")?,
            image!(93, "ui/battle/status/defense")?,
            image!(91, "ui/battle/status/up")?,
            image!(87, "ui/battle/status/magic-attack")?,
            image!(89, "ui/battle/status/physical-barrier")?,
            image!(90, "ui/battle/status/magical-barrier")?,
            image!(42, "ui/battle/status/accuracy-curse")?,
            image!(43, "ui/battle/status/reduced-item-effect")?,
            image!(88, "ui/battle/status/holy-song")?,
            image!(92, "ui/battle/status/down")?,
        ],
        unison_buttons: [
            image!(25, "ui/battle/unison/a-b")?,
            image!(24, "ui/battle/unison/x-y")?,
        ],
        commands: commands(&module)?,
        feedback: FeedbackArt {
            item_label: module.text(4, 0x44d4)?,
            item_bar: colors(&module, 0x439c)?,
            item_text: colors(&module, 0x43ac)?,
            scan_vitals: array(|side| colors(&module, 0x4c0 + side * 8))?,
            scan_icons: array(|index| {
                let names = [
                    "neutral",
                    "water",
                    "wind",
                    "fire",
                    "earth",
                    "lightning",
                    "ice",
                    "light",
                    "darkness",
                    "headings",
                ];
                image!(48 + index, &format!("ui/battle/scan/{}", names[index]))
            })?,
            unison_frame: image!(27, "ui/battle/unison/frame")?,
            unison_ready: module.text(4, 0x4560)?,
            unison_text: colors(&module, 0x4274)?,
            escape: [
                image!(29, "ui/battle/escape/frame")?,
                image!(30, "ui/battle/escape/needle")?,
                image!(31, "ui/battle/escape/fill")?,
            ],
            escape_positions: array(|index| {
                array(|axis| half(&module, 0x420c + index * 4 + axis * 2))
            })?,
        },
    };
    let atlas = command_atlas(&tpl, &mut art.commands)?;
    files.push((art.commands.texture.path.clone(), atlas));
    files.push((
        "ui/battle/battle.json".into(),
        resonance_asset_writer::json::encode(&art)?,
    ));
    Ok(files)
}

/// Cook battle HUD data and artwork. Fonts and character portraits have their own owners.
pub fn cook_battle_ui_hud(root: &Path, library: &Path) -> Result<()> {
    let files = convert(&root.join("disc1"))?;
    ensure!(
        files.iter().all(|(path, _)| !library.join(path).exists()),
        "battle HUD already exists"
    );
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}

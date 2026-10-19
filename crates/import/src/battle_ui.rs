//! Party HUD and result artwork.
use crate::{read::Field, rel::Rel, source_assets::section, texture::Texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_ui::{
        Art, ComboArt, CommandArt, CommandMotion, DamageNumbers, FeedbackArt, MeterColors,
        NoticeArt,
    },
    font::{BitmapFont, Glyph, UiTexture},
};
use std::{collections::BTreeSet, fs, path::Path};

/// All source discovery and conversion finishes before the descriptor is published.
pub fn publish(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = fs::read(extracted.join("files").join(sources.usual))?;
    let module = Rel::read(&extracted.join("files").join(sources.module))?;
    publish_source(&usual, &module, output, "battle")
}

pub(crate) fn publish_source(
    usual: &[u8],
    module: &Rel,
    output: &Path,
    prefix: &str,
) -> Result<Vec<String>> {
    let images = section(usual, 4)?;
    let portraits = crate::texture::decode_source(section(images, 2)?)?
        .write(output)?
        .textures
        .into_iter()
        .map(|texture| {
            let texture = texture.context("missing battle portrait")?;
            sampler(&texture, true)?;
            texture.image(0)
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected nine battle portraits"))?;
    let atlas = crate::texture::decode_source(section(images, 3)?)?.write(output)?;
    ensure!(atlas.textures.len() == 1, "expected one battle HUD atlas");
    let atlas = atlas.textures[0]
        .as_ref()
        .context("missing battle HUD atlas")?;
    ensure!(
        atlas.dimensions == [512, 512] && atlas.images.len() == 96,
        "unsupported battle HUD atlas"
    );
    sampler(atlas, false)?;
    let mut art = read(usual, module, atlas, portraits)?;
    art.commands.texture = publish_commands(section(images, 3)?, &mut art.commands, output)?;
    art.validate()?;
    let path = format!("{prefix}/ui.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&art)?)?;
    Ok(std::iter::once(path)
        .chain(art.files().map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn sampler(texture: &Texture, repeat: bool) -> Result<()> {
    use resonance_content::{TextureWrap, texture::Filter};
    ensure!(
        texture.sampler.wrap.iter().all(|wrap| if repeat {
            matches!(wrap, TextureWrap::Repeat)
        } else {
            matches!(wrap, TextureWrap::Clamp)
        }) && matches!(texture.sampler.min_filter, Filter::Linear)
            && matches!(texture.sampler.mag_filter, Filter::Linear),
        "unsupported battle UI sampler"
    );
    Ok(())
}

fn read(usual: &[u8], module: &Rel, atlas: &Texture, portraits: [UiTexture; 9]) -> Result<Art> {
    let at = |offset: usize| module.at((4, offset));
    ensure!(
        at(0xd8)?.get(..5) == Some(&[10, 0, 11, 12, 1]),
        "unsupported battle texture bindings"
    );
    ensure!(
        at(0xe74)?.get(..4) == Some(&[25, 25, 24, 24]),
        "unsupported Unison button palettes"
    );
    let punctuation = <[[u16; 2]; 15]>::read(at(0x26e0)?, 0)?;
    let font = font(usual, module, atlas.image(0)?, punctuation);
    Ok(Art {
        version: Art::VERSION,
        commands: commands(module)?,
        feedback: FeedbackArt {
            item_label: module.text((4, 0x44d4))?,
            item_bar: <[[u8; 4]; 4]>::read(at(0x439c)?, 0)?,
            item_text: <[[u8; 4]; 2]>::read(at(0x43ac)?, 0)?,
            scan_vitals: <[[[u8; 4]; 2]; 2]>::read(at(0x4c0)?, 0)?,
            scan_icons: (48..=57)
                .map(|page| atlas.image(page))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            unison_frame: atlas.image(27)?,
            unison_ready: module.text((4, 0x4560))?,
            unison_text: <[[u8; 4]; 2]>::read(at(0x4274)?, 0)?,
            escape: [atlas.image(29)?, atlas.image(30)?, atlas.image(31)?],
            escape_positions: <[[u16; 2]; 2]>::read(at(0x420c)?, 0)?,
        },
        font,
        portraits: portraits.map(Some),
        meters: MeterColors {
            bars: <[[[u8; 4]; 4]; 2]>::read(at(0x4190)?, 0)?,
            numbers: <[[[u8; 4]; 2]; 2]>::read(at(0x4180)?, 0)?,
            trail: <[u8; 4]>::read(at(0x41b0)?, 0)?,
            shadow: <[u8; 4]>::read(at(0x41b4)?, 0)?,
        },
        damage: DamageNumbers {
            colors: <[[[u8; 4]; 2]; 2]>::read(at(0x4350)?, 0)?,
            sizes: <[[u16; 2]; 3]>::read(at(0x4368)?, 0)?,
        },
        recovery_colors: <[[[u8; 4]; 2]; 2]>::read(at(0x4290)?, 0)?,
        result_colors: [
            <[[u8; 4]; 2]>::read(at(0x2db0)?, 0)?,
            <[[u8; 4]; 2]>::read(at(0x2dc8)?, 0)?,
            <[[u8; 4]; 2]>::read(at(0x2de0)?, 0)?,
        ],
        targets: (16..=20)
            .map(|page| atlas.image(page))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        stun: atlas.image(62)?,
        notices: NoticeArt {
            frame: atlas.image(80)?,
            fill: atlas.image(81)?,
            icons: [
                atlas.image(82)?,
                atlas.image(83)?,
                atlas.image(84)?,
                atlas.image(85)?,
            ],
            icon_origins: <[[u16; 2]; 4]>::read(at(0x4280)?, 0)?,
            defeated: module.text((4, 0x2c3c))?,
            faces: (32..41)
                .map(|page| atlas.image(page))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
        },
        stat_colors: <[[u8; 4]; 2]>::read(at(0x4374)?, 0)?,
        stat_backdrops: <[[[u8; 4]; 4]; 2]>::read(at(0x437c)?, 0)?,
        stat_icons: [
            atlas.image(94)?,
            atlas.image(93)?,
            atlas.image(91)?,
            atlas.image(87)?,
            atlas.image(89)?,
            atlas.image(90)?,
            atlas.image(42)?,
            atlas.image(43)?,
            atlas.image(88)?,
            atlas.image(92)?,
        ],
        unison_buttons: [atlas.image(25)?, atlas.image(24)?],
        combo: ComboArt {
            anchors: <[u16; 2]>::read(at(0x4304)?, 0)?,
            number_colors: <[[u8; 4]; 2]>::read(at(0x4308)?, 0)?,
            bar_colors: <[[[u8; 4]; 4]; 2]>::read(at(0x4328)?, 0)?,
        },
    })
}

fn commands(module: &Rel) -> Result<CommandArt> {
    let at = |offset| module.at((4, offset));
    Ok(CommandArt {
        texture: UiTexture {
            path: String::new(),
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
        names: (0..6)
            .map(|i| module.text(module.pointer(5, 0x90 + 4 * i)?))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        cancel_escape: module.text((4, 0x3b4))?,
        player_format: module.text((4, 0x3d8))?,
        player_colors: <[[u8; 4]; 2]>::read(at(0x35c)?, 0)?,
        text_color: <[u8; 4]>::read(at(0x2758)?, 0)?,
        shadow_color: <[u8; 4]>::read(at(0x384)?, 0)?,
        pending_item_color: <[u8; 4]>::read(at(0x388)?, 0)?,
        selector_labels: [module.text((4, 0x3c4))?, module.text((4, 0x3cc))?],
        selector_colors: [
            <[[u8; 4]; 2]>::read(at(0x364)?, 0)?,
            <[[u8; 4]; 2]>::read(at(0x36c)?, 0)?,
        ],
        selector_plate_colors: <[[u8; 4]; 4]>::read(at(0x374)?, 0)?,
        motion: CommandMotion {
            selected_shade: <[f32; 2]>::read(at(0x38c)?, 0)?,
            bob: f32::read(at(0x394)?, 0)?,
            tilt: f32::read(at(0x398)?, 0)?,
            small_bob: f32::read(at(0x39c)?, 0)?,
            strategy_rotation: f32::read(at(0x3a0)?, 0)?,
            lift: f32::read(at(0x3a4)?, 0)?,
            label_bob: f32::read(at(0x3a8)?, 0)?,
            item_sway: f32::read(at(0x3ac)?, 0)?,
            escape_rotation: f32::read(at(0x3b0)?, 0)?,
            cursor_bob: f32::read(at(0x4554)?, 0)?,
            selector_bob: f32::read(at(0x3d4)?, 0)?,
        },
    })
}

/// Pack only the command tiles, with a transparent gutter for linear sampling.
fn publish_commands(bytes: &[u8], art: &mut CommandArt, output: &Path) -> Result<UiTexture> {
    const COLORS_PER_PALETTE: usize = 16;
    let texture = &crate::tpl::parse_tpl(bytes)?[0];
    ensure!(texture.format == 8, "unsupported command atlas index width");
    let mut pixels = vec![0; 512 * 1024 * 4];
    let mut tile = 0;
    let mut copy = |palette, rect: &mut [u16; 4]| -> Result<()> {
        let source =
            crate::tpl::decode_palette_window(bytes, texture, palette * COLORS_PER_PALETTE)?;
        let [u, v, width, height] = rect.map(usize::from);
        let x = tile % 4 * 128 + 2;
        let y = tile / 4 * 64 + 2;
        for row in 0..height {
            let from = ((v + row) * usize::from(texture.width) + u) * 4;
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
    let path = format!("textures/{}/battle-commands.ktx2", crate::digest(&pixels));
    crate::texture::cook(512, 1024, &pixels, &output.join(&path))?;
    Ok(UiTexture {
        path,
        width: 512,
        height: 1024,
    })
}

fn font(usual: &[u8], module: &Rel, image: UiTexture, punctuation: [[u16; 2]; 15]) -> BitmapFont {
    let glyphs = (b'!'..=b'Z')
        .filter_map(|ch| {
            let [x, y] = match ch {
                b'(' => [328, 136],
                b')' => [336, 136],
                b'!'..=b'/' => punctuation[usize::from(ch - b'!')].map(u32::from),
                b'0'..=b'9' => [u32::from(ch - b'0') * 16, 0],
                b'A'..=b'F' => [160 + u32::from(ch - b'A') * 16, 0],
                b'G'..=b'V' => [u32::from(ch - b'G') * 16, 24],
                b'W'..=b'Z' => [u32::from(ch - b'W') * 16, 48],
                _ => return None,
            };
            Some((
                char::from(ch),
                Glyph {
                    rect: [x, y, if matches!(ch, b'(' | b')') { 8 } else { 16 }, 23],
                    advance: 13,
                },
            ))
        })
        .collect();
    BitmapFont {
        version: 1,
        texture: image.path,
        width: image.width,
        height: image.height,
        line_height: 24,
        glyphs,
        source_sha256: crate::digest(usual),
        executable_sha256: crate::digest(&module.bytes),
    }
}

#[cfg(test)]
mod tests;

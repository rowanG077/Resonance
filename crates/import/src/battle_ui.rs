//! Original party HUD and result artwork. REL 6CDEC/4C7A0/553E0 are the consumers.
use crate::{read::Field, rel::Rel, source_assets::section, texture::Texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_ui::{
        Art, Combo, CommandArt, CommandMotion, Gauge, Intro, Markers, Number, Orders, Overlays,
        PartyPanel, Radar, RecoveryNumbers, ResultsPanel, Sprite,
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
    let art = read(usual, module, atlas, portraits)?;
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
    let punctuation = <[[u16; 2]; 15]>::read(at(0x26e0)?, 0)?;
    let font = font(usual, module, atlas.image(0)?, punctuation);
    let gauge = |offset: usize, number_offset: [i16; 2], bar_offset: [i16; 2]| -> Result<Gauge> {
        Ok(Gauge {
            number_offset,
            bar_offset,
            bar_size: [56, 8],
            skew: 8,
            colors: <[[u8; 4]; 4]>::read(at(0x4190 + offset * 2)?, 0)?,
            number_colors: <[[u8; 4]; 2]>::read(at(0x4180 + offset)?, 0)?,
        })
    };
    let sprite = |palette, rect| -> Result<Sprite> {
        Ok(Sprite {
            texture: atlas.image(palette)?,
            rect,
        })
    };
    let colors = <[[u8; 4]; 14]>::read(at(0x2db0)?, 0)?;
    let strings = |offsets: &[usize]| -> Result<Vec<String>> {
        offsets
            .iter()
            .map(|&offset| module.text((4, offset)))
            .collect()
    };
    Ok(Art {
        version: Art::VERSION,
        commands: CommandArt {
            background: atlas.image(2)?,
            icons: [
                vec![3, 4, 5],
                vec![75, 76, 77],
                vec![6, 7, 8, 9],
                vec![10, 11],
                vec![12, 13, 14],
                vec![15, 15],
            ]
            .map(|palettes| {
                palettes
                    .into_iter()
                    .map(|p| atlas.image(p))
                    .collect::<Result<Vec<_>>>()
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
            disabled: atlas.image(47)?,
            plate: atlas.image(58)?,
            shadow: atlas.image(86)?,
            cursor: atlas.image(21)?,
            cursor_shadow: atlas.image(0)?,
            names: (0..6)
                .map(|i| module.text(module.pointer(5, 0x90 + 4 * i)?))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            player_format: module.text((4, 0x3d8))?,
            player_colors: <[[u8; 4]; 2]>::read(at(0x35c)?, 0)?,
            text_color: <[u8; 4]>::read(at(0x2758)?, 0)?,
            shadow_color: <[u8; 4]>::read(at(0x384)?, 0)?,
            motion: CommandMotion {
                selected_shade_center: f32::read(at(0x38c)?, 0)?,
                selected_shade_amplitude: f32::read(at(0x390)?, 0)?,
                bob_amplitude: f32::read(at(0x394)?, 0)?,
                y_rotation: f32::read(at(0x398)?, 0)?,
                small_bob_amplitude: f32::read(at(0x39c)?, 0)?,
                strategy_rotation_amplitude: f32::read(at(0x3a0)?, 0)?,
                lift_amplitude: f32::read(at(0x3a4)?, 0)?,
                // Original3A8 is shared by label bounce and item rotation.
                label_bob_amplitude: f32::read(at(0x3a8)?, 0)?,
                item_rotation_amplitude: f32::read(at(0x3a8)?, 0)?,
                item_sway_amplitude: f32::read(at(0x3ac)?, 0)?,
                escape_rotation_amplitude: f32::read(at(0x3b0)?, 0)?,
            },
            cursor_amplitude: f32::read(at(0x4554)?, 0)?,
        },
        font,
        overlay_punctuation: <[[u16; 2]; 15]>::read(at(0x271c)?, 0)?,
        portraits,
        sine: <[f32; 450]>::read(section(usual, 5)?, 0)?.to_vec(),
        combat_number_colors: <[[[u8; 4]; 2]; 3]>::read(at(0x4350)?, 0)?,
        combat_number_sizes: <[[i16; 2]; 3]>::read(at(0x4368)?, 0)?,
        recovery: RecoveryNumbers {
            origin: [104, 396],
            party_spacing: 110,
            row_spacing: 24,
            rect: <[u16; 4]>::read(module.at((5, 0x3d18))?, 0)?,
            number: Number {
                glyph_size: [16, 24],
                advance: 13,
                skew: 12,
            },
            colors: <[[[u8; 4]; 2]; 2]>::read(at(0x4290)?, 0)?,
        },
        intro: Intro {
            panel_colors: <[[u8; 4]; 4]>::read(at(0x4224)?, 0)?,
            text_color: <[u8; 4]>::read(at(0x4234)?, 0)?,
            hidden_name_symbol: module.text((4, 0x2400))?,
            group_count_format: module.text((4, 0x457c))?,
        },
        orders: Orders {
            cancel_text: module.text((4, 0x4584))?,
            name_format: module.text((4, 0x44bc))?,
            panel_colors: <[[u8; 4]; 2]>::read(at(0x4214)?, 0)?,
            shadow: <[u8; 4]>::read(at(0x4220)?, 0)?,
        },
        radar: Radar {
            number_color: <[u8; 4]>::read(at(0x4238)?, 0)?,
            initial_ordinals: <[u8; 4]>::read(at(0x423c)?, 0)?,
            panel_colors: <[[u8; 4]; 4]>::read(at(0x4240)?, 0)?,
            shadow: <[u8; 4]>::read(at(0x4250)?, 0)?,
            effect: sprite(22, [128, 72, 32, 32])?,
            pulse_amplitude: f32::read(at(0x44ec)?, 0)?,
            shade_center: f32::read(at(0x455c)?, 0)?,
            target_size_amplitude: f32::read(at(0x4554)?, 0)?,
            target_size_center: f64::from_be_bytes(<[u8; 8]>::read(at(0x4570)?, 0)?),
            effect_size_amplitude: f32::read(at(0x4550)?, 0)?,
            effect_size_center: f32::read(at(0x4568)?, 0)?,
            selection_color_amplitude: f32::read(at(0x4530)?, 0)?,
            selection_blue_center: f32::read(at(0x4578)?, 0)?,
            radians_per_degree: f32::read(at(0x261c)?, 0)?,
            depth: f32::read(at(0x2620)?, 0)?,
        },
        combo: Combo {
            anchors: <[i16; 2]>::read(at(0x4348)?, 0)?,
            destinations: <[i16; 2]>::read(at(0x4304)?, 0)?,
            colors: <[[u8; 4]; 8]>::read(at(0x4308)?, 0)?,
            panel_colors: <[[u8; 4]; 8]>::read(at(0x4328)?, 0)?,
            hits: module.text((4, 0x43b4))?,
            count_format: module.text((4, 0x44e8))?,
            damage_format: module.text((4, 0x44f4))?,
            damage_suffix: module.text((4, 0x4500))?,
        },
        // Immediate draw arguments in 6EEE4 and 6CDEC; no timeline is cooked.
        party: PartyPanel {
            origin: [12, 388],
            spacing: 112,
            portrait_offset: [0, 8],
            portrait_size: [64, 64],
            portrait_inset: 1,
            hp: gauge(0, [50, 16], [48, 34])?,
            tp: gauge(8, [50, 44], [48, 62])?,
            number: Number {
                glyph_size: [16, 24],
                advance: 13,
                skew: 10,
            },
            number_shadow_offset: [1, 1],
            bar_shadow_offset: [1, 4],
            shadow: <[u8; 4]>::read(at(0x41b4)?, 0)?,
            lost_value_color: <[u8; 4]>::read(at(0x41b0)?, 0)?,
        },
        markers: Markers {
            // 7A90C addresses the original sine table beyond index 359.
            // Wrapping angles or regenerating trig changes its stored bits.
            shadow_circle: (0..15)
                .map(|vertex| -> Result<[f32; 2]> {
                    let sine = section(usual, 5)?;
                    Ok([
                        f32::read(sine, (90 + vertex * 24) * 4)?,
                        f32::read(sine, vertex * 24 * 4)?,
                    ])
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            target_background: sprite(16, [0, 256, 48, 64])?,
            target_foregrounds: (17..=20)
                .map(|palette| atlas.image(palette))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            target_frames: [[48, 256, 48, 64], [96, 256, 48, 64], [144, 256, 48, 64]],
            stun: atlas.image(62)?,
            stun_frames: [[256, 25, 48, 14], [256, 41, 48, 14]],
            stun_period_ticks: 15,
        },
        overlays: Overlays {
            notice_texts: [module.text((4, 0x2c3c))?, module.text((4, 0x220))?],
            notice_background: atlas.image(80)?,
            notice_shadow: atlas.image(81)?,
            notice_symbols: (0..4)
                .map(|index| {
                    let [x, y] = <[u16; 2]>::read(at(0x4280 + index * 4)?, 0)?.map(u32::from);
                    sprite(82 + index, [x, y, 24, 24])
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            notice_text_color: <[u8; 4]>::read(at(0x427c)?, 0)?,
            actor_lines: [6, 12, 0]
                .map(|index| -> Result<[String; 2]> {
                    Ok([
                        module.text(module.pointer(4, 0x14f4 + index * 8)?)?,
                        module.text(module.pointer(4, 0x14f4 + index * 8 + 4)?)?,
                    ])
                })
                .into_iter()
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            actor_colors: <[[u8; 4]; 2]>::read(at(0x4374)?, 0)?,
            actor_bars: <[[u8; 4]; 8]>::read(at(0x437c)?, 0)?,
        },
        results: ResultsPanel {
            positions: <[[i16; 2]; 6]>::read(at(0x2d94)?, 0)?,
            strips: <[[i16; 4]; 6]>::read(at(0x2d64)?, 0)?,
            window_y_scale: f32::read(at(0x2e54)?, 0)?,
            character_icons: (0..9)
                .map(|character| {
                    sprite(
                        character + 32,
                        <[u16; 4]>::read(at(0x4450 + character * 8)?, 0)?.map(u32::from),
                    )
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            notice_formats: strings(&[0x2fb8, 0x2fcc, 0x3018])?.try_into().unwrap(),
            formats: strings(&[0x2e60, 0x2e6c, 0x2e78, 0x2e88, 0x2e94, 0x2eac, 0x2ec0])?
                .try_into()
                .unwrap(),
            headings: strings(&[0x2ed4, 0x2ee4, 0x2eec, 0x2efc])?
                .try_into()
                .unwrap(),
            colors: std::array::from_fn(|row| [colors[row * 2], colors[row * 2 + 1]]),
            heading_colors: [colors[12], colors[13]],
            shadows: <[[u8; 4]; 3]>::read(at(0x2de8)?, 0)?,
            strip_color: <[u8; 4]>::read(at(0x2df4)?, 0)?,
            item_colors: <[[u8; 4]; 5]>::read(at(0x2df8)?, 0)?,
            number: Number {
                glyph_size: [20, 28],
                advance: 18,
                skew: 14,
            },
            next_button: [
                sprite(25, [384, 64, 32, 32])?,
                sprite(25, [384, 96, 32, 32])?,
            ],
            cook_button: [
                sprite(24, [448, 64, 32, 32])?,
                sprite(24, [448, 96, 32, 32])?,
            ],
        },
    })
}

fn font(usual: &[u8], module: &Rel, image: UiTexture, punctuation: [[u16; 2]; 15]) -> BitmapFont {
    let glyphs = (b'!'..=b'Z')
        .filter_map(|ch| {
            let [x, y] = match ch {
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
                    rect: [x, y, 16, 23],
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

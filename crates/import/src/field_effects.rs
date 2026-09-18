//! Cook emotes, particles and refraction into ordinary textures and effect recipes.
use crate::{dol, tpl, write_atomic};
use anyhow::{Result, ensure};
use resonance_content::effect::{EmoteTrack, FieldEffects, Sprite, VerticalAnchor};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::Path,
};

pub(crate) fn blink(extracted: &Path) -> Result<resonance_content::effect::BlinkCycle> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let word = |address| -> Result<u32> {
        Ok(u32::from_be_bytes(
            dol::slice(&executable, address, 4)?.try_into()?,
        ))
    };
    // The initializer selects a sequence entry and randomizes its elapsed time.
    let entry = word(0x8001D4A8)?;
    let spread = word(0x8001D4CC)?;
    ensure!(
        entry >> 16 == 0x3800 && spread >> 16 == 0x1C00,
        "unexpected blink initializer"
    );
    let mut frames = Vec::new();
    let mut initial_tick = None;
    let table = dol::slice(&executable, 0x801E3840, 20)?;
    ensure!(
        table[16..] == [0xFD, 0, 0, 1],
        "unexpected blink loop terminator"
    );
    for (index, row) in table[..16].chunks_exact(4).enumerate() {
        if index == (entry & 0xFFFF) as usize {
            initial_tick = Some(frames.len() as u16);
        }
        let ticks = usize::from(u16::from_be_bytes([row[2], row[3]])) + 1;
        ensure!(
            row[0] < 16 && row[1] == 0 && ticks <= 1024,
            "invalid blink frame"
        );
        frames.extend(std::iter::repeat_n(row[0], ticks));
    }
    let blink = resonance_content::effect::BlinkCycle {
        frames,
        initial_tick: initial_tick.ok_or_else(|| anyhow::anyhow!("invalid initial blink entry"))?,
        initial_spread: spread as u16,
    };
    blink.validate()?;
    Ok(blink)
}

pub fn cook_all(extracted: &Path, output: &Path) -> Result<()> {
    let (effects, mut files) = cook(extracted, output)?;
    files.push(effects);
    crate::field::refresh_shared(output, &files)
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<(String, Vec<String>)> {
    let mut archive =
        cab::Cabinet::new(Cursor::new(fs::read(extracted.join("files/effect.cab"))?))?;
    let mut bytes = Vec::new();
    archive
        .read_file("EFFECT.TPL")?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "effect atlas exceeds limit"
    );
    let textures = tpl::decode(&bytes)?;
    let mut files = Vec::new();
    for (index, name) in [
        (0, "dust"),
        (1, "emotes"),
        (2, "particles"),
        (5, "refraction"),
    ] {
        let (width, height, rgba) = &textures[index];
        ensure!(
            (*width, *height) == if index == 5 { (64, 64) } else { (256, 256) },
            "unexpected effect atlas size"
        );
        let path = format!("effects/{name}.ktx2");
        fs::create_dir_all(output.join("effects"))?;
        crate::texture::cook(*width, *height, rgba, &output.join(&path))?;
        files.push(path);
    }
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(&executable, address, 4)?.try_into()?,
        ))
    };
    let double = |address| -> Result<f64> {
        Ok(f64::from_be_bytes(
            dol::slice(&executable, address, 8)?.try_into()?,
        ))
    };
    let height = value(0x8035B118)?;
    let right = value(0x8035B0B4)?;
    let ring_z = height + value(0x8035B058)?;
    let mut emotes = BTreeMap::new();
    for kind in [0, 1, 2, 3, 4, 5, 6, 10, 11, 12, 14] {
        let mut frames = Vec::new();
        let (intro_ticks, cycle_ticks, phase_count): (usize, usize, u8) = match kind {
            0 => (64, 1, 16),
            2 => (1, 16, 8),
            3 => (16, 16, 1),
            4 => (80, 1, 16),
            11 => (1, 20, 1),
            _ => (24, 24, 1),
        };
        for age in 0..intro_ticks + cycle_ticks {
            for phase in 0..usize::from(phase_count) {
                let mut sprites = Vec::new();
                let mut emit = |offset, size, uv: [f32; 4], rotation| {
                    sprites.push(Sprite {
                        offset,
                        size,
                        uv: uv.map(|v| v / 256.),
                        rotation,
                        vertical_anchor: VerticalAnchor::Center,
                        alpha: 255,
                    });
                };
                // The first update initializes the controller before drawing.
                if age > 0 && matches!(kind, 0 | 4) {
                    let ring_size = (2 + age * 10).min(80) as f32;
                    emit([right, 0., ring_z], [ring_size; 2], [0., 0., 96., 96.], 0.);
                    // Marks appear after each seeded 16-tick boundary, then stay full.
                    let count =
                        ((age + phase) / 16 - (phase + 1) / 16).min(if kind == 0 { 3 } else { 4 });
                    for mark in 0..count {
                        let (x, z, u, v) = if kind == 0 {
                            (
                                12. + mark as f32 * 12.,
                                height + value(0x8035B11C)?,
                                223.,
                                144.,
                            )
                        } else {
                            (
                                10. + mark as f32 * 12.,
                                height + value(0x8035B124)?,
                                192.,
                                112.,
                            )
                        };
                        emit([x, 0., z], [52.; 2], [u, v, u + 32., v + 32.], 0.);
                    }
                } else if age > 0 && kind == 2 {
                    // Five independently sized/rotated strokes alternate two authored poses.
                    let pose = ((age - 1 + phase) / 8) % 2;
                    for index in 0..5 {
                        let address = 0x801E3938 + ((pose * 5 + index) * 20) as u32;
                        let row = dol::slice(&executable, address, 20)?;
                        let u = f32::from(row[12]);
                        let v = f32::from(row[13]);
                        emit(
                            [
                                value(address)?,
                                value(address + 4)?,
                                value(0x8035B124)? + (height + value(address + 8)?),
                            ],
                            [
                                u16::from_be_bytes([row[16], row[17]]) as f32,
                                i16::from_be_bytes([row[18], row[19]]) as f32,
                            ],
                            [u, v, u + 32., v + 32.],
                            i16::from_be_bytes([row[14], row[15]]) as f32,
                        );
                    }
                } else if age > 0 && kind == 11 {
                    let elapsed = (age - 1) % 20;
                    for (index, (horizontal, vertical)) in [
                        (0x8035B0D8, 0x8035B140),
                        (0x8035B144, 0x8035B148),
                        (0x8035B01C, 0x8035B14C),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let address = 0x801E3A00 + index as u32 * 12;
                        let mut position =
                            [value(address)?, value(address + 4)?, value(address + 8)?];
                        let mut velocity = value(vertical)?;
                        for _ in 0..elapsed {
                            position[0] += value(horizontal)?;
                            position[2] += velocity;
                            // Gravity is a double operation rounded back to a float each tick.
                            velocity = (f64::from(velocity) - double(0x8035B030)?) as f32;
                        }
                        position[2] = position[2] + height + value(0x8035B054)?;
                        // The helper adds six to the controller's requested size of sixteen.
                        emit(
                            position,
                            [22.; 2],
                            [223., 112., 255., 144.],
                            if index == 2 { 105. } else { 135. },
                        );
                    }
                } else if age > 0 && kind == 3 {
                    // Case 3 draws the two-frame sweat glyph without a balloon.
                    let u = ((age / 8) % 2) as f32 * 48.;
                    emit(
                        [value(0x8035B0B8)?, 0., ring_z - 2. * value(0x8035B0E4)?],
                        [80.; 2],
                        [u, 176., u + 48., 224.],
                        0.,
                    );
                } else if age > 0 && kind == 5 {
                    // Exasperation uses a separate atlas cell and a 12-step pop/settle sequence.
                    let ring_size = (2 + age * 10).min(80) as f32;
                    emit(
                        [right, 0., ring_z],
                        [ring_size; 2],
                        [96., 0., 192., 96.],
                        0.,
                    );
                    let row = dol::slice(
                        &executable,
                        0x801E37E0 + 0x244 + ((age - 1).min(11) * 2) as u32,
                        2,
                    )?;
                    let size = u16::from_be_bytes(row.try_into()?) as f32;
                    emit(
                        [right, 0., height + value(0x8035B0E4)?],
                        [size; 2],
                        [96., 144., 128., 176.],
                        0.,
                    );
                } else if age > 0 && kind != 14 {
                    let ring_size = (2 + age * 10).min(80) as f32;
                    emit([right, 0., ring_z], [ring_size; 2], [0., 0., 96., 96.], 0.);
                    // Move the balloon from −15 toward 52 at six units per update.
                    let size = (-15 + age as i32 * 6).min(52);
                    if size >= 2 {
                        let frame = if matches!(kind, 6 | 10) {
                            0.
                        } else {
                            ((age / 8) % 3) as f32
                        };
                        let (x, z, u, v) = if kind == 1 {
                            (value(0x8035B120)?, height + value(0x8035B11C)?, 96., 112.)
                        } else if kind == 6 {
                            // The hole interaction uses a static question glyph;
                            // it shares the balloon and glyph growth with surprise.
                            (value(0x8035B120)?, height + value(0x8035B124)?, 64., 144.)
                        } else if kind == 10 {
                            // Once full-sized, the top-anchored glyph drops four units per tick.
                            let drop = (age.saturating_sub(12) as f64 * double(0x8035B138)?)
                                .min(f64::from(value(0x8035B078)?))
                                as f32;
                            (
                                value(0x8035B12C)?,
                                value(0x8035B120)?
                                    + (value(0x8035B11C)? + height)
                                    + value(0x8035B130)?
                                    - drop,
                                0.,
                                144.,
                            )
                        } else {
                            // The sleep glyph is four units ABOVE the balloon
                            // center (paired Dolphin sprite positions 210/214).
                            // The flattened C's reused height temporary loses
                            // the preceding 30-unit ring offset in this case.
                            (right, ring_z + value(0x8035AFD4)?, 128., 144.)
                        };
                        emit(
                            [x, 0., z],
                            [size as f32; 2],
                            [u + frame * 32., v, u + frame * 32. + 32., v + 32.],
                            0.,
                        );
                    }
                } else if age > 0 {
                    // Four rotating strokes form the emphasis emote.
                    let radius = (value(0x8035B078)? + (age - 1) as f32 * value(0x8035B170)?)
                        .min(value(0x8035B058)?);
                    for i in 0..4 {
                        let angle = value(0x8035B168)? + i as f32 * value(0x8035B16C)?;
                        emit(
                            [
                                // The original SDK calls are sin (80129AD4)
                                // for horizontal displacement and cos (8012928C)
                                // for height. Swapping them buries the strokes
                                // behind the character's head.
                                radius * (-angle).to_radians().sin(),
                                0.,
                                radius * angle.to_radians().cos() + height + value(0x8035B054)?
                                    - value(0x8035B074)?,
                            ],
                            [12., 30.],
                            [192., 176., 224., 208.],
                            angle,
                        );
                    }
                }
                match kind {
                    2 => sprites
                        .iter_mut()
                        .for_each(|s| s.vertical_anchor = VerticalAnchor::Bottom),
                    4 | 5 | 6 | 10 => sprites.iter_mut().skip(1).for_each(|s| {
                        s.vertical_anchor = if kind == 10 {
                            VerticalAnchor::Top
                        } else {
                            VerticalAnchor::Bottom
                        };
                    }),
                    11 if age > 0 => {
                        let remaining = 20 - (age - 1) % 20;
                        let alpha = (value(0x8035AFD0)?
                            - value(0x8035B150)? * (value(0x8035B054)? - remaining as f32).max(0.))
                            as u8;
                        sprites.iter_mut().for_each(|s| s.alpha = alpha);
                    }
                    _ => {}
                }
                frames.push(sprites);
            }
        }
        let cycle = frames.split_off(intro_ticks * usize::from(phase_count));
        emotes.insert(
            kind,
            EmoteTrack {
                anchor: "Bone_atama".into(),
                phase_count,
                intro: frames,
                cycle,
            },
        );
    }
    let sprite = |address,
                  texture: &str,
                  additive,
                  inset: u8|
     -> Result<resonance_content::effect::SpriteRecipe> {
        let row = dol::slice(&executable, address, 12)?;
        Ok(resonance_content::effect::SpriteRecipe {
            texture: texture.into(),
            uv: [
                row[4],
                row[5],
                row[4] + row[0] - inset,
                row[5] + row[1] - inset,
            ]
            .map(|v| f32::from(v) / 256.),
            additive,
        })
    };
    let effects = FieldEffects {
        version: 3,
        emote_texture: files[1].clone(),
        status_texture: "ui/system-0.ktx2".into(),
        paralysis: EmoteTrack {
            anchor: dol::text(&executable, 0x8017A498)?,
            phase_count: 1,
            intro: Vec::new(),
            cycle: [16., 0.]
                .into_iter()
                .map(|y| {
                    Ok(vec![Sprite {
                        offset: [0., 0., value(0x8035AFD8)?],
                        size: [72., 24.],
                        uv: [137., y, 184., y + 15.].map(|v| v / 256.),
                        rotation: 0.,
                        vertical_anchor: VerticalAnchor::Center,
                        alpha: 255,
                    }])
                })
                .collect::<Result<_>>()?,
        },
        sprites: [
            (0, sprite(0x8020A43C, &files[0], false, 0)?),
            (8, sprite(0x8020A4D8, &files[2], true, 1)?),
            (10, sprite(0x8020A4E4, &files[2], true, 1)?),
        ]
        .into(),
        refraction: resonance_content::effect::RefractionRecipe {
            sprite: sprite(0x8020A778, &files[3], false, 1)?,
            displacement: [value(0x801E3828)? * 2., value(0x801E3838)? * 2.],
        },
        emotes,
        // Each mouth frame lasts duration + 1 updates; 0xFD loops the sequence.
        // The dialogue player enables the sequence during text reveal and speech.
        mouth_cycle: {
            let table = dol::slice(&executable, 0x801E3854, 12)?;
            ensure!(table[8] == 0xFD, "unexpected mouth cycle terminator");
            let mut frames = Vec::new();
            for row in table[..8].chunks_exact(4) {
                let ticks = usize::from(u16::from_be_bytes([row[2], row[3]])) + 1;
                ensure!(ticks <= 120 && row[0] < 8, "invalid mouth cycle frame");
                frames.extend(std::iter::repeat_n(row[0], ticks));
            }
            frames
        },
    };
    effects.validate()?;
    let path = "effects/field.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&effects)?)?;
    Ok((path.into(), files))
}

pub(crate) fn particles(
    extracted: &Path,
) -> Result<BTreeMap<i32, resonance_content::effect::FlutterRecipe>> {
    use resonance_content::effect::FlutterRecipe;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let float = |at| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(&executable, at, 4)?.try_into()?,
        ))
    };
    let row = dol::slice(&executable, 0x8020A584, 12)?;
    ensure!(
        row == [63, 63, 0, 1, 192, 192, 0, 60, 0, 0, 255, 255],
        "unexpected flutter atlas recipe"
    );
    let recipe = FlutterRecipe {
        texture: "effects/particles.ktx2".into(),
        uv: [row[4], row[5], row[4] + row[0] - 1, row[5] + row[1] - 1].map(|v| f32::from(v) / 256.),
        aspect_ratio: float(0x8035C1C0)?,
        palette: dol::slice(&executable, 0x8020A240, 0x1B8)?
            .chunks_exact(4)
            .map(|color| color.try_into().unwrap())
            .collect(),
        fall_speed: f64::from_be_bytes(dol::slice(&executable, 0x8035C1C8, 8)?.try_into()?) as f32
            * float(0x8035C224)?,
        fall_variation: float(0x8035C224)? / float(0x8035C220)?,
        spin: f64::from_be_bytes(dol::slice(&executable, 0x8035C228, 8)?.try_into()?) as f32,
    };
    recipe.validate()?;
    Ok([(25, recipe)].into())
}

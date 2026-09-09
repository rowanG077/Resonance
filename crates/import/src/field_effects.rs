//! Cook the original emote recipes and dust atlas into ordinary sprite tracks.
use crate::{dol, tpl, write_atomic};
use anyhow::{Result, ensure};
use resonance_content::effect::{EmoteTrack, FieldEffects, Sprite};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::Path,
};

pub(crate) fn cook(extracted: &Path, output: &Path, ktx: &Path) -> Result<(String, Vec<String>)> {
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
    // The effect atlas stores dust in image 0 and emotes in image 1.
    for (index, name) in [(0, "dust"), (1, "emotes")] {
        let (width, height, rgba) = &textures[index];
        ensure!(
            (*width, *height) == (256, 256),
            "unexpected effect atlas size"
        );
        let png = output.join(format!("intermediate/effects/{name}.png"));
        fs::create_dir_all(png.parent().unwrap())?;
        image::save_buffer(&png, rgba, *width, *height, image::ColorType::Rgba8)?;
        let path = format!("effects/{name}.ktx2");
        fs::create_dir_all(output.join("effects"))?;
        crate::texture::cook(ktx, &png, &output.join(&path))?;
        files.push(path);
    }
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(&executable, address, 4)?.try_into()?,
        ))
    };
    let height = value(0x8035B118)?;
    let right = value(0x8035B0B4)?;
    let ring_z = height + value(0x8035B058)?;
    let mut emotes = BTreeMap::new();
    for kind in [1, 3, 5, 12, 14] {
        let mut frames = Vec::new();
        let cycle_start = if kind == 3 { 16 } else { 24 };
        for age in 0..cycle_start * 2 {
            let mut sprites = Vec::new();
            let mut emit = |offset, size, uv: [f32; 4], rotation| {
                sprites.push(Sprite {
                    offset,
                    size,
                    uv: uv.map(|v| v / 256.),
                    rotation,
                });
            };
            // The first update initializes the controller before drawing.
            if age > 0 && kind == 3 {
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
                    let frame = ((age / 8) % 3) as f32;
                    let (x, z, u, v) = if kind == 1 {
                        (value(0x8035B120)?, height + value(0x8035B11C)?, 96., 112.)
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
            frames.push(sprites);
        }
        let cycle = frames.split_off(cycle_start);
        emotes.insert(
            kind,
            EmoteTrack {
                anchor: "Bone_atama".into(),
                intro: frames,
                cycle,
            },
        );
    }
    // Recipe zero uses the dust billboard atlas sequence.
    let row = dol::slice(&executable, 0x8020A43C, 12)?;
    let dust_uv = [row[4], row[5], row[4] + row[0], row[5] + row[1]].map(|v| f32::from(v) / 256.);
    let effects = FieldEffects {
        version: 1,
        dust_texture: files[0].clone(),
        emote_texture: files[1].clone(),
        dust_uv,
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

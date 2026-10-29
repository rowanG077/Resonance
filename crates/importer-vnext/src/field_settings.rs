//! Shared field effects from the executable's small sprite tables and native TPLs.
use crate::{compression, dol, menu_sprites, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    effect::{
        self, BlinkCycle, FieldEffects, FlutterRecipe, RefractionRecipe, SpriteFrame, SpriteRecipe,
    },
    field::{
        ContactShadow, EnvironmentDefaults,
        settings::{SavePointText, Settings},
    },
    font::{SystemText, TextSpan},
};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

const IMAGE_ROOT: &str = "effects/field/textures";
const ATLAS_TABLE: u32 = 0x8020_A414;

fn path(role: &str) -> String {
    format!("{IMAGE_ROOT}/{role}.ktx2")
}

fn declared(exe: &[u8], address: u32) -> Result<String> {
    let name = text::string(exe, address)?;
    let name = name.trim_start_matches('/').trim_start_matches("./");
    ensure!(
        !name.contains("..") && !name.contains('\\'),
        "invalid native field bank"
    );
    Ok(name.to_owned())
}

fn source(extracted: &Path, exe: &[u8], address: u32) -> Result<Vec<u8>> {
    let name = declared(exe, address)?;
    let bytes = fs::read(extracted.join("files").join(&name))
        .with_context(|| format!("read native field bank {name}"))?;
    if bytes.starts_with(b"MSCF") {
        compression::cabinet(&bytes)
    } else {
        Ok(bytes)
    }
}

fn image(tpl: &[u8], index: usize) -> Result<Vec<u8>> {
    let (width, height, rgba) = menu_sprites::decode_image(tpl, index)?;
    Ok(resonance_asset_writer::ktx2::encode_rgba8(
        width, height, &rgba,
    )?)
}

fn toon_image(tpl: &[u8]) -> Result<Vec<u8>> {
    let mut tpl = tpl.to_vec();
    let table = dol::u32(&tpl, 8)? as usize;
    let descriptor = dol::u32(&tpl, table)? as usize;
    ensure!(
        dol::u16(&tpl, descriptor)? == 32
            && dol::u16(&tpl, descriptor + 2)? == 256
            && dol::u32(&tpl, descriptor + 4)? == 8,
        "unsupported toon ramp"
    );
    // The original ramp indexes an intentionally synthetic three-color palette.
    let colors = tpl.len();
    for i in 0..16 {
        tpl.extend_from_slice(
            &(match i {
                14 => 0xFC00u16,
                15 => 0x83E0,
                _ => 0x801F,
            })
            .to_be_bytes(),
        );
    }
    let header = tpl.len();
    tpl.extend_from_slice(&16u16.to_be_bytes());
    tpl.extend_from_slice(&[0, 0]);
    tpl.extend_from_slice(&2u32.to_be_bytes());
    tpl.extend_from_slice(&(colors as u32).to_be_bytes());
    tpl[table + 4..table + 8].copy_from_slice(&(header as u32).to_be_bytes());
    image(&tpl, 0)
}

fn float(exe: &[u8], address: u32) -> Result<f32> {
    Ok(f32::from_be_bytes(dol::slice(exe, address, 4)?.try_into()?))
}

fn palette(exe: &[u8]) -> Result<Vec<[u8; 4]>> {
    Ok(
        dol::slice(exe, 0x8020_A240, effect::FIELD_PALETTE_COLORS * 4)?
            .chunks_exact(4)
            .map(|row| row.try_into().unwrap())
            .collect(),
    )
}

fn frames(exe: &[u8], address: u32, size: usize, limit: u8) -> Result<Vec<u8>> {
    let table = dol::slice(exe, address, size)?;
    let mut result = Vec::new();
    for row in table.chunks_exact(4) {
        if row[0] == 0xFD {
            break;
        }
        let ticks = usize::from(u16::from_be_bytes([row[2], row[3]])) + 1;
        ensure!(
            row[0] < limit && row[1] == 0 && ticks <= 1024,
            "invalid field frame cycle"
        );
        result.extend(std::iter::repeat_n(row[0], ticks));
    }
    Ok(result)
}

fn sprite_offset(id: u16) -> Result<(usize, bool)> {
    Ok(match id {
        0 => (0x028, false),
        1 => (0x000, false),
        4 => (0x0a0, true),
        5 | 6 | 41 => (0x0b8, true),
        7 | 8 => (0x0c4, true),
        9 => (0x358, false),
        10 => (0x0d0, true),
        11 => (0x0dc, true),
        12 => (0x0e8, true),
        13 => (0x0f4, true),
        14 => (0x100, true),
        18 => (0x118, true),
        21 => (0x14c, true),
        22 => (0x158, true),
        23 => (0x164, true),
        25 => (0x170, false),
        27 => (0x364, false),
        42 => (0x1dc, true),
        44 => (0x220, true),
        52 => (0x04c, false),
        53 => (0x058, false),
        54 => (0x064, false),
        68 => (0x214, true),
        69 => (0x244, true),
        70 => (0x1ac, true),
        74 => (0x28c, true),
        75 => (0x250, true),
        76 => (0x25c, true),
        77 => (0x268, true),
        78 => (0x274, true),
        80 => (0x298, true),
        82 => (0x2b0, true),
        95 => (0x34c, true),
        _ => anyhow::bail!("unknown field sprite {id}"),
    })
}

fn sprite(exe: &[u8], id: u16) -> Result<SpriteRecipe> {
    let (offset, additive) = sprite_offset(id)?;
    let table = dol::slice(exe, ATLAS_TABLE, 0x3f8)?;
    let rows = table
        .get(offset..)
        .context("field sprite offset exceeds table")?;
    let header = rows.get(..4).context("field sprite lacks header")?;
    let texture = match i16::from_be_bytes([header[2], header[3]]) {
        0 => 0,
        1 => 2,
        3 => 4,
        4 => 3,
        5 => 5,
        6 => 6,
        15 => 7,
        _ => anyhow::bail!("unsupported field sprite atlas"),
    };
    let mut frames = Vec::new();
    let mut repeat = false;
    for row in rows[4..].chunks_exact(4) {
        let duration = i16::from_be_bytes([row[2], row[3]]);
        if !frames.is_empty() && matches!(duration, -1 | 0) {
            repeat = duration == -1;
            break;
        }
        let uv = [
            row[0],
            row[1],
            row[0].wrapping_add(header[0].wrapping_sub(1)),
            row[1].wrapping_add(header[1].wrapping_sub(1)),
        ]
        .map(|v| f32::from(v) / 256.);
        frames.push(SpriteFrame {
            uv,
            ticks: u16::try_from(i32::from(duration) + 1).unwrap_or(0),
        });
    }
    let uv = frames.first().context("field sprite lacks frames")?.uv;
    if frames.len() == 1 {
        frames.clear();
    }
    let name = match texture {
        0 => "particles",
        1 => "emotes",
        2 => "field-effects",
        3 => "air-refraction",
        4 => "sparks-and-rays",
        5 => "refraction",
        6 => "heat-haze",
        7 => "lightning-and-flames",
        _ => unreachable!(),
    };
    let repeat = repeat && !frames.is_empty();
    let sprite = SpriteRecipe {
        texture: path(name),
        uv,
        additive,
        frames,
        repeat,
    };
    sprite.validate()?;
    Ok(sprite)
}

fn system_text(bytes: &[u8]) -> Result<SystemText> {
    let mut end = bytes
        .iter()
        .position(|&v| v == 0)
        .context("unterminated field notice")?;
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    let mut spans = Vec::new();
    let (mut start, mut at, mut color) = (0, 0, 0);
    while at <= end {
        if at == end || bytes[at] == 3 {
            if start < at {
                let (value, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[start..at]);
                ensure!(!invalid, "invalid field notice text");
                spans.push(TextSpan {
                    text: value.into_owned(),
                    color,
                });
            }
            if at == end {
                break;
            }
            ensure!(at + 1 < end, "truncated field notice color");
            color = match bytes[at + 1] {
                b'9' => 0,
                x @ 1..=6 => x,
                _ => anyhow::bail!("unknown field notice color"),
            };
            at += 2;
            start = at;
        } else {
            at += 1;
        }
    }
    Ok(SystemText(spans))
}

fn save_point(exe: &[u8]) -> Result<SavePointText> {
    let tutorial = system_text(dol::slice(exe, 0x8017A274, 256)?)?;
    let unlock = system_text(dol::slice(exe, 0x8017A31A, 32)?)?;
    let mut missing = dol::slice(exe, 0x8017A3DC + 0x1C, 32)?.to_vec();
    for i in 1..missing.len() {
        if missing[i - 1] == 3 && missing[i] == b'8' {
            missing[i] = b'9';
        }
    }
    let mut no_gem = system_text(&missing)?;
    let item = dol::slice(exe, 0x801f_ad98 + 491 * 60, 60)?;
    let item_name = text::string(exe, dol::u32(item, 0)?)?;
    for span in &mut no_gem.0 {
        span.text = span.text.replace("%s", &item_name);
    }
    Ok(SavePointText {
        tutorial,
        unlock,
        no_gem,
    })
}

fn environment() -> Result<EnvironmentDefaults> {
    Ok(serde_json::from_value(json!({
        "drifting":{"sprite":{"texture":path("dust"),"uv":[0.0,0.0,255.0/256.0,255.0/256.0]}},
        "fire":{"sprite":{"additive":true,"texture":path("particles"),"uv":[0.0,0.25,0.25,0.5]}},
        "heat_haze":{"alpha":192,"rotation_degrees":95.0,"scale":0.8,"scroll_per_tick":-1.0/1600.0,
            "sprite":{"texture":path("heat-haze"),"uv":[0.0,0.0,1.0,1.0]}},
        "lights":{"ambient":[50,60,70],"attenuation":[1.0,0.002,0.0],"color":[255,159,159],"target":"scenery"},
        "snow":{"color":[240,240,255,112],"count":4096,"fall_speed_per_tick":[3,18],
            "fog_color":[0,0,31,16],"fog_range":[100.0,10000.0],"horizontal_offsets":[-1023,1024],
            "size":12.0,"spawn_height":800.0,"sprite":{"texture":path("particles"),
            "uv":[0.0,192.0/256.0,15.0/256.0,207.0/256.0]}},
        "wind":{"count":32,"first":[-0.8,0.0,-0.8],"step":[-0.033333335,0.0,-0.033333335]}
    }))?)
}

pub fn cook_field_settings(root: &Path, library: &Path) -> Result<()> {
    let extracted = &root.join("disc1");
    let exe = fs::read(extracted.join("sys/main.dol"))?;
    let effects = source(extracted, &exe, 0x8017a568)?;
    let status = source(extracted, &exe, 0x8017a55c)?;
    let sand = fs::read(extracted.join("files/sand.tpl"))?;
    let toon = source(extracted, &exe, 0x8017cf7c)?;
    for (role, index) in [
        ("particles", 0),
        ("emotes", 1),
        ("field-effects", 2),
        ("air-refraction", 3),
        ("sparks-and-rays", 4),
        ("refraction", 5),
        ("heat-haze", 6),
        ("lightning-and-flames", 7),
    ] {
        crate::output::write(&library.join(&path(role)), &image(&effects, index)?)?;
    }
    crate::output::write(&library.join(&path("status")), &image(&status, 0)?)?;
    crate::output::write(&library.join(&path("dust")), &image(&sand, 0)?)?;
    crate::output::write(&library.join(&path("toon-ramp")), &toon_image(&toon)?)?;
    let palette = palette(&exe)?;
    let mut sprites = BTreeMap::new();
    for id in effect::sprite::all() {
        sprites.insert(id, sprite(&exe, id)?);
    }
    let field_effects = FieldEffects {
        emote_texture: path("emotes"),
        status_texture: path("status"),
        sprites,
        palette: palette.clone(),
        refraction: RefractionRecipe {
            sprite: sprite(&exe, 27)?,
            displacement: [float(&exe, 0x801E3828)? * 2., float(&exe, 0x801E3838)? * 2.],
        },
        air_refraction: sprite(&exe, 9)?,
        mouth_cycle: frames(&exe, 0x801E3854, 12, 8)?,
        rising_light_destination: [
            float(&exe, 0x8018_D214)?,
            float(&exe, 0x8018_D218)?,
            float(&exe, 0x8018_D21C)?,
        ],
    };
    field_effects.validate()?;
    let flutter = sprite(&exe, 25)?;
    let particles = BTreeMap::from([(
        25,
        FlutterRecipe {
            texture: flutter.texture,
            uv: flutter.uv,
            aspect_ratio: float(&exe, 0x8035C1C0)?,
            palette: Vec::new(),
            fall_speed: f64::from_be_bytes(dol::slice(&exe, 0x8035C1C8, 8)?.try_into()?) as f32
                * float(&exe, 0x8035C224)?,
            fall_variation: float(&exe, 0x8035C224)? / float(&exe, 0x8035C220)?,
            spin: f64::from_be_bytes(dol::slice(&exe, 0x8035C228, 8)?.try_into()?) as f32,
        },
    )]);
    let blink = BlinkCycle {
        frames: frames(&exe, 0x801E3840, 20, 16)?,
    };
    blink.validate()?;
    let settings = Settings {
        effects: field_effects,
        blink,
        contact_shadow: ContactShadow {
            texture: path("field-effects"),
            uv_size: [0.25; 2],
            half_size: 42.,
            height_offset: 2.,
            alpha: 64,
            anchor_node: 1,
        },
        toon_ramp: path("toon-ramp"),
        particles,
        save_point: save_point(&exe)?,
        environment: environment()?,
    };
    crate::output::json(&library.join("fields/settings.json"), &settings)
}

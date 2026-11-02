//! Native field atlases and notices in the GC shared-settings document.
use crate::{output, package::Package, pe, source::Source, text::Bank, texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    effect::{
        self, BlinkCycle, FieldEffects, FlutterRecipe, RefractionRecipe, SpriteFrame, SpriteRecipe,
    },
    field::{
        ContactShadow,
        settings::{SavePointText, Settings},
    },
    font::{SystemText, TextSpan},
};
use std::{collections::BTreeMap, path::Path};
fn path(role: &str) -> String {
    format!("effects/field/textures/{role}.ktx2")
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

fn sprite(image: &pe::Image<'_>, id: u16) -> Result<SpriteRecipe> {
    let (offset, additive) = sprite_offset(id)?;
    let table = image.slice(0x8e060c, 0x3f8)?;
    let rows = table
        .get(offset..)
        .context("field sprite offset exceeds table")?;
    let header = rows.get(..4).context("field sprite lacks header")?;
    let texture = match i16::from_le_bytes([header[2], header[3]]) {
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
        let duration = i16::from_le_bytes([row[2], row[3]]);
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
            ticks: u16::try_from(duration)
                .map(|n| (n / 2 + 1) * 2)
                .unwrap_or(0),
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

fn face_cycle(bytes: &[u8], limit: u8) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 8 && bytes.len().is_multiple_of(4),
        "invalid face cycle"
    );
    let end = bytes.len() - 4;
    ensure!(
        bytes[end..] == [0xfd, 0, 1, 0],
        "invalid face cycle terminator"
    );
    let mut frames = Vec::new();
    for row in bytes[..end].chunks_exact(4) {
        let duration = u16::from_le_bytes([row[2], row[3]]);
        ensure!(
            row[0] < limit && row[1] == 0 && duration <= 1022,
            "invalid face frame"
        );
        frames.extend(std::iter::repeat_n(
            row[0],
            (usize::from(duration) / 2 + 1) * 2,
        ));
        ensure!(frames.len() <= 1024, "face cycle exceeds frame budget");
    }
    Ok(frames)
}

fn save_point(source: &Source, image: &pe::Image<'_>) -> Result<SavePointText> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let slot = source.language.slot();
    let text = |table| -> Result<SystemText> {
        let table = pe::word(image.slice(table + slot * 4, 4)?, 0)?;
        Ok(SystemText(
            bank.system(pe::word(image.slice(table, 4)?, 0)?)?,
        ))
    };
    let tutorial = text(0x8d7e10)?;
    let unlock = text(0x8d7e40)?;
    let mut no_gem = text(0x8d7e70)?;
    let names = pe::word(image.slice(0x8ced28 + slot * 4, 4)?, 0)?;
    let item = pe::word(image.slice(0x8d73dc, 4)?, 0)?;
    let name = bank.menu(
        pe::word(image.slice(names + item * 4, 4)?, 0)?,
        encoding_rs::WINDOWS_1252,
    )?;
    no_gem.0.extend([
        TextSpan {
            text: " ".into(),
            color: 0,
        },
        TextSpan {
            text: name,
            color: 5,
        },
        TextSpan {
            text: ".".into(),
            color: 0,
        },
    ]);
    Ok(SavePointText {
        tutorial,
        unlock,
        no_gem,
    })
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let effect = source.require("EFFECT.PAC")?;
    let effect = Package::parse(&effect)?;
    for (role, slot) in [
        ("particles", 0),
        ("emotes", 1),
        ("field-effects", 2),
        ("air-refraction", 3),
        ("sparks-and-rays", 4),
        ("refraction", 5),
        ("heat-haze", 6),
        ("lightning-and-flames", 7),
    ] {
        let name = effect.reference(slot)?.context("missing field atlas")?;
        let descriptor = texture::Descriptor::read(&source.require(name)?)?;
        let pixels = descriptor.decode(&source.require(&descriptor.payload)?)?;
        output::write(&library.join(path(role)), &pixels.ktx2()?)?;
    }
    let system = source.require("SYSTEM.PAC")?;
    let system = Package::parse(&system)?;
    for (role, name) in [
        (
            "status",
            system.reference(0)?.context("missing status atlas")?,
        ),
        ("dust", "SAND_PIC000.TOTEXB_D"),
    ] {
        let descriptor = texture::Descriptor::read(&source.require(name)?)?;
        let pixels = descriptor.decode(&source.require(&descriptor.payload)?)?;
        output::write(&library.join(path(role)), &pixels.ktx2()?)?;
    }
    // This is the shared GC two-band mask, not a Steam shader reconstruction.
    let pixels = (0..32)
        .flat_map(|_| {
            (0..256).flat_map(|x| {
                if x < 84 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                }
            })
        })
        .collect::<Vec<u8>>();
    output::write(
        &library.join(path("toon-ramp")),
        &resonance_asset_writer::ktx2::encode_rgba8(256, 32, &pixels)?,
    )?;
    let palette = image
        .slice(0x8e0438, effect::FIELD_PALETTE_COLORS * 4)?
        .chunks_exact(4)
        .map(|row| row.try_into().unwrap())
        .collect::<Vec<[u8; 4]>>();
    let double = |at| -> Result<f32> {
        let value = f64::from_le_bytes(image.slice(at, 8)?.try_into()?) as f32;
        ensure!(value.is_finite(), "invalid field effect constant");
        Ok(value)
    };
    let effects = FieldEffects {
        emote_texture: path("emotes"),
        status_texture: path("status"),
        palette,
        sprites: effect::sprite::all()
            .map(|id| Ok((id, sprite(&image, id)?)))
            .collect::<Result<_>>()?,
        refraction: RefractionRecipe {
            sprite: sprite(&image, 27)?,
            displacement: [1.; 2],
        },
        air_refraction: sprite(&image, 9)?,
        mouth_cycle: face_cycle(image.slice(0x8cee58, 12)?, 8)?,
        rising_light_destination: [0., double(0x882f90)?, double(0x882f88)?],
    };
    effects.validate()?;
    let flutter = sprite(&image, 25)?;
    let particles = BTreeMap::from([(
        25,
        FlutterRecipe {
            texture: flutter.texture,
            uv: flutter.uv,
            aspect_ratio: 3.,
            palette: Vec::new(),
            fall_speed: double(0x882fe0)? * 2.,
            fall_variation: 2. / double(0x882968)?,
            spin: double(0x882c30)?,
        },
    )]);
    let blink = BlinkCycle {
        frames: face_cycle(image.slice(0x8cee3c, 20)?, 16)?,
    };
    blink.validate()?;
    let contact_shadow = ContactShadow {
        texture: path("field-effects"),
        uv_size: [0.25; 2],
        half_size: 42.,
        height_offset: 2.,
        alpha: 64,
        anchor_node: 1,
    };
    contact_shadow.validate()?;
    let settings = Settings {
        effects,
        blink,
        contact_shadow,
        toon_ramp: path("toon-ramp"),
        particles,
        save_point: save_point(source, &image)?,
        environment: resonance_importer::field_environment_defaults()?,
    };
    for recipe in settings.resolve_particles(None).values() {
        recipe.validate()?;
    }
    output::json(
        &library.join(resonance_content::field::settings::PATH),
        &settings,
    )
}

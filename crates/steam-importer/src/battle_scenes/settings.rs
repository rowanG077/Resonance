use super::*;
mod paths;
mod patterns;
mod unison_points;
#[derive(Deserialize)]
struct Style {
    stage_color: [u8; 3],
    camera: battle_scene::CameraBounds,
}
pub(super) fn bind(image: &pe::Image<'_>, id: u16, rules: &mut Definition) -> Result<()> {
    let styles: BTreeMap<u16, Style> = serde_json::from_str(include_str!("style.json"))?;
    if let Some(style) = styles.get(&id) {
        rules.stage_color = Some(style.stage_color);
        rules.camera = Some(style.camera);
    }
    if id == 271 {
        let entry = pe::word(image.slice(0x8e9b08 + 271 * 4, 4)?, 0)?;
        ensure!(
            pe::word(image.slice(entry, 4)?, 0)? == 0x418f60
                && pe::word(image.slice(0x41901e, 4)?, 0)? == 0x419030,
            "Grand Cross handlers changed"
        );
        for (address, bytes) in [
            (0x418f8b, &[0xb8, 0xe6, 0, 0, 0][..]),
            (0x41903f, &[0x66, 0x83, 0x7f, 0x36, 0x0a]),
            (0x419099, &[0x8d, 0x81, 0x90, 0x01, 0, 0]),
            (0x4190c0, &[0x66, 0x83, 0x7f, 0x36, 0x5a]),
            (0x41910a, &[0x8d, 0x81, 0x20, 0x03, 0, 0]),
            (0x41911f, &[0x83, 0xc1, 0x1c]),
            (0x419134, &[0xba, 0x82, 0, 0, 0]),
            (0x419183, &[0x8d, 0x81, 0xb0, 0x04, 0, 0]),
            (0x419198, &[0x83, 0xc1, 0x38]),
        ] {
            ensure!(
                image.slice(address, bytes.len())? == bytes,
                "Grand Cross callback changed at {address:#x}"
            );
        }
        // This added angel spell uses the common GameCube Judgment framing.
        let styles: BTreeMap<u16, Style> = serde_json::from_str(include_str!("style.json"))?;
        let style = styles
            .get(&269)
            .context("missing shared angel-spell style")?;
        rules.stage_color = Some(style.stage_color);
        rules.camera = Some(style.camera);
        rules.duration = 230;
    }
    if id == 281 {
        for (address, expected) in [
            (0x425c8e, &[0xb8, 0x19, 0x01, 0, 0][..]),
            (0x40f805, &[0xb8, 0x72, 0x01, 0, 0][..]),
            (0x40fe3e, &[0x66, 0x83, 0xf8, 0x46][..]),
            (0x40fe48, &[0xb9, 0xc6, 0, 0, 0][..]),
        ] {
            ensure!(
                image.slice(address, expected.len())? == expected,
                "Divine Judgment consumer changed at {address:#x}"
            );
        }
        // Use the shared GameCube Judgment framing for the added angel finisher.
        let styles: BTreeMap<u16, Style> = serde_json::from_str(include_str!("style.json"))?;
        let style = styles.get(&269).context("missing shared Judgment style")?;
        rules.stage_color = Some(style.stage_color);
        rules.camera = Some(style.camera);
        rules.duration = 370;
    }
    if id == 279 {
        for (address, expected) in [
            (0x425b6d, &[0xba, 0x17, 0x01, 0, 0][..]),
            (0x419ac5, &[0xb8, 0x68, 0x01, 0, 0][..]),
            (0x41a048, &[0x6a, 0x4b][..]),
            (0x41a248, &[0x8b, 0x44, 0x24, 0x18][..]),
            (0x419f7f, &[0xb9, 0xb4, 0, 0, 0][..]),
            (0x41a24c, &[0xb9, 0xbe, 0, 0, 0][..]),
        ] {
            ensure!(
                image.slice(address, expected.len())? == expected,
                "Fairy Circle consumer changed at {address:#x}"
            );
        }
        // Steam adds this scene. Frame it with the common GameCube healing
        // scene profile used by its ordinary input, Revitalize.
        let styles: BTreeMap<u16, Style> = serde_json::from_str(include_str!("style.json"))?;
        let style = styles.get(&260).context("missing shared healing style")?;
        rules.stage_color = Some(style.stage_color);
        rules.camera = Some(style.camera);
        rules.duration = 360;
    }
    if matches!(id, 297 | 298) {
        // The added variants retain their own packages but share Indignation
        // Judgment's native handler. Use its common GameCube-style scene bounds
        // and stage tint so the shared spell lifecycle can prepare either alias.
        let dispatch = |scene: u32| pe::word(image.slice(0x8e9b08 + scene * 4, 4)?, 0);
        ensure!(
            dispatch(u32::from(id))? == dispatch(280)?,
            "Steam finisher handler changed"
        );
        let styles: BTreeMap<u16, Style> = serde_json::from_str(include_str!("style.json"))?;
        let style = styles.get(&280).context("missing shared finisher style")?;
        rules.stage_color = Some(style.stage_color);
        rules.camera = Some(style.camera);
    }
    rules.strike_points = if id >= 300 {
        unison_points::read(image, id)?
    } else {
        strike_points(image, id)?
    };
    rules.spell_recipe = patterns::read(image, id, &rules.strike_points)?;
    rules.effect_paths = paths::read(image, id, rules.duration)?;
    // Each callback compares the recipient's distance from the scene origin.
    // Purify: 0x4143fe; Healing Stream/Wind/Circle: 0x416869/0x416a99/0x416e79.
    rules.recovery_radius = match id {
        240 => Some(0x882f88),
        255 | 256 | 258 => Some(0x882e08),
        _ => None,
    }
    .map(|address| -> Result<_> {
        let radius = f64::from_le_bytes(image.slice(address, 8)?.try_into()?) as f32;
        ensure!(
            radius.is_finite() && radius > 0.,
            "invalid Steam recovery radius"
        );
        Ok(radius)
    })
    .transpose()?;
    Ok(())
}

fn strike_points(image: &pe::Image<'_>, id: u16) -> Result<Vec<[f32; 3]>> {
    // Callback 0x40fa50..0x40fcb3 initializes sixteen native Z/Y/X vectors;
    // 0x40fe3a..0x40ffbc rotates and advances them at eight-tick intervals.
    const DIVINE_JUDGMENT: [[u32; 3]; 16] = [
        [0, 0, 0x8830a0],
        [0, 0, 0x883178],
        [0x8830a0, 0, 0],
        [0x883178, 0, 0],
        [0x8ad124, 0, 0x8ad124],
        [0x8ad124, 0, 0x883130],
        [0x883130, 0, 0x8ad124],
        [0x883130, 0, 0x883130],
        [0, 0, 0x72f400],
        [0, 0, 0x883d48],
        [0x72f400, 0, 0],
        [0x883d48, 0, 0],
        [0x8ad124, 0, 0x8ad124],
        [0x8ad124, 0, 0x883130],
        [0x883130, 0, 0x8ad124],
        [0x883130, 0, 0x883130],
    ];
    // Judgment's 0x418390 and Holy Judgment's 0x41eb90 initialize the same
    // seventeen vectors on the stack. The consumers at 0x4185b7/0x41ef8a
    // advance once every six updates. Zero denotes an x87 fldz literal.
    const POINTS: [[u32; 3]; 17] = [
        [0, 0, 0x88396c],
        [0x8833f0, 0, 0x8833f0],
        [0x88396c, 0, 0x883f0c],
        [0x8833f0, 0, 0x883f24],
        [0, 0, 0x7a36d4],
        [0x883f24, 0, 0x883f24],
        [0x7a36d4, 0, 0],
        [0x883f24, 0, 0x8833f0],
        [0, 0, 0],
        [0, 0, 0x883f20],
        [0x883f1c, 0, 0x883f1c],
        [0x883f20, 0, 0x883f0c],
        [0x883f1c, 0, 0x88377c],
        [0, 0, 0x883f18],
        [0x88377c, 0, 0x88377c],
        [0x883f18, 0, 0],
        [0x88377c, 0, 0x883f1c],
    ];
    // Maxwell's callback at 0x41ddb0 initializes fourteen vectors at
    // 0x41ddca..0x41deef, then indexes them from 0x41e0a6. This is not the
    // Meteor Storm array: several axes differ even though both use 14 points.
    const MAXWELL: [[u32; 3]; 14] = [
        [0, 0, 0],
        [0x8838a4, 0, 0x7a36d4],
        [0x8834a0, 0, 0x88396c],
        [0, 0, 0x7a36d4],
        [0x7a36d4, 0, 0x8834a0],
        [0x8834a0, 0, 0x7a36d4],
        [0, 0, 0x88396c],
        [0x7a36d4, 0, 0x8838a4],
        [0x88396c, 0, 0x8834a0],
        [0x88396c, 0, 0x8838a4],
        [0x7a36d4, 0, 0],
        [0x88396c, 0, 0],
        [0x8838a4, 0, 0x88396c],
        [0, 0, 0],
    ];
    let points = match id {
        281 => DIVINE_JUDGMENT.as_slice(),
        269 | 290 | 296 => POINTS.as_slice(),
        292 => MAXWELL.as_slice(),
        _ => return Ok(Vec::new()),
    };
    let scalar = |address| -> Result<f32> {
        if address == 0 {
            Ok(0.)
        } else {
            Ok(f32::from_bits(pe::word(image.slice(address, 4)?, 0)?))
        }
    };
    points
        .iter()
        // Native vectors use forward/height/lateral order. The shared scene
        // strategy rotates X/height/Z, just like resident projectile offsets.
        .map(|&[z, y, x]| Ok([scalar(x)?, scalar(y)?, scalar(z)?]))
        .collect()
}

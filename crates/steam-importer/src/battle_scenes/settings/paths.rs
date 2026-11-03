//! Sample native emitter anchors into the common scene coordinate system.
use super::*;
use resonance_content::battle_scene::EffectPath;

pub(super) fn read(image: &pe::Image<'_>, id: u16, duration: u16) -> Result<Vec<EffectPath>> {
    if id != 279 {
        return Ok(Vec::new());
    }
    // Initializer 0x419bee..0x419c8f allocates four anchors, each passed to
    // effect member 3 + slot. Callback 0x419e11..0x419ebb updates them.
    for (address, expected) in [
        (0x419c11, &[0xdc, 0x0d, 0x90, 0x2e, 0x88, 0x00][..]),
        (0x419e52, &[0x66, 0x83, 0xf8, 0x28][..]),
        (0x419e9e, &[0xdc, 0x05, 0x58, 0x31, 0x88, 0x00][..]),
    ] {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "Fairy Circle anchor consumer changed at {address:#x}"
        );
    }
    let scalar = |address| -> Result<f64> {
        let value = f64::from_le_bytes(image.slice(address, 8)?.try_into()?);
        ensure!(value.is_finite(), "non-finite scene anchor scalar");
        Ok(value)
    };
    let spacing = scalar(0x882e90)?;
    let initial = scalar(0x8829a8)?;
    let radius = f32::from_le_bytes(image.slice(0x7d9220, 4)?.try_into()?);
    let radians = scalar(0x882fe8)?;
    let rise_speed = scalar(0x882e18)?;
    let rise_height = scalar(0x882d60)?;
    let base_height = scalar(0x882dd8)?;
    let first_turn = scalar(0x882dc0)?;
    let expand = scalar(0x883158)?;
    let later_turn = scalar(0x882fb0)?;
    (0..4)
        .map(|slot| {
            let mut angle = (initial + f64::from(slot) * spacing) as f32;
            let mut radius = radius;
            let mut height = 0.;
            let mut positions = vec![[0.; 3]];
            for age in 0..duration {
                // Native vectors are Z/Y/X. Retain f32 stores at callback boundaries.
                let phase = (f64::from(angle) * radians) as f32;
                let x = phase.sin() * radius;
                let z = phase.cos() * radius;
                if age < 40 {
                    let phase = (f64::from(age) * rise_speed * radians) as f32;
                    height = (f64::from(phase.sin()) * rise_height + base_height) as f32;
                    angle = (f64::from(angle) + first_turn) as f32;
                } else {
                    radius = (f64::from(radius) + expand) as f32;
                    angle = (f64::from(angle) + later_turn) as f32;
                }
                positions.push([x, height, z]);
            }
            let path = EffectPath {
                member: 3 + slot,
                positions,
            };
            path.validate(duration)?;
            Ok(path)
        })
        .collect()
}

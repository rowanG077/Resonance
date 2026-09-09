//! Compile title light UV motion into ordinary sampled texture animations.
use anyhow::{Context, Result, ensure};
use resonance_content::TextureAnimation;

pub(crate) fn cook(dol: &[u8]) -> Result<Vec<TextureAnimation>> {
    // Bake two stepped vertical scrolls and one continuous horizontal scroll.
    let bytes = crate::dol::slice(dol, 0x8035B354, 0x18)?;
    let value = |at| -> f32 { f32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) };
    bake(value(0), value(4), value(12), value(16), value(20))
}

fn bake(
    steps: f32,
    step_size: f32,
    first: f32,
    speed: f32,
    second: f32,
) -> Result<Vec<TextureAnimation>> {
    let integer = |value: f32| -> Result<usize> {
        ensure!(
            value.is_finite() && (1.0..=1000.0).contains(&value) && value.fract() == 0.,
            "invalid UV animation step count"
        );
        Ok(value as usize)
    };
    let steps = integer(steps)?;
    ensure!(
        step_size.is_finite() && step_size > 0. && step_size <= 1.,
        "invalid UV animation step size"
    );
    ensure!(
        speed.is_finite() && (1.0 / 36000.0..=1.0).contains(&speed),
        "invalid UV animation scroll speed"
    );
    let stepped = |texture, interval| -> Result<TextureAnimation> {
        let interval = integer(interval)?;
        let period = steps.checked_mul(interval).context("UV period overflow")?;
        ensure!(period <= 36000, "UV animation period exceeds limit");
        Ok(TextureAnimation {
            texture,
            delay_ticks: 1,
            loop_start: 0,
            offsets: (0..period)
                .map(|tick| [0., (tick / interval) as f32 * step_size])
                .collect(),
        })
    };
    let mut offsets = vec![[0., 0.]];
    let mut position = 0f32;
    loop {
        // The source accumulates f32 and resets only AFTER uploading the
        // overshooting value. Multiplying time by speed loses that boundary.
        position += speed;
        offsets.push([position, 0.]);
        ensure!(offsets.len() <= 36000, "UV scroll period exceeds limit");
        if position > 1. {
            break;
        }
    }
    Ok(vec![
        stepped(2, first)?,
        TextureAnimation {
            texture: 0,
            delay_ticks: 1,
            loop_start: 1,
            offsets,
        },
        stepped(1, second)?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_light_motion_preserves_wrap_and_saved_state_phase() {
        let tracks = bake(32., 1. / 32., 22., 1. / 120., 7.).unwrap();
        for track in &tracks {
            track.validate().unwrap();
        }
        assert_eq!(tracks[0].offset(22), [0., 0.]);
        assert_eq!(tracks[0].offset(23), [0., 1. / 32.]);
        assert_eq!(tracks[0].offset(705), [0., 0.]);
        assert_eq!(tracks[1].offset(0), [0., 0.]);
        assert_eq!(tracks[1].offset(121)[0], 0.9999994);
        assert!(tracks[1].offset(122)[0] > 1.);
        assert_eq!(tracks[1].offset(123), [1. / 120., 0.]);
        // Independent Dolphin checkpoint, title idle counter 968.
        assert_eq!(tracks[0].offset(968), [0., 11. / 32.]);
        assert_eq!(tracks[1].offset(968), [0.9999994, 0.]);
        assert_eq!(tracks[2].offset(968), [0., 10. / 32.]);
        assert!(bake(0., 1., 22., 0.1, 7.).is_err());
        assert!(bake(32., 1., 22., f32::NAN, 7.).is_err());
        assert!(bake(32., 1., 22.5, 0.1, 7.).is_err());
    }
}

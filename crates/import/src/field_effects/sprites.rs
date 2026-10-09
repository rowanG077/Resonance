//! Decode the atlas sequences used by field effects.
use super::{Atlas, SpriteRecipe, dol};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::effect::SpriteFrame;

pub(super) fn read(executable: &[u8], kind: u16) -> Result<SpriteRecipe<Atlas>> {
    // Atlas offsets and blending belong to the effect's content definition.
    let (offset, additive) = match kind {
        0 => (0x028, false),
        1 => (0x000, false),
        4 => (0x0a0, true),
        5 => (0x0b8, true),
        6 => (0x0b8, true),
        7 => (0x0c4, true),
        8 => (0x0c4, true),
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
        41 => (0x0b8, true),
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
        _ => bail!("unsupported field sprite {kind}"),
    };
    const ATLAS_SEQUENCES: u32 = 0x8020_A414;
    const SEQUENCE_BYTES: usize = 0x3f8;
    decode(
        &dol::slice(executable, ATLAS_SEQUENCES, SEQUENCE_BYTES)?[offset..],
        additive,
    )
}

fn decode(bytes: &[u8], additive: bool) -> Result<SpriteRecipe<Atlas>> {
    let mut rows = bytes.chunks_exact(4);
    let header = rows.next().context("missing sprite header")?;
    let texture = match i16::from_be_bytes([header[2], header[3]]) {
        0 => 0,
        1 => 2,
        3 => 4,
        4 => 3,
        5 => 5,
        6 => 6,
        15 => 7,
        _ => bail!("sprite requires an unsupported texture binding"),
    };
    let mut frames = Vec::new();
    let repeat = loop {
        let row = rows.next().context("unterminated sprite sequence")?;
        let duration = i16::from_be_bytes([row[2], row[3]]);
        if !frames.is_empty() && matches!(duration, -1 | 0) {
            break duration == -1;
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
    };
    let uv = frames[0].uv;
    if frames.len() == 1 {
        frames.clear();
    } else {
        ensure!(
            frames.iter().all(|f| f.ticks > 0),
            "invalid sprite frame duration"
        );
    }
    Ok(SpriteRecipe {
        texture: Atlas::Effect(texture),
        uv,
        additive,
        repeat: repeat && !frames.is_empty(),
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_samples_atlas_frames_and_rejects_truncation() -> Result<()> {
        let bytes = [64, 64, 0, 1, 0, 0, 0, 2, 64, 0, 0, 1, 0, 0, 255, 255];
        let sprite = decode(&bytes, true)?;
        assert!(matches!(sprite.texture, Atlas::Effect(2)));
        assert_eq!(sprite.uv_at(2), [0., 0., 63. / 256., 63. / 256.]);
        assert_eq!(sprite.uv_at(3), [64. / 256., 0., 127. / 256., 63. / 256.]);
        assert_eq!(sprite.uv_at(5), sprite.uv_at(0));
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end], true).is_err());
        }
        Ok(())
    }
}

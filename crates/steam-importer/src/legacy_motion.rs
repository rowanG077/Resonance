//! Retained GC curves have little-endian records and original big-endian values.
//! Normalize only the record tables and reuse the GameCube motion converter.
use crate::pe;
use anyhow::{Context, Result, ensure};

pub(crate) fn convert(bytes: &[u8], mesh: &[u8], names: &[String]) -> Result<Vec<u8>> {
    let bindings = names
        .iter()
        .enumerate()
        .map(|(id, name)| Ok((u16::try_from(id)?, name.clone())))
        .collect::<Result<Vec<_>>>()?;
    resonance_importer::convert_motion(&normalize(bytes)?, mesh, &bindings)
}

fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    let bytes = if bytes.get(..4) == Some(&0x007b7960u32.to_le_bytes())
        || bytes.get(..4) == Some(&0x007b7960u32.to_be_bytes())
    {
        bytes.to_vec()
    } else {
        resonance_importer::expand_resource(bytes)?
    };
    if bytes.get(..4) == Some(&0x007b7960u32.to_be_bytes()) {
        return Ok(bytes);
    }
    ensure!(
        pe::word(&bytes, 0)? == 0x007b7960 && pe::word(&bytes, 4)? == 24,
        "unsupported retained motion header"
    );
    let descriptors = usize::from(pe::half(&bytes, 10)?);
    let tracks = usize::from(pe::half(&bytes, 12)?);
    let keys = usize::from(pe::half(&bytes, 14)?);
    let track_start = 24 + descriptors * 12;
    let key_start = track_start + tracks * 16;
    let end = key_start + keys * 12;
    ensure!(
        descriptors > 0 && tracks > 0 && end <= bytes.len(),
        "retained motion tables exceed source"
    );
    let mut result = bytes.clone();
    let mut reverse = |at: usize, width: usize| -> Result<()> {
        result
            .get_mut(at..at + width)
            .context("retained motion record exceeds source")?
            .reverse();
        Ok(())
    };
    for at in [0, 4, 16, 20] {
        reverse(at, 4)?;
    }
    for at in [8, 10, 12, 14] {
        reverse(at, 2)?;
    }
    for at in (24..track_start).step_by(12) {
        let start = pe::word(&bytes, at + 4)? as usize;
        let count = usize::from(pe::half(&bytes, at + 8)?);
        ensure!(
            start >= track_start
                && (start - track_start).is_multiple_of(16)
                && start + count * 16 <= key_start,
            "retained descriptor exceeds track table"
        );
        for offset in [0, 4] {
            reverse(at + offset, 4)?;
        }
        for offset in [8, 10] {
            reverse(at + offset, 2)?;
        }
    }
    for at in (track_start..key_start).step_by(16) {
        let start = pe::word(&bytes, at + 4)? as usize;
        let count = usize::from(pe::half(&bytes, at + 8)?);
        ensure!(
            count > 0
                && start >= key_start
                && (start - key_start).is_multiple_of(12)
                && start + count * 12 <= end,
            "retained track exceeds key table"
        );
        for offset in [0, 4] {
            reverse(at + offset, 4)?;
        }
        for offset in [8, 10] {
            reverse(at + offset, 2)?;
        }
    }
    for at in (key_start..end).step_by(12) {
        for offset in [4, 8] {
            let pointer = pe::word(&bytes, at + offset)? as usize;
            ensure!(
                pointer == 0 || (end..bytes.len()).contains(&pointer),
                "retained key overlaps tables or exceeds source"
            );
        }
        for offset in [0, 4, 8] {
            reverse(at + offset, 4)?;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::{Motion, Skeleton, Transform};
    use serde_json::json;
    #[test]
    fn mixed_records_keep_gc_values_and_bind_to_the_current_glb() -> Result<()> {
        let mut bytes = vec![0; 88];
        for (at, value) in [
            (0, 0x007b7960u32),
            (4, 24),
            (28, 36),
            (36, 10f32.to_bits()),
            (40, 52),
            (52, 0),
            (56, 76),
            (64, 10f32.to_bits()),
            (68, 76),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (at, value) in [(10, 1u16), (12, 1), (14, 2), (32, 1), (44, 2)] {
            bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48..52].copy_from_slice(&[0x40, 1, 1, 1]);
        for (at, value) in [(76, 3f32), (80, 4.), (84, 5.)] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        let mesh = resonance_asset_writer::gltf::pack_glb(
            &json!({"asset":{"version":"2.0"}, "nodes":[{"name":"root","extras":{"resonance_bone":0}}],"scene":0,"scenes":[{"nodes":[0]}]}),
            &[],
        )?;
        let normalized = normalize(&bytes)?;
        assert_eq!(&normalized[76..], &bytes[76..]);
        assert_eq!(normalize(&normalized)?, normalized);
        let skeleton = Skeleton::single("root");
        let motion =
            Motion::decode_for_skeleton(&convert(&bytes, &mesh, &["root".into()])?, &skeleton)?;
        assert_eq!(motion.duration_frames, 10.);
        assert_eq!(
            motion.tracks[0]
                .sample(5., Transform::default())?
                .translation,
            [3., 4., 5.]
        );
        assert!(normalize(&bytes[..60]).is_err());
        bytes[56..60].copy_from_slice(&4u32.to_le_bytes());
        assert!(normalize(&bytes).is_err());
        Ok(())
    }
}

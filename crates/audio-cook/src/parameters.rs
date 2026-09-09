//! Original little-endian envelope tables, converted once during import.
use crate::read;
use anyhow::Result;
use resonance_audio::{dls, envelope};

pub fn ordinary(bytes: &[u8]) -> Result<envelope::Parameters> {
    let bytes = read::slice(bytes, 0, 8)?;
    let half = |at| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    Ok(envelope::Parameters {
        attack_ms: half(0),
        decay_ms: half(2),
        sustain: (u32::from(half(4)) << 3).min(32767) as u16,
        release_ms: half(6),
    })
}

pub fn dls(bytes: &[u8]) -> Result<dls::Definition> {
    let bytes = read::slice(bytes, 0, 20)?;
    let word = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let half = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    Ok(dls::Definition {
        attack_timecents: word(0),
        decay_timecents: word(4),
        sustain_index: half(8) >> 5,
        release_ms: half(10),
        attack_velocity_scale: word(12),
        decay_key_scale: word(16),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn little_endian_envelopes_preserve_level_and_signed_scales() {
        let p = ordinary(&[60, 0, 0, 0, 0, 12, 237, 1]).unwrap();
        assert_eq!(
            (p.attack_ms, p.decay_ms, p.sustain, p.release_ms),
            (60, 0, 24576, 493)
        );
        assert_eq!(ordinary(&[255; 8]).unwrap().sustain, 32767);
        assert!(ordinary(&[0; 7]).is_err());
        let mut bytes = [0; 20];
        bytes[8..10].copy_from_slice(&(127u16 << 5).to_le_bytes());
        bytes[10..12].copy_from_slice(&493u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&(-1200i32 * 65536).to_le_bytes());
        bytes[16..20].copy_from_slice(&i32::MIN.to_le_bytes());
        let p = dls(&bytes).unwrap();
        assert_eq!((p.sustain_index, p.release_ms), (127, 493));
        assert_eq!(p.attack_velocity_scale, -1200 * 65536);
        assert_eq!(p.decay_key_scale, i32::MIN);
        assert!(dls(&bytes[..19]).is_err());
    }
}

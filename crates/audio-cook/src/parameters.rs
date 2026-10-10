//! Original little-endian envelope tables, converted once during import.
use crate::read;
use anyhow::{Result, ensure};
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

/// Source lookup data needed only while compiling fixed sustain levels.
pub struct Sustains {
    pub(crate) volume: [u16; 129],
    pub(crate) pitch: [u16; 1024],
}

impl Sustains {
    pub fn new(inverse: [u8; 1024], curve: [f32; 129]) -> Result<Self> {
        ensure!(
            inverse.iter().all(|&v| v <= 193)
                && curve
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid source sustain tables"
        );
        let pitch = inverse.map(|v| 193 - u16::from(v));
        let volume = curve.map(|v| pitch[usize::from(((4096.0 * v) as u16 >> 2).min(1023))]);
        Ok(Self { volume, pitch })
    }
}

pub fn timing(bytes: &[u8]) -> Result<dls::Timing> {
    let bytes = read::slice(bytes, 0, 20)?;
    let word = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    Ok(dls::Timing {
        attack_timecents: word(0),
        decay_timecents: word(4),
        release_ms: u16::from_le_bytes(bytes[10..12].try_into().unwrap()),
        attack_velocity_scale: word(12),
        decay_key_scale: word(16),
    })
}

pub fn dls(bytes: &[u8], sustains: &Sustains) -> Result<dls::Definition> {
    let timing = timing(bytes)?;
    let index = usize::from(u16::from_le_bytes(bytes[8..10].try_into()?) >> 5);
    ensure!(index < sustains.volume.len(), "invalid DLS sustain index");
    Ok(dls::Definition {
        timing,
        sustain: sustains.volume[index],
    })
}

pub(crate) fn pitch_sustain(bytes: &[u8], sustains: &Sustains) -> Result<u16> {
    let linear = u16::from_le_bytes(read::slice(bytes, 8, 2)?.try_into()?).min(4095);
    Ok(sustains.pitch[usize::from(linear >> 2)])
}

#[cfg(test)]
pub(crate) fn test_sustains() -> Sustains {
    Sustains::new(
        std::array::from_fn(|i| (193 - i * 193 / 1023) as u8),
        std::array::from_fn(|i| i as f32 / 128.),
    )
    .unwrap()
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
        let p = dls(&bytes, &test_sustains()).unwrap();
        assert_eq!((p.sustain, p.timing.release_ms), (191, 493));
        assert_eq!(p.timing.attack_velocity_scale, -1200 * 65536);
        assert_eq!(p.timing.decay_key_scale, i32::MIN);
        assert!(dls(&bytes[..19], &test_sustains()).is_err());
        let levels = test_sustains();
        bytes[8..10].copy_from_slice(&(128u16 << 5).to_le_bytes());
        assert_eq!(dls(&bytes, &levels).unwrap().sustain, 193);
        bytes[8..10].copy_from_slice(&(129u16 << 5).to_le_bytes());
        assert!(dls(&bytes, &levels).is_err());
        bytes[8..10].copy_from_slice(&2048u16.to_le_bytes());
        assert_eq!(pitch_sustain(&bytes, &levels).unwrap(), 96);
    }
}

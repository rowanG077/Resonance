//! Native fractional-semitone conversion to a bounded 16.16 sample increment.
use crate::{SOURCE_RATE, sample::Sample};
use anyhow::{Result, ensure};

/// Preserve fractional notes and saturate only at the phase representation's
/// maximum: just under 65,536 source samples per output sample.
pub fn ratio(pitch: i32, sample: &Sample) -> Result<u32> {
    ensure!(sample.key < 128 && sample.rate != 0, "invalid sample pitch");
    let semitones = f64::from(pitch) / 65536.0 - f64::from(sample.key);
    let rate = f64::from(sample.rate) / f64::from(SOURCE_RATE) * (semitones / 12.0).exp2();
    Ok((rate * 65536.0).min(f64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            key: 60,
            rate: SOURCE_RATE as u16,
            loop_start: 0,
            loop_length: 0,
            pcm: vec![],
            loop_pcm: vec![],
        }
    }

    #[test]
    fn fractional_pitch_tracks_octaves_and_source_rate() {
        let mut sample = sample();
        assert_eq!(ratio(60 << 16, &sample).unwrap(), 65536);
        assert_eq!(ratio(72 << 16, &sample).unwrap(), 131072);
        assert_eq!(ratio(48 << 16, &sample).unwrap(), 32768);
        // Six semitones are half an octave; fractions lie strictly between notes.
        let half_octave = f64::from(ratio(66 << 16, &sample).unwrap()) / 65536.0;
        assert!((half_octave * half_octave - 2.0).abs() < 0.00005);
        let half_step = ratio((60 << 16) + 32768, &sample).unwrap();
        assert!(65536 < half_step && half_step < ratio(61 << 16, &sample).unwrap());
        sample.rate /= 2;
        assert_eq!(ratio(60 << 16, &sample).unwrap(), 32768);
        sample.rate = 0;
        assert!(ratio(60 << 16, &sample).is_err());
    }

    #[test]
    fn wide_bends_are_monotonic_and_saturate_without_wrapping() {
        let sample = sample();
        let mut previous = 0;
        for quarter_note in -512..=1536 {
            let current = ratio(quarter_note * 16384, &sample).unwrap();
            assert!(current >= previous);
            previous = current;
        }
        assert_eq!(previous, u32::MAX);
        assert_eq!(ratio(i32::MAX, &sample).unwrap(), u32::MAX);
        assert_eq!(ratio(i32::MIN, &sample).unwrap(), 0);
    }
}

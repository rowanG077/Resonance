//! Resonance's windowed-sinc interpolation filters, generated during cooking.
//! This implements standard filter equations, not a console ROM reconstruction.
use std::f64::consts::PI;

/// Four banks of 128 phases, four signed Q15 taps per phase, in big-endian order.
/// Banks 0/1/2 have half/three-quarter/full Nyquist bandwidth. Bank 3 is reserved
/// and stays zero. The player consumes these bytes through its cooked tables.
pub fn coefficients() -> [u8; 4096] {
    static BYTES: std::sync::OnceLock<[u8; 4096]> = std::sync::OnceLock::new();
    *BYTES.get_or_init(generate)
}

fn generate() -> [u8; 4096] {
    let mut bytes = [0; 4096];
    for (bank, cutoff) in [0.5, 0.75, 1.0].into_iter().enumerate() {
        for phase in 0..128 {
            let fraction = phase as f64 / 128.0;
            let weights: [f64; 4] = std::array::from_fn(|tap| {
                // The history runs oldest to newest. At integer phase the
                // reconstruction is centered on history[1], advancing toward [2].
                let distance = tap as f64 - 1.0 - fraction;
                let window = if bank == 1 {
                    // Kaiser window, beta=7, supported on [-2, 2].
                    bessel_i0(7.0 * (1.0 - (distance / 2.0).powi(2)).sqrt()) / bessel_i0(7.0)
                } else {
                    // Continuous, symmetric Hamming window on [-2, 2].
                    0.54 + 0.46 * (PI * distance / 2.0).cos()
                };
                sinc(cutoff * distance) * window
            });
            // Normalize DC independently at each phase. Very near integer phase
            // a full-band tap can exceed unity slightly; keep every tap in i16.
            let dc: f64 = weights.iter().sum();
            let peak = weights.iter().map(|v| v.abs()).fold(0.0, f64::max);
            let scale = 32767.0 / dc.max(peak);
            for (tap, weight) in weights.into_iter().enumerate() {
                let coefficient = (weight * scale).round_ties_even() as i16;
                let offset = ((bank * 128 + phase) * 4 + tap) * 2;
                bytes[offset..offset + 2].copy_from_slice(&coefficient.to_be_bytes());
            }
        }
    }
    bytes
}

fn sinc(x: f64) -> f64 {
    if x.abs() < f64::EPSILON {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// I0(x) = sum((x*x/4)^k / (k!)^2). Here |x| <= 7; 32 terms suffice for f64.
fn bessel_i0(x: f64) -> f64 {
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..32 {
        term *= x * x / (4.0 * f64::from(k * k));
        sum += term;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resample::{Coefficients, Mode, Resampler};

    #[test]
    fn filters_have_stable_gain_symmetry_and_bandwidth() {
        let tables = Coefficients::from_be_bytes(&coefficients()).unwrap().0;
        assert!(tables[3].iter().flatten().all(|&value| value == 0));
        for table in &tables[..3] {
            for (phase, taps) in table.iter().enumerate() {
                let dc: i32 = taps.iter().map(|&v| i32::from(v)).sum();
                assert!((32759..=32769).contains(&dc), "phase {phase}: {dc}");
                if phase != 0 {
                    for tap in 0..4 {
                        assert_eq!(taps[tap], table[128 - phase][3 - tap]);
                    }
                }
            }
        }
        // Narrower banks suppress high input frequencies more strongly.
        let response = |table: &[[i16; 4]; 128], frequency: f64| {
            table
                .iter()
                .map(|taps| {
                    let mut real = 0.0;
                    let mut imaginary = 0.0;
                    for (tap, &value) in taps.iter().enumerate() {
                        let angle = 2.0 * PI * frequency * tap as f64;
                        real += f64::from(value) * angle.cos() / 32768.0;
                        imaginary += f64::from(value) * angle.sin() / 32768.0;
                    }
                    real.hypot(imaginary)
                })
                .sum::<f64>()
                / 128.0
        };
        let high = tables[..3]
            .iter()
            .map(|table| response(table, 0.4))
            .collect::<Vec<_>>();
        assert!(high[0] < high[1] && high[1] < high[2]);
        // A four-tap half-band filter provides at least 12 dB mean attenuation here.
        assert!(high[0] < 0.25);
        for table in &tables[..3] {
            assert!(response(table, 0.02) > 0.99);
        }
    }

    #[test]
    fn full_band_preserves_integer_samples_and_half_phase_timing() {
        let tables = Coefficients::from_be_bytes(&coefficients()).unwrap().0;
        assert_eq!(tables[2][0], [0, 32767, 0, 0]);
        let mut integer = Resampler::new(Mode::Polyphase(&tables[2]), 65536);
        let sample = resonance_audio::sample::Sample {
            key: 60,
            rate: 32000,
            loop_start: 0,
            loop_length: 0,
            pcm: (1..=16).map(|n| n * 1000).collect(),
            loop_pcm: Vec::new(),
        };
        let mut cursor = resonance_audio::resample::SampleCursor::new(&sample).unwrap();
        let actual: Vec<_> = (0..6).map(|_| integer.next_sample(&mut cursor)).collect();
        assert_eq!(actual, [0, 0, 999, 1999, 2999, 3999]);
        let half = tables[2][64];
        assert_eq!(half, [half[3], half[2], half[1], half[0]]);
        assert!(half[0] < 0 && half[1] > 16384);
    }

    #[test]
    fn resampling_constants_stays_stable_at_fractional_and_extreme_ratios() {
        let tables = Coefficients::from_be_bytes(&coefficients()).unwrap().0;
        for table in &tables[..3] {
            for ratio in [8192, 32768, 98304, 0x3fff0] {
                for level in [12000, -12000] {
                    let sample = resonance_audio::sample::Sample {
                        key: 60,
                        rate: 32000,
                        loop_start: 0,
                        loop_length: 1,
                        pcm: vec![level],
                        loop_pcm: vec![level],
                    };
                    let mut cursor = resonance_audio::resample::SampleCursor::new(&sample).unwrap();
                    let mut resampler = Resampler::new(Mode::Polyphase(table), ratio);
                    for _ in 0..64 {
                        resampler.next_sample(&mut cursor);
                    }
                    for _ in 0..256 {
                        let value = resampler.next_sample(&mut cursor);
                        assert!((i32::from(value) - i32::from(level)).abs() <= 4);
                    }
                }
            }
        }
    }
}

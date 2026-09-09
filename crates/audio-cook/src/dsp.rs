//! Nintendo `GameCube` DSP-ADPCM primitives used by `MusyX` sample directories.
//!
//! The `GameCube` DSP consumes eight-byte frames: one predictor/scale byte and
//! fourteen signed four-bit samples.  This module intentionally contains no
//! container assumptions; callers provide the coefficient table and the exact
//! decoded sample count from a `MusyX` section-two entry.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DspError {
    #[error("DSP sample count {0} is too large")]
    SampleCount(u32),
    #[error("DSP payload is {actual} bytes, but at least {required} are required")]
    PayloadShort { actual: usize, required: usize },
    #[error("DSP frame {frame} selects invalid predictor {predictor}")]
    Predictor { frame: usize, predictor: u8 },
    #[error("DSP sample range {start}..{end} is reversed")]
    SampleRange { start: u32, end: u32 },
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    pub predictor_scale: u8,
    /// (yn1, yn2), in decoder recurrence order.
    pub history: [i16; 2],
}

/// Return the encoded bytes needed for `sample_count` samples in ordinary
/// `GameCube` DSP-ADPCM.  The final partial frame uses two header/rounding bytes
/// plus one byte per pair of nibbles, just as the native DSP address code does.
#[must_use]
pub fn encoded_size(sample_count: u32) -> usize {
    let full_frames = sample_count / 14;
    let remainder = sample_count % 14;
    (full_frames * 8
        + if remainder == 0 {
            0
        } else {
            (remainder + 2).div_ceil(2)
        }) as usize
}

/// Decode a mono `GameCube` DSP-ADPCM payload.
///
/// `coefficients[predictor]` contains the two signed predictor coefficients.
/// `history` is the `(yn1, yn2)` state before the first frame; ordinary `MusyX`
/// samples use `[0, 0]`.  The decoder follows the native `DSPDecompressFrame`
/// sequence, including its `+1024` rounding term and signed 16-bit clamp.
///
/// # Errors
///
/// Returns an error when the payload is too short, the sample count cannot be
/// represented by the host, or a frame selects a predictor outside the eight
/// coefficient pairs supplied by the sample directory.
#[allow(clippy::cast_possible_truncation)]
pub fn decode(
    payload: &[u8],
    sample_count: u32,
    coefficients: [[i16; 2]; 8],
    history: [i16; 2],
) -> Result<Vec<i16>, DspError> {
    decode_range(
        payload,
        0..sample_count,
        coefficients,
        State {
            predictor_scale: payload.first().copied().unwrap_or(0),
            history,
        },
    )
}

/// Decode a sample interval with an explicit predictor/scale and history at
/// its first nibble. MusyX restores this state on every loop, even when the
/// loop begins partway through a fourteen-sample ADPCM frame.
pub fn decode_range(
    payload: &[u8],
    range: std::ops::Range<u32>,
    coefficients: [[i16; 2]; 8],
    state: State,
) -> Result<Vec<i16>, DspError> {
    if range.start > range.end {
        return Err(DspError::SampleRange {
            start: range.start,
            end: range.end,
        });
    }
    if range.end > 0x00ff_ffff {
        return Err(DspError::SampleCount(range.end));
    }
    let required = encoded_size(range.end);
    if payload.len() < required {
        return Err(DspError::PayloadShort {
            actual: payload.len(),
            required,
        });
    }
    let sample_count = (range.end - range.start) as usize;
    let mut output = Vec::with_capacity(sample_count);
    let mut previous = i64::from(state.history[0]);
    let mut previous_previous = i64::from(state.history[1]);
    let mut frame_index = (range.start / 14) as usize;
    let mut frame_offset = frame_index * 8;
    let mut first_sample = (range.start % 14) as usize;
    let mut header = state.predictor_scale;
    while output.len() < sample_count {
        let predictor = header >> 4;
        if predictor >= 8 {
            return Err(DspError::Predictor {
                frame: frame_index,
                predictor,
            });
        }
        let scale = i32::from(header & 0x0F);
        let remaining = sample_count - output.len();
        let frame_samples = remaining.min(14 - first_sample);
        let coefficient_a = i64::from(coefficients[predictor as usize][0]);
        let coefficient_b = i64::from(coefficients[predictor as usize][1]);
        for sample_index in first_sample..first_sample + frame_samples {
            let packed = payload[frame_offset + 1 + sample_index / 2];
            let nibble = if sample_index.is_multiple_of(2) {
                packed >> 4
            } else {
                packed & 0x0F
            };
            let signed_nibble = if nibble < 8 {
                i64::from(nibble)
            } else {
                i64::from(nibble) - 16
            };
            let mut sample = (signed_nibble << scale) << 11;
            sample += 1024;
            sample += coefficient_a * previous + coefficient_b * previous_previous;
            sample >>= 11;
            sample = sample.clamp(i64::from(i16::MIN), i64::from(i16::MAX));
            output.push(sample as i16);
            previous_previous = previous;
            previous = sample;
        }
        frame_offset += 8;
        frame_index += 1;
        first_sample = 0;
        if output.len() < sample_count {
            header = payload[frame_offset];
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_signed_nibbles_and_predictor_state() {
        let mut frame = [0u8; 8];
        frame[0] = 0x00;
        frame[1] = 0x17;
        frame[2] = 0xF8;
        let coefficients = [[0i16; 2]; 8];
        let output = decode(&frame, 4, coefficients, [0, 0]).unwrap();
        assert_eq!(output, vec![1, 7, -1, -8]);

        frame[0] = 0x01;
        frame[1] = 0x12;
        let output = decode(&frame, 2, coefficients, [0, 0]).unwrap();
        assert_eq!(output, vec![2, 4]);
    }

    #[test]
    fn rejects_short_payload_and_bad_predictor() {
        assert_eq!(
            decode(&[], 1, [[0; 2]; 8], [0, 0]),
            Err(DspError::PayloadShort {
                actual: 0,
                required: 2
            })
        );
        let mut frame = [0u8; 8];
        frame[0] = 0x80;
        assert_eq!(
            decode(&frame, 1, [[0; 2]; 8], [0, 0]),
            Err(DspError::Predictor {
                frame: 0,
                predictor: 8
            })
        );
    }

    #[test]
    fn predictor_accumulation_does_not_overflow_before_saturation() {
        assert_eq!(
            decode(&[0x0f, 0x70], 1, [[32767; 2]; 8], [32767; 2]).unwrap(),
            [32767]
        );
        assert_eq!(
            decode(&[], u32::MAX, [[0; 2]; 8], [0; 2]),
            Err(DspError::SampleCount(u32::MAX))
        );
    }

    #[test]
    fn partial_frame_loop_restores_history_then_reads_the_next_header() {
        let payload = [0, 0x01, 0xf0];
        assert_eq!(
            decode_range(
                &payload,
                1..3,
                [[1024, 0]; 8],
                State {
                    predictor_scale: 0,
                    history: [100, 0],
                }
            )
            .unwrap(),
            [51, 25]
        );
        let mut payload = [0; 10];
        payload[7] = 1;
        payload[8] = 1;
        payload[9] = 0x77;
        assert_eq!(
            decode_range(
                &payload,
                13..16,
                [[0; 2]; 8],
                State {
                    predictor_scale: 0,
                    history: [0; 2],
                }
            )
            .unwrap(),
            [1, 14, 14]
        );
        assert!(
            decode_range(
                &payload,
                std::ops::Range { start: 10, end: 9 },
                [[0; 2]; 8],
                State {
                    predictor_scale: 0,
                    history: [0; 2],
                }
            )
            .is_err()
        );
    }
}

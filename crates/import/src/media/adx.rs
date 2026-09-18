//! CRI's fixed-block ADPCM stream, decoded to its declared native PCM clock.
use crate::read::{u16 as be_u16, u32 as be_u32};
use anyhow::{Context, Result, ensure};

pub(super) struct Decoder<'a> {
    data: &'a [u8],
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u32,
    encoding: u8,
    version: u16,
    coefficients: [i32; 2],
    history: [[i32; 2]; 2],
    remaining: usize,
    block: [i16; 64],
}

impl<'a> Decoder<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= 20 && be_u16(bytes, 0)? == 0x8000,
            "invalid ADX header"
        );
        let start = usize::from(be_u16(bytes, 2)?) + 4;
        let channels = u16::from(bytes[7]);
        let sample_rate = be_u32(bytes, 8)?;
        let frames = be_u32(bytes, 12)?;
        let cutoff = be_u16(bytes, 16)?;
        let version = be_u16(bytes, 18)?;
        ensure!(
            (2..=4).contains(&bytes[4])
                && bytes[5..7] == [18, 4]
                && (1..=2).contains(&channels)
                && (8_000..=96_000).contains(&sample_rate)
                && (1..=32_000_000).contains(&frames)
                && matches!(version, 0x0300 | 0x0400 | 0x0500)
                && u32::from(cutoff) < sample_rate / 2,
            "unsupported ADX encoding or dimensions"
        );
        let minimum = if version == 0x0400 { 0x20 } else { 0x14 };
        ensure!(
            start >= minimum + 6 && bytes.get(start - 6..start) == Some(b"(c)CRI"),
            "invalid ADX data offset or signature"
        );
        let data = bytes.get(start..).context("truncated ADX header")?;
        ensure!(
            data.len() >= (frames as usize).div_ceil(32) * usize::from(channels) * 18,
            "truncated ADX frames"
        );
        let z = (std::f32::consts::TAU * f32::from(cutoff) / sample_rate as f32).cos();
        let a = std::f32::consts::SQRT_2 - z;
        let b = std::f32::consts::SQRT_2 - 1.;
        let c = (a - ((a + b) * (a - b)).sqrt()) / b;
        let mut history = [[0; 2]; 2];
        if version == 0x0400 {
            for (channel, values) in history.iter_mut().take(usize::from(channels)).enumerate() {
                for (index, value) in values.iter_mut().enumerate() {
                    *value = i32::from(be_u16(bytes, 0x18 + channel * 4 + index * 2)? as i16);
                }
            }
        }
        Ok(Self {
            data,
            channels,
            sample_rate,
            frames,
            encoding: bytes[4],
            version,
            coefficients: [(c * 8192.) as i16 as i32, (c * c * -4096.) as i16 as i32],
            history,
            remaining: frames as usize,
            block: [0; 64],
        })
    }

    pub fn next_block(&mut self) -> Result<Option<&[i16]>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let channels = usize::from(self.channels);
        let count = self.remaining.min(32);
        for channel in 0..channels {
            let frame = &self.data[channel * 18..(channel + 1) * 18];
            let header = be_u16(frame, 0)?;
            let (scale, [c1, c2]) = match self.encoding {
                2 => {
                    const FILTERS: [[i32; 2]; 4] =
                        [[0, 0], [0x0f00, 0], [0x1cc0, -0x0d00], [0x1880, -0x0dc0]];
                    let filter = *FILTERS
                        .get(usize::from(header >> 13))
                        .context("invalid ADX predictor")?;
                    (i32::from(header & 0x1fff) + 1, filter)
                }
                4 => {
                    ensure!(header <= 12, "invalid ADX exponential scale");
                    (1 << (12 - header), self.coefficients)
                }
                _ => {
                    ensure!(
                        header < 0x8000,
                        "unexpected ADX end marker before declared sample count"
                    );
                    (i32::from(header) + 1, self.coefficients)
                }
            };
            let [mut h1, mut h2] = self.history[channel];
            for index in 0..count {
                let nibble = (frame[2 + index / 2] >> (4 * (1 - index % 2))) & 15;
                let sample = i32::from((nibble << 4) as i8) >> 4;
                let prediction = if self.version == 0x0300 {
                    ((c1 * h1) >> 12) + ((c2 * h2) >> 12)
                } else {
                    (c1 * h1 + c2 * h2) >> 12
                };
                let sample =
                    (sample * scale + prediction).clamp(i32::from(i16::MIN), i32::from(i16::MAX));
                self.block[index * channels + channel] = sample as i16;
                h2 = h1;
                h1 = sample;
            }
            self.history[channel] = [h1, h2];
        }
        self.data = &self.data[18 * channels..];
        self.remaining -= count;
        Ok(Some(&self.block[..count * channels]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_stereo_nibbles_and_trims_last_block() -> Result<()> {
        let mut bytes = vec![0; 0x26 + 36];
        bytes[..8].copy_from_slice(&[0x80, 0, 0, 0x22, 2, 18, 4, 2]);
        bytes[8..12].copy_from_slice(&32_000u32.to_be_bytes());
        bytes[12..16].copy_from_slice(&3u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&[1, 0xf4, 4, 0]);
        bytes[0x20..0x26].copy_from_slice(b"(c)CRI");
        bytes[0x28..0x2a].copy_from_slice(&[0x12, 0x30]);
        bytes[0x3a..0x3c].copy_from_slice(&[0xef, 0x80]);
        let mut decoder = Decoder::new(&bytes)?;
        assert_eq!(decoder.next_block()?.unwrap(), &[1, -2, 2, -1, 3, -8]);
        assert!(decoder.next_block()?.is_none());
        assert!(Decoder::new(&bytes[..bytes.len() - 1]).is_err());
        bytes[19] = 8;
        assert!(Decoder::new(&bytes).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires original discs and independently cooked PCM; no devices or codecs"]
    fn original_adx_matches_independent_cooked_pcm() -> Result<()> {
        use std::{
            fs,
            path::{Path, PathBuf},
        };
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let reference = PathBuf::from(
            std::env::var_os("RESONANCE_ADX_REFERENCE")
                .context("set RESONANCE_ADX_REFERENCE to independently decoded audio/streams")?,
        );
        for disc in [1, 2] {
            let bytes = fs::read(local.join(format!("extracted/disc{disc}/files/tos_ending.adx")))?;
            let mut expected =
                hound::WavReader::open(reference.join(format!("{}.wav", crate::digest(&bytes))))?;
            let mut decoder = Decoder::new(&bytes)?;
            assert_eq!(expected.spec().channels, decoder.channels);
            assert_eq!(expected.spec().sample_rate, decoder.sample_rate);
            assert_eq!(expected.duration(), decoder.frames);
            let mut samples = expected.samples::<i16>();
            while let Some(block) = decoder.next_block()? {
                for &sample in block {
                    assert_eq!(samples.next().context("missing reference PCM")??, sample);
                }
            }
            assert!(samples.next().is_none());
        }
        Ok(())
    }
}

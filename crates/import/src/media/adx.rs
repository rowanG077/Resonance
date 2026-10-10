//! CRI's fixed-block ADPCM stream, decoded to its declared native PCM clock.
use crate::read::{u16 as be_u16, u32 as be_u32};
use anyhow::{Context, Result, ensure};

pub(super) struct Decoder<'a> {
    data: &'a [u8],
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u32,
    encoding: u8,
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
                // Combine both prediction products before truncation.
                let prediction = (c1 * h1 + c2 * h2) >> 12;
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
    fn version_three_combines_predictor_products_before_truncating() -> Result<()> {
        for channels in [1u8, 2] {
            let mut bytes = vec![0; 0x26 + usize::from(channels) * 18];
            bytes[..8].copy_from_slice(&[0x80, 0, 0, 0x22, 3, 18, 4, channels]);
            bytes[8..12].copy_from_slice(&22_050u32.to_be_bytes());
            bytes[12..16].copy_from_slice(&6u32.to_be_bytes());
            bytes[16..20].copy_from_slice(&[1, 0xf4, 3, 0]);
            bytes[0x20..0x26].copy_from_slice(b"(c)CRI");
            for channel in 0..usize::from(channels) {
                bytes[0x28 + channel * 18] = 0x10;
            }
            let mut decoder = Decoder::new(&bytes)?;
            // Original combined arithmetic yields -3; separate shifts yield -4.
            let expected: Vec<_> = [1, 1, 0, -1, -2, -3]
                .into_iter()
                .flat_map(|sample| std::iter::repeat_n(sample, usize::from(channels)))
                .collect();
            assert_eq!(decoder.next_block()?.unwrap(), expected);
            assert!(decoder.next_block()?.is_none());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original battle archive and captured CRI prefill PCM; no devices or codecs"]
    fn original_battle_adx_matches_captured_cri_prefill() -> Result<()> {
        use std::{
            fs,
            io::{Read, Seek, SeekFrom},
            path::{Path, PathBuf},
        };
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let reference =
            PathBuf::from(std::env::var_os("RESONANCE_CRI_ADX_REFERENCE").context(
                "set RESONANCE_CRI_ADX_REFERENCE to the captured 8192-byte BE PCM buffer",
            )?);
        let expected = fs::read(reference)?;
        ensure!(expected.len() == 8192, "unexpected CRI prefill length");
        ensure!(
            crate::digest(&expected)
                == "a0a169f74cec7b22fc359a342488c119747aa037c972924ac1c5d6d1704f2a42",
            "CRI prefill differs from the pinned first-activation observation"
        );
        let mut archive = fs::File::open(local.join("extracted/disc1/files/BTL/btlvoice.afs"))?;
        let entries = crate::afs::index(&mut archive)?;
        let entry = entries.get(257).context("missing observed battle voice")?;
        archive.seek(SeekFrom::Start(entry.offset))?;
        let mut bytes = vec![0; entry.size];
        archive.read_exact(&mut bytes)?;
        ensure!(
            crate::digest(&bytes)
                == "164c05aa40e220d8dd55148ce22df2fcfa6d19a9259a918ad423f24487383291",
            "battle voice differs from the observed source member"
        );
        let mut decoder = Decoder::new(&bytes)?;
        assert_eq!(
            (decoder.channels, decoder.sample_rate, decoder.frames),
            (1, 22_050, 32_937)
        );
        for block in expected.chunks_exact(64) {
            let samples: Vec<_> = block
                .chunks_exact(2)
                .map(|pair| i16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            assert_eq!(
                decoder.next_block()?.context("missing decoded block")?,
                samples
            );
        }
        Ok(())
    }
}

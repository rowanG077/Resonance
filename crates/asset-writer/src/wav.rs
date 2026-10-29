use anyhow::Result;
use std::path::Path;

/// Write only the supplied path; callers validate temporary files before publishing them.
pub fn write_pcm16(
    path: &Path,
    channels: u16,
    sample_rate: u32,
    samples: impl IntoIterator<Item = i16>,
) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Standard RIFF `smpl` metadata: one endless forward loop, in sample frames.
/// Only chunk headers and the small sampler record are read; PCM is skipped.
pub fn read_loop(
    reader: &mut (impl std::io::Read + std::io::Seek),
) -> Result<Option<std::ops::Range<u32>>> {
    use anyhow::{Context, ensure};
    use std::io::SeekFrom;
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut header = [0; 12];
    reader.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"RIFF" && &header[8..] == b"WAVE",
        "music requires RIFF/WAVE"
    );
    let end = u64::from(u32::from_le_bytes(header[4..8].try_into()?)) + 8;
    ensure!(end >= 12 && end <= length, "truncated WAV container");
    let mut offset = 12;
    let mut seen = false;
    let mut result = None;
    let mut chunks = 0;
    while offset < end {
        chunks += 1;
        ensure!(
            chunks <= 4096 && offset + 8 <= end,
            "invalid WAV chunk table"
        );
        reader.seek(SeekFrom::Start(offset))?;
        let mut header = [0; 8];
        reader.read_exact(&mut header)?;
        let size = u64::from(u32::from_le_bytes(header[4..].try_into()?));
        let next = offset + 8 + size + size % 2;
        ensure!(next <= end, "truncated WAV chunk");
        if &header[..4] == b"smpl" {
            ensure!(!seen && size >= 36, "invalid or repeated WAV sampler chunk");
            seen = true;
            let mut sampler = [0; 36];
            reader.read_exact(&mut sampler)?;
            let count = u32::from_le_bytes(sampler[28..32].try_into()?);
            let extra = u32::from_le_bytes(sampler[32..36].try_into()?);
            ensure!(
                count <= 1 && 36 + u64::from(count) * 24 + u64::from(extra) <= size,
                "music supports one WAV sampler loop"
            );
            if count == 1 {
                let mut sample_loop = [0; 24];
                reader.read_exact(&mut sample_loop)?;
                let word = |at| u32::from_le_bytes(sample_loop[at..at + 4].try_into().unwrap());
                ensure!(
                    word(4) == 0 && word(16) == 0 && word(20) == 0,
                    "music requires an endless forward WAV loop with whole-frame boundaries"
                );
                let end = word(12).checked_add(1).context("WAV loop end overflows")?;
                ensure!(word(8) < end, "reversed WAV loop");
                result = Some(word(8)..end);
            }
        }
        offset = next;
    }
    Ok(result)
}

/// Append a standard sampler loop to a freshly encoded WAV with no `smpl` chunk.
/// Existing PCM bytes are untouched. The end of `repeat` is exclusive; RIFF
/// stores the last included sample frame instead.
pub fn append_loop(
    writer: &mut (impl std::io::Read + std::io::Write + std::io::Seek),
    sample_rate: u32,
    repeat: std::ops::Range<u32>,
) -> Result<()> {
    use anyhow::ensure;
    use std::io::SeekFrom;
    ensure!(
        sample_rate > 0 && repeat.start < repeat.end,
        "invalid WAV loop"
    );
    writer.seek(SeekFrom::Start(0))?;
    let mut header = [0; 12];
    writer.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"RIFF" && &header[8..] == b"WAVE",
        "loop output requires RIFF/WAVE"
    );
    let length = writer.seek(SeekFrom::End(0))?;
    ensure!(
        length == u64::from(u32::from_le_bytes(header[4..8].try_into()?)) + 8,
        "WAV length differs from its RIFF header"
    );
    let size = u32::try_from(length + 68 - 8)?;
    writer.write_all(b"smpl")?;
    writer.write_all(&60u32.to_le_bytes())?;
    for word in [
        0,
        0,
        1_000_000_000 / sample_rate,
        60,
        0,
        0,
        0,
        1,
        0,
        0,
        0,
        repeat.start,
        repeat.end - 1,
        0,
        0,
    ] {
        writer.write_all(&word.to_le_bytes())?;
    }
    writer.seek(SeekFrom::Start(4))?;
    writer.write_all(&size.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
mod loop_tests {
    use super::*;
    use std::io::{Cursor, Read, Seek, SeekFrom};

    fn container(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        for (name, content) in chunks {
            bytes.extend(name);
            bytes.extend((content.len() as u32).to_le_bytes());
            bytes.extend(content);
            if content.len() % 2 != 0 {
                bytes.push(0);
            }
        }
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    #[test]
    fn standard_loops_survive_chunk_order_padding_and_skip_pcm() -> Result<()> {
        let original = container(&[(*b"data", vec![7; 10000])]);
        let mut wave = Cursor::new(original.clone());
        append_loop(&mut wave, 32000, 2..9)?;
        let bytes = wave.into_inner();
        assert_eq!(&bytes[8..original.len()], &original[8..]);
        let sampler = bytes[original.len() + 8..].to_vec();
        // Literal expected standard fields, independent of the writer/parser.
        assert_eq!(u32::from_le_bytes(sampler[28..32].try_into()?), 1);
        assert_eq!(u32::from_le_bytes(sampler[44..48].try_into()?), 2);
        assert_eq!(u32::from_le_bytes(sampler[48..52].try_into()?), 8);
        for chunks in [
            vec![
                (*b"data", vec![7; 10000]),
                (*b"JUNK", vec![1]),
                (*b"smpl", sampler.clone()),
            ],
            vec![
                (*b"smpl", sampler.clone()),
                (*b"JUNK", vec![1]),
                (*b"data", vec![7; 10000]),
            ],
        ] {
            struct Count {
                inner: Cursor<Vec<u8>>,
                bytes: usize,
            }
            impl Read for Count {
                fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                    let n = self.inner.read(out)?;
                    self.bytes += n;
                    Ok(n)
                }
            }
            impl Seek for Count {
                fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
                    self.inner.seek(from)
                }
            }
            let mut reader = Count {
                inner: Cursor::new(container(&chunks)),
                bytes: 0,
            };
            assert_eq!(read_loop(&mut reader)?, Some(2..9));
            assert_eq!(
                reader.bytes,
                12 + 3 * 8 + 60,
                "PCM and unknown chunks must be skipped"
            );
        }
        assert_eq!(read_loop(&mut Cursor::new(original))?, None);
        for (offset, value) in [(28, 2u32), (40, 1), (44, 10), (52, 1), (56, 1)] {
            let mut invalid = sampler.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(read_loop(&mut Cursor::new(container(&[(*b"smpl", invalid)]))).is_err());
        }
        assert!(
            read_loop(&mut Cursor::new(container(&[
                (*b"smpl", sampler.clone()),
                (*b"smpl", sampler)
            ])))
            .is_err()
        );
        let mut truncated = bytes;
        truncated.pop();
        assert!(read_loop(&mut Cursor::new(truncated)).is_err());
        Ok(())
    }
}

//! Voice storage and casting durations read by battle 9D80/9D60.
use anyhow::{Result, ensure};
use resonance_content::battle_voice::Table;
use std::path::Path;

fn read(usual: &[u8]) -> Result<Table> {
    let streams = crate::source_assets::section(usual, 11)?;
    let durations = crate::source_assets::section(usual, 12)?;
    ensure!(
        durations.len().is_multiple_of(2),
        "misaligned battle voice durations"
    );
    let table = Table {
        source_sha256: crate::digest(usual),
        streams: streams.to_vec(),
        durations: durations
            .chunks_exact(2)
            .map(|row| u16::from_be_bytes([row[0], row[1]]))
            .collect(),
    };
    table.validate()?;
    Ok(table)
}

pub fn publish(usual: &[u8], output: &Path, prefix: &str) -> Result<String> {
    let path = format!("{prefix}/voices.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&read(usual)?)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_each_flag_and_rejects_missing_member() -> Result<()> {
        let mut bytes = vec![0; 56];
        bytes[..4].copy_from_slice(&13_u32.to_be_bytes());
        bytes[48..52].copy_from_slice(&56_u32.to_be_bytes());
        bytes[52..56].copy_from_slice(&59_u32.to_be_bytes());
        bytes.extend([0x81, 0x24, 0]);
        bytes.extend(
            [0_u16, 1, 62, 0x1234]
                .into_iter()
                .flat_map(u16::to_be_bytes),
        );
        let table = read(&bytes)?;
        assert_eq!(table.streams, [0x81, 0x24, 0]);
        assert_eq!(table.durations, [0, 1, 62, 0x1234]);
        assert_eq!(table.source_sha256, crate::digest(&bytes));
        for end in 0..61 {
            assert!(read(&bytes[..end]).is_err());
        }
        assert!(read(&bytes[..bytes.len() - 1]).is_err());
        bytes[48..52].fill(0);
        assert!(read(&bytes).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_voice_flags_match_across_discs_and_select_nurse_lines() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let sources = crate::source_assets::Sources::read(&extracted)?;
            let bytes = std::fs::read(extracted.join("files").join(sources.usual))?;
            let table = read(&bytes)?;
            assert_eq!(table.streams.len(), 320);
            assert_eq!(table.durations.len(), 2512);
            assert_eq!(
                table
                    .durations
                    .iter()
                    .flat_map(|value| value.to_be_bytes())
                    .collect::<Vec<_>>(),
                crate::source_assets::section(&bytes, 12)?
            );
            for (line, duration) in [(458, 62), (423, 37), (369, 41), (248, 37), (312, 45)] {
                assert_eq!(table.durations[line], duration);
            }
            for (line, streamed) in [(43, false), (163, false), (404, false), (423, true)] {
                assert_eq!(table.streams[line / 8] & (1 << (line % 8)) != 0, streamed);
            }
            if let Some((streams, durations)) = &previous {
                assert_eq!(&table.streams, streams);
                assert_eq!(&table.durations, durations);
            }
            previous = Some((table.streams, table.durations));
        }
        Ok(())
    }
}

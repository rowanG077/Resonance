//! Each character has five fixed-size result packages, each containing a program and a clip.
use super::geometry;
use crate::{battle::animation_table, field::sections, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::visual::VictoryStyle;
use serde::Serialize;
use std::{ops::Range, path::Path};

const PACKAGE_BYTES: usize = 0x38000;
const PACKAGE_COUNT: usize = 9 * VictoryStyle::ALL.len();

#[derive(Serialize)]
struct Program {
    character: u8,
    style: VictoryStyle,
    animations: animation_table::Parsed,
    motion: String,
    /// Header alignment, package-relative. Program storage is pool-relative.
    unused_storage: Vec<serde_json::Value>,
}

fn program(
    package: &[u8],
    ranges: &[Option<Range<usize>>],
    index: usize,
    motion: String,
) -> Result<Program> {
    let range = ranges[0]
        .as_ref()
        .context("missing victory animation program")?;
    let animations = animation_table::decode(&package[range.clone()], [0])?;
    let first = ranges.iter().flatten().map(|r| r.start).min().unwrap();
    let unused_storage = [12..first]
        .into_iter()
        .filter(|range| !range.is_empty())
        .map(|range| serde_json::json!({"offset": range.start, "bytes": &package[range]}))
        .collect();
    Ok(Program {
        character: (index / VictoryStyle::ALL.len() + 1) as u8,
        style: VictoryStyle::ALL[index % VictoryStyle::ALL.len()],
        animations,
        motion,
        unused_storage,
    })
}

pub(super) fn cook(
    bytes: &[u8],
    name: &str,
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<()> {
    ensure!(
        bytes.len() == PACKAGE_COUNT * PACKAGE_BYTES,
        "invalid victory package extent"
    );
    for (index, package) in bytes.chunks_exact(PACKAGE_BYTES).enumerate() {
        let name = format!("{name}/{index}");
        let ranges = match sections(package).and_then(|ranges| {
            ensure!(
                ranges.len() == 2,
                "victory package must contain a program and a clip"
            );
            Ok(ranges)
        }) {
            Ok(ranges) => ranges,
            Err(error) => {
                report(&name, Err(error));
                continue;
            }
        };
        let motion = format!("{name}/motion");
        let program = (|| -> Result<()> {
            let program = program(package, &ranges, index, motion.clone())?;
            write_atomic(
                &output.join(&name).join("program.json"),
                &serde_json::to_vec(&program)?,
            )
        })();
        report(&format!("{name}/program"), program);
        let clip = (|| -> Result<()> {
            let range = ranges[1]
                .as_ref()
                .context("missing victory skeletal clip")?;
            ensure!(
                geometry::cook(
                    &package[range.clone()],
                    &motion,
                    output,
                    None,
                    geometry::Input::File,
                    report
                ),
                "unrecognized victory skeletal clip"
            );
            Ok(())
        })();
        if clip.is_err() {
            report(&motion, clip);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::{u16 as half, u32 as word};

    #[test]
    fn preserves_header_and_terminator_storage_without_decoding_it() -> Result<()> {
        // Initialization binds a full descriptor even though dispatch can stop
        // on a two-byte terminator.
        assert!(animation_table::decode(&(-2i16).to_be_bytes(), [0]).is_err());
        let mut bytes = vec![0; 80];
        for (at, value) in [(0, 2u32), (4, 32), (8, 64)] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[12..32].fill(0xfe);
        bytes[34] = 15;
        bytes[40..44].copy_from_slice(&0.5f32.to_be_bytes());
        bytes[44..46].copy_from_slice(&(-2i16).to_be_bytes());
        bytes[46..64].fill(0x81);
        let decoded = program(&bytes, &sections(&bytes)?, 0, "motion".into())?;
        let animations = serde_json::to_value(&decoded.animations)?;
        assert_eq!(animations["records"][0]["forced_bind"]["clip"], 15);
        assert_eq!(animations["records"][1]["dispatch"]["kind"], "end");
        assert_eq!(
            decoded.unused_storage,
            [serde_json::json!({"offset": 12, "bytes": vec![0xfe; 20]})]
        );
        assert_eq!(
            animations["unreferenced_storage"],
            serde_json::json!([
                {"offset": 14, "bytes": vec![0x81; 18]}
            ])
        );
        bytes[44..46].copy_from_slice(&(-5i16).to_be_bytes());
        let stalled = program(&bytes, &sections(&bytes)?, 0, "motion".into())?;
        assert_eq!(
            serde_json::to_value(stalled.animations)?["records"][1]["dispatch"]["kind"],
            "stalled"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires both locally extracted original discs"]
    fn original_victory_packages_account_for_header_and_program_storage() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let bytes = std::fs::read(extracted.join(disc).join("files/BTL/BTLwin.bfp"))?;
            assert_eq!(bytes.len(), PACKAGE_COUNT * PACKAGE_BYTES);
            for (index, package) in bytes.chunks_exact(PACKAGE_BYTES).enumerate() {
                assert_eq!(word(package, 0)?, 2);
                let start = word(package, 4)? as usize;
                let clip = word(package, 8)? as usize;
                let mut rows = package[start..clip].chunks_exact(12);
                let terminator = rows
                    .position(|row| half(row, 0).unwrap() == 0xfffe)
                    .context("original victory program has no terminator")?;
                let end = start + terminator * 12 + 2;
                let decoded = program(package, &sections(package)?, index, "motion".into())?;
                assert_eq!(
                    decoded.character as usize,
                    index / VictoryStyle::ALL.len() + 1
                );
                let animations = serde_json::to_value(&decoded.animations)?;
                assert_eq!(
                    animations["records"].as_array().unwrap().len(),
                    terminator + 1
                );
                assert_eq!(
                    decoded.unused_storage,
                    [serde_json::json!({"offset": 12, "bytes": &package[12..start]})]
                );
                let mut storage = Vec::new();
                // Initialization uses every operand in row zero. Subsequent
                // texture/rate records leave part of their payload unread.
                for at in (start + 12..end - 2).step_by(12) {
                    let range = match package[at + 2] {
                        255 => at + 5..at + 12,
                        254 => at + 3..at + 8,
                        _ => continue,
                    };
                    storage.push(serde_json::json!({
                        "offset": range.start - start, "bytes": &package[range]
                    }));
                }
                if end < clip {
                    storage.push(
                        serde_json::json!({"offset": end - start, "bytes": &package[end..clip]}),
                    );
                }
                assert_eq!(
                    animations["unreferenced_storage"],
                    serde_json::json!(storage)
                );
            }
        }
        Ok(())
    }
}

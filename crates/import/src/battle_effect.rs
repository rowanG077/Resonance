//! Original `ef1` timelines and particle inputs. This reader neither executes
//! commands nor generates authored script behavior.
pub(crate) mod art;
mod declaration;
mod source;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::Record;
pub use source::{publish, read};

pub fn publish_tints(
    file: &std::path::Path,
    output: &std::path::Path,
    prefix: &str,
) -> Result<String> {
    use resonance_content::battle_effect::Tints;
    let module = crate::rel::Rel::read(file)?;
    let tints = Tints {
        source_sha256: crate::digest(&module.bytes),
        palettes: module
            .at((4, 0x2174))?
            .get(..10)
            .context("truncated effect palette table")?
            .try_into()?,
        colors: crate::read::Field::read(module.at((4, 0x2180))?, 0)?,
        actors: crate::read::Field::read(module.at((4, 0x1564))?, 0)?,
        contact_effects: crate::read::Field::read(module.at((5, 0x13f8))?, 0)?,
        contact_colors: crate::read::Field::read(module.at((5, 0x13d0))?, 0)?,
        admission_colors: crate::read::Field::read(module.at((4, 0x11b0))?, 0)?,
    };
    let path = format!("{prefix}/effects/tints.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&tints)?)?;
    Ok(path)
}

/// Decode a requested simulation input. Full cooking uses `read` and retains
/// controllers independently of runtime admission.
pub fn program(
    bytes: &[u8],
    member: usize,
) -> Result<resonance_content::battle_effect::ProgramSource> {
    read(bytes)?.program(member)
}

#[cfg(test)]
fn particle(bytes: &[u8]) -> Result<resonance_content::battle_effect::ParticleTemplate> {
    declaration::read(bytes)?.particle(&[])
}

pub fn modifier(bytes: &[u8], at: usize) -> Result<Vec<u16>> {
    let mut bytes = bytes.get(at..).context("effect modifier outside bank")?;
    let mut words = Vec::new();
    loop {
        let opcode = u16::from_be_bytes(
            bytes
                .get(..2)
                .context("truncated effect modifier")?
                .try_into()
                .unwrap(),
        );
        let count = match opcode {
            0xffff => 1,
            0 | 2..=6 | 10..=12 | 16 | 17 | 19 | 21 | 23..=25 => 4,
            1 | 20 | 22 => 8,
            7 | 8 | 13..=15 | 18 | 26..=30 => 6,
            9 => 12,
            _ => anyhow::bail!("unknown effect modifier opcode {opcode}"),
        };
        let row = bytes
            .get(..count * 2)
            .context("truncated effect modifier operands")?;
        words.extend(
            row.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]])),
        );
        if opcode == 0xffff {
            return Ok(words);
        }
        bytes = &bytes[count * 2..];
    }
}

pub fn timelines(bytes: &[u8]) -> Result<Vec<Vec<Record>>> {
    ensure!(
        bytes.len() >= 20 && &bytes[..4] == b"ef1\0",
        "invalid battle effect bank"
    );
    let half = |offset| u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    let offsets: Vec<_> = (8..20).step_by(2).map(half).collect();
    ensure!(
        offsets
            .iter()
            .all(|&offset| (20..=bytes.len()).contains(&offset)),
        "battle effect section outside bank"
    );
    let events = offsets[1];
    let end = offsets
        .iter()
        .copied()
        .filter(|&offset| offset > events)
        .min()
        .unwrap_or(bytes.len());
    let table = offsets[4];
    let count = usize::from(bytes[4]);
    let roots = bytes
        .get(table..table + count * 2)
        .context("truncated effect program table")?;
    roots
        .chunks_exact(2)
        .map(|root| {
            let start = events
                .checked_add_signed(i16::from_be_bytes([root[0], root[1]]) as isize)
                .context("effect program offset underflow")?;
            ensure!(
                (events..end).contains(&start),
                "effect program outside command section"
            );
            let mut records = Vec::new();
            let mut payload = false;
            for bytes in bytes[start..end].chunks_exact(6) {
                let record = Record::from_bytes(bytes.try_into().unwrap());
                records.push(record);
                if payload {
                    ensure!(
                        record.command < 254,
                        "effect repeat requires an emission, sound or modification"
                    );
                    payload = false;
                } else if record.command == 254 {
                    return Ok(records);
                } else {
                    payload = record.command == 255;
                }
            }
            anyhow::bail!("unterminated effect program")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(records: &[[u8; 6]]) -> Vec<u8> {
        let mut bytes = b"ef1\0\x01\x00\x00\x00".to_vec();
        let end = 20 + records.len() as u16 * 6;
        for offset in [20, 20, end, end, end, end + 2] {
            bytes.extend(offset.to_be_bytes());
        }
        bytes.extend(records.iter().flatten());
        bytes.extend([0, 0]);
        bytes
    }

    #[test]
    fn retains_original_records_including_ignored_operands_and_payload_age() {
        let records = [
            [0, 0, 255, 2, 0, 4],
            [0x7f, 0xff, 38, 250, 0x78, 0xe4],
            [0, 8, 254, 37, 1, 2],
        ];
        let decoded = timelines(&bank(&records)).unwrap();
        assert_eq!(
            decoded[0].iter().map(|r| r.to_bytes()).collect::<Vec<_>>(),
            records
        );
        let json = serde_json::to_vec(&decoded).unwrap();
        assert_eq!(
            serde_json::from_slice::<Vec<Vec<Record>>>(&json).unwrap(),
            decoded
        );
    }

    #[test]
    fn rejects_truncation_missing_end_bad_roots_and_control_payloads() {
        for records in [
            vec![[0, 0, 1, 0, 0, 0]],
            vec![[0, 0, 255, 2, 0, 4]],
            vec![[0, 0, 255, 2, 0, 4], [0, 8, 254, 0, 0, 0]],
        ] {
            assert!(timelines(&bank(&records)).is_err());
        }
        let valid = bank(&[[0, 8, 254, 0, 0, 0]]);
        for len in 0..valid.len() {
            assert!(timelines(&valid[..len]).is_err(), "length {len}");
        }
        for root in [0xffff_u16, 6, 0x7fff] {
            let mut invalid = valid.clone();
            invalid[26..28].copy_from_slice(&root.to_be_bytes());
            assert!(timelines(&invalid).is_err());
        }
    }

    fn particle_record(kind: u8) -> [u8; 352] {
        let mut bytes = [0; 352];
        bytes[0] = kind;
        bytes[0x32] = 255;
        bytes
    }

    #[test]
    fn decodes_signed_lifetimes_colors_and_distinct_geometry_layouts() {
        let mut bytes = particle_record(7);
        bytes[0x10..0x12].copy_from_slice(&(-2_i16).to_be_bytes());
        bytes[0x18..0x1a].copy_from_slice(&(-17_i16).to_be_bytes());
        bytes[0x40..0x44].copy_from_slice(&1.25_f32.to_be_bytes());
        bytes[0xc8..0xcc].copy_from_slice(&(-0.5_f32).to_be_bytes());
        bytes[0xe0..0xe4].copy_from_slice(&3_f32.to_be_bytes());
        let data = particle(&bytes).unwrap();
        assert_eq!(data.lifetime, -2);
        assert_eq!(data.state.colors[0][0], -17);
        assert_eq!(data.state.velocity, [1.25, 0., 0.]);
        assert!(matches!(
            data.state.geometry,
            resonance_content::battle_effect::ParticleGeometry::Size {
                acceleration: [-0.5, 0., 0.],
                ..
            }
        ));
        bytes[0] = 15;
        let data = particle(&bytes).unwrap();
        let resonance_content::battle_effect::ParticleGeometry::Quad { vertices, velocity } =
            data.state.geometry
        else {
            panic!("quad particle");
        };
        assert_eq!(vertices[2], [-0.5, 0., 0.]);
        assert_eq!(velocity[0], [3., 0., 0.]);
    }

    #[test]
    fn rejects_unprepared_particle_controllers_and_nonfinite_parameters() {
        let valid = particle_record(4);
        for length in 0..valid.len() {
            assert!(particle(&valid[..length]).is_err());
        }
        for (at, value) in [(0, 0), (0x32, 0), (0x91, 1)] {
            let mut bytes = valid;
            bytes[at] = value;
            assert!(particle(&bytes).is_err());
        }
        for flags in [0x80_u32, 0x8000, 0x0800_0000, 0x2000_0000] {
            let mut bytes = valid;
            bytes[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
            assert!(particle(&bytes).is_err(), "flags {flags:#x}");
        }
        for at in [0x34, 0x64, 0x70, 0xa4, 0xc8] {
            let mut bytes = valid;
            bytes[at..at + 4].copy_from_slice(&f32::NAN.to_be_bytes());
            assert!(particle(&bytes).is_err(), "offset {at:#x}");
        }
    }

    #[test]
    fn target_draw_order_and_element_tint_are_independent_particle_flags() {
        let mut bytes = particle_record(7);
        for draw_order in [0, 0x2000, 0x200000] {
            bytes[0x14..0x18].copy_from_slice(&(0x00400000_u32 | draw_order).to_be_bytes());
            for blend in 0..4 {
                bytes[1] = blend;
                let data = particle(&bytes).unwrap();
                assert_eq!(data.draw_after_target, draw_order != 0);
                assert!(data.element_tint);
            }
        }
    }

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_element_tints_match_the_published_fixture() -> Result<()> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = tempfile::tempdir()?;
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../game/tests/fixtures/effect-tints.json"))?;
        for disc in [1, 2] {
            let file = root.join(format!("disc{disc}/files/US_r_Top2Btl.rel"));
            let path = publish_tints(&file, output.path(), "battle")?;
            let actual: serde_json::Value =
                serde_json::from_slice(&std::fs::read(output.path().join(path))?)?;
            assert_eq!(actual, fixture);
        }
        Ok(())
    }

    #[test]
    fn modifier_boundaries_preserve_padding_and_reject_invalid_input() {
        let words = [7_u16, 0x40, 0x3f80, 0, 0, 0xbeef, 0xffff];
        let bytes: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
        assert_eq!(modifier(&bytes, 0).unwrap(), words);
        for length in 0..bytes.len() {
            assert!(modifier(&bytes[..length], 0).is_err(), "length {length}");
        }
        assert!(modifier(&bytes, usize::MAX).is_err());
        assert!(modifier(&bytes, bytes.len()).is_err());
        assert!(modifier(&[0, 31, 0xff, 0xff], 0).is_err());
    }

    #[test]
    fn program_prepares_unique_dependencies_and_checks_every_attachment() {
        let events = 20 + 352;
        let modifiers = events + 18;
        let roots = modifiers + 10;
        let mut bytes = b"ef1\0\x01\x00\x00\x00".to_vec();
        for offset in [20_u16, events, modifiers, modifiers, roots, roots + 2] {
            bytes.extend(offset.to_be_bytes());
        }
        bytes.extend(particle_record(15));
        for (age, command) in [(0_i16, 0), (4, 0), (8, 254)] {
            bytes.extend(
                Record {
                    age,
                    command,
                    argument: 0,
                    operand: modifiers,
                }
                .to_bytes(),
            );
        }
        let words = [11_u16, 0x5c, 1800, 0, 0xffff];
        bytes.extend(words.into_iter().flat_map(u16::to_be_bytes));
        bytes.extend([0, 0]);
        let source = program(&bytes, 0).unwrap();
        assert_eq!(source.records.len(), 3);
        assert_eq!(source.particles.len(), 1);
        assert_eq!(source.modifiers.len(), 1);
        assert_eq!(source.modifiers[&modifiers], words);
        assert!(program(&bytes, 1).is_err());

        // A later use of an already-decoded recipe must still validate its attachment.
        bytes[events as usize + 9] = 1;
        assert!(program(&bytes, 0).is_err());
        bytes[events as usize + 9] = 0;
        // Recipe 1 would alias the command section instead of an actor record.
        bytes[events as usize + 8] = 1;
        assert!(program(&bytes, 0).is_err());
    }

    #[test]
    #[ignore = "requires the extracted original disc"]
    fn original_casting_particle_dependencies_match_the_runtime_fixture() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat");
        let bytes = std::fs::read(path)?;
        let word = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let common = &bytes[word(12)..word(16)];
        let sources = [
            program(common, 3)?,
            program(common, 5)?,
            program(common, 7)?,
            program(common, 8)?,
        ];
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../game/tests/fixtures/casting-particle-sources.json"
        ))?;
        assert_eq!(serde_json::to_value(sources)?, expected);
        Ok(())
    }

    #[test]
    #[ignore = "requires the extracted original disc"]
    fn original_common_and_technique_banks_retain_all_190_timelines() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat");
        let bytes = std::fs::read(path)?;
        let word = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        for (member, count) in [(2, 52), (3, 138)] {
            let bank = &bytes[word(4 + member * 4)..word(8 + member * 4)];
            let decoded = timelines(bank)?;
            assert_eq!(decoded.len(), count);
            let half = |at| u16::from_be_bytes(bank[at..at + 2].try_into().unwrap()) as usize;
            for (i, records) in decoded.iter().enumerate() {
                let root = half(10) + half(half(16) + i * 2);
                let raw: Vec<_> = records.iter().flat_map(|r| r.to_bytes()).collect();
                assert_eq!(raw, bank[root..root + raw.len()]);
            }
        }
        Ok(())
    }
}

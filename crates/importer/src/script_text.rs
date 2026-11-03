//! Original scenario text becomes UTF-8 at import; executable words stay intact.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use symphonia_script::{message::TextEncoding, scenario};

/// Rebuild only the auxiliary offset table and its textual spans. Binary message
/// controls, alias indices, executable PCs and registry addresses are preserved.
pub fn normalize_scenario_text(bytes: &[u8], encoding: TextEncoding) -> Result<Vec<u8>> {
    let header = scenario::parse_header(bytes)?;
    if header.auxiliary_words == 0 {
        return Ok(bytes.to_vec());
    }
    let start = header.auxiliary_offset();
    let auxiliary = bytes
        .get(start..)
        .context("scenario auxiliary offset exceeds input")?;
    if auxiliary.is_empty() || auxiliary == [0, 0] || auxiliary.starts_with(&[0; 4]) {
        return Ok(bytes.to_vec());
    }
    let (_, offsets) = symphonia_script::message::auxiliary_offsets(auxiliary)
        .context("invalid scenario auxiliary offset table")?;
    let mut bodies = BTreeMap::new();
    for (index, &offset) in offsets.iter().enumerate() {
        if bodies.contains_key(&offset) {
            continue;
        }
        // The shared table reader guarantees nondecreasing starts. Its first
        // larger start is therefore the next body; equal starts are aliases.
        let end = offsets
            .iter()
            .copied()
            .find(|&next| next > offset)
            .unwrap_or(auxiliary.len());
        let body = normalize_body(&auxiliary[offset..end], encoding)
            .with_context(|| format!("scenario auxiliary string {index}"))?;
        bodies.insert(offset, body);
    }
    // One cooked table layout for both releases; source halfword offsets never
    // become another runtime mode. String/message indices remain unchanged.
    const WIDTH: usize = 4;
    let mut output = bytes[..start].to_vec();
    let table = output.len();
    output.resize(table + offsets.len() * WIDTH, 0);
    let mut relocated = BTreeMap::new();
    for (source, body) in bodies {
        relocated.insert(source, output.len() - table);
        output.extend(body);
    }
    for (index, source) in offsets.into_iter().enumerate() {
        let target = relocated[&source];
        let at = table + index * WIDTH;
        output[at..at + WIDTH].copy_from_slice(&u32::try_from(target)?.to_be_bytes());
    }
    // Code/header units are halfwords. Auxiliary text itself remains byte-addressed.
    if !output.len().is_multiple_of(2) {
        output.push(0);
    }
    ensure!(
        output.len() <= 4 * 1024 * 1024,
        "normalized scenario exceeds 4 MiB"
    );
    Ok(output)
}

fn normalize_body(bytes: &[u8], encoding: TextEncoding) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut at = 0;
    let mut text_start = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            0 | 1..=9 | 0x11 | 0x12 | 0x1f => {
                let text = &bytes[text_start..at];
                if let Some(decoded) = encoding.decode(text) {
                    result.extend(decoded.as_bytes());
                } else {
                    // Invalid text spans are source glyph bytes, already kept as
                    // Raw tokens in messages.json. They are never decoded at runtime.
                    result.extend_from_slice(text);
                }
                result.push(byte);
                at += 1;
                if byte == 0 {
                    result.extend_from_slice(&bytes[at..]);
                    return Ok(result);
                }
                if matches!(byte, 1..=9 | 0x11 | 0x12) {
                    let length = bytes[at..]
                        .windows(2)
                        .position(|word| word == [0x20, 0xff])
                        .context("unterminated scenario message expression")?
                        + 2;
                    result.extend_from_slice(&bytes[at..at + length]);
                    at += length;
                }
                text_start = at;
            }
            byte if encoding.takes_trailing_byte(byte, bytes.get(at + 1).copied()) => {
                ensure!(
                    bytes.get(at + 1).is_some_and(|&byte| byte != 0),
                    "truncated scenario character"
                );
                at += 2;
            }
            _ => at += 1,
        }
    }
    anyhow::bail!("unterminated scenario auxiliary string")
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia_script::{
        Program,
        message::{self, Token},
    };
    fn script(auxiliary: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff];
        bytes.extend_from_slice(auxiliary);
        bytes
    }
    #[test]
    fn aliases_offsets_and_binary_expressions_survive_utf8_expansion() -> Result<()> {
        let mut auxiliary = vec![0, 0, 0, 12, 0, 0, 0, 12, 0, 0, 0, 26];
        auxiliary.extend([
            0x82, 0xa0, 0x1f, 9, 0x02, 0x00, 0x12, 0x34, 0, 0x0b, 0x30, 0, 0x20, 0xff,
        ]);
        auxiliary.push(0);
        // Correct the second distinct start after the expression/terminator.
        let next = auxiliary.len() as u32;
        auxiliary[8..12].copy_from_slice(&next.to_be_bytes());
        auxiliary.extend([0x82, 0xa2, 0]);
        let before = script(&auxiliary);
        let expected = message::parse_with_encoding(&auxiliary, TextEncoding::ShiftJis)?;
        let after = normalize_scenario_text(&before, TextEncoding::ShiftJis)?;
        assert_eq!(&after[..10], &before[..10]);
        assert_eq!(message::parse(&after[10..])?, expected);
        assert_eq!(&after[10..14], &after[14..18]);
        assert_eq!(Program::decode(&after)?.string(2).as_deref(), Some("い"));
        assert!(matches!(
            &expected[0].tokens[1],
            Token::Control { opcode: 9, .. }
        ));
        Ok(())
    }

    #[test]
    fn descending_starts_and_nonadjacent_aliases_are_rejected_before_relocation() {
        for starts in [[12u32, 18, 15], [12, 15, 12]] {
            let mut auxiliary = starts
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect::<Vec<_>>();
            auxiliary.extend([0x82, 0xa0, 0, 0x82, 0xa2, 0, 0x82, 0xa4, 0]);
            assert!(message::auxiliary_offsets(&auxiliary).is_none());
            let error =
                normalize_scenario_text(&script(&auxiliary), TextEncoding::ShiftJis).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("invalid scenario auxiliary offset table")
            );
        }
    }

    #[test]
    fn short_source_tables_use_canonical_wide_offsets_without_reindexing() -> Result<()> {
        let mut auxiliary = vec![0, 4, 0, 4];
        for _ in 0..23000 {
            auxiliary.extend([0x82, 0xa0]);
        }
        auxiliary.push(0);
        let after = normalize_scenario_text(&script(&auxiliary), TextEncoding::ShiftJis)?;
        assert_eq!(&after[10..18], &[0, 0, 0, 8, 0, 0, 0, 8]);
        let program = Program::decode(&after)?;
        assert_eq!(program.string(0), program.string(1));
        assert_eq!(program.string(0).unwrap().chars().count(), 23000);
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_EXTRACTED; verifies native GC field, skit, world and battle script emission"]
    fn native_gc_emitters_preserve_messages_and_executable_words_as_utf8() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_EXTRACTED").context("set RESONANCE_TEST_EXTRACTED")?,
        );
        let output = tempfile::tempdir()?;
        crate::world_script::cook_world_script(&root, output.path())?;
        crate::battle_event_scripts::cook_battle_event_scripts(&root, output.path(), None)?;
        crate::field_core::cook_field_core(&root, output.path(), Some(332))?;
        crate::skit_assets::cook_skit_script(&root, output.path(), 1)?;
        for (source, target) in [
            ("field_sil.so", "world/scripts/scenario.ssb"),
            ("btl.so", "battle/scripts/common.ssb"),
        ] {
            let original = std::fs::read(root.join("disc1/files").join(source))?;
            let cooked = std::fs::read(output.path().join(target))?;
            let header = scenario::parse_header(&original)?;
            assert_eq!(
                &cooked[..header.auxiliary_offset()],
                &original[..header.auxiliary_offset()]
            );
            assert_eq!(
                message::parse(&cooked[header.auxiliary_offset()..])?,
                message::parse_with_encoding(
                    &original[header.auxiliary_offset()..],
                    TextEncoding::ShiftJis
                )?
            );
            Program::decode(&cooked)?;
        }
        for (script, messages) in [
            (
                "fields/iselia-0332/scripts/scenario.ssb",
                "fields/iselia-0332/messages.json",
            ),
            (
                "skits/whats-the-temple-like/scenario.ssb",
                "skits/whats-the-temple-like/messages.json",
            ),
        ] {
            let bytes = std::fs::read(output.path().join(script))?;
            let expected: Vec<message::Message> =
                serde_json::from_slice(&std::fs::read(output.path().join(messages))?)?;
            let header = scenario::parse_header(&bytes)?;
            assert_eq!(
                message::parse(&bytes[header.auxiliary_offset()..])?,
                expected
            );
            Program::decode(&bytes)?;
        }
        Ok(())
    }
}

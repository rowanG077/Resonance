//! Skit scripts, expression atlases and media clocks extracted without playback.
use crate::{dol, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::skit::{SkitCatalog, SkitResourcePaths};
use std::{collections::BTreeMap, fs, path::Path};
mod media;
mod portraits;

pub fn cook(extracted: &Path, output: &Path, ktx: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let mut catalog = SkitCatalog {
        version: 1,
        skits: definitions(&executable)?,
        resources: BTreeMap::new(),
        portraits: portraits::cook(extracted, output, ktx, &executable)?,
        media: media::cook(extracted, output, &executable)?,
    };
    for skit in &catalog.skits {
        let (table, index) = if skit.id < 120 {
            (0x8020f850, skit.id - 1)
        } else {
            (0x8020fad0, skit.id - 600)
        };
        let pointer = u32::from_be_bytes(
            dol::slice(&executable, table + u32::from(index) * 4, 4)?.try_into()?,
        );
        let source = extracted
            .join("files")
            .join(source_path(&executable, pointer)?);
        let package =
            fs::read(&source).with_context(|| format!("skit {}: {}", skit.id, source.display()))?;
        ensure!(
            package.len() >= 0x20 && package[..4] == 2u32.to_be_bytes(),
            "skit package is truncated: {}",
            source.display()
        );
        let offset = u32::from_be_bytes(package[4..8].try_into().unwrap()) as usize;
        let size = u32::from_be_bytes(package[8..12].try_into().unwrap()) as usize;
        ensure!(
            offset == 0x20 && offset.checked_add(size) == Some(package.len()),
            "invalid skit package: {}",
            source.display()
        );
        let scenario = &package[offset..];
        let header = symphonia_script::scenario::parse_header(scenario)
            .map_err(|e| anyhow::anyhow!("{}: {e}", source.display()))?;
        let messages = symphonia_script::message::parse(&scenario[header.auxiliary_offset()..])
            .map_err(|e| anyhow::anyhow!("{}: {e}", source.display()))?;
        symphonia_script::Program::decode(scenario)
            .map_err(|e| anyhow::anyhow!("{}: {e}", source.display()))?;
        let script_path = format!("game/skits/{:03}.ssb", skit.id);
        let messages_path = format!("game/skits/{:03}.messages.json", skit.id);
        write_atomic(&output.join(&script_path), scenario)?;
        write_atomic(
            &output.join(&messages_path),
            &serde_json::to_vec_pretty(&messages)?,
        )?;
        catalog.resources.insert(
            skit.id,
            SkitResourcePaths {
                script: script_path,
                messages: messages_path,
            },
        );
    }
    catalog.validate()?;
    let path = "game/skits.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&catalog)?)?;
    Ok(path.into())
}

fn source_path(executable: &[u8], pointer: u32) -> Result<String> {
    let bytes = dol::slice(executable, pointer, 96)?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .context("unterminated skit resource path")?;
    let path = std::str::from_utf8(&bytes[..end])?;
    let (dir, file) = path
        .split_once('/')
        .context("skit resource directory missing")?;
    let path = format!("{}/{file}", dir.to_ascii_uppercase());
    resonance_content::validate_asset_path(&path)?;
    Ok(path)
}

/// Refresh existing fields after cooking shared skit content independently.
pub fn cook_all(extracted: &Path, output: &Path, ktx: &Path) -> Result<()> {
    let path = cook(extracted, output, ktx)?;
    let names = crate::session::cook_text(extracted, output)?;
    crate::field::refresh_shared(output, &[path, names])
}

pub(crate) fn definitions(
    executable: &[u8],
) -> Result<Vec<resonance_content::skit::SkitDefinition>> {
    use resonance_content::skit::{SkitCondition, SkitDefinition, SkitLocation};
    let mut definitions = Vec::new();
    // Story and timed notifications share the same 24-byte definition layout.
    for (base, first, count) in [(0x8020ac10, 1, 119), (0x8020bc78, 600, 259)] {
        for (index, row) in dol::slice(executable, base, count * 24)?
            .chunks_exact(24)
            .enumerate()
        {
            let half = |at| u16::from_be_bytes(row[at..at + 2].try_into().unwrap());
            let word = |at| i32::from_be_bytes(row[at..at + 4].try_into().unwrap());
            let id = half(0);
            ensure!(usize::from(id) == first + index, "invalid skit table order");
            let range = [word(4), word(8)];
            if range == [-1; 2] {
                continue;
            }
            let address = word(20) as u32;
            let mut bytes = Vec::new();
            for offset in 0..=160 {
                let byte = dol::slice(executable, address + offset, 1)?[0];
                if byte == 0 {
                    break;
                }
                bytes.push(byte);
            }
            ensure!(bytes.len() <= 160, "unterminated skit title");
            let (title, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
            ensure!(!invalid, "invalid skit title encoding");
            let location = match half(14) as i16 {
                -9999 => SkitLocation::Anywhere,
                -1 => SkitLocation::Overworld(None),
                -2 => SkitLocation::Overworld(Some(0)),
                -3 => SkitLocation::Overworld(Some(1)),
                -4 => SkitLocation::Field,
                id if id >= 0 => SkitLocation::Map(id as u16),
                other => anyhow::bail!("unknown skit location {other}"),
            };
            definitions.push(SkitDefinition {
                id,
                title: title.into_owned(),
                story: (range != [-999_999_999; 2]).then_some(range),
                party_mask: half(12),
                location,
                condition: match (row[16], id) {
                    (0, _) => SkitCondition::None,
                    (_, 600) => SkitCondition::Maps([330, 346]),
                    _ => SkitCondition::Unimplemented,
                },
            });
        }
    }
    Ok(definitions)
}

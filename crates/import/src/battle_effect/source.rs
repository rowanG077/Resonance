//! Cook original declarations without requiring runtime controller support.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{SourceBank, UvRecord};
use std::{collections::BTreeMap, path::Path};

pub fn read(bytes: &[u8]) -> Result<SourceBank> {
    let programs = super::timelines(bytes)?;
    let half = |i| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
    let offsets: Vec<_> = (8..20).step_by(2).map(half).collect();
    let pool = |start| {
        let end = offsets
            .iter()
            .copied()
            .filter(|&i| i > start)
            .min()
            .unwrap_or(bytes.len());
        &bytes[start..end]
    };
    // The table starts immediately after the data pools, even when an empty pool
    // shares its offset. Do not interpret the table as actor/UV declarations.
    let data_pool = |start| {
        if start == offsets[4] {
            &bytes[start..start]
        } else {
            pool(start)
        }
    };
    // Empty actor pools can share the next pool's start (the stage bank's
    // first bytes are a modifier terminator, not a partial actor declaration).
    let actors = if offsets[1..].contains(&offsets[0]) {
        &bytes[offsets[0]..offsets[0]]
    } else {
        data_pool(offsets[0])
    };
    ensure!(
        actors.len().is_multiple_of(352),
        "misaligned effect actor pool"
    );
    let actors = actors
        .chunks_exact(352)
        .map(super::declaration::read)
        .collect::<Result<Vec<_>>>()?;
    let mut modifiers = BTreeMap::new();
    for record in programs.iter().flatten() {
        if record.command < 252 {
            ensure!(
                usize::from(record.command) < actors.len(),
                "effect actor outside declaration pool"
            );
        }
        if record.command < 254 && record.command != 252 && record.operand != 0 {
            let at = usize::from(record.operand);
            let section = data_pool(offsets[2]);
            ensure!(
                (offsets[2]..offsets[2] + section.len()).contains(&at),
                "effect modifier outside modifier pool"
            );
            let words = super::modifier(&bytes[..offsets[2] + section.len()], at)?;
            modifiers.insert(record.operand, words);
        }
    }
    let uv = data_pool(offsets[3]);
    ensure!(uv.len().is_multiple_of(10), "misaligned effect UV pool");
    let uv: Vec<_> = uv
        .chunks_exact(10)
        .map(|row| UvRecord {
            timing: row[0],
            control: row[1],
            values: std::array::from_fn(|i| i16::from_be_bytes([row[2 + 2 * i], row[3 + 2 * i]])),
        })
        .collect();
    let uv_roots = bytes
        .get(offsets[5]..offsets[5] + usize::from(bytes[5]) * 2)
        .context("truncated effect UV table")?
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect::<Vec<_>>();
    ensure!(
        uv_roots
            .iter()
            .all(|&r| r.is_multiple_of(10) && usize::from(r / 10) < uv.len()),
        "effect UV root outside row pool"
    );
    Ok(SourceBank {
        art: None,
        source_sha256: crate::digest(bytes),
        programs,
        actors,
        modifiers,
        uv,
        uv_roots,
    })
}

pub fn publish(usual: &[u8], output: &Path, prefix: &str) -> Result<Vec<String>> {
    let banks = [2, 3]
        .into_iter()
        .map(|member| read(crate::source_assets::section(usual, member)?))
        .collect::<Result<Vec<_>>>()?;
    let art = super::art::publish(usual, &banks, output)?;
    let mut paths = ["common", "techniques"]
        .into_iter()
        .zip(banks)
        .map(|(name, mut bank)| {
            bank.art = Some(art.clone());
            let path = format!("{prefix}/effects/{name}.json");
            crate::write_atomic(&output.join(&path), &serde_json::to_vec(&bank)?)?;
            Ok(path)
        })
        .collect::<Result<Vec<_>>>()?;
    paths.extend(art.files.into_keys());
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(kind: u8) -> Vec<u8> {
        let mut bytes = b"ef1\0\x01\x00\x00\x00".to_vec();
        for offset in [20_u16, 372, 384, 384, 384, 386] {
            bytes.extend(offset.to_be_bytes());
        }
        let mut actor = [0; 352];
        actor[0] = kind;
        actor[0x32] = 255;
        bytes.extend(actor);
        bytes.extend([0, 0, 0, 0, 0, 0, 0, 2, 254, 0, 0, 0]);
        bytes.extend([0, 0]);
        bytes
    }

    #[test]
    fn empty_actor_pool_can_alias_the_modifier_pool() {
        let mut bytes = b"ef1\0\x01\x00\x00\x00".to_vec();
        for offset in [32_u16, 36, 32, 42, 42, 44] {
            bytes.extend(offset.to_be_bytes());
        }
        bytes.resize(32, 0);
        bytes.extend([255, 255, 0, 0, 0, 0, 254, 0, 0, 0, 0, 0]);
        bytes.resize(64, 0);
        let bank = read(&bytes).unwrap();
        assert!(bank.actors.is_empty());
        assert_eq!(bank.programs.len(), 1);
        assert_eq!(bank.programs[0].len(), 1);
        assert_eq!(bank.programs[0][0].command, 254);
    }

    #[test]
    fn complete_source_cooking_is_independent_of_controller_admission() {
        for kind in [3, 4, 5, 22, 23, 24, 25, 255] {
            let bytes = bank(kind);
            let source = read(&bytes).unwrap();
            assert_eq!(source.actors[0].prefix.kind, kind);
            assert_eq!(source.program(0).is_ok(), matches!(kind, 3..=5));
            for length in 0..bytes.len() {
                assert!(read(&bytes[..length]).is_err(), "length {length}");
            }
        }
        let mut bytes = bank(4);
        bytes[374] = 1;
        assert!(read(&bytes).is_err()); // missing actor declaration
        bytes[374] = 0;
        bytes[376..378].copy_from_slice(&20_u16.to_be_bytes());
        assert!(read(&bytes).is_err()); // modifier points into the actor pool
    }

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_effect_banks_preserve_all_declarations_and_command_dependencies() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = Vec::new();
        for disc in [1, 2] {
            for (index, programs, actors) in [(2, 52, 87), (3, 138, 142)] {
                let bytes =
                    std::fs::read(local.join(format!("disc{disc}/files/BTL/BTLusual.dat")))?;
                let bytes = crate::source_assets::section(&bytes, index)?;
                let bank = read(bytes)?;
                assert_eq!(bank.programs.len(), programs);
                assert_eq!(bank.actors.len(), actors);
                let json = serde_json::to_vec(&bank)?;
                let restored: SourceBank = serde_json::from_slice(&json)?;
                let start = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;
                for (i, actor) in restored.actors.iter().enumerate() {
                    assert_eq!(
                        super::super::declaration::source_bytes(actor)?,
                        bytes[start + i * 352..start + (i + 1) * 352]
                    );
                }
                for (&at, words) in &restored.modifiers {
                    let raw: Vec<_> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
                    assert_eq!(raw, bytes[usize::from(at)..usize::from(at) + raw.len()]);
                }
                if index == 2 {
                    let fixture: serde_json::Value = serde_json::from_str(include_str!(
                        "../../../battle/tests/fixtures/stun-particle-source.json"
                    ))?;
                    assert_eq!(restored.source_sha256, fixture["source_sha256"]);
                    assert_eq!(
                        serde_json::to_value(&restored.actors[19])?,
                        fixture["declaration"]
                    );
                    assert_eq!(serde_json::to_value(&restored.uv[..5])?, fixture["uv"]);
                    assert_eq!(restored.particle(19)?.uv_track, restored.uv);
                    assert!(!restored.particle(19)?.late);
                    for member in [49, 50] {
                        assert!(restored.particle(member)?.late);
                    }
                    let fixture: serde_json::Value = serde_json::from_str(include_str!(
                        "../../../battle/tests/fixtures/nurse-followed-particle.json"
                    ))?;
                    assert_eq!(restored.source_sha256, fixture["source_sha256"]);
                    assert_eq!(
                        serde_json::to_value(&restored.actors[61])?,
                        fixture["declaration"]
                    );
                    assert!(restored.particle(61)?.follow_origin);
                    let fixture: serde_json::Value = serde_json::from_str(include_str!(
                        "../../../game/tests/fixtures/nurse-trails.json"
                    ))?;
                    assert_eq!(restored.source_sha256, fixture["source_sha256"]);
                    assert_eq!(
                        serde_json::to_value(&restored.actors[63])?,
                        fixture["declaration"]
                    );
                    assert_eq!(
                        serde_json::to_value(&restored.modifiers[&32948])?,
                        fixture["modifier"]
                    );
                    assert!(matches!(
                        restored.particle(63)?.state.geometry,
                        resonance_content::battle_effect::ParticleGeometry::BillboardTrail { .. }
                    ));
                } else {
                    let fixture: serde_json::Value = serde_json::from_str(include_str!(
                        "../../../game/tests/fixtures/lightning-effect-source.json"
                    ))?;
                    assert_eq!(restored.source_sha256, fixture["source_sha256"]);
                    assert_eq!(
                        serde_json::to_value(restored.program(28)?)?,
                        fixture["program"]
                    );
                }
                if disc == 1 {
                    previous.push(json);
                } else {
                    assert_eq!(json, previous[index - 2]);
                }
            }
        }
        Ok(())
    }
}

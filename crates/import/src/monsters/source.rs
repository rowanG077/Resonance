//! Shared enemy statistics consumed by the Monster Book and battle preparation.
use crate::read::{u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use resonance_content::monster::MonsterStats;

pub(super) struct Record<'a> {
    pub metadata: &'a [u8],
    pub statistics: Vec<MonsterStats>,
    pub attack_element: u8,
    pub affinities: [i8; 9],
    pub family: u16,
    pub drops: [u16; 2],
    pub drop_chances: [u8; 2],
    pub grade: i16,
    pub steal: u16,
}

pub(super) fn read(bytes: &[u8]) -> Result<Record<'_>> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("enemy metadata exceeds package")?;
    ensure!(metadata.len() >= 0x1ec, "truncated enemy metadata");
    let stats = bytes
        .get(usize::from(half(bytes, 6)?)..)
        .context("enemy statistics exceed package")?;
    ensure!(stats.len() >= 60, "truncated enemy statistics");
    let positive =
        |source: &[u8], at| -> Result<u16> { Ok((half(source, at)? as i16).try_into()?) };
    let mut statistics = vec![MonsterStats {
        hp: word(stats, 32)?,
        tp: positive(stats, 40)?,
        initial_hp: word(stats, 36)?,
        initial_tp: positive(stats, 42)?,
        attack: positive(stats, 44)?,
        thrust: half(stats, 46)? as i16,
        defense: positive(stats, 48)?,
        intelligence: half(stats, 50)? as i16,
        accuracy: half(stats, 52)? as i16,
        evasion: half(stats, 54)? as i16,
        luck: stats[56],
        level: stats[29],
        experience: word(metadata, 0x40)?,
        gald: word(metadata, 0x44)?,
    }];
    let count = usize::from(metadata[0x1e7]);
    if count != 0 {
        let start = word(bytes, 0x1e0)? as usize;
        ensure!(start >= 0x1e8, "missing enemy variant table");
        for row in bytes
            .get(start..start + count * 36)
            .context("truncated enemy variants")?
            .chunks_exact(36)
        {
            statistics.push(MonsterStats {
                hp: word(row, 0)?,
                tp: positive(row, 8)?,
                initial_hp: word(row, 4)?,
                initial_tp: positive(row, 10)?,
                attack: positive(row, 20)?,
                thrust: half(row, 22)? as i16,
                defense: positive(row, 24)?,
                intelligence: half(row, 26)? as i16,
                accuracy: half(row, 28)? as i16,
                evasion: half(row, 30)? as i16,
                luck: row[32],
                level: row[33],
                experience: word(row, 12)?,
                gald: word(row, 16)?,
            });
        }
    }
    Ok(Record {
        metadata,
        statistics,
        attack_element: metadata[0],
        affinities: std::array::from_fn(|i| metadata[1 + i] as i8),
        family: half(metadata, 0x2a)?,
        drops: [half(metadata, 0x48)?, half(metadata, 0x4a)?],
        drop_chances: [metadata[0x4e], metadata[0x4f]],
        grade: half(metadata, 0x1ea)? as i16,
        steal: half(metadata, 0x4c)?,
    })
}

pub(super) fn clips(bytes: &[u8]) -> Result<Vec<u16>> {
    let count = word(bytes, 0x14)?.max(31) as usize;
    ensure!(
        count <= (0x160 - 0x20) / 4,
        "enemy motion count overlaps attachments"
    );
    let mut clips = Vec::new();
    for index in 0..count {
        let offset = word(bytes, 0x20 + index * 4)? as usize;
        if offset == 0 {
            continue;
        }
        let clip = bytes
            .get(offset..)
            .context("enemy motion exceeds package")?;
        ensure!(
            clip.starts_with(&0x007b7960u32.to_be_bytes()),
            "invalid enemy motion {index}"
        );
        clips.push(index as u16);
    }
    Ok(clips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_enemy_variants_account_for_formations_and_match_opening_oracle_stats() -> Result<()>
    {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../battle/tests/fixtures/opening-damage.json"
        ))?;
        let expected = &oracle["observations"][0]["target"];
        let mut first = None;
        for disc in [1, 2] {
            let extracted = root.join(format!("disc{disc}"));
            let sources = crate::source_assets::Sources::read(&extracted)?;
            let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
            let archive = extracted.join("files").join(sources.enemy);
            let mut records = Vec::new();
            for id in 0..resonance_content::monster::MONSTER_COUNT {
                let bytes = crate::source_assets::enemy_package(&archive, &usual, id as u16)?;
                let record = read(&bytes)?;
                assert!(record.drop_chances.iter().all(|&chance| chance <= 100));
                match id {
                    36 => {
                        assert_eq!(record.grade, 0);
                        assert_eq!(record.drops, [1, 50]);
                        assert_eq!(record.drop_chances, [20, 8]);
                        assert_eq!(
                            (record.statistics[0].experience, record.statistics[0].gald),
                            (8, 12)
                        );
                    }
                    49 => {
                        assert_eq!(record.grade, 0);
                        assert_eq!(record.drops, [1, 10]);
                        assert_eq!(record.drop_chances, [15, 5]);
                        assert_eq!(
                            (record.statistics[1].experience, record.statistics[1].gald),
                            (9, 8)
                        );
                    }
                    _ => {}
                }
                records.push((record.statistics, record.affinities));
            }
            let mut unresolved = Vec::new();
            let formations = crate::battle_formation::read(&usual)?.records;
            for (index, formation) in formations.iter().enumerate() {
                for actor in &formation.actors[..usize::from(formation.actor_count)] {
                    let id = usize::try_from(formation.resources[usize::from(actor.resource)])?;
                    if usize::from(actor.variant) >= records[id].0.len() {
                        unresolved.push((index, id, actor.variant));
                    }
                }
            }
            // Formation 90 asks for Sword Dancer variant 3. Its actual table
            // contains two variants, followed by model bytes. Keep this source
            // discrepancy visible; reachability is not established here.
            assert_eq!(unresolved, [(90, 191, 3)]);
            let opening = &formations[2];
            let actor = opening.actors[0];
            assert_eq!(opening.resources[usize::from(actor.resource)], 49);
            assert_eq!(actor.variant, 1);
            let stats = &records[49].0[usize::from(actor.variant)];
            for (name, value) in [
                ("slash", i64::from(stats.attack)),
                ("thrust", i64::from(stats.thrust)),
                ("defense", i64::from(stats.defense)),
                ("intelligence", i64::from(stats.intelligence)),
                ("accuracy", i64::from(stats.accuracy)),
                ("evasion", i64::from(stats.evasion)),
                ("level", i64::from(stats.level)),
            ] {
                assert_eq!(Some(value), expected["stats"][name].as_i64(), "{name}");
            }
            assert_eq!(Some(u64::from(stats.hp)), expected["max_hp"].as_u64());
            assert_eq!(Some(u64::from(stats.luck)), expected["luck"].as_u64());
            assert_eq!(records[49].1[0], 2);
            assert_eq!(oracle["observations"][0]["affinity"], "resistant");
            if let Some(first) = &first {
                assert_eq!(&records, first);
            }
            first = Some(records);
        }
        Ok(())
    }
}

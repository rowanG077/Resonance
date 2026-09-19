//! Only the enemy fields consumed by the Monster Book, read from original packages.
use crate::read::{u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use resonance_content::monster::MonsterStats;

pub(super) struct Record<'a> {
    pub metadata: &'a [u8],
    pub statistics: Vec<MonsterStats>,
    pub attack_element: u8,
    pub affinities: [i8; 8],
    pub family: u16,
    pub drops: [u16; 2],
    pub steal: u16,
}

pub(super) fn read(bytes: &[u8]) -> Result<Record<'_>> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("enemy metadata exceeds package")?;
    ensure!(metadata.len() >= 0x1e8, "truncated enemy metadata");
    let stats = bytes
        .get(usize::from(half(bytes, 6)?)..)
        .context("enemy statistics exceed package")?;
    ensure!(stats.len() >= 60, "truncated enemy statistics");
    let positive =
        |source: &[u8], at| -> Result<u16> { Ok((half(source, at)? as i16).try_into()?) };
    let mut statistics = vec![MonsterStats {
        hp: word(stats, 32)?,
        tp: positive(stats, 40)?,
        attack: positive(stats, 44)?,
        defense: positive(stats, 48)?,
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
                attack: positive(row, 20)?,
                defense: positive(row, 24)?,
                experience: word(row, 12)?,
                gald: word(row, 16)?,
            });
        }
    }
    Ok(Record {
        metadata,
        statistics,
        attack_element: metadata[0],
        affinities: std::array::from_fn(|i| metadata[2 + i] as i8),
        family: half(metadata, 0x2a)?,
        drops: [half(metadata, 0x48)?, half(metadata, 0x4a)?],
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

//! Native battle package directories into human-readable scene and skill IDs.
use crate::{dol, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::library::Mappings;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::{BattleSceneId, BattleSkillId};

fn physical(offsets: &[u32], length: u64) -> Result<Vec<u16>> {
    let eof = offsets
        .iter()
        .position(|&offset| u64::from(offset) == length)
        .context("battle package directory has no EOF")?;
    ensure!(
        eof > 1 && offsets[eof + 1..].iter().all(|&offset| offset == 0),
        "invalid battle package EOF"
    );
    let mut seen = BTreeSet::new();
    let mut ranges = Vec::new();
    for slot in 1..eof {
        let start = offsets[slot];
        if (start == 0 && slot != 1) || !seen.insert(start) {
            continue;
        }
        let end = offsets[slot + 1..=eof]
            .iter()
            .copied()
            .find(|&end| end > start)
            .context("battle package has no end")?;
        ensure!(
            start < end && u64::from(end) <= length,
            "invalid battle package range"
        );
        ranges.push((u16::try_from(slot)?, start, end));
    }
    ranges.sort_by_key(|&(_, start, _)| start);
    let mut cursor = 0;
    for &(_, start, end) in &ranges {
        ensure!(
            start == cursor,
            "battle package directory has a gap or overlap"
        );
        cursor = end;
    }
    ensure!(
        u64::from(cursor) == length,
        "battle package directory stops before EOF"
    );
    Ok(ranges.into_iter().map(|(slot, _, _)| slot).collect())
}

fn directory(module: &Rel, offset: usize, count: usize, archive: &Path) -> Result<Vec<u16>> {
    let row = module.slice(5, offset, count * 4)?;
    let offsets = (0..count)
        .map(|i| dol::u32(row, i * 4))
        .collect::<Result<Vec<_>>>()?;
    physical(&offsets, fs::metadata(archive)?.len())
}

fn write(library: &Path, relative: &str, names: BTreeMap<u32, String>) -> Result<()> {
    let output = library.join(relative);
    ensure!(!output.exists(), "battle scene mappings already exist");
    crate::output::json(&output, &Mappings::new(names)?)?;
    Ok(())
}

/// Read the physical spell directory; absent resident spells have no stored scene.
pub fn cook_battle_scene_mappings(root: &Path, library: &Path) -> Result<()> {
    let btl = root.join("disc1/files/BTL");
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let slots = directory(&module, 0xfe0, 0x1e4 / 4, &btl.join("BTLmagic.dat"))?;
    let mut names = BTreeMap::new();
    for slot in slots {
        let id = slot + 200;
        let name = BattleSceneId::try_from(id)?
            .resource_name()
            .context("stored scene has no human-readable name")?;
        names.insert(u32::from(id), name.to_owned());
    }
    ensure!(
        names.len() == BattleSceneId::ALL.len(),
        "incomplete stored scene directory"
    );
    write(library, "battle/scenes/mappings.json", names)
}

/// Bind playable skill IDs to their native physical package slots.
pub fn cook_battle_skill_mappings(root: &Path, library: &Path) -> Result<()> {
    let btl = root.join("disc1/files/BTL");
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let slots = directory(&module, 0x11c4, 4, &btl.join("BTLskill.dat"))?;
    let executable = fs::read(root.join("disc1/sys/main.dol"))?;
    let definitions = dol::slice(&executable, 0x80202f90, 253 * 88)?;
    let mut names = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (id, row) in definitions.chunks_exact(88).enumerate() {
        let slot = dol::u32(row, 0x50)?;
        if slot == 0 {
            continue;
        }
        ensure!(
            slots.contains(&u16::try_from(slot)?),
            "skill selects missing package {slot}"
        );
        let id = u16::try_from(id)?;
        let name = BattleSkillId::try_from(id)?
            .resource_name()
            .context("stored skill has no human-readable name")?;
        names.insert(u32::from(id), name.to_owned());
        used.insert(slot);
    }
    ensure!(
        used.len() == slots.len() && names.len() == BattleSkillId::ALL.len(),
        "incomplete native skill bindings"
    );
    write(library, "battle/skills/mappings.json", names)
}

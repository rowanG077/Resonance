//! Native enemy directory IDs into readable battle resource names.
use crate::dol;
use anyhow::{Context, Result, ensure};
use resonance_content::library::Mappings;
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::MonsterId;

pub fn cook_battle_enemy_mappings(root: &Path, library: &Path) -> Result<()> {
    let disc = root.join("disc1/files/BTL");
    let usual = fs::read(disc.join("BTLusual.dat"))?;
    ensure!(dol::u32(&usual, 0)? > 10, "enemy directory is absent");
    let start = dol::u32(&usual, 4 + 10 * 4)? as usize;
    let end = dol::u32(&usual, 4 + 11 * 4)? as usize;
    let directory = usual.get(start..end).context("invalid enemy directory")?;
    let length = fs::metadata(disc.join("BTLenemy.dat"))?.len();
    let mut names = BTreeMap::new();
    for id in MonsterId::ALL {
        let slot = usize::from(id.raw()) * 4;
        let first = u64::from(dol::u32(directory, slot)?);
        let last = u64::from(dol::u32(directory, slot + 4)?);
        ensure!(
            first < last && last <= length,
            "enemy {} has no native package",
            id.raw()
        );
        let name = id
            .resource_name()
            .context("enemy ID has no readable name")?;
        names.insert(u32::from(id.raw()), name.to_owned());
    }
    ensure!(names.len() == 251, "incomplete enemy catalogue");
    let mappings = Mappings::new(names)?;
    let output = library.join("battle/enemies/mappings.json");
    ensure!(!output.exists(), "battle enemy mappings already exist");
    crate::output::json(&output, &mappings)?;
    Ok(())
}

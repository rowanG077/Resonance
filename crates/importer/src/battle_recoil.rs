//! Native battle hit impulses and guard defaults in one editable table.
use crate::rel::Rel;
use anyhow::Result;
use resonance_content::{
    battle_recoil::{self, Table, strategy_guard_recovery},
    battle_rules::Recoil,
    library::Mappings,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::{CharacterId, KnownId};

fn number(row: &[u8]) -> f32 {
    f32::from_bits(u32::from_be_bytes(row.try_into().unwrap()))
}

pub fn cook_battle_recoil(extracted: &Path, library: &Path) -> Result<()> {
    let rel = Rel::read(fs::read(extracted.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let constants = rel.slice(4, 0x3a58, 20)?;
    let defaults = rel.slice(4, 0x10ef, 11)?;
    let impulses = rel
        .slice(5, 0x5600, 19 * 8)?
        .chunks_exact(8)
        .map(|row| [number(&row[..4]), number(&row[4..])])
        .collect();
    let table = Table {
        impulses,
        suppression_distance: number(&constants[..4]),
        guard_speed: number(&constants[16..20]),
        light_vertical_scale: number(&constants[8..12]),
        heavy_vertical_scale: number(&constants[12..16]),
        default_guard_recovery_bonuses: (1..=9)
            .map(|id| (id as u8, strategy_guard_recovery(defaults[id])))
            .collect(),
    };
    let names = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect::<BTreeMap<_, _>>(),
    )?;
    let document = Recoil::from_table(table, &names)?;
    let path = library.join(battle_recoil::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

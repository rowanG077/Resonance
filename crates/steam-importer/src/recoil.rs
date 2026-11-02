//! Executable recoil constants into the same document as the GameCube importer.
use crate::{
    pe::{self, Image},
    source::Source,
};
use anyhow::{Result, ensure};
use resonance_content::{
    battle_recoil::{self, Table, strategy_guard_recovery},
    battle_rules::Recoil,
    library::Mappings,
};
use std::{collections::BTreeMap, path::Path};
use symphonia_constants::{CharacterId, KnownId};

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    let image = Image::parse(&source.executable)?;
    let strategy = strategy_defaults(&image)?;
    let double = |address| -> Result<f32> {
        let value = f64::from_le_bytes(image.slice(address, 8)?.try_into()?) as f32;
        ensure!(value.is_finite(), "non-finite Steam recoil constant");
        Ok(value)
    };
    let table = Table {
        impulses: image
            .slice(0x918f98, 19 * 8)?
            .chunks_exact(8)
            .map(|row| {
                let value = [
                    f32::from_bits(pe::word(row, 0)?),
                    f32::from_bits(pe::word(row, 4)?),
                ];
                ensure!(
                    value.iter().all(|v| v.is_finite()),
                    "invalid Steam recoil impulse"
                );
                Ok(value)
            })
            .collect::<Result<_>>()?,
        suppression_distance: double(0x882e08)?,
        guard_speed: f32::from_bits(pe::word(image.slice(0x882bd0, 4)?, 0)?),
        light_vertical_scale: double(0x882e00)?,
        heavy_vertical_scale: double(0x882a80)?,
        default_guard_recovery_bonuses: (1..=9)
            .map(|id| (id as u8, strategy_guard_recovery(strategy[19 + id])))
            .collect(),
    };
    let names = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect::<BTreeMap<_, _>>(),
    )?;
    crate::output::json(
        &output.join(battle_recoil::PATH),
        &Recoil::from_table(table, &names)?,
    )
}

pub(crate) fn strategy_defaults(image: &Image<'_>) -> Result<Vec<u8>> {
    let mut strategy = Vec::new();
    for index in 0..7u32 {
        let instruction = image.slice(0x44446c + index * 7, 7)?;
        ensure!(
            instruction[..3] == [0xc7, 0x45, 0xdc + index as u8 * 4],
            "Steam strategy initializer changed"
        );
        strategy.extend_from_slice(&instruction[3..]);
    }
    let tail = image.slice(0x44449d, 6)?;
    ensure!(
        tail[..4] == [0x66, 0xc7, 0x45, 0xf8],
        "Steam strategy initializer tail changed"
    );
    strategy.extend_from_slice(&tail[4..]);
    Ok(strategy)
}

//! Native field declarations directly into named field mappings and foraging rules.
use crate::{
    pe,
    source::{Source, validate_name},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    foraging,
    library::{FIELDS, Mappings},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_constants::{FieldId, ItemId};

const PHASES: u32 = 0x8cb908;
const POOLS: [&str; 6] = [
    "meat",
    "fish",
    "vegetables",
    "fruit",
    "starches",
    "other-food",
];

fn field_name(id: u32) -> String {
    u16::try_from(id)
        .ok()
        .and_then(|raw| FieldId::try_from(raw).ok())
        .and_then(FieldId::resource_name)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("field-{id:04}"))
}

fn choices(selector: u8) -> Result<&'static [usize]> {
    Ok(match selector {
        0 => &[],
        1 => &[0],
        2 => &[1],
        3 => &[2],
        4 => &[0, 2],
        5 => &[3, 2],
        6 => &[3],
        7 => &[0, 3],
        8 => &[4],
        9 => &[0, 4],
        10 => &[5, 4],
        11 => &[5],
        12 => &[0, 1, 2, 3, 4, 5],
        13 => &[0, 3, 2],
        _ => anyhow::bail!("unknown Item Finder selector {selector}"),
    })
}

fn convert(source: &Source) -> Result<(Mappings, foraging::Document)> {
    let image = pe::Image::parse(&source.executable)?;
    let bound = image.slice(0x4c37e2, 7)?;
    ensure!(
        bound[0] == 0x3d && bound[5] == 0x77,
        "Steam field bound changed"
    );
    let count = pe::word(bound, 1)? as usize + 1;
    ensure!(count <= 4096, "invalid Steam field count");
    ensure!(
        pe::word(image.slice(PHASES + count as u32 * 24, 4)?, 0)? == 0,
        "missing Steam field terminator"
    );
    let mut fields = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut areas: BTreeMap<Vec<usize>, Vec<String>> = BTreeMap::new();
    for (id, row) in image
        .slice(PHASES, count * 24)?
        .chunks_exact(24)
        .enumerate()
    {
        let pointer = pe::word(row, 0)?;
        if pointer == 0 {
            continue;
        }
        let resource = image.text(pointer)?;
        validate_name(resource)?;
        if !source.contains(resource)? {
            continue;
        }
        let name = field_name(id as u32);
        ensure!(names.insert(name.clone()), "duplicate field name {name}");
        fields.insert(id as u32, name.clone());
        let selected = choices(row[5] >> 4).with_context(|| format!("field {id}"))?;
        if !selected.is_empty() {
            areas.entry(selected.to_vec()).or_default().push(name);
        }
    }
    let items: BTreeMap<_, _> = ItemId::ALL
        .iter()
        .filter_map(|id| id.resource_name().map(|name| (id.raw(), name)))
        .collect();
    let pools = POOLS
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let function = image.slice(0x4a83b0 + index as u32 * 32, 23)?;
            ensure!(
                function[7] == 0xb9
                    && function[12..18] == [0xf7, 0xf9, 0x66, 0x8b, 4, 0x55]
                    && function[22] == 0xc3,
                "Steam Item Finder pool changed"
            );
            let count = pe::word(function, 8)? as usize;
            ensure!((1..=32).contains(&count), "invalid Item Finder pool size");
            let address = pe::word(function, 18)?;
            let values = image
                .slice(address, count * 2)?
                .chunks_exact(2)
                .map(|row| {
                    let id = pe::half(row, 0)?;
                    Ok(items
                        .get(&id)
                        .with_context(|| format!("unnamed forage item {id}"))?
                        .to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((name.to_owned(), values))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let areas = areas
        .into_iter()
        .map(|(selected, fields)| foraging::Area {
            fields,
            pools: selected
                .into_iter()
                .map(|index| POOLS[index].to_owned())
                .collect(),
        })
        .collect();
    Ok((Mappings::new(fields)?, foraging::Document { pools, areas }))
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let (mappings, foraging) = convert(source)?;
    crate::output::json(&library.join(FIELDS), &mappings)?;
    crate::output::json(&library.join(foraging::PATH), &foraging)
}

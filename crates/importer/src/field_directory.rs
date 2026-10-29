//! Native field declarations directly into named field mappings and foraging rules.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    foraging,
    library::{FIELDS, Mappings},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::{FieldId, ItemId};

const PHASES: u32 = 0x801e4060;
const COUNT: usize = 547;
const POOLS: [(&str, u32, usize); 6] = [
    ("meat", 0x801f9f40, 5),
    ("fish", 0x801f9f4c, 6),
    ("vegetables", 0x801f9f58, 10),
    ("fruit", 0x801f9f6c, 11),
    ("starches", 0x801f9f84, 6),
    ("other-food", 0x801f9f90, 12),
];

fn source_files(root: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(root.join("disc1/files/MAP"))? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "non-file in GameCube MAP directory"
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF8 field source name"))?;
        let name = name.to_ascii_lowercase();
        ensure!(files.insert(name.clone()), "ambiguous field source {name}");
    }
    Ok(files)
}

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

fn convert(exe: &[u8], files: &BTreeSet<String>) -> Result<(Mappings, foraging::Document)> {
    let mut fields = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut areas: BTreeMap<Vec<usize>, Vec<String>> = BTreeMap::new();
    for (id, row) in dol::slice(exe, PHASES, COUNT * 24)?
        .chunks_exact(24)
        .enumerate()
    {
        let pointer = dol::u32(row, 0)?;
        if pointer == 0 {
            continue;
        }
        let resource = text::string(exe, pointer)?;
        ensure!(
            !resource.is_empty()
                && !resource.contains(['/', '\\'])
                && resource != "."
                && resource != "..",
            "invalid native field resource {resource:?} for ID {id}"
        );
        if !files.contains(&resource.to_ascii_lowercase()) {
            continue;
        }
        let name = field_name(id as u32);
        ensure!(names.insert(name.clone()), "duplicate field name {name}");
        fields.insert(id as u32, name.clone());
        let selected = choices(row[4] >> 4).with_context(|| format!("field {id}"))?;
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
        .map(|(name, address, count)| {
            let values = dol::slice(exe, address, count * 2)?
                .chunks_exact(2)
                .map(|row| {
                    let id = dol::u16(row, 0)?;
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
                .map(|index| POOLS[index].0.to_owned())
                .collect(),
        })
        .collect();
    Ok((Mappings::new(fields)?, foraging::Document { pools, areas }))
}

pub fn cook_field_directory(root: &Path, library: &Path) -> Result<Vec<u32>> {
    let mapping_path = library.join(FIELDS);
    let forage_path = library.join(foraging::PATH);
    ensure!(
        !mapping_path.exists() && !forage_path.exists(),
        "field directory output already exists"
    );
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let files = source_files(root)?;
    let (mappings, foraging) = convert(&exe, &files)?;
    let selected = mappings.iter().map(|(id, _)| id).collect();
    for (path, document) in [
        (
            &mapping_path,
            resonance_asset_writer::json::encode(&mappings)?,
        ),
        (
            &forage_path,
            resonance_asset_writer::json::encode(&foraging)?,
        ),
    ] {
        crate::output::write(&path, &document)?;
    }
    Ok(selected)
}

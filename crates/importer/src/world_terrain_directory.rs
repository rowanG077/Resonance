//! Native world tile declarations directly into the two continent directories.
use crate::{rel::Rel, world_terrain_tile};
use anyhow::{Result, ensure};
use resonance_content::{
    library::Mappings,
    overworld::{
        TILE_COLUMNS, TILE_ROWS,
        document::{Continent, Tile},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const AXIS: usize = 0x2a574;
const TEMPLATES: [usize; 4] = [0x994, 0x9a4, 0x9b8, 0x9cc];
const NAMES: [&str; 2] = ["sylvarant", "tethe-alla"];

struct SourceTile {
    owner: String,
    base: String,
    alternate: Option<String>,
}

fn source_name(template: &str, row: u8, column: u8) -> Result<String> {
    let parts: Vec<_> = template.split("%c").collect();
    ensure!(
        parts.len() == 3 && parts.iter().all(|part| !part.contains('%')),
        "unsupported world tile filename template"
    );
    let native = format!(
        "{}{}{}{}{}",
        parts[0],
        char::from(row),
        parts[1],
        char::from(column),
        parts[2]
    );
    let path = native.trim_start_matches('/').replace('\\', "/");
    ensure!(
        path.to_ascii_lowercase().starts_with("field/")
            && !path.contains("..")
            && !path.ends_with('/'),
        "invalid world terrain source {path}"
    );
    Ok(path.rsplit('/').next().unwrap().to_ascii_lowercase())
}

fn convert(rel: &Rel, sources: &BTreeSet<String>) -> Result<([Continent; 2], Vec<SourceTile>)> {
    let axis = rel.text(6, AXIS)?;
    ensure!(
        axis.len() >= TILE_COLUMNS
            && axis.bytes().all(|byte| byte.is_ascii_alphanumeric())
            && axis.bytes().collect::<BTreeSet<_>>().len() == axis.len(),
        "invalid world tile axis"
    );
    let labels = axis.as_bytes();
    let templates = TEMPLATES
        .map(|address| rel.text(5, address))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let mut continents = [
        Continent {
            terrain: Vec::new(),
        },
        Continent {
            terrain: Vec::new(),
        },
    ];
    let mut tiles = Vec::new();
    for (world, continent) in continents.iter_mut().enumerate() {
        for row in 0..TILE_ROWS {
            for column in 0..TILE_COLUMNS {
                let root = format!(
                    "world/{}/terrain/column-{column:02}-row-{row:02}",
                    NAMES[world]
                );
                let native = source_name(&templates[world * 2], labels[row], labels[column])?;
                ensure!(
                    sources.contains(&native),
                    "missing world terrain source {native}"
                );
                let alternate =
                    source_name(&templates[world * 2 + 1], labels[row], labels[column])?;
                let alternate = sources.contains(&alternate).then_some(alternate);
                continent.terrain.push(Tile {
                    column: column as u8,
                    row: row as u8,
                    base: format!("{root}/terrain.json"),
                    alternate: alternate.as_ref().map(|_| format!("{root}/alternate.json")),
                });
                tiles.push(SourceTile {
                    owner: root,
                    base: native,
                    alternate,
                });
            }
        }
        continent.validate(&format!("world/{}/world.json", NAMES[world]))?;
    }
    Ok((continents, tiles))
}

fn declarations(root: &Path) -> Result<([Continent; 2], Vec<SourceTile>)> {
    let extracted = root.join("disc1/files");
    let sources = fs::read_dir(extracted.join("FIELD"))?
        .map(|entry| {
            let entry = entry?;
            ensure!(
                entry.file_type()?.is_file(),
                "non-file in native FIELD directory"
            );
            Ok(entry.file_name().to_string_lossy().to_ascii_lowercase())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let rel = Rel::read(fs::read(extracted.join("US_r_Top2field.rel"))?)?;
    convert(&rel, &sources)
}

fn write_directory(library: &Path, [first, second]: [Continent; 2]) -> Result<()> {
    let paths = [
        library.join("world/mappings.json"),
        library.join("world/sylvarant/world.json"),
        library.join("world/tethe-alla/world.json"),
    ];
    let mappings = Mappings::new(BTreeMap::from([
        (0, NAMES[0].to_owned()),
        (1, NAMES[1].to_owned()),
    ]))?;
    for (path, bytes) in [
        (&paths[0], resonance_asset_writer::json::encode(&mappings)?),
        (&paths[1], resonance_asset_writer::json::encode(&first)?),
        (&paths[2], resonance_asset_writer::json::encode(&second)?),
    ] {
        crate::output::write(&path, &bytes)?;
    }
    Ok(())
}

pub fn cook_world_terrain_directory(root: &Path, library: &Path) -> Result<()> {
    write_directory(library, declarations(root)?.0)
}

pub fn cook_world_terrain(root: &Path, library: &Path) -> Result<()> {
    let (continents, tiles) = declarations(root)?;
    let sources = root.join("disc1/files/FIELD");
    for tile in tiles {
        world_terrain_tile::cook_world_terrain_tile(
            &sources.join(tile.base),
            library,
            &tile.owner,
            false,
        )?;
        if let Some(alternate) = tile.alternate {
            world_terrain_tile::cook_world_terrain_tile(
                &sources.join(alternate),
                library,
                &tile.owner,
                true,
            )?;
        }
    }
    write_directory(library, continents)
}

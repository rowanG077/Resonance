//! World terrain lookup data. Native story logic selects between authored variants.
use super::WorldKind;
use crate::rel::Rel;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

const ROWS: u8 = 9;
const COLUMNS: u8 = 12;

#[derive(Serialize)]
pub(crate) struct Catalogue {
    axis_labels: String,
    worlds: [World; 2],
}

#[derive(Serialize)]
struct World {
    kind: WorldKind,
    /// Format arguments are row label followed by column label.
    templates: [String; 2],
    tiles: Vec<Tile>,
}

#[derive(Serialize)]
struct Tile {
    column: u8,
    row: u8,
    base: Asset,
    /// Availability is authored; selection depends on native story/location state.
    alternate: Option<Asset>,
}

#[derive(Serialize)]
struct Asset {
    source: String,
    /// Root-relative directory containing the converted package and its scene recipes.
    package: String,
}

struct Layout {
    axis: (usize, usize),
    templates: [(usize, usize); 4],
}

fn layout(file: &Path) -> Option<Layout> {
    let (section, axis, templates) = match file.file_name()?.to_str()? {
        "US_r_Top2field.rel" | "US_m_Top2field.rel" | "US_Top2field.rel" => {
            (5, 0x2a574, [0x994, 0x9a4, 0x9b8, 0x9cc])
        }
        "r_Top2field.rel" => (5, 0x2a508, [0x894, 0x8a4, 0x8b8, 0x8cc]),
        "m_Top2field.rel" | "Top2field.rel" => (5, 0x2a508, [0x9bc, 0x9cc, 0x9e0, 0x9f4]),
        "Top2fieldD.rel" => (6, 0x2ac59, [0x2ac6a, 0x2ac7a, 0x2ac8b, 0x2ac9c]),
        _ => return None,
    };
    Some(Layout {
        axis: (6, axis),
        templates: templates.map(|offset| (section, offset)),
    })
}

fn filename(template: &str, row: u8, column: u8) -> Result<String> {
    let parts = template.split("%c").collect::<Vec<_>>();
    ensure!(
        parts.len() == 3 && parts.iter().all(|p| !p.contains('%')),
        "world tile template requires two character arguments"
    );
    let path = format!(
        "{}{}{}{}{}",
        parts[0],
        char::from(row),
        parts[1],
        char::from(column),
        parts[2]
    );
    let path = path.strip_prefix('/').unwrap_or(&path);
    resonance_content::validate_asset_path(path)?;
    Ok(path.to_owned())
}

fn decode(rel: &Rel, layout: Layout, sources: &BTreeMap<String, String>) -> Result<Catalogue> {
    let axis_labels = rel.text(layout.axis)?;
    ensure!(
        axis_labels.len() >= usize::from(COLUMNS)
            && axis_labels
                .bytes()
                .all(|label| label.is_ascii_alphanumeric())
            && axis_labels
                .bytes()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == axis_labels.len(),
        "invalid world tile axis labels"
    );
    let mut paths = BTreeMap::new();
    for (path, hash) in sources {
        ensure!(
            paths
                .insert(path.to_ascii_lowercase(), (path, hash))
                .is_none(),
            "ambiguous source path {path}"
        );
    }
    let asset = |template: &str, row, column| -> Result<Option<Asset>> {
        let path = filename(
            template,
            axis_labels.as_bytes()[usize::from(row)],
            axis_labels.as_bytes()[usize::from(column)],
        )?;
        paths
            .get(&path.to_ascii_lowercase())
            .map(|(source, hash)| {
                ensure!(
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    "invalid world tile source hash"
                );
                Ok(Asset {
                    source: (*source).clone(),
                    package: format!("assets/{hash}"),
                })
            })
            .transpose()
    };
    let world = |kind, index: usize| -> Result<World> {
        let templates = [
            rel.text(layout.templates[index * 2])?,
            rel.text(layout.templates[index * 2 + 1])?,
        ];
        let mut tiles = Vec::new();
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                tiles.push(Tile {
                    column,
                    row,
                    base: asset(&templates[0], row, column)?.with_context(|| {
                        format!("missing world {index} terrain tile ({column}, {row})")
                    })?,
                    alternate: asset(&templates[1], row, column)?,
                });
            }
        }
        Ok(World {
            kind,
            templates,
            tiles,
        })
    };
    let worlds = [
        world(WorldKind::Sylvarant, 0)?,
        world(WorldKind::TetheAlla, 1)?,
    ];
    Ok(Catalogue {
        axis_labels,
        worlds,
    })
}

pub(crate) fn read(file: &Path, sources: &BTreeMap<String, String>) -> Result<Option<Catalogue>> {
    layout(file)
        .map(|layout| decode(&Rel::read(file)?, layout, sources))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn templates_keep_native_argument_order_and_reject_other_formatters() -> Result<()> {
        assert_eq!(
            filename("/Terrain/r%c-c%c.mesh", b'8', b'b')?,
            "Terrain/r8-cb.mesh"
        );
        for invalid in [
            "tiles/%d%c",
            "tiles/%c",
            "tiles/%c%c%c",
            "../%c%c",
            "tiles/%s%c%c",
        ] {
            assert!(filename(invalid, b'0', b'1').is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; reads original declarations, no geometry conversion"]
    fn original_world_templates_cover_every_physical_terrain_tile() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let files = root.join(format!("disc{disc}/files"));
            let terrain = fs::read_dir(files.join("FIELD"))?
                .map(|entry| {
                    Ok(format!(
                        "FIELD/{}",
                        entry?.file_name().to_str().context("invalid filename")?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let sources = terrain
                .iter()
                .map(|path| (path.clone(), "a".repeat(64)))
                .collect();
            let expected: BTreeSet<_> = terrain
                .iter()
                .filter(|path| {
                    let name = path.rsplit('/').next().unwrap();
                    let name = name.strip_prefix('t').unwrap_or(name);
                    name.as_bytes()[0].is_ascii_hexdigit() && name.ends_with(".dat")
                })
                .cloned()
                .collect();
            let mut modules = 0;
            for entry in fs::read_dir(&files)? {
                let file = entry?.path();
                let Some(layout) = layout(&file) else {
                    continue;
                };
                let rel = Rel::read(&file)?;
                for pointer in [layout.axis].into_iter().chain(layout.templates) {
                    ensure!(
                        rel.local_targets().contains(&pointer),
                        "unreferenced tile declaration in {}",
                        file.display()
                    );
                }
                let catalogue = decode(&rel, layout, &sources)?;
                let actual = catalogue
                    .worlds
                    .iter()
                    .flat_map(|world| &world.tiles)
                    .flat_map(|tile| std::iter::once(&tile.base).chain(&tile.alternate))
                    .map(|asset| asset.source.clone())
                    .collect::<BTreeSet<_>>();
                assert_eq!(actual, expected, "{}", file.display());
                assert_eq!(actual.len(), 228);
                assert!(
                    catalogue
                        .worlds
                        .iter()
                        .all(|world| world.tiles.len() == 108)
                );
                modules += 1;
            }
            assert_eq!(modules, 7);
        }
        Ok(())
    }
}

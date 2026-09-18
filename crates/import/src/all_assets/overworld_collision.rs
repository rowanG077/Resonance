//! World collision maps each surface and movement mode to a signed response class.
use crate::{embedded, read::f32 as float, rel::Rel};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "overworld-collision";
const SECTION: usize = 5;
const SURFACES: usize = 16;
const MODES: usize = 4;
const TABLE_BYTES: usize = SURFACES * MODES;

#[derive(Clone, Copy)]
struct Layout {
    surface_responses: usize,
    probe_half_extents: usize,
}

fn layout(file: &Path) -> Option<Layout> {
    let (surface_responses, probe_half_extents) = match file.file_name()?.to_str()? {
        "US_r_Top2field.rel" | "US_m_Top2field.rel" | "US_Top2field.rel" => (0xcd8, 0xd44),
        "r_Top2field.rel" => (0xd00, 0xd6c),
        "m_Top2field.rel" | "Top2field.rel" => (0xf18, 0xf84),
        "Top2fieldD.rel" => (0x9b8, 0xa3c),
        _ => return None,
    };
    Some(Layout {
        surface_responses,
        probe_half_extents,
    })
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Tables {
    /// Indexed [surface][movement mode]. -1 rejects the surface; other values
    /// are returned as response classes, which can differ from the surface ID.
    surface_responses: [[i8; MODES]; SURFACES],
    /// Collision tests all four corners at these offsets on both map axes.
    mode2_probe_half_extent: f32,
    other_probe_half_extent: f32,
}

fn decode(responses: &[u8], probes: &[u8]) -> Result<Tables> {
    ensure!(
        responses.len() == TABLE_BYTES && probes.len() == 8,
        "invalid overworld collision table extent"
    );
    let mode2_probe_half_extent = float(probes, 0)?;
    let other_probe_half_extent = float(probes, 4)?;
    ensure!(
        mode2_probe_half_extent > 0. && other_probe_half_extent > 0.,
        "invalid overworld collision probe extent"
    );
    Ok(Tables {
        surface_responses: std::array::from_fn(|surface| {
            std::array::from_fn(|mode| responses[surface * MODES + mode] as i8)
        }),
        mode2_probe_half_extent,
        other_probe_half_extent,
    })
}

fn read(rel: &Rel, layout: Layout) -> Result<Tables> {
    let slice = |offset, size| {
        rel.at((SECTION, offset))?
            .get(..size)
            .context("truncated overworld collision data")
    };
    decode(
        slice(layout.surface_responses, TABLE_BYTES)?,
        slice(layout.probe_half_extents, 8)?,
    )
}

pub(super) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some(layout) = layout(file) else {
        return Ok(None);
    };
    embedded::write(file, output, FAMILY, &read(&Rel::read(file)?, layout)?).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn collision_tables_preserve_signed_classes_and_check_extents() -> Result<()> {
        let responses = std::array::from_fn::<_, TABLE_BYTES, _>(|index| index as u8 + 192);
        let mut probes = [50_f32.to_be_bytes(), 120_f32.to_be_bytes()].concat();
        let tables = decode(&responses, &probes)?;
        assert_eq!(
            tables
                .surface_responses
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            (-64..0).collect::<Vec<i8>>()
        );
        assert_eq!(tables.mode2_probe_half_extent, 50.);
        assert_eq!(tables.other_probe_half_extent, 120.);
        assert!(decode(&responses[..TABLE_BYTES - 1], &probes).is_err());
        assert!(decode(&responses, &probes[..7]).is_err());
        for invalid in [f32::NAN, f32::INFINITY, 0., -1.] {
            probes[..4].copy_from_slice(&invalid.to_be_bytes());
            assert!(decode(&responses, &probes).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both locally extracted original discs; no media conversion"]
    fn original_overworld_collision_reconstructs_all_field_modules() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let mut publications = BTreeSet::new();
        for disc in [1, 2] {
            for module in [
                "US_r_Top2field.rel",
                "US_m_Top2field.rel",
                "US_Top2field.rel",
                "r_Top2field.rel",
                "m_Top2field.rel",
                "Top2field.rel",
                "Top2fieldD.rel",
            ] {
                let file = root.join(format!("disc{disc}/files/{module}"));
                let rel = Rel::read(&file)?;
                let layout = layout(&file).unwrap();
                let tables = read(&rel, layout)?;
                let original = &rel.at((SECTION, layout.surface_responses))?[..TABLE_BYTES];
                let reconstructed = tables
                    .surface_responses
                    .iter()
                    .flatten()
                    .map(|&class| class as u8)
                    .collect::<Vec<_>>();
                assert_eq!(reconstructed, original);
                assert_eq!(tables.surface_responses[0], [-1; MODES]);
                assert_eq!(tables.surface_responses[7], [7, 1, -1, -1]);
                assert_eq!(tables.surface_responses[10], [10, -1, -1, 1]);
                assert_eq!(tables.surface_responses[11], [-1, -1, 11, -1]);
                assert_eq!(tables.mode2_probe_half_extent, 120.);
                assert_eq!(tables.other_probe_half_extent, 50.);
                let probes = [
                    tables.mode2_probe_half_extent.to_be_bytes(),
                    tables.other_probe_half_extent.to_be_bytes(),
                ]
                .concat();
                assert_eq!(probes, rel.at((SECTION, layout.probe_half_extents))?[..8]);
                for offset in [
                    layout.surface_responses,
                    layout.probe_half_extents,
                    layout.probe_half_extents + 4,
                ] {
                    assert!(rel.local_targets().contains(&(SECTION, offset)), "{module}");
                }
                let paths = cook(&file, &output)?.unwrap();
                assert_eq!(paths.len(), 2);
                assert_eq!(embedded::read::<Tables>(&output, FAMILY, module)?, tables);
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(source["module"], module);

                assert_eq!(source["source_sha256"], crate::digest(&fs::read(&file)?));
                publications.insert(paths[0].clone());
            }
        }
        assert_eq!(publications.len(), 1);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}

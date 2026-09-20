//! Recover every party-exchange roster and its voice from the result tables.
use super::actions::Rel;
use super::embedded::{self, Layout};
use crate::{
    digest,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::victory_group::{GROUP_COUNT, Group, GroupMotion, Groups};
use std::{collections::BTreeMap, fs, path::Path};

const RECORD_BYTES: usize = 8;
const PACKAGE_BYTES: usize = 2048;
const RETURN: u32 = 0x4e80_0020;

pub(super) fn cook(extracted: &Path) -> Result<Groups> {
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
    // Both start and update callbacks really return immediately in this release.
    // Reject other implementations instead of silently discarding choreography.
    for callback in [0x80a78, 0x80a7c] {
        ensure!(
            word(rel.at((1, callback))?, 0)? == RETURN,
            "victory group choreography requires a supported callback"
        );
    }
    let packages = fs::read(extracted.join("files/BTL/BTLskit.dat"))?;
    ensure!(
        packages.len().is_multiple_of(PACKAGE_BYTES)
            && packages.len() > usize::from(GROUP_COUNT) * PACKAGE_BYTES,
        "truncated victory group packages"
    );
    let table = rel
        .at((5, Layout::RETAIL.victory_groups))?
        .get(..usize::from(GROUP_COUNT) * RECORD_BYTES)
        .context("truncated victory group table")?;
    let mut source = table.to_vec();
    source.extend_from_slice(&packages);
    let groups = Groups {
        source_sha256: digest(&source),
        motion: GroupMotion::RetainStagedPose,
        entries: parse(table)?,
    };
    groups.validate()?;
    Ok(groups)
}

fn parse(table: &[u8]) -> Result<BTreeMap<u8, Group>> {
    ensure!(
        table.len() == usize::from(GROUP_COUNT) * RECORD_BYTES,
        "invalid victory roster size"
    );
    (1..=GROUP_COUNT)
        .map(|id| {
            let row = &table[usize::from(id - 1) * RECORD_BYTES..];
            let count = usize::from(half(row, 2)?);
            ensure!(
                (1..=4).contains(&count)
                    && row[4 + count..8].iter().all(|&b| b == 0)
                    && row[4..4 + count].iter().all(|id| (1..=9).contains(id))
                    && row[4..4 + count]
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == count
                    && half(row, 0)? & 0x8000 != 0,
                "invalid victory group roster {id}"
            );
            Ok((
                id,
                Group {
                    voice: half(row, 0)?,
                    members: row[4..4 + count].to_vec(),
                },
            ))
        })
        .collect()
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    let rel = Rel::read(file)?;
    let offset = layout.victory_groups;
    let table = rel
        .at((5, offset))?
        .get(..usize::from(GROUP_COUNT) * RECORD_BYTES)
        .context("truncated victory roster")?;
    // Data extraction is independent of the module's choreography callbacks.
    embedded::write(
        file,
        output,
        "battle-victory-groups",
        &parse(table)?,
        serde_json::json!({
            "section": 5, "offset": offset, "stride": RECORD_BYTES, "count": GROUP_COUNT,
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires original battle modules; no media conversion"]
    fn original_victory_rosters_cover_every_module_and_match_selected() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("victory-rosters"));
        let mut shared = std::collections::BTreeSet::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let expected = serde_json::to_vec(&cook(&extracted)?.entries)?;
            let mut modules = 0;
            for file in fs::read_dir(extracted.join("files"))? {
                let file = file?.path();
                let Some(paths) = cook_all(&file, &output)? else {
                    continue;
                };
                modules += 1;
                assert_eq!(fs::read(output.join(&paths[0]))?, expected);
                let manifest: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(manifest["source_sha256"], digest(&fs::read(&file)?));
                let rel = Rel::read(&file)?;
                let offset = manifest["offset"].as_u64().unwrap() as usize;
                let table = &rel.at((5, offset))?[..usize::from(GROUP_COUNT) * RECORD_BYTES];
                assert_eq!(
                    digest(table),
                    "53ee08c956f5d34a668de72b38934070200143760470efa1387ca2fefff6bb8e"
                );
                let mut broken = table.to_vec();
                broken[4] = 10;
                assert!(parse(&broken).is_err());
                assert!(parse(&table[..table.len() - 1]).is_err());
                shared.insert(paths[0].clone());
            }
            assert_eq!(modules, 7);
        }
        assert_eq!(shared.len(), 1);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}

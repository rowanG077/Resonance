//! Original field resource, placement and render callback table.
use crate::dol;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{collections::BTreeSet, fs, path::Path};

const TABLE: u32 = 0x801e4060;
// startup_callbacks.c declares 547 rows; the next object starts at 0x801e73a8.
const COUNT: usize = 547;
const STRIDE: usize = 24;

#[cfg(test)]
pub(crate) fn map_paths(extracted: &Path) -> Result<BTreeSet<String>> {
    read(&fs::read(extracted.join("sys/main.dol"))?)?.map_paths(extracted)
}

fn map_path(files: &Path, resource: &str) -> Result<Option<String>> {
    resonance_content::validate_asset_path(resource)?;
    let path = crate::field_resources::find_path(files, &format!("MAP/{resource}"))?;
    if let Some(path) = &path {
        ensure!(
            fs::metadata(files.join(path))?.is_file(),
            "field resource is not a file: {path}"
        );
    }
    Ok(path)
}

#[derive(Clone, Serialize)]
pub(crate) struct Phases {
    /// Item Finder chooses a pool, then chooses an item within that pool.
    pub item_finder_pools: Vec<Vec<u16>>,
    pub records: Vec<Phase>,
}

impl Phases {
    /// Files present on this disc; some declarations belong only to the other disc.
    pub(crate) fn map_paths(&self, extracted: &Path) -> Result<BTreeSet<String>> {
        let files = extracted.join("files");
        let mut paths = BTreeSet::new();
        for phase in &self.records {
            if let Some(resource) = &phase.resource
                && let Some(path) = map_path(&files, resource)
                    .with_context(|| format!("invalid field {} resource {resource:?}", phase.id))?
            {
                paths.insert(path);
            }
        }
        Ok(paths)
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Phase {
    pub id: usize,
    pub resource: Option<String>,
    /// 0, 0x100 and 0x200 do not register a visited world-map location.
    pub location: u16,
    /// Signed XYZ before fn_80024EDC adds its transient low-three-bit X bias.
    pub default_position: [i16; 3],
    pub framebuffer_passes: FramebufferPasses,
    /// Pool indices in the exact random-choice order used by fn_8007F564.
    /// Empty disables Item Finder; one entry needs no pool-selection draw.
    pub item_finder_choices: Vec<usize>,
    /// Native function identities, not executable payloads. fn_8002F200 calls
    /// +16 before object dispatch; fn_8002F1B8 calls +20 after object dispatch.
    pub render_before_objects: Option<NativeCallback>,
    pub render_after_objects: Option<NativeCallback>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct NativeCallback(u32);

impl NativeCallback {
    pub const TITLE_SCENE: Self = Self(0x8002f440);
}

#[derive(Clone, Serialize)]
pub(crate) struct FramebufferPasses {
    /// fn_80023C10 first draws the copied framebuffer with GX_LEQUAL.
    pub less_equal: bool,
    /// Its second quad uses GX_GEQUAL. Neither pass writes depth.
    pub greater_equal: bool,
}

pub(crate) fn read(executable: &[u8]) -> Result<Phases> {
    // Exact original arrays, including their independent alignment gaps.
    let item_finder_pools = [
        (0x801f9f40, 5),
        (0x801f9f4c, 6),
        (0x801f9f58, 10),
        (0x801f9f6c, 11),
        (0x801f9f84, 6),
        (0x801f9f90, 12),
    ]
    .into_iter()
    .map(|(address, count)| {
        Ok(dol::slice(executable, address, count * 2)?
            .chunks_exact(2)
            .map(|row| u16::from_be_bytes([row[0], row[1]]))
            .collect())
    })
    .collect::<Result<Vec<Vec<u16>>>>()?;
    for address in [0x801f9f4a, 0x801f9f82] {
        ensure!(
            dol::slice(executable, address, 2)? == [0, 0],
            "nonzero Item Finder pool alignment padding"
        );
    }
    let records = dol::slice(executable, TABLE, COUNT * STRIDE)?
        .chunks_exact(STRIDE)
        .enumerate()
        .map(|(id, row)| {
            let word = |at| u32::from_be_bytes(row[at..at + 4].try_into().unwrap());
            let half = |at| u16::from_be_bytes(row[at..at + 2].try_into().unwrap());
            ensure!(
                row[5] == 0 && row[14..16] == [0, 0],
                "nonzero phase {id} record padding"
            );
            ensure!(row[4] & 0x0c == 0, "unknown phase {id} render flags");
            let item_finder_choices: &[usize] = match row[4] >> 4 {
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
                other => anyhow::bail!("unknown phase {id} Item Finder selector {other}"),
            };
            let callback = |address| -> Result<Option<NativeCallback>> {
                if address == 0 {
                    return Ok(None);
                }
                ensure!(address % 4 == 0, "unaligned phase {id} native callback");
                dol::slice(executable, address, 4)?;
                Ok(Some(NativeCallback(address)))
            };
            Ok(Phase {
                id,
                resource: (word(0) != 0)
                    .then(|| dol::text(executable, word(0)))
                    .transpose()?,
                location: half(6),
                default_position: [half(8) as i16, half(10) as i16, half(12) as i16],
                // fn_80024EDC initializes both and applies bits 2 and 1.
                framebuffer_passes: FramebufferPasses {
                    less_equal: row[4] & 2 == 0,
                    greater_equal: row[4] & 1 == 0,
                },
                item_finder_choices: item_finder_choices.to_vec(),
                render_before_objects: callback(word(16))?,
                render_after_objects: callback(word(20))?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Phases {
        item_finder_pools,
        records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_declarations_validate_before_adding_the_map_namespace() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("field-catalogue"));
        let result = (|| {
            fs::create_dir_all(root.join("Map/directory.bin"))?;
            fs::write(root.join("Map/Room.bin"), [])?;
            assert_eq!(map_path(&root, "room.BIN")?, Some("Map/Room.bin".into()));
            assert_eq!(map_path(&root, "absent.bin")?, None);
            for invalid in [
                "",
                "/Room.bin",
                "../Room.bin",
                "absent//file",
                "directory.bin",
            ] {
                assert!(map_path(&root, invalid).is_err(), "{invalid:?}");
            }
            fs::write(root.join("Map/ROOM.bin"), [])?;
            assert!(map_path(&root, "room.bin").is_err());
            assert!(map_path(&root, "Room.bin").is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both original executable catalogues and extracted directories"]
    fn original_field_roles_resolve_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let paths = read(&fs::read(extracted.join("sys/main.dol"))?)?.map_paths(&extracted)?;
            assert!(!paths.is_empty());
            assert!(
                paths
                    .iter()
                    .all(|path| extracted.join("files").join(path).is_file())
            );
        }
        Ok(())
    }
}

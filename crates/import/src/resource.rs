//! Complete resource declarations, independent of installed files and runtime selection.
use crate::{
    dol, embedded,
    read::{c_string, u32 as word},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "resource-catalogue";
const STANDALONE: u32 = 0x801f85e4;
const GROUPS: u32 = 0x801f86b8;
const BODIES: u32 = 0x801face4;
const BATTLE_MOTIONS: u32 = 0x801fabf4;
const FIELD_MOTIONS: u32 = 0x8017e4c0;
const FIELD_SERVICES: u32 = 0x8017a33c;
const SAVE_POINT: u32 = 0x8017a52c;
const INLINE_BYTES: usize = 12;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PartyResource {
    Body,
    #[cfg(test)]
    BattleMotion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Group {
    pub(crate) path: Option<String>,
    pub(crate) storage: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InlineName {
    pub(crate) name: String,
    pub(crate) storage: Vec<u8>,
}

impl InlineName {
    fn decode(row: &[u8]) -> Result<Self> {
        let bytes = c_string(row, 0)?;
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(bytes);
        ensure!(!invalid, "invalid inline resource name");
        let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode(&name);
        ensure!(
            encoded.as_ref() == bytes,
            "non-roundtrippable resource name"
        );
        Ok(Self {
            name: name.into_owned(),
            storage: row[bytes.len() + 1..].to_vec(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalogue {
    pub(crate) standalone: Vec<Option<String>>,
    pub(crate) groups: Vec<Group>,
    pub(crate) party_bodies: Vec<[Option<String>; 5]>,
    pub(crate) party_battle_motions: Vec<[Option<String>; 5]>,
    pub(crate) party_field_motions: Vec<InlineName>,
    pub(crate) field_services: Vec<InlineName>,
    pub(crate) save_point: String,
}

impl Catalogue {
    pub(crate) fn party(&self, kind: PartyResource, character: u8, costume: u8) -> Result<&str> {
        let rows = match kind {
            PartyResource::Body => &self.party_bodies,
            #[cfg(test)]
            PartyResource::BattleMotion => &self.party_battle_motions,
        };
        rows.get(character.checked_sub(1).context("zero character ID")? as usize)
            .and_then(|row| row.get(usize::from(costume)))
            .context("party resource outside catalogue")?
            .as_deref()
            .context("null party resource declaration")
    }

    pub(crate) fn field_motion(&self, character: u8) -> Result<&str> {
        Ok(&self
            .party_field_motions
            .get(character.checked_sub(1).context("zero character ID")? as usize)
            .context("field motion outside catalogue")?
            .name)
    }

    pub(crate) fn field_service(&self, character: u8) -> Result<&str> {
        Ok(&self
            .field_services
            .get(character.checked_sub(1).context("zero character ID")? as usize)
            .context("field service outside catalogue")?
            .name)
    }

    pub(crate) fn source(&self, id: u32) -> Result<&str> {
        let path = match id >> 16 {
            0 => self.standalone.get(id as usize),
            group => self.groups.get((group - 1) as usize).map(|row| &row.path),
        };
        path.context("resource outside catalogue")?
            .as_deref()
            .context("null resource declaration")
    }
}

fn paths(executable: &[u8], address: u32, count: usize) -> Result<Vec<Option<String>>> {
    dol::slice(executable, address, count * 4)?
        .chunks_exact(4)
        .map(|row| dol::optional_text(executable, word(row, 0)?))
        .collect()
}

fn inline_names(
    executable: &[u8],
    address: u32,
    count: usize,
    stride: usize,
) -> Result<Vec<InlineName>> {
    dol::slice(executable, address, count * stride)?
        .chunks_exact(stride)
        .map(InlineName::decode)
        .collect()
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let party = |address: u32, count: usize| -> Result<Vec<[Option<String>; 5]>> {
        Ok(paths(executable, address, count * 5)?
            .chunks_exact(5)
            .map(|row| std::array::from_fn(|i| row[i].clone()))
            .collect())
    };
    Ok(Catalogue {
        standalone: paths(executable, STANDALONE, 53)?,
        groups: dol::slice(executable, GROUPS, 14 * 12)?
            .chunks_exact(12)
            .map(|row| {
                Ok(Group {
                    path: dol::optional_text(executable, word(row, 0)?)?,
                    // +4..8 hold the allocated directory and valid flag, rebuilt by the loader.
                    storage: row[9..12].try_into()?,
                })
            })
            .collect::<Result<_>>()?,
        party_bodies: party(BODIES, 9)?,
        party_battle_motions: party(BATTLE_MOTIONS, 12)?,
        party_field_motions: inline_names(executable, FIELD_MOTIONS, 9, INLINE_BYTES)?,
        field_services: inline_names(executable, FIELD_SERVICES, 10, 16)?,
        save_point: dol::text(executable, SAVE_POINT)?,
    })
}

pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    embedded::write(file, output, FAMILY, &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn fixture() -> (Vec<u8>, [usize; 8]) {
        let mut executable = vec![0; 0x100];
        let mut offsets = [0; 8];
        for (i, (address, size)) in [
            (STANDALONE, 53 * 4),
            (GROUPS, 14 * 12),
            (BODIES, 9 * 20),
            (BATTLE_MOTIONS, 12 * 20),
            (FIELD_MOTIONS, 9 * INLINE_BYTES),
            (FIELD_SERVICES, 10 * 16),
            (0x80001000, 8),
            (SAVE_POINT, 16),
        ]
        .into_iter()
        .enumerate()
        {
            offsets[i] = executable.len();
            executable[i * 4..i * 4 + 4].copy_from_slice(&(offsets[i] as u32).to_be_bytes());
            executable[0x48 + i * 4..0x4c + i * 4].copy_from_slice(&address.to_be_bytes());
            executable[0x90 + i * 4..0x94 + i * 4].copy_from_slice(&(size as u32).to_be_bytes());
            executable.resize(executable.len() + size, 0);
        }
        for offset in [
            offsets[0],
            offsets[0] + 4,
            offsets[1],
            offsets[2],
            offsets[3] + 10 * 20 + 16,
        ] {
            executable[offset..offset + 4].copy_from_slice(&0x80001000u32.to_be_bytes());
        }
        executable[offsets[1] + 9..offsets[1] + 12].copy_from_slice(&[0x81, 0xfe, 0x7f]);
        executable[offsets[6]..offsets[6] + 8].copy_from_slice(b"absent\0\0");
        executable[offsets[7]..offsets[7] + 12].copy_from_slice(b"renamed.cab\0");
        for (index, stride) in [(4, INLINE_BYTES), (5, 16)] {
            for row in executable[offsets[index]..offsets[index + 1]].chunks_exact_mut(stride) {
                row[..7].copy_from_slice(&[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0]);
                row[7..].fill(0xff);
            }
        }
        (executable, offsets)
    }

    #[test]
    fn portrait_archive_follows_its_declaration_with_case_and_ambiguity_checks() -> Result<()> {
        let (mut executable, offsets) = fixture();
        let root = crate::temporary_path(&std::env::temp_dir().join("portrait-declaration"));
        fs::create_dir_all(root.join("files"))?;
        let result = (|| -> Result<()> {
            assert!(crate::skit::portrait_path(&root, &executable).is_err());
            let at = offsets[1] + 12 * 12;
            executable[at..at + 4].copy_from_slice(&0x80001000u32.to_be_bytes());
            assert!(crate::skit::portrait_path(&root, &executable).is_err());
            fs::write(root.join("files/ABSENT"), [])?;
            assert_eq!(crate::skit::portrait_path(&root, &executable)?, "ABSENT");
            fs::write(root.join("files/absent"), [])?;
            assert!(crate::skit::portrait_path(&root, &executable).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn resource_catalogue_preserves_declarations_and_excludes_loader_state() -> Result<()> {
        let (mut executable, offsets) = fixture();
        let catalogue = read(&executable)?;
        assert_eq!(catalogue.source(0)?, "absent");
        assert_eq!(catalogue.source(1)?, catalogue.source(0x1ffff)?);
        assert_eq!(catalogue.party(PartyResource::Body, 1, 0)?, "absent");
        assert_eq!(
            catalogue.party(PartyResource::BattleMotion, 11, 4)?,
            "absent"
        );
        assert_eq!(catalogue.field_motion(9)?, "テスト");
        assert_eq!(catalogue.party_field_motions[8].storage, [0xff; 5]);
        assert_eq!(catalogue.field_service(10)?, "テスト");
        assert_eq!(catalogue.field_services[9].storage, [0xff; 9]);
        assert_eq!(catalogue.save_point, "renamed.cab");
        for id in [2, 53, 0x20000, 0xf0000] {
            assert!(catalogue.source(id).is_err());
        }
        for (kind, character, costume) in [
            (PartyResource::Body, 0, 0),
            (PartyResource::Body, 10, 0),
            (PartyResource::Body, 1, 5),
            (PartyResource::Body, 1, 1),
            (PartyResource::BattleMotion, 12, 0),
            (PartyResource::BattleMotion, 13, 0),
        ] {
            assert!(catalogue.party(kind, character, costume).is_err());
        }
        assert!(catalogue.field_motion(0).is_err() && catalogue.field_motion(10).is_err());
        assert!(catalogue.field_service(0).is_err() && catalogue.field_service(11).is_err());
        let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&catalogue)?)?;
        assert_eq!(restored, catalogue);
        for row in executable[offsets[1]..offsets[2]].chunks_exact_mut(12) {
            row[4..9].fill(0xab);
        }
        assert_eq!(read(&executable)?, catalogue);
        executable[offsets[1] + 9] ^= 1;
        assert_ne!(read(&executable)?, catalogue);
        assert!(read(&executable[..offsets[6] - 1]).is_err());
        executable[offsets[0]..offsets[0] + 4].copy_from_slice(&0xffffffffu32.to_be_bytes());
        assert!(read(&executable).is_err());
        executable[offsets[0]..offsets[0] + 4].copy_from_slice(&0x80001000u32.to_be_bytes());
        executable[offsets[6]..offsets[7]].fill(b'a');
        assert!(read(&executable).is_err());
        executable[offsets[6]..offsets[7]].fill(0);
        for (index, stride) in [(4, INLINE_BYTES), (5, 16)] {
            let at = offsets[index];
            let original = executable[at..at + stride].to_vec();
            executable[at..at + stride].fill(b'a');
            assert!(read(&executable).is_err());
            executable[at..at + 2].copy_from_slice(&[0x81, 0]);
            assert!(read(&executable).is_err());
            executable[at..at + stride].copy_from_slice(&original);
        }
        Ok(())
    }

    fn check_path(executable: &[u8], pointer: u32, path: &Option<String>) -> Result<()> {
        if let Some(path) = path {
            assert_ne!(pointer, 0);
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(path);
            assert!(!invalid);
            assert_eq!(
                encoded.as_ref(),
                dol::slice(executable, pointer, encoded.len())?
            );
            assert_eq!(
                dol::slice(executable, pointer + encoded.len() as u32, 1)?,
                [0]
            );
        } else {
            assert_eq!(pointer, 0);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes resource JSON"]
    fn original_resource_catalogue_reconstructs_and_publishes_both_discs() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let source = extracted.join(format!("disc{disc}/sys/main.dol"));
                let executable = fs::read(&source)?;
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&source, &executable, &destination)?;
                let catalogue: Catalogue = embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(catalogue, read(&executable)?);
                check_path(&executable, SAVE_POINT, &Some(catalogue.save_point.clone()))?;
                assert_eq!(
                    (
                        catalogue.standalone.len(),
                        catalogue.groups.len(),
                        catalogue.party_bodies.len(),
                        catalogue.party_battle_motions.len(),
                        catalogue.party_field_motions.len(),
                        catalogue.field_services.len()
                    ),
                    (53, 14, 9, 12, 9, 10)
                );
                for (address, values) in [
                    (STANDALONE, catalogue.standalone.iter().collect::<Vec<_>>()),
                    (BODIES, catalogue.party_bodies.iter().flatten().collect()),
                    (
                        BATTLE_MOTIONS,
                        catalogue.party_battle_motions.iter().flatten().collect(),
                    ),
                ] {
                    for (row, path) in dol::slice(&executable, address, values.len() * 4)?
                        .chunks_exact(4)
                        .zip(values)
                    {
                        check_path(&executable, word(row, 0)?, path)?;
                    }
                }
                for (row, group) in dol::slice(&executable, GROUPS, 14 * 12)?
                    .chunks_exact(12)
                    .zip(&catalogue.groups)
                {
                    check_path(&executable, word(row, 0)?, &group.path)?;
                    assert_eq!(group.storage, row[9..12]);
                }
                for (address, stride, names) in [
                    (FIELD_MOTIONS, INLINE_BYTES, &catalogue.party_field_motions),
                    (FIELD_SERVICES, 16, &catalogue.field_services),
                ] {
                    for (row, name) in dol::slice(&executable, address, names.len() * stride)?
                        .chunks_exact(stride)
                        .zip(names)
                    {
                        let mut rebuilt = encoding_rs::SHIFT_JIS.encode(&name.name).0.into_owned();
                        rebuilt.push(0);
                        rebuilt.extend(&name.storage);
                        assert_eq!(rebuilt, row);
                    }
                }
                assert_eq!(catalogue.field_service(10)?, "minini_ex.bin");
                assert_eq!(catalogue.groups[13].path, None);
                assert_eq!(
                    catalogue.party_battle_motions[9],
                    catalogue.party_battle_motions[10]
                );
                assert!(
                    catalogue.party_battle_motions[11]
                        .iter()
                        .all(Option::is_none)
                );
                for path in [
                    catalogue.party(PartyResource::Body, 9, 4)?,
                    catalogue.party(PartyResource::BattleMotion, 10, 1)?,
                    catalogue.party(PartyResource::BattleMotion, 11, 2)?,
                ] {
                    assert!(
                        crate::field_resources::find_path(
                            &extracted.join(format!("disc{disc}/files")),
                            path
                        )?
                        .is_none(),
                        "expected absent declared resource {path}"
                    );
                }
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(provenance["data"], paths[0]);
                payloads.insert(paths[0].clone());
            }
            assert_eq!(payloads.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}

//! Physical skit tables and portrait recipes, independent of runtime availability.
use super::embedded::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
    skit::recipe::{self, Recipe},
    tpl,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Group {
    Story,
    Support,
    Tech,
    Timed,
    Direct,
}

struct Table {
    group: Group,
    definitions: u32,
    definition_count: usize,
    filenames: u32,
    filename_count: usize,
}

// Definition and filename arrays have different physical lengths. In particular,
// disabled timed rows extend beyond their filename array into the direct array.
const TABLES: [Table; 5] = [
    Table {
        group: Group::Story,
        definitions: 0x8020ac10,
        definition_count: 120,
        filenames: 0x8020f850,
        filename_count: 120,
    },
    Table {
        group: Group::Support,
        definitions: 0x8020b750,
        definition_count: 35,
        filenames: 0x8020fa30,
        filename_count: 40,
    },
    Table {
        group: Group::Tech,
        definitions: 0x8020ba98,
        definition_count: 20,
        filenames: 0x8035a070,
        filename_count: 2,
    },
    Table {
        group: Group::Timed,
        definitions: 0x8020bc78,
        definition_count: 260,
        filenames: 0x8020fad0,
        filename_count: 237,
    },
    Table {
        group: Group::Direct,
        definitions: 0x8020d4d8,
        definition_count: 94,
        filenames: 0x8020fe84,
        filename_count: 94,
    },
];

#[derive(Serialize)]
struct Catalog {
    #[serde(flatten)]
    physical: Physical,
    portrait_archive: String,
    portraits: Vec<Portrait>,
    portrait_recipes: Vec<Recipe>,
    preview_order: PreviewOrder,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Physical {
    definitions: Vec<Definition>,
    scripts: Vec<Script>,
    texts: Vec<String>,
}

fn physical(executable: &[u8]) -> Result<(Physical, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    Ok((
        Physical {
            definitions: definitions(executable, &mut texts)?,
            scripts: scripts(executable, &mut texts)?,
            texts: texts.values,
        },
        texts.sources,
    ))
}

const PREVIEW_ORDER: u32 = 0x8020fffc;
const PREVIEW_ORDER_SIZE: usize = 0x2fc;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct PreviewOrder {
    /// Skit IDs returned by the preview-list native call retain authored order.
    ids: Vec<u16>,
    terminator: u16,
    /// Physical halfwords following the first terminator are not preview entries.
    storage: Vec<u16>,
}

fn preview_order(executable: &[u8]) -> Result<PreviewOrder> {
    let values: Vec<_> = dol::slice(executable, PREVIEW_ORDER, PREVIEW_ORDER_SIZE)?
        .chunks_exact(2)
        .map(|value| u16::from_be_bytes(value.try_into().unwrap()))
        .collect();
    let end = values
        .iter()
        .position(|&id| id == u16::MAX)
        .context("unterminated skit preview order")?;
    Ok(PreviewOrder {
        ids: values[..end].to_vec(),
        terminator: values[end],
        storage: values[end + 1..].to_vec(),
    })
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Definition {
    group: Group,
    id: u16,
    title: Option<TextRef>,
    storage: DefinitionStorage,
    story: Story,
    party_mask: u16,
    location: Location,
    availability: Availability,
    script: ScriptLookup,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DefinitionStorage {
    /// The halfword at +2 and bytes at +17..20 have no established meaning.
    halfword: u16,
    bytes: [u8; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Story {
    Disabled,
    Any,
    Range { first: i32, last: i32 },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Location {
    Any,
    Overworld,
    Sylvarant,
    TetheAlla,
    Field,
    Map { id: u16 },
    Unknown { value: i16 },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Availability {
    /// The controller receives this definition's ID and shared rule parameters.
    controller: AvailabilityController,
    /// Zero uses only shared rules; nonzero selects the ID's native condition.
    selector: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AvailabilityController {
    Story,
    Support,
    Timed,
    Unconditional,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct ScriptReference {
    group: Group,
    index: usize,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct ScriptLookup {
    index: usize,
    /// None denotes an authored row without a physical filename slot.
    binding: Option<ScriptReference>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Script {
    #[serde(flatten)]
    binding: ScriptReference,
    /// Original filename text; null slots and shared source pointers are distinct.
    file: Option<TextRef>,
}

#[derive(Serialize)]
struct Portrait {
    member: usize,
    images: Vec<PortraitImage>,
}

#[derive(Serialize)]
struct PortraitImage {
    width: u16,
    height: u16,
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    let file = extracted.join("sys/main.dol");
    let executable = fs::read(&file)?;
    let (physical, sources) = physical(&executable)?;
    let portrait_archive = crate::skit::portrait_path(extracted, &executable)?;
    let catalog = Catalog {
        physical,
        portraits: portraits(&fs::read(extracted.join("files").join(&portrait_archive))?)?,
        portrait_recipes: recipe::read(&executable)?,
        preview_order: preview_order(&executable)?,
        portrait_archive,
    };
    crate::embedded::write(
        &file,
        output,
        "skits",
        &catalog,
        serde_json::json!({
            "tables": TABLES.iter().map(|t| serde_json::json!({
                "group": t.group,
                "definitions": {"address": t.definitions, "count": t.definition_count, "stride":24, "source_size":t.definition_count*24},
                "filenames": {"address": t.filenames, "count": t.filename_count, "stride":4, "source_size":t.filename_count*4},
            })).collect::<Vec<_>>(),
            "preview_order": {"address": PREVIEW_ORDER, "source_size": PREVIEW_ORDER_SIZE},
            "texts": sources,
        }),
    )
}

fn definitions(executable: &[u8], texts: &mut TextPool) -> Result<Vec<Definition>> {
    let mut definitions = Vec::new();
    for table in &TABLES {
        for (index, row) in dol::slice(executable, table.definitions, table.definition_count * 24)?
            .chunks_exact(24)
            .enumerate()
        {
            let id = half(row, 0)?;
            let range = [word(row, 4)? as i32, word(row, 8)? as i32];
            let story = match range {
                [-1, -1] => Story::Disabled,
                [-999_999_999, -999_999_999] => Story::Any,
                [first, last] => Story::Range { first, last },
            };
            let location = match half(row, 14)? as i16 {
                -9999 => Location::Any,
                -1 => Location::Overworld,
                -2 => Location::Sylvarant,
                -3 => Location::TetheAlla,
                -4 => Location::Field,
                id if id >= 0 => Location::Map { id: id as u16 },
                value => Location::Unknown { value },
            };
            let controller = match table.group {
                Group::Story => AvailabilityController::Story,
                Group::Support => AvailabilityController::Support,
                Group::Timed => AvailabilityController::Timed,
                Group::Tech | Group::Direct => AvailabilityController::Unconditional,
            };
            definitions.push(Definition {
                group: table.group,
                id,
                title: texts.reference(executable, word(row, 20)?)?,
                storage: DefinitionStorage {
                    halfword: half(row, 2)?,
                    bytes: row[17..20].try_into()?,
                },
                story,
                party_mask: half(row, 12)?,
                location,
                availability: Availability {
                    controller,
                    selector: row[16],
                },
                script: ScriptLookup {
                    index,
                    binding: script_reference(table.filenames + index as u32 * 4),
                },
            });
        }
    }
    Ok(definitions)
}

fn script_reference(address: u32) -> Option<ScriptReference> {
    TABLES.iter().find_map(|table| {
        let offset = address.checked_sub(table.filenames)?;
        (offset % 4 == 0 && offset / 4 < table.filename_count as u32).then_some(ScriptReference {
            group: table.group,
            index: offset as usize / 4,
        })
    })
}

fn scripts(executable: &[u8], texts: &mut TextPool) -> Result<Vec<Script>> {
    let mut scripts = Vec::new();
    for table in &TABLES {
        for (index, row) in dol::slice(executable, table.filenames, table.filename_count * 4)?
            .chunks_exact(4)
            .enumerate()
        {
            scripts.push(Script {
                binding: ScriptReference {
                    group: table.group,
                    index,
                },
                file: texts.reference(executable, word(row, 0)?)?,
            });
        }
    }
    Ok(scripts)
}

fn portraits(archive: &[u8]) -> Result<Vec<Portrait>> {
    crate::skit::portraits::members(archive)?
        .into_iter()
        .enumerate()
        .map(|(member, bytes)| {
            let images = if bytes.is_empty() {
                Vec::new()
            } else {
                tpl::parse_tpl(bytes)?
                    .into_iter()
                    .map(|image| PortraitImage {
                        width: image.width,
                        height: image.height,
                    })
                    .collect()
            };
            Ok(Portrait { member, images })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both locally extracted original discs; only publishes JSON"]
    fn original_skit_records_preserve_complete_fields_pointers_and_publication() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("skit-records"));
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let extracted = local.join(format!("disc{disc}"));
                let mut executable = fs::read(extracted.join("sys/main.dol"))?;
                let (original, sources) = physical(&executable)?;
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&extracted, &destination)?;
                let published: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[0]))?)?;
                let restored: Physical = serde_json::from_value(published)?;
                assert_eq!(restored, original);
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["data"], paths[0]);
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(restored.definitions.len(), 529);
                assert_eq!(restored.scripts.len(), 493);
                let pointer =
                    |reference: Option<TextRef>| reference.map_or(0, |r| sources[r.0].address);
                let mut definition_at = 0;
                let mut script_at = 0;
                for table in TABLES.iter() {
                    let rows = &restored.definitions
                        [definition_at..definition_at + table.definition_count];
                    let mut bytes = Vec::new();
                    for (index, row) in rows.iter().enumerate() {
                        assert_eq!(row.group, table.group);
                        assert_eq!(row.script.index, index);
                        assert_eq!(
                            row.script.binding,
                            script_reference(table.filenames + index as u32 * 4)
                        );
                        bytes.extend(row.id.to_be_bytes());
                        bytes.extend(row.storage.halfword.to_be_bytes());
                        let bounds = match row.story {
                            Story::Disabled => [-1; 2],
                            Story::Any => [-999_999_999; 2],
                            Story::Range { first, last } => [first, last],
                        };
                        bytes.extend(bounds.into_iter().flat_map(i32::to_be_bytes));
                        bytes.extend(row.party_mask.to_be_bytes());
                        let location: i16 = match row.location {
                            Location::Any => -9999,
                            Location::Overworld => -1,
                            Location::Sylvarant => -2,
                            Location::TetheAlla => -3,
                            Location::Field => -4,
                            Location::Map { id } => id as i16,
                            Location::Unknown { value } => value,
                        };
                        bytes.extend(location.to_be_bytes());
                        bytes.push(row.availability.selector);
                        bytes.extend(row.storage.bytes);
                        bytes.extend(pointer(row.title).to_be_bytes());
                    }
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, table.definitions, table.definition_count * 24)?
                    );
                    let scripts = &restored.scripts[script_at..script_at + table.filename_count];
                    for (index, script) in scripts.iter().enumerate() {
                        assert_eq!(
                            script.binding,
                            ScriptReference {
                                group: table.group,
                                index
                            }
                        );
                    }
                    let bytes: Vec<_> = scripts
                        .iter()
                        .flat_map(|s| pointer(s.file).to_be_bytes())
                        .collect();
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, table.filenames, table.filename_count * 4)?
                    );
                    definition_at += table.definition_count;
                    script_at += table.filename_count;
                }
                for (source, text) in sources.iter().zip(&restored.texts) {
                    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
                    assert!(!invalid);
                    assert_eq!(
                        [bytes.as_ref(), &[0]].concat(),
                        dol::slice(&executable, source.address, source.source_size as usize)?
                    );
                }
                let tech: Vec<_> = restored
                    .definitions
                    .iter()
                    .filter(|d| d.group == Group::Tech)
                    .collect();
                assert!(tech.iter().all(|d| d.title == tech[0].title));
                let null_slot = ScriptReference {
                    group: Group::Tech,
                    index: 1,
                };
                assert_eq!(tech[1].script.binding, Some(null_slot));
                assert_eq!(
                    restored
                        .scripts
                        .iter()
                        .find(|s| s.binding == null_slot)
                        .unwrap()
                        .file,
                    None
                );
                if let Some(first) = &first {
                    assert_eq!(&restored, first);
                } else {
                    first = Some(restored);
                }

                // Unknown storage, IDs, predicates and nullable/aliased pointers
                // are source data; availability belongs to runtime preparation.
                let alias = word(dol::slice(&executable, TABLES[0].filenames, 4)?, 0)?;
                for (address, bytes) in [
                    (TABLES[0].definitions, vec![0x23, 0x28, 0xab, 0xcd]),
                    (
                        TABLES[0].definitions + 4,
                        [i32::MIN.to_be_bytes(), i32::MAX.to_be_bytes()].concat(),
                    ),
                    (
                        TABLES[0].definitions + 14,
                        (-3210i16).to_be_bytes().to_vec(),
                    ),
                    (TABLES[0].definitions + 16, vec![240, 1, 2, 3, 0, 0, 0, 0]),
                    (TABLES[0].definitions + 24, 9000u16.to_be_bytes().to_vec()),
                    (TABLES[0].filenames + 4, alias.to_be_bytes().to_vec()),
                    (TABLES[2].filenames + 4, alias.to_be_bytes().to_vec()),
                    (TABLES[0].filenames + 8, vec![0; 4]),
                ] {
                    let source = dol::slice(&executable, address, bytes.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + bytes.len()].copy_from_slice(&bytes);
                }
                let (changed, _) = physical(&executable)?;
                let row = &changed.definitions[0];
                assert_eq!(row.id, 9000);
                assert_eq!(row.id, changed.definitions[1].id);
                assert_eq!(
                    row.storage,
                    DefinitionStorage {
                        halfword: 0xabcd,
                        bytes: [1, 2, 3]
                    }
                );
                assert_eq!(
                    row.story,
                    Story::Range {
                        first: i32::MIN,
                        last: i32::MAX
                    }
                );
                assert_eq!(row.location, Location::Unknown { value: -3210 });
                assert_eq!(row.availability.selector, 240);
                assert!(row.title.is_none());
                assert_eq!(changed.scripts[0].file, changed.scripts[1].file);
                assert_eq!(
                    changed.scripts[0].file,
                    changed
                        .scripts
                        .iter()
                        .find(|s| s.binding == null_slot)
                        .unwrap()
                        .file
                );
                assert!(changed.scripts[2].file.is_none());
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(output);
        result
    }

    #[test]
    #[ignore = "requires both locally extracted original discs; only publishes JSON"]
    fn original_skit_preview_order_preserves_ids_terminator_and_storage() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("skit-preview-order"));
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let extracted = local.join(format!("disc{disc}"));
                let mut executable = fs::read(extracted.join("sys/main.dol"))?;
                let order = preview_order(&executable)?;
                let paths = cook(&extracted, &output)?;
                let published: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[0]))?)?;
                let restored: PreviewOrder =
                    serde_json::from_value(published["preview_order"].clone())?;
                assert_eq!(restored, order);
                let bytes: Vec<_> = restored
                    .ids
                    .iter()
                    .copied()
                    .chain([restored.terminator])
                    .chain(restored.storage.iter().copied())
                    .flat_map(u16::to_be_bytes)
                    .collect();
                assert_eq!(
                    bytes,
                    dol::slice(&executable, PREVIEW_ORDER, PREVIEW_ORDER_SIZE)?
                );
                assert_eq!(restored.ids.len(), 380);
                assert_eq!(restored.terminator, u16::MAX);
                assert_eq!(restored.storage, [0]);
                if let Some(first) = &first {
                    assert_eq!(&order, first);
                } else {
                    first = Some(order);
                }

                // Recover aliases and arbitrary IDs verbatim; the first terminator
                // ends the list, while later halfwords remain physical storage.
                let original = dol::slice(&executable, PREVIEW_ORDER, PREVIEW_ORDER_SIZE)?;
                let offset = original.as_ptr() as usize - executable.as_ptr() as usize;
                let replacement: Vec<_> = [9000u16, 9000, u16::MAX, 4711]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect();
                executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
                let changed = preview_order(&executable)?;
                assert_eq!(changed.ids, [9000, 9000]);
                assert_eq!(changed.terminator, u16::MAX);
                assert_eq!(changed.storage[0], 4711);
                assert_eq!(changed.storage.len(), PREVIEW_ORDER_SIZE / 2 - 3);
                executable[offset..offset + PREVIEW_ORDER_SIZE].fill(0);
                assert!(preview_order(&executable).is_err());
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(output);
        result
    }

    #[test]
    #[ignore = "requires both locally extracted original discs"]
    fn all_physical_skit_portraits_and_recipes_are_retained() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let root = root.join(disc);
            let executable = fs::read(root.join("sys/main.dol"))?;
            let archive = fs::read(root.join("files/skit.skt"))?;
            let portraits = portraits(&archive)?;
            assert_eq!(portraits.len(), word(&archive, 0)? as usize);
            assert_eq!(portraits.len(), 224);
            assert_eq!(
                portraits
                    .iter()
                    .filter(|row| !row.images.is_empty())
                    .count(),
                108
            );
            assert_eq!(recipe::read(&executable)?.len(), 230);
        }
        Ok(())
    }
}

//! Native skit packages directly into named scenario bytecode and messages.
//! The source filename arrays determine playable IDs; physical source paths
//! never enter the cooked library.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Mappings,
    overworld::PartyCharacter,
    skit::{SkitCondition, SkitDefinition, SkitLocation, SkitResourcePaths, SkitTrigger, document},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use symphonia_script::{NativeCall, Program, message, scenario};

// Definition and filename tables have unequal lengths. Rows past a table's
// own filename array can overlap the next one and are not skit bindings.
const TABLES: [(u32, usize, u32, usize); 5] = [
    (0x8020ac10, 120, 0x8020f850, 120),
    (0x8020b750, 35, 0x8020fa30, 40),
    (0x8020ba98, 20, 0x8035a070, 2),
    (0x8020bc78, 260, 0x8020fad0, 237),
    (0x8020d4d8, 94, 0x8020fe84, 94),
];

fn sources(root: &Path) -> Result<(Mappings, BTreeMap<u16, PathBuf>)> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let titles: BTreeMap<u16, String> = serde_json::from_str(include_str!("skit-titles.json"))?;
    let mut mappings = BTreeMap::new();
    let mut files = BTreeMap::new();
    for (definitions, rows, filenames, slots) in TABLES {
        for index in 0..rows.min(slots) {
            let id = dol::u16(dol::slice(&exe, definitions + index as u32 * 24, 2)?, 0)?;
            if !(1..=860).contains(&id) {
                continue;
            }
            let pointer = dol::u32(dol::slice(&exe, filenames + index as u32 * 4, 4)?, 0)?;
            if pointer == 0 {
                continue;
            }
            let source = text::string(&exe, pointer)?;
            let filename = source
                .strip_prefix("cht/")
                .or_else(|| source.strip_prefix("CHT/"))
                .context("skit source is outside CHT")?;
            ensure!(
                filename.ends_with(".skp") && !filename.contains(['/', '\\']),
                "invalid native skit source {source}"
            );
            let path = root.join("disc1/files/CHT").join(filename);
            if !path.is_file() {
                continue;
            }
            let title = titles
                .get(&id)
                .with_context(|| format!("missing human title for skit {id}"))?
                .clone();
            ensure!(
                mappings.insert(u32::from(id), title).is_none(),
                "duplicate skit ID {id}"
            );
            ensure!(
                files.insert(id, path).is_none(),
                "duplicate skit source ID {id}"
            );
        }
    }
    Ok((Mappings::new(mappings)?, files))
}

fn definitions(exe: &[u8]) -> Result<Vec<SkitDefinition>> {
    let conditions: BTreeMap<u16, SkitCondition> =
        serde_json::from_str(include_str!("skit-conditions.json"))?;
    let mut definitions = Vec::new();
    for (address, first, count, trigger) in [
        (TABLES[0].0, 1, 119, SkitTrigger::Entry),
        (TABLES[3].0, 600, 259, SkitTrigger::Exploration),
    ] {
        for index in 0..count {
            let row = dol::slice(exe, address + index as u32 * 24, 24)?;
            let id = dol::u16(row, 0)?;
            ensure!(usize::from(id) == first + index, "invalid skit table order");
            let range = [dol::u32(row, 4)? as i32, dol::u32(row, 8)? as i32];
            let story = match range {
                [-1, -1] => continue,
                [-999_999_999, -999_999_999] => None,
                [first, last] => Some([first, last]),
            };
            let title = text::string(exe, dol::u32(row, 20)?)?;
            let location = match dol::u16(row, 14)? as i16 {
                -9999 => SkitLocation::Anywhere,
                -1 => SkitLocation::Overworld,
                -2 => SkitLocation::Sylvarant,
                -3 => SkitLocation::TetheAlla,
                -4 => SkitLocation::Field,
                at if at >= 0 => SkitLocation::Map((at as u16).into()),
                at => anyhow::bail!("unknown skit {id} location {at}"),
            };
            let mask = dol::u16(row, 12)?;
            ensure!(mask & !0x3fe == 0, "invalid skit {id} party mask");
            let party = (1..=9)
                .filter(|member| mask & (1 << member) != 0)
                .map(|member| PartyCharacter::try_from(member as u8).map_err(Into::into))
                .collect::<Result<Vec<_>>>()?;
            let condition = if row[16] == 0 {
                SkitCondition::None
            } else {
                conditions
                    .get(&id)
                    .with_context(|| format!("missing skit {id} condition"))?
                    .clone()
            };
            condition.validate()?;
            definitions.push(SkitDefinition {
                id,
                title,
                trigger,
                story,
                party,
                location,
                condition,
            });
        }
    }
    Ok(definitions)
}

fn preview_order(exe: &[u8]) -> Result<Vec<u16>> {
    let values = dol::slice(exe, 0x8020fffc, 0x2fc)?;
    let order = values
        .chunks_exact(2)
        .map(|word| u16::from_be_bytes(word.try_into().unwrap()))
        .take_while(|&id| id != u16::MAX)
        .collect::<Vec<_>>();
    ensure!(
        order.len() < values.len() / 2,
        "unterminated skit preview order"
    );
    Ok(order)
}

fn skip_disabled(exe: &[u8]) -> Result<BTreeSet<u16>> {
    let bytes = dol::slice(exe, 0x8009b2ac, 84)?;
    let mut ids = BTreeSet::new();
    for offset in [0, 12, 20, 28, 36, 44, 52, 60, 68] {
        let word = dol::u32(bytes, offset)?;
        ensure!(word & 0xffff0000 == 0x2c030000, "invalid skit skip branch");
        ids.insert(word as u16);
    }
    Ok(ids)
}

fn requested_voices(script: &[u8]) -> Result<BTreeSet<u32>> {
    scenario::calls::arguments(script, NativeCall::PlayVoice, 1)?
        .into_values()
        .map(|args| args[0].context("dynamic skit voice ID"))
        .filter(|id| !matches!(id, Ok(-1)))
        .map(|id| Ok(u32::try_from(id?)?))
        .collect()
}

fn catalogue(
    root: &Path,
    library: &Path,
    files: &BTreeMap<u16, PathBuf>,
    media: BTreeSet<u32>,
) -> Result<document::Definition> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let mut skits = definitions(&exe)?;
    let fields: Mappings =
        serde_json::from_slice(&fs::read(library.join(resonance_content::library::FIELDS))?)?;
    resonance_content::skit::fields::name_fields(&mut skits, &fields)?;
    let preview_order = preview_order(&exe)?;
    let ambient: BTreeSet<_> = skits.iter().map(|skit| skit.id).collect();
    let denied = skip_disabled(&exe)?;
    let mut resources = BTreeMap::new();
    for (definitions, rows, _, _) in TABLES {
        for index in 0..rows {
            let row = dol::slice(&exe, definitions + index as u32 * 24, 24)?;
            let id = dol::u16(row, 0)?;
            if files.contains_key(&id) {
                let title = if ambient.contains(&id) || dol::u32(row, 20)? == 0 {
                    None
                } else {
                    Some(text::string(&exe, dol::u32(row, 20)?)?).filter(|title| !title.is_empty())
                };
                let allow_skip = !denied.contains(&id);
                if title.is_some() || !allow_skip {
                    resources.insert(
                        id,
                        SkitResourcePaths {
                            script: String::new(),
                            messages: None,
                            allow_skip,
                            title,
                        },
                    );
                }
            }
        }
    }
    Ok(document::Definition {
        skits,
        preview_order,
        resources,
        media: document::Media {
            ids: media,
            speed: 1.000875,
        },
    })
}

fn catalogue_media(mappings: &Mappings, files: &BTreeMap<u16, PathBuf>) -> Result<BTreeSet<u32>> {
    let mut media = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for (id, path) in files {
        let name = mappings.target(u32::from(*id))?;
        if seen.insert(name.to_owned()) {
            let (bytes, _) = script(path)?;
            media.extend(requested_voices(&bytes)?);
        }
    }
    Ok(media)
}

fn script(path: &Path) -> Result<(Vec<u8>, Vec<message::Message>)> {
    let source = fs::read(path).with_context(|| format!("read skit {}", path.display()))?;
    parse_script(&source, path)
}

fn parse_script(source: &[u8], path: &Path) -> Result<(Vec<u8>, Vec<message::Message>)> {
    ensure!(
        source.len() >= 40
            && dol::u32(&source, 0)? == 2
            && dol::u32(&source, 4)? == 32
            && dol::u32(&source, 8)? as usize == source.len() - 32,
        "invalid native skit package {}",
        path.display()
    );
    let bytes = source[32..].to_vec();
    let header = scenario::parse_header(&bytes)?;
    ensure!(
        header.code_base() >= 8 + usize::from(header.registry_count) * 12
            && header.auxiliary_offset() > header.code_base()
            && header.auxiliary_offset() <= bytes.len(),
        "invalid skit script header"
    );
    Program::decode(&bytes)?;
    let messages = message::parse(&bytes[header.auxiliary_offset()..])?;
    Ok((bytes, messages))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::output::write(&path, bytes)?;
    Ok(())
}

/// Focused conversion of one ID, preserving the original SSB bytecode.
pub fn cook_skit_script(root: &Path, library: &Path, id: u16) -> Result<()> {
    let (mapping, files) = sources(root)?;
    let name = mapping.target(u32::from(id))?;
    let path = files.get(&id).context("skit has no native package")?;
    let (bytes, messages) = script(path)?;
    let directory = library.join("skits").join(name);
    write_new(&directory.join("scenario.ssb"), &bytes)?;
    if !messages.is_empty() {
        write_new(
            &directory.join("messages.json"),
            &resonance_asset_writer::json::encode(&messages)?,
        )?;
    }
    Ok(())
}

/// Read all tiny native skit packages for voice-ID metadata, but publish only
/// the global catalogue and mappings. No scenario files are rewritten here.
pub fn cook_skit_catalogue(root: &Path, library: &Path) -> Result<()> {
    let (mappings, files) = sources(root)?;
    let media = catalogue_media(&mappings, &files)?;
    let catalogue = catalogue(root, library, &files, media)?;
    write_new(
        &library.join(document::MAPPINGS),
        &resonance_asset_writer::json::encode(&mappings)?,
    )?;
    write_new(
        &library.join(document::PATH),
        &resonance_asset_writer::json::encode(&catalogue)?,
    )?;
    Ok(())
}

/// Full direct owner, for the final one-time cook after metadata review.
pub fn cook_skits(root: &Path, library: &Path) -> Result<()> {
    let (mappings, files) = sources(root)?;
    let mut written = BTreeMap::<String, Vec<u8>>::new();
    let mut media = BTreeSet::new();
    for (id, path) in &files {
        let name = mappings.target(u32::from(*id))?.to_owned();
        let source = fs::read(path)?;
        if let Some(previous) = written.get(&name) {
            ensure!(
                *previous == source,
                "skit {id} shares a name but has different source"
            );
            continue;
        }
        let (bytes, messages) = parse_script(&source, path)?;
        written.insert(name.clone(), source);
        media.extend(requested_voices(&bytes)?);
        let directory = library.join("skits").join(name);
        write_new(&directory.join("scenario.ssb"), &bytes)?;
        if !messages.is_empty() {
            write_new(
                &directory.join("messages.json"),
                &resonance_asset_writer::json::encode(&messages)?,
            )?;
        }
    }
    cook_unassigned_skits(root, library)?;
    let catalogue = catalogue(root, library, &files, media)?;
    write_new(
        &library.join(document::MAPPINGS),
        &resonance_asset_writer::json::encode(&mappings)?,
    )?;
    write_new(
        &library.join(document::PATH),
        &resonance_asset_writer::json::encode(&catalogue)?,
    )?;
    Ok(())
}

/// Native test programs have no gameplay ID; preserve their ordinary SSB payload.
pub fn cook_unassigned_skits(root: &Path, library: &Path) -> Result<()> {
    let (_, selected) = sources(root)?;
    for entry in fs::read_dir(root.join("disc1/files/CHT"))? {
        let path = entry?.path();
        if path.extension().and_then(|x| x.to_str()) != Some("skp")
            || selected.values().any(|p| p == &path)
        {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|x| x.to_str())
            .context("invalid skit filename")?;
        let name = if stem.eq_ignore_ascii_case("testskit") {
            "test-skit".to_owned()
        } else {
            stem.to_ascii_lowercase().replace('_', "-")
        };
        let (bytes, messages) = script(&path)?;
        let directory = library.join("skits").join(name);
        write_new(&directory.join("scenario.ssb"), &bytes)?;
        if !messages.is_empty() {
            write_new(
                &directory.join("messages.json"),
                &resonance_asset_writer::json::encode(&messages)?,
            )?;
        }
    }
    Ok(())
}

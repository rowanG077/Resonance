//! Battle party IDs and costumes from native directory entries and maintained names.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::library::Mappings;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use symphonia_constants::PartyCharacterId;

#[derive(Deserialize)]
struct Costume {
    name: String,
}

pub(crate) fn costume_name(character: u8, costume: u8) -> Result<String> {
    let names: BTreeMap<u8, BTreeMap<u8, Costume>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    Ok(names
        .get(&character)
        .and_then(|row| row.get(&costume))
        .context("unnamed party costume")?
        .name
        .clone())
}

pub(crate) fn native_path(files: &Path, name: &str) -> Result<Option<PathBuf>> {
    let mut path = files.to_path_buf();
    for component in name.split('/') {
        ensure!(
            !component.is_empty()
                && component != "."
                && component != ".."
                && !component.contains(char::from(92)),
            "invalid native party path {name}"
        );
        let matches = fs::read_dir(&path)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(component)
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "ambiguous native party path {name}");
        let Some(next) = matches.into_iter().next() else {
            return Ok(None);
        };
        path = next;
    }
    Ok(path.is_file().then_some(path))
}

pub(crate) fn source_name(exe: &[u8], table: u32, character: u8, costume: u8) -> Result<String> {
    let pointer = dol::u32(
        dol::slice(
            exe,
            table + u32::from(character - 1) * 20 + u32::from(costume) * 4,
            4,
        )?,
        0,
    )?;
    text::string(exe, pointer)
}

fn write(path: &Path, entries: BTreeMap<u32, String>) -> Result<()> {
    let data = resonance_asset_writer::json::encode(&Mappings::new(entries)?)?;
    crate::output::write(&path, &data)?;
    Ok(())
}

/// Publish the stable lookup without converting any party media.
pub fn cook_battle_party_mappings(extracted: &Path, library: &Path) -> Result<Vec<(u8, u8)>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let files = extracted.join("disc1/files");
    let costumes: BTreeMap<u8, BTreeMap<u8, Costume>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    let mut party = BTreeMap::new();
    let mut outputs = Vec::new();
    let mut selected = Vec::new();
    for character in 1..=9_u8 {
        let name = PartyCharacterId::try_from(character)?
            .resource_name()
            .context("unnamed party character")?;
        let mut variants = BTreeMap::new();
        for costume in 0..5_u8 {
            let body = source_name(&exe, 0x801f_ace4, character, costume)?;
            let motion = source_name(&exe, 0x801f_abf4, character, costume)?;
            let present =
                native_path(&files, &body)?.is_some() && native_path(&files, &motion)?.is_some();
            match costumes.get(&character).and_then(|row| row.get(&costume)) {
                Some(costume_name) if present => {
                    if !variants.values().any(|name| name == &costume_name.name) {
                        selected.push((character, costume));
                    }
                    variants.insert(u32::from(costume), costume_name.name.clone());
                }
                None if !present => {}
                _ => anyhow::bail!(
                    "native party costume {character}:{costume} disagrees with maintained names"
                ),
            }
        }
        ensure!(
            variants.contains_key(&0),
            "party character {character} lacks a default body"
        );
        party.insert(u32::from(character), name.to_owned());
        outputs.push((
            library.join(format!("battle/party/{name}/mappings.json")),
            variants,
        ));
    }
    ensure!(
        outputs.iter().all(|(path, _)| !path.exists())
            && !library.join("battle/party/mappings.json").exists(),
        "battle party mappings already exist"
    );
    for (path, variants) in outputs {
        write(&path, variants)?;
    }
    write(&library.join("battle/party/mappings.json"), party)?;
    Ok(selected)
}

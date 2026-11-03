//! Original battle programs retain native IDs; identical programs share one file.
use anyhow::{Result, ensure};
use resonance_content::library::Mappings;
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::BattleEventId;
use symphonia_script::{Program, message, scenario};

fn program(library: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let header = scenario::parse_header(bytes)?;
    ensure!(
        header.auxiliary_offset() <= bytes.len(),
        "invalid battle message offset"
    );
    Program::decode(bytes)?;
    let messages = message::parse_with_encoding(
        &bytes[header.auxiliary_offset()..],
        message::TextEncoding::ShiftJis,
    )?;
    let path = library.join(format!("battle/scripts/{name}.ssb"));
    let cooked =
        crate::script_text::normalize_scenario_text(bytes, message::TextEncoding::ShiftJis)?;
    crate::output::write(&path, &cooked)?;
    let path = library.join(format!("battle/scripts/{name}-messages.json"));
    if !messages.is_empty() {
        crate::output::json(&path, &messages)?;
    }
    Ok(())
}

/// Preserve all declared native programs. Only events 2/3 have playable actor recipes.
pub fn cook_battle_event_scripts(root: &Path, library: &Path, selected: Option<u16>) -> Result<()> {
    if let Some(id) = selected {
        BattleEventId::try_from(id)?;
    }
    let mut owners = BTreeMap::<Vec<u8>, String>::new();
    let mut names = BTreeMap::new();
    for event in BattleEventId::ALL {
        let id = event.raw();
        let bytes = fs::read(root.join(format!("disc1/files/btl_ev{id:03}.so")))?;
        let owner = owners
            .entry(bytes.clone())
            .or_insert_with(|| event.resource_name().unwrap().into())
            .clone();
        if selected.is_none_or(|wanted| wanted == id) {
            if !names.values().any(|name| name == &format!("{owner}.ssb")) {
                program(library, &owner, &bytes)?;
            }
            names.insert(u32::from(id), format!("{owner}.ssb"));
        }
    }
    const MAPPINGS: &str = "battle/scripts/mappings.json";
    if selected.is_some() && crate::authoring::document_exists(library, MAPPINGS) {
        let mut previous: BTreeMap<u32, String> = crate::authoring::read_json(library, MAPPINGS)?;
        previous.extend(names);
        names = previous;
    }
    crate::output::json(&library.join(MAPPINGS), &Mappings::new(names)?)?;
    if selected.is_none() {
        program(
            library,
            "common",
            &fs::read(root.join("disc1/files/btl.so"))?,
        )?;
        let playable = [BattleEventId::VidarrOpening, BattleEventId::VidarrRescue]
            .into_iter()
            .map(|event| {
                (
                    u32::from(event.raw()),
                    event.resource_name().unwrap().to_owned(),
                )
            })
            .collect();
        crate::output::json(
            &library.join("battle/events/mappings.json"),
            &Mappings::new(playable)?,
        )?;
    }
    Ok(())
}

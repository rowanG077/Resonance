//! Pre-flight checks for a library and its enabled mods. The game admits most
//! content when a field or battle first needs it; this reports the same
//! failures up front, with the mods behind each document.
use crate::{diagnostics::Diagnostics, library::Library, prepared::Files};
use anyhow::{Context, Result};

#[derive(Debug, Default)]
pub struct Report {
    /// Content the game would reject or skip.
    pub errors: Vec<String>,
    /// Legal content that probably does not do what its author intended.
    pub warnings: Vec<String>,
}

impl Report {
    fn check(&mut self, scope: &str, result: Result<()>) {
        if let Err(error) = result {
            self.errors.push(format!("{scope}: {error:#}"));
        }
    }
}

/// Admit the gameplay catalogues and every asset-authored action exactly as
/// the game does, but strictly: a caption or binding the game would skip with
/// a logged diagnostic is an error here.
pub fn validate(library: &Library) -> Report {
    let mut report = Report::default();
    for conflict in library.conflicts() {
        report.warnings.push(format!(
            "{} is supplied by both {} and {}; {} wins",
            conflict.path, conflict.previous, conflict.replacement, conflict.replacement
        ));
    }
    for (layer, directory) in library.unfamiliar_roots() {
        report.warnings.push(format!(
            "{layer} has a top-level {directory:?}, which the base library does not; nothing reads it unless another file refers to it"
        ));
    }
    let files = match gameplay(library) {
        Ok(files) => files,
        Err(error) => {
            report.errors.push(format!("gameplay documents: {error:#}"));
            return report;
        }
    };
    // A catalogue-wide decode failure cannot say which entry is wrong, and
    // every later check would repeat it.
    entries::<crate::items::Entry>(&mut report, &files, crate::items::PATH, "items", "item");
    entries::<crate::arte::document::Entry>(
        &mut report,
        &files,
        crate::arte::PATH,
        "techniques",
        "technique",
    );
    if !report.errors.is_empty() {
        return report;
    }
    report.check(
        "gameplay rules",
        crate::session::SessionData::load(&files).map(drop),
    );
    report.check(
        "names and captions",
        crate::session::GameText::load(&files).map(drop),
    );
    report.check("item captions", item_captions(&files));
    report.check("item equipment", item_equipment(library, &files));
    report.check("authored actions and spells", actions(library, &files));
    report
}

fn gameplay(library: &Library) -> Result<Files> {
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    crate::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    Files::from_snapshot(snapshot, || false, diagnostics)
}

fn item_captions(files: &Files) -> Result<()> {
    let items = crate::items::load(files)?;
    let captions = items.captions(files.diagnostics())?;
    for (id, name) in items.mappings.iter() {
        // The original catalogue has unnamed placeholders; an added item
        // without text can be obtained but not shown.
        anyhow::ensure!(
            captions[id as usize].is_some() || !items.allocated.contains_key(&(id as u16)),
            "item {name} has no text.json"
        );
    }
    Ok(())
}

fn item_equipment(library: &Library, files: &Files) -> Result<()> {
    let document: crate::items::Document = files.json(crate::items::PATH)?;
    let mut snapshot = library.snapshot();
    for (name, item) in &document.items {
        let Some(equipment) = &item.equipment else {
            continue;
        };
        let path = format!("meshes/equipment/{equipment}/equipment.json");
        snapshot
            .json::<crate::battle_model::document::Equipment>(&path, crate::preparation::JSON_LIMIT)
            .with_context(|| format!("equipment of item {name}"))?;
    }
    Ok(())
}

fn actions(library: &Library, files: &Files) -> Result<()> {
    let document: crate::arte::document::Document = files.json(crate::arte::PATH)?;
    let mut snapshot = library.snapshot();
    for (name, technique) in &document.techniques {
        if let Some(path) = &technique.rules.spell {
            let scope = || format!("spell of technique {name} ({path})");
            let spell: crate::battle_spell::AuthoredSpell = snapshot
                .json(path, crate::preparation::JSON_LIMIT)
                .with_context(scope)?;
            spell.validate().with_context(scope)?;
            anyhow::ensure!(
                spell.unison.is_some() || !technique.rules.capabilities.offensive,
                "{}: an offensive spell needs a unison placement",
                scope()
            );
            if let Some(effects) = &spell.effects {
                snapshot
                    .json::<serde_json::Value>(effects, crate::preparation::JSON_LIMIT)
                    .with_context(scope)?;
            }
        }
        let Some(path) = &technique.rules.action else {
            continue;
        };
        let scope = || format!("action of technique {name} ({path})");
        let action: crate::battle_martial::AuthoredAction = snapshot
            .json(path, crate::preparation::JSON_LIMIT)
            .with_context(scope)?;
        for motion in action.motions.values() {
            snapshot
                .read(motion, crate::preparation::ASSET_LIMIT)
                .with_context(scope)?;
        }
        if let Some(effects) = &action.effects {
            snapshot
                .json::<serde_json::Value>(effects, crate::preparation::JSON_LIMIT)
                .with_context(scope)?;
        }
    }
    Ok(())
}

/// Decode each named entry on its own, so a mistake names its entry.
fn entries<T: serde::de::DeserializeOwned>(
    report: &mut Report,
    files: &Files,
    path: &str,
    collection: &str,
    kind: &str,
) {
    let document = match files.json::<serde_json::Value>(path) {
        Ok(document) => document,
        Err(error) => return report.errors.push(format!("{path}: {error:#}")),
    };
    let Some(entries) = document[collection].as_object() else {
        return report.errors.push(format!("{path}: missing {collection}"));
    };
    for (name, entry) in entries {
        if let Err(error) = serde_json::from_value::<T>(entry.clone()) {
            report.errors.push(format!(
                "{kind} {name} ({}/{collection}/{name}/): {error}",
                path.trim_end_matches(".json")
            ));
        }
    }
}

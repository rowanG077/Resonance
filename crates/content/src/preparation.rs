//! Dependency discovery from editable runtime documents. These are semantic
//! entry points, not a persisted list of files, hashes or importer publications.
use crate::{
    diagnostics::Diagnostics,
    font::{BitmapFont, DialogueArt},
    library::Snapshot,
    menu::MenuArt,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub const JSON_LIMIT: usize = 64 * 1024 * 1024;
pub const ASSET_LIMIT: usize = 129 * 1024 * 1024;

pub const GAMEPLAY_ROOTS: &[&str] = &[
    "game/session-data.json",
    "game/menu-data.json",
    "game/text.json",
    crate::menu_data::MANUAL_PATH,
    crate::menu_data::FIGURINES_PATH,
    crate::menu_data::SYNOPSIS_PATH,
    crate::menu_data::CUSTOMIZE_PATH,
    crate::menu_data::RENAME_PATH,
    "game/techniques.json",
];

pub fn scripts(
    snapshot: &mut Snapshot,
    cancelled: &impl Fn() -> bool,
) -> Result<BTreeMap<String, String>> {
    let paths = snapshot.files_under("scripts")?.clone();
    let mut sources = BTreeMap::new();
    for path in paths.into_iter().filter(|path| path.ends_with(".sym")) {
        ensure!(!cancelled(), "script preparation cancelled");
        let bytes = snapshot.read(&path, JSON_LIMIT)?;
        let source = std::str::from_utf8(&bytes)
            .with_context(|| format!("script source is not UTF-8: {path}"))?;
        let module = path
            .strip_prefix("scripts/")
            .unwrap()
            .strip_suffix(".sym")
            .unwrap();
        sources.insert(module.replace('/', "::"), source.to_owned());
    }
    Ok(sources)
}

pub fn gameplay(
    snapshot: &mut Snapshot,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<BTreeMap<String, String>> {
    for &path in GAMEPLAY_ROOTS {
        ensure!(!cancelled(), "gameplay preparation cancelled");
        snapshot.read(path, JSON_LIMIT)?;
    }
    catalogue_models(snapshot, diagnostics, cancelled)?;
    scripts(snapshot, cancelled)
}

pub fn catalogue_models(
    snapshot: &mut Snapshot,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    menu_models(snapshot, diagnostics, cancelled)?;
    ensure!(!cancelled(), "catalogue preparation cancelled");
    let result = snapshot
        .json(crate::menu_data::FIGURINES_PATH, JSON_LIMIT)
        .and_then(|catalogue| {
            crate::model_preview::document::capture_catalogue(&catalogue, snapshot, cancelled)
        });
    ensure!(!cancelled(), "catalogue preparation cancelled");
    diagnostics.attempt("figurine directory", result)?;
    Ok(())
}

pub fn menu_models(
    snapshot: &mut Snapshot,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let menu: serde_json::Value = snapshot.json("game/menu-data.json", JSON_LIMIT)?;
    ensure!(!cancelled(), "catalogue preparation cancelled");
    let result = crate::model_preview::document::capture_catalogue(
        &menu["presentation"]["monsters"],
        snapshot,
        cancelled,
    );
    // Cancellation cannot become an optional page diagnostic.
    ensure!(!cancelled(), "catalogue preparation cancelled");
    diagnostics.attempt("monster directory", result)?;
    Ok(())
}

pub fn font(snapshot: &mut Snapshot, path: &str) -> Result<BitmapFont> {
    let font: BitmapFont = snapshot.json(path, JSON_LIMIT)?;
    font.validate().with_context(|| path.to_owned())?;
    snapshot
        .read(&font.texture, ASSET_LIMIT)
        .with_context(|| format!("font {path} refers to {}", font.texture))?;
    Ok(font)
}

/// Capture every declared UI image. Optional malformed menu groups retain the
/// existing diagnostic policy; cancellation is always fatal.
pub fn ui(
    snapshot: &mut Snapshot,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    ensure!(!cancelled(), "UI preparation cancelled");
    let dialogue: DialogueArt = snapshot.json("ui/dialogue.json", JSON_LIMIT)?;
    dialogue.validate()?;
    font(snapshot, &dialogue.font)?;
    for texture in dialogue.textures.iter().chain([&dialogue.cursor]) {
        ensure!(!cancelled(), "UI preparation cancelled");
        snapshot
            .read(&texture.path, ASSET_LIMIT)
            .with_context(|| format!("ui/dialogue.json refers to {}", texture.path))?;
    }
    let menu = MenuArt::decode(&snapshot.read("ui/menu.json", JSON_LIMIT)?, diagnostics)?;
    menu.validate_structure()?;
    for texture in menu.textures.values() {
        ensure!(!cancelled(), "UI preparation cancelled");
        diagnostics.attempt(
            "menu texture",
            snapshot
                .read(&texture.path, ASSET_LIMIT)
                .with_context(|| format!("ui/menu.json refers to {}", texture.path)),
        )?;
    }
    snapshot.read("ui/story-subtitles.json", JSON_LIMIT)?;
    ensure!(!cancelled(), "UI preparation cancelled");
    Ok(())
}

/// Prepare shared gameplay and selected fields for CPU tools and behavior tests.
/// Presentation workers extend their own snapshots with audio and screen needs.
pub fn capture(
    library: &crate::library::Library,
    fields: &[u32],
    cache: &mut crate::prepared::Cache,
    cancelled: impl Fn() -> bool,
    diagnostics: Diagnostics,
) -> Result<crate::prepared::Files> {
    let mut snapshot = library.snapshot();
    gameplay(&mut snapshot, &diagnostics, &cancelled)?;
    crate::battle_rules::capture(&mut snapshot, &cancelled)?;
    for &id in fields {
        crate::field::document::capture_scene(&mut snapshot, id, &cancelled)?;
    }
    crate::skit::document::capture(&mut snapshot, &cancelled)?;
    let identity = crate::save_identity::Identity::capture(&mut snapshot, &cancelled)?;
    crate::prepared::Files::from_snapshot(snapshot, cache, cancelled, diagnostics)?
        .with_identity(identity)
}

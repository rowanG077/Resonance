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
    crate::characters::PATH,
    crate::menu_data::GRADE_SHOP_PATH,
    crate::menu_data::CRAFTING_PATH,
    crate::menu_data::WORLD_MAP_PATH,
    crate::menu_data::PRESENTATION_PATH,
    crate::menu_data::MONSTER_BOOK_PATH,
    crate::items::PATH,
    crate::titles::PATH,
    crate::cooking::PATH,
    crate::ex_skills::PATH,
    crate::strategy::PATH,
    crate::foraging::PATH,
    crate::library::FIELDS,
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
    menu_resources(snapshot, diagnostics, cancelled)?;
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

pub fn menu_resources(
    snapshot: &mut Snapshot,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    crate::titles::capture(snapshot)?;
    let world_map: serde_json::Value =
        snapshot.json(crate::menu_data::WORLD_MAP_PATH, JSON_LIMIT)?;
    let landmarks = world_map["landmarks"]
        .as_str()
        .context("menu world map needs its landmark path")?;
    snapshot.read(landmarks, JSON_LIMIT)?;
    ensure!(!cancelled(), "catalogue preparation cancelled");
    let monsters: serde_json::Value =
        snapshot.json(crate::menu_data::MONSTER_BOOK_PATH, JSON_LIMIT)?;
    let result = crate::model_preview::document::capture_catalogue(&monsters, snapshot, cancelled);
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
    let mut dialogue: DialogueArt = snapshot.json("ui/dialogue.json", JSON_LIMIT)?;
    dialogue.validate()?;
    font(snapshot, &dialogue.font)?;
    dialogue.prepare_images(|path| {
        ensure!(!cancelled(), "UI preparation cancelled");
        snapshot
            .read(path, ASSET_LIMIT)
            .with_context(|| format!("ui/dialogue.json refers to {path}"))
    })?;
    let menu = MenuArt::decode(&snapshot.read("ui/menu.json", JSON_LIMIT)?, diagnostics)?;
    menu.validate_structure()?;
    // Named sprite selectors reuse the owning catalogues' mappings and names.
    for (group, path) in [
        (crate::menu::Sprite::ItemImages, crate::items::PATH),
        (crate::menu::Sprite::Recipes, crate::cooking::PATH),
    ] {
        if !menu.sprites.named_rects.contains_key(&group) {
            continue;
        }
        ensure!(!cancelled(), "UI preparation cancelled");
        diagnostics.attempt("menu sprite catalogue", snapshot.read(path, JSON_LIMIT))?;
    }
    if menu
        .sprites
        .rects
        .contains_key(&crate::menu::Sprite::Portraits)
        && menu.sprites.name_overrides.len() < 9
    {
        diagnostics.attempt(
            "menu character names",
            snapshot.read("game/characters.json", JSON_LIMIT),
        )?;
    }
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
    crate::prepared::Files::from_snapshot(snapshot, cancelled, diagnostics)
}

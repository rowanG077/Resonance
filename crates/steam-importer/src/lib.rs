//! Direct Steam conversion into the same editable library as the GameCube importer.
mod actor_model;
mod animation;
mod archive;
mod character_models;
mod characters;
mod cooking;
mod coordinates;
mod dialogue;
mod dialogue_font;
mod ex_skills;
mod field_actors;
mod field_core;
mod field_directory;
mod field_models;
mod field_scene;
mod field_settings;
mod formations;
mod game_over;
mod items;
mod legacy_motion;
mod menu_art;
mod menu_catalogues;
mod menu_labels;
mod menu_presentation;
mod menu_settings;
mod menu_synopsis;
mod menu_world_map;
mod model;
mod output;
mod package;
mod pe;
mod recoil;
mod resources;
mod script;
mod script_input;
mod source;
mod startup;
mod strategy;
mod techniques;
mod text;
mod texture;
mod titles;
mod ui_images;
mod world_landmarks;
use anyhow::{Context, Result, ensure};
pub use source::Language;
use std::{path::Path, time::Instant};

/// Categories write final assets in dependency order and overwrite previous output.
/// Additional Steam categories are being ported to the current library format.
pub fn cook_all(installation: &Path, output: &Path, language: Language) -> Result<()> {
    let source = source::Source::open(installation, language)?;
    let root = installation.canonicalize()?;
    let destination = std::path::absolute(output)?;
    ensure!(
        !destination.starts_with(&root),
        "cooked output must be outside the Steam installation"
    );
    let start = Instant::now();
    cook_tables(&source, output)?;
    eprintln!("[asset type 12/28] Cooking shared field settings");
    field_settings::cook(&source, output).context("Steam field settings")?;
    eprintln!("[asset type 13/28] Cooking field scripts and collision");
    field_core::cook(&source, output, None).context("Steam field scripts and collision")?;
    eprintln!("[asset type 14/28] Cooking field scenery and cameras");
    field_scene::cook(&source, output, None).context("Steam field scenery and cameras")?;
    eprintln!("[asset type 15/28] Cooking party models and field motions");
    character_models::cook(&source, output, None)
        .context("Steam party models and field motions")?;
    eprintln!("[asset type 16/28] Cooking shared field models");
    let mut models = field_models::cook(&source, output).context("Steam shared field models")?;
    eprintln!("[asset type 17/28] Cooking field actors and animation bindings");
    field_actors::cook(&source, output, &mut models, None).context("Steam field actors")?;
    eprintln!("[asset type 18/28] Cooking manual, name editor and options");
    menu_settings::cook(&source, output).context("Steam menu settings")?;
    eprintln!("[asset type 19/28] Cooking dialogue font, artwork and subtitles");
    let font = dialogue_font::cook(&source, output).context("Steam dialogue font")?;
    dialogue::cook(&source, output, &font).context("Steam dialogue artwork and subtitles")?;
    eprintln!("[asset type 20/28] Cooking boot logos and title artwork");
    startup::cook(&source, output).context("Steam startup artwork")?;
    eprintln!("[asset type 21/28] Cooking menu artwork and captions");
    menu_art::cook(&source, output).context("Steam menu artwork")?;
    eprintln!("[asset type 22/28] Cooking defeat artwork and captions");
    game_over::cook(&source, output).context("Steam defeat artwork")?;
    eprintln!("[asset type 23/28] Cooking strategy rules and presets");
    strategy::cook(&source, output).context("Steam strategy")?;
    eprintln!("[asset type 24/28] Cooking Grade Shop and crafting");
    menu_catalogues::cook(&source, output).context("Steam menu catalogues")?;
    eprintln!("[asset type 25/28] Cooking journal entries");
    menu_synopsis::cook(&source, output, &font).context("Steam journal")?;
    eprintln!("[asset type 26/28] Cooking world landmarks and guideposts");
    world_landmarks::cook(&source, output).context("Steam world landmarks")?;
    eprintln!("[asset type 27/28] Cooking world-map shops and destinations");
    menu_world_map::cook(&source, output).context("Steam world-map menu")?;
    eprintln!("[asset type 28/28] Cooking shared menu page captions");
    menu_presentation::cook(&source, output).context("Steam menu presentation")?;
    eprintln!(
        "Steam categories cooked in {:.3} seconds. Remaining legacy field resources, dynamic animation bank routes, game rules and other media are not yet ported.",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cook_tables(source: &source::Source, output: &Path) -> Result<()> {
    eprintln!("[asset type 1/28] Cooking descriptor");
    resonance_importer::cook_descriptor(output).context("library descriptor")?;
    eprintln!("[asset type 2/28] Cooking scripts");
    resonance_importer::cook_scripts(output).context("shared script definitions")?;
    eprintln!("[asset type 3/28] Cooking battle formations");
    formations::cook(source, output).context("Steam battle formations")?;
    eprintln!("[asset type 4/28] Cooking recoil");
    recoil::cook(source, output).context("Steam battle recoil")?;
    eprintln!("[asset type 5/28] Cooking items");
    items::cook(source, output).context("Steam items")?;
    eprintln!("[asset type 6/28] Cooking techniques");
    techniques::cook(source, output).context("Steam techniques")?;
    eprintln!("[asset type 7/28] Cooking field directory and foraging");
    field_directory::cook(source, output).context("Steam field directory")?;
    eprintln!("[asset type 8/28] Cooking EX skills");
    ex_skills::cook(source, output).context("Steam EX skills")?;
    eprintln!("[asset type 9/28] Cooking recipes and ingredient groups");
    cooking::cook(source, output).context("Steam cooking rules")?;
    eprintln!("[asset type 10/28] Cooking titles and award bindings");
    titles::cook(source, output).context("Steam titles")?;
    eprintln!("[asset type 11/28] Cooking character defaults and progression");
    characters::cook(source, output).context("Steam characters")?;
    Ok(())
}

#[cfg(test)]
mod tests;

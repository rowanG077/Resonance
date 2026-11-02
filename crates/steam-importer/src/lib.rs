//! Direct Steam conversion into the same editable library as the GameCube importer.
mod actor_model;
mod animation;
mod archive;
mod audio_effects;
mod audio_fields;
mod audio_settings;
mod audio_streams;
mod awb;
mod battle_party_mappings;
mod battle_party_model;
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
mod field_overlays;
mod field_scene;
mod field_services;
mod field_settings;
mod figurines;
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
mod monsters;
mod movies;
mod output;
mod package;
mod pe;
mod preview_model;
mod recoil;
mod resources;
mod script;
mod script_audio;
mod script_input;
mod skit_portraits;
mod skits;
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
    eprintln!("[asset type 12/42] Cooking shared field settings");
    field_settings::cook(&source, output).context("Steam field settings")?;
    eprintln!("[asset type 13/42] Cooking field scripts and collision");
    field_core::cook(&source, output, None).context("Steam field scripts and collision")?;
    eprintln!("[asset type 14/42] Cooking field scenery and cameras");
    field_scene::cook(&source, output, None).context("Steam field scenery and cameras")?;
    eprintln!("[asset type 15/42] Cooking party models and field motions");
    character_models::cook(&source, output, None)
        .context("Steam party models and field motions")?;
    eprintln!("[asset type 16/42] Cooking shared field models");
    let mut models = field_models::cook(&source, output).context("Steam shared field models")?;
    eprintln!("[asset type 17/42] Cooking field actors and animation bindings");
    field_actors::cook(&source, output, &mut models, None).context("Steam field actors")?;
    eprintln!("[asset type 18/42] Cooking manual, name editor and options");
    menu_settings::cook(&source, output).context("Steam menu settings")?;
    eprintln!("[asset type 19/42] Cooking dialogue font, artwork and subtitles");
    let font = dialogue_font::cook(&source, output).context("Steam dialogue font")?;
    dialogue::cook(&source, output, &font).context("Steam dialogue artwork and subtitles")?;
    eprintln!("[asset type 20/42] Cooking boot logos and title artwork");
    startup::cook(&source, output).context("Steam startup artwork")?;
    eprintln!("[asset type 21/42] Cooking menu artwork and captions");
    menu_art::cook(&source, output).context("Steam menu artwork")?;
    eprintln!("[asset type 22/42] Cooking defeat artwork and captions");
    game_over::cook(&source, output).context("Steam defeat artwork")?;
    eprintln!("[asset type 23/42] Cooking strategy rules and presets");
    strategy::cook(&source, output).context("Steam strategy")?;
    eprintln!("[asset type 24/42] Cooking Grade Shop and crafting");
    menu_catalogues::cook(&source, output).context("Steam menu catalogues")?;
    eprintln!("[asset type 25/42] Cooking journal entries");
    menu_synopsis::cook(&source, output, &font).context("Steam journal")?;
    eprintln!("[asset type 26/42] Cooking world landmarks and guideposts");
    world_landmarks::cook(&source, output).context("Steam world landmarks")?;
    eprintln!("[asset type 27/42] Cooking world-map shops and destinations");
    menu_world_map::cook(&source, output).context("Steam world-map menu")?;
    eprintln!("[asset type 28/42] Cooking shared menu page captions");
    menu_presentation::cook(&source, output).context("Steam menu presentation")?;
    eprintln!("[asset type 29/42] Cooking Monster Book records");
    monsters::catalogue(&source, output).context("Steam Monster Book")?;
    eprintln!("[asset type 30/42] Cooking monster previews");
    monsters::models(&source, output).context("Steam monster previews")?;
    eprintln!("[asset type 31/42] Cooking figurine records");
    figurines::catalogue(&source, output).context("Steam figurine catalogue")?;
    eprintln!("[asset type 32/42] Cooking figurine previews");
    figurines::models(&source, output).context("Steam figurine previews")?;
    eprintln!("[asset type 33/42] Cooking battle party costume mappings");
    battle_party_mappings::cook(&source, output).context("Steam battle party mappings")?;
    eprintln!("[asset type 34/42] Cooking battle party models and motions");
    battle_party_model::cook(&source, output).context("Steam battle party models")?;
    eprintln!("[asset type 35/42] Cooking music and speech recordings");
    let stream_effects = audio_streams::cook(&source, output).context("Steam stream audio")?;
    eprintln!("[asset type 36/42] Cooking effect recordings and mappings");
    audio_effects::cook(&source, output, stream_effects).context("Steam effect audio")?;
    eprintln!("[asset type 37/42] Cooking menu audio and mix settings");
    audio_settings::cook(output).context("Steam audio settings")?;
    eprintln!("[asset type 38/42] Cooking skit catalogue and scenarios");
    skits::cook(&source, output).context("Steam skit catalogue")?;
    eprintln!("[asset type 39/42] Cooking skit portrait images and tracks");
    skit_portraits::cook(&source, output).context("Steam skit portraits")?;
    eprintln!("[asset type 40/42] Cooking movies and field movie selections");
    movies::cook(&source, output, None).context("Steam movies")?;
    eprintln!("[asset type 41/42] Cooking save-point and treasure models");
    field_services::cook(&source, output).context("Steam field services")?;
    eprintln!("[asset type 42/42] Cooking field audio selections");
    audio_fields::cook(&source, output).context("Steam field audio")?;
    eprintln!(
        "Steam categories cooked in {:.3} seconds. Remaining legacy field resources, dynamic animation bank routes, game rules and other media are not yet ported.",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cook_tables(source: &source::Source, output: &Path) -> Result<()> {
    eprintln!("[asset type 1/42] Cooking descriptor");
    resonance_importer::cook_descriptor(output).context("library descriptor")?;
    eprintln!("[asset type 2/42] Cooking scripts");
    resonance_importer::cook_scripts(output).context("shared script definitions")?;
    eprintln!("[asset type 3/42] Cooking battle formations");
    formations::cook(source, output).context("Steam battle formations")?;
    eprintln!("[asset type 4/42] Cooking recoil");
    recoil::cook(source, output).context("Steam battle recoil")?;
    eprintln!("[asset type 5/42] Cooking items");
    items::cook(source, output).context("Steam items")?;
    eprintln!("[asset type 6/42] Cooking techniques");
    techniques::cook(source, output).context("Steam techniques")?;
    eprintln!("[asset type 7/42] Cooking field directory and foraging");
    field_directory::cook(source, output).context("Steam field directory")?;
    eprintln!("[asset type 8/42] Cooking EX skills");
    ex_skills::cook(source, output).context("Steam EX skills")?;
    eprintln!("[asset type 9/42] Cooking recipes and ingredient groups");
    cooking::cook(source, output).context("Steam cooking rules")?;
    eprintln!("[asset type 10/42] Cooking titles and award bindings");
    titles::cook(source, output).context("Steam titles")?;
    eprintln!("[asset type 11/42] Cooking character defaults and progression");
    characters::cook(source, output).context("Steam characters")?;
    Ok(())
}

#[cfg(test)]
mod tests;

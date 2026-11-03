//! Direct Steam conversion into the same editable library as the GameCube importer.
mod actor_model;
mod animation;
mod archive;
mod audio_effects;
mod audio_fields;
mod audio_settings;
mod audio_streams;
mod awb;
mod battle_audio;
mod battle_defense;
mod battle_effects;
mod battle_enemies;
pub use battle_enemies::actions as decode_enemy_actions;
mod battle_martial;
mod battle_normal;
mod battle_party_mappings;
mod battle_party_model;
mod battle_party_rules;
mod battle_profiles;
mod battle_projectiles;
mod battle_unison;
mod battle_voices;
mod character_models;
mod characters;
mod cooking;
mod coordinates;
mod dialogue;
mod dialogue_font;
mod effect_atlas;
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
    eprintln!("[asset type 12/52] Cooking shared field settings");
    field_settings::cook(&source, output).context("Steam field settings")?;
    eprintln!("[asset type 13/52] Cooking field scripts and collision");
    field_core::cook(&source, output, None).context("Steam field scripts and collision")?;
    eprintln!("[asset type 14/52] Cooking field scenery and cameras");
    field_scene::cook(&source, output, None).context("Steam field scenery and cameras")?;
    eprintln!("[asset type 15/52] Cooking party models and field motions");
    character_models::cook(&source, output, None)
        .context("Steam party models and field motions")?;
    eprintln!("[asset type 16/52] Cooking shared field models");
    let mut models = field_models::cook(&source, output).context("Steam shared field models")?;
    eprintln!("[asset type 17/52] Cooking field actors and animation bindings");
    field_actors::cook(&source, output, &mut models, None).context("Steam field actors")?;
    eprintln!("[asset type 18/52] Cooking manual, name editor and options");
    menu_settings::cook(&source, output).context("Steam menu settings")?;
    eprintln!("[asset type 19/52] Cooking dialogue font, artwork and subtitles");
    let font = dialogue_font::cook(&source, output).context("Steam dialogue font")?;
    dialogue::cook(&source, output, &font).context("Steam dialogue artwork and subtitles")?;
    eprintln!("[asset type 20/52] Cooking boot logos and title artwork");
    startup::cook(&source, output).context("Steam startup artwork")?;
    eprintln!("[asset type 21/52] Cooking menu artwork and captions");
    menu_art::cook(&source, output).context("Steam menu artwork")?;
    eprintln!("[asset type 22/52] Cooking defeat artwork and captions");
    game_over::cook(&source, output).context("Steam defeat artwork")?;
    eprintln!("[asset type 23/52] Cooking strategy rules and presets");
    strategy::cook(&source, output).context("Steam strategy")?;
    eprintln!("[asset type 24/52] Cooking Grade Shop and crafting");
    menu_catalogues::cook(&source, output).context("Steam menu catalogues")?;
    eprintln!("[asset type 25/52] Cooking journal entries");
    menu_synopsis::cook(&source, output, &font).context("Steam journal")?;
    eprintln!("[asset type 26/52] Cooking world landmarks and guideposts");
    world_landmarks::cook(&source, output).context("Steam world landmarks")?;
    eprintln!("[asset type 27/52] Cooking world-map shops and destinations");
    menu_world_map::cook(&source, output).context("Steam world-map menu")?;
    eprintln!("[asset type 28/52] Cooking shared menu page captions");
    menu_presentation::cook(&source, output).context("Steam menu presentation")?;
    eprintln!("[asset type 29/52] Cooking Monster Book records");
    monsters::catalogue(&source, output).context("Steam Monster Book")?;
    eprintln!("[asset type 30/52] Cooking enemy models and battle motions");
    monsters::models(&source, output).context("Steam enemy model media")?;
    eprintln!("[asset type 31/52] Cooking figurine records");
    figurines::catalogue(&source, output).context("Steam figurine catalogue")?;
    eprintln!("[asset type 32/52] Cooking figurine previews");
    figurines::models(&source, output).context("Steam figurine previews")?;
    eprintln!("[asset type 33/52] Cooking battle party costume mappings");
    battle_party_mappings::cook(&source, output).context("Steam battle party mappings")?;
    eprintln!("[asset type 34/52] Cooking battle party models and motions");
    battle_party_model::cook(&source, output).context("Steam battle party models")?;
    eprintln!("[asset type 35/52] Cooking music and speech recordings");
    let stream_effects = audio_streams::cook(&source, output).context("Steam stream audio")?;
    eprintln!("[asset type 36/52] Cooking effect recordings and mappings");
    audio_effects::cook(&source, output, stream_effects).context("Steam effect audio")?;
    eprintln!("[asset type 37/52] Cooking menu audio and mix settings");
    audio_settings::cook(output).context("Steam audio settings")?;
    eprintln!("[asset type 38/52] Cooking skit catalogue and scenarios");
    skits::cook(&source, output).context("Steam skit catalogue")?;
    eprintln!("[asset type 39/52] Cooking skit portrait images and tracks");
    skit_portraits::cook(&source, output).context("Steam skit portraits")?;
    eprintln!("[asset type 40/52] Cooking movies and field movie selections");
    movies::cook(&source, output, None).context("Steam movies")?;
    eprintln!("[asset type 41/52] Cooking save-point and treasure models");
    field_services::cook(&source, output).context("Steam field services")?;
    eprintln!("[asset type 42/52] Cooking party normal attacks");
    battle_normal::cook(&source, output).context("Steam normal attacks")?;
    eprintln!("[asset type 43/52] Cooking party battle speech");
    battle_voices::cook(&source, output).context("Steam party speech")?;
    eprintln!("[asset type 44/52] Cooking party martial attacks");
    battle_martial::cook(&source, output).context("Steam martial attacks")?;
    eprintln!("[asset type 45/52] Cooking party battle profiles and policies");
    battle_party_rules::cook(&source, output).context("Steam party battle rules")?;
    eprintln!("[asset type 46/52] Cooking resident projectiles and spell rules");
    battle_projectiles::cook(&source, output).context("Steam resident projectiles")?;
    eprintln!("[asset type 47/52] Cooking Defense System rules");
    battle_defense::cook(&source, output).context("Steam Defense System")?;
    eprintln!("[asset type 48/52] Cooking battle audio settings and common cues");
    battle_audio::cook(&source, output).context("Steam battle audio")?;
    eprintln!("[asset type 49/52] Cooking Unison rules and scene mappings");
    battle_unison::cook(&source, output).context("Steam Unison rules")?;
    eprintln!("[asset type 50/52] Cooking field audio selections");
    audio_fields::cook(&source, output).context("Steam field audio")?;
    eprintln!("[asset type 51/52] Cooking enemy battle speech");
    battle_voices::enemies(&source, output).context("Steam enemy speech")?;
    eprintln!("[asset type 52/52] Cooking enemy effect programs, textures and models");
    battle_effects::cook_enemies(&source, output, None)
        .context("Steam enemy effect programs, textures and models")?;
    eprintln!(
        "Steam categories cooked in {:.3} seconds. Remaining legacy field resources, dynamic animation bank routes, game rules and other media are not yet ported.",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cook_tables(source: &source::Source, output: &Path) -> Result<()> {
    eprintln!("[asset type 1/52] Cooking descriptor");
    resonance_importer::cook_descriptor(output).context("library descriptor")?;
    eprintln!("[asset type 2/52] Cooking scripts");
    resonance_importer::cook_scripts(output).context("shared script definitions")?;
    eprintln!("[asset type 3/52] Cooking battle formations");
    formations::cook(source, output).context("Steam battle formations")?;
    eprintln!("[asset type 4/52] Cooking recoil");
    recoil::cook(source, output).context("Steam battle recoil")?;
    eprintln!("[asset type 5/52] Cooking items");
    items::cook(source, output).context("Steam items")?;
    eprintln!("[asset type 6/52] Cooking techniques");
    techniques::cook(source, output).context("Steam techniques")?;
    eprintln!("[asset type 7/52] Cooking field directory and foraging");
    field_directory::cook(source, output).context("Steam field directory")?;
    eprintln!("[asset type 8/52] Cooking EX skills");
    ex_skills::cook(source, output).context("Steam EX skills")?;
    eprintln!("[asset type 9/52] Cooking recipes and ingredient groups");
    cooking::cook(source, output).context("Steam cooking rules")?;
    eprintln!("[asset type 10/52] Cooking titles and award bindings");
    titles::cook(source, output).context("Steam titles")?;
    eprintln!("[asset type 11/52] Cooking character defaults and progression");
    characters::cook(source, output).context("Steam characters")?;
    Ok(())
}

#[cfg(test)]
mod tests;

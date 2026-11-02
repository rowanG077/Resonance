//! Direct Steam conversion into the same editable library as the GameCube importer.
mod animation;
mod archive;
mod character_models;
mod coordinates;
mod ex_skills;
mod field_core;
mod field_directory;
mod field_scene;
mod field_settings;
mod formations;
mod items;
mod model;
mod output;
mod package;
mod pe;
mod recoil;
mod script;
mod script_input;
mod source;
mod techniques;
mod text;
mod texture;
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
    eprintln!("[asset type 9/12] Cooking shared field settings");
    field_settings::cook(&source, output).context("Steam field settings")?;
    eprintln!("[asset type 10/12] Cooking field scripts and collision");
    field_core::cook(&source, output, None).context("Steam field scripts and collision")?;
    eprintln!("[asset type 11/12] Cooking field scenery and cameras");
    field_scene::cook(&source, output, None).context("Steam field scenery and cameras")?;
    eprintln!("[asset type 12/12] Cooking party models and field motions");
    character_models::cook(&source, output, None)
        .context("Steam party models and field motions")?;
    eprintln!(
        "Steam categories cooked in {:.3} seconds. Remaining game rules, field actors and other media categories are not yet ported.",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cook_tables(source: &source::Source, output: &Path) -> Result<()> {
    eprintln!("[asset type 1/12] Cooking descriptor");
    resonance_importer::cook_descriptor(output).context("library descriptor")?;
    eprintln!("[asset type 2/12] Cooking scripts");
    resonance_importer::cook_scripts(output).context("shared script definitions")?;
    eprintln!("[asset type 3/12] Cooking battle formations");
    formations::cook(source, output).context("Steam battle formations")?;
    eprintln!("[asset type 4/12] Cooking recoil");
    recoil::cook(source, output).context("Steam battle recoil")?;
    eprintln!("[asset type 5/12] Cooking items");
    items::cook(source, output).context("Steam items")?;
    eprintln!("[asset type 6/12] Cooking techniques");
    techniques::cook(source, output).context("Steam techniques")?;
    eprintln!("[asset type 7/12] Cooking field directory and foraging");
    field_directory::cook(source, output).context("Steam field directory")?;
    eprintln!("[asset type 8/12] Cooking EX skills");
    ex_skills::cook(source, output).context("Steam EX skills")?;
    Ok(())
}

#[cfg(test)]
mod tests;

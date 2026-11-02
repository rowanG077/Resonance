//! Direct Steam conversion into the same editable library as the GameCube importer.
mod archive;
mod ex_skills;
mod field_directory;
mod formations;
mod items;
mod output;
mod package;
mod pe;
mod recoil;
mod source;
mod techniques;
mod text;
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
    eprintln!("[asset type 1/8] Cooking descriptor");
    resonance_importer::cook_descriptor(output).context("library descriptor")?;
    eprintln!("[asset type 2/8] Cooking scripts");
    resonance_importer::cook_scripts(output).context("shared script definitions")?;
    eprintln!("[asset type 3/8] Cooking battle formations");
    formations::cook(&source, output).context("Steam battle formations")?;
    eprintln!("[asset type 4/8] Cooking recoil");
    recoil::cook(&source, output).context("Steam battle recoil")?;
    eprintln!("[asset type 5/8] Cooking items");
    items::cook(&source, output).context("Steam items")?;
    eprintln!("[asset type 6/8] Cooking techniques");
    techniques::cook(&source, output).context("Steam techniques")?;
    eprintln!("[asset type 7/8] Cooking field directory and foraging");
    field_directory::cook(&source, output).context("Steam field directory")?;
    eprintln!("[asset type 8/8] Cooking EX skills");
    ex_skills::cook(&source, output).context("Steam EX skills")?;
    eprintln!(
        "Steam categories cooked in {:.3} seconds. Remaining game, field and media categories are not yet ported.",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests;

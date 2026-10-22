//! Refresh enemy gameplay metadata without converting body or effect artwork.
use anyhow::{Context, Result, ensure};
use std::{fs, path::PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let extracted = PathBuf::from(
        args.next()
            .context("usage: cook_enemy_definitions EXTRACTED OUTPUT")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: cook_enemy_definitions EXTRACTED OUTPUT")?,
    );
    ensure!(args.next().is_none(), "unexpected argument");
    if output.exists() {
        ensure!(
            fs::read_dir(&output)?.next().is_none(),
            "output must be empty"
        );
    } else {
        fs::create_dir_all(&output)?;
    }
    let paths = resonance_import::battle_enemy::publish_all(&extracted, &output)?;
    ensure!(
        paths.iter().all(|path| path.ends_with("/definition.json")),
        "enemy metadata refresh unexpectedly published artwork"
    );
    fs::write(
        output.join(".enemy-definitions-publication.json"),
        serde_json::to_vec(&paths)?,
    )?;
    println!("Enemy gameplay definitions: {}", paths.len());
    Ok(())
}

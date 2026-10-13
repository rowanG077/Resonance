//! Bind complete audio metadata against existing PCM; no other battle cook.
use anyhow::{Result, ensure};
use std::{fs, path::PathBuf};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(
        args.len() == 4,
        "usage: cook_battle_audio_catalogue EXTRACTED1 EXTRACTED2 LIBRARY OUTPUT"
    );
    let [first, second, library, output] = args.as_slice() else {
        unreachable!()
    };
    fs::create_dir_all(output)?;
    ensure!(
        library.canonicalize()? != output.canonicalize()?,
        "stage into a separate output directory"
    );
    let paths = resonance_import::battle_audio::publish_all(
        &[first.clone(), second.clone()],
        library,
        output,
    )?;
    let report = output.join("battle-audio-report.json");
    fs::write(
        &report,
        serde_json::to_vec(&serde_json::json!({"paths": paths}))?,
    )?;
    println!(
        "Audio: {} closure paths; report {}",
        paths.len(),
        report.display()
    );
    Ok(())
}

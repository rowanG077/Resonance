use super::{PLAYBACK_RATE, Workspace, hash_file, write_json};
use anyhow::Result;
use resonance_audio::cue::package::{Asset, Manifest, Program};
use serde_json::json;
use std::{collections::BTreeMap, fs};

/// Prepare menu cues in Rust, without an audio output device.
pub(crate) fn prepare_title_sounds(workspace: Workspace) -> Result<()> {
    let _publications = crate::publication::Session::start_if_needed(&workspace.output)?;
    let executable = workspace.extracted.join("sys/main.dol");
    let executable_bytes = fs::read(&executable)?;
    let [_, bank] =
        crate::all_assets::roles::resident_banks(&workspace.extracted, &executable_bytes)?;
    let bank_path = workspace.extracted.join("files").join(bank);
    let auxiliary_reverbs = super::music::title_reverbs(&executable_bytes)?;
    let ids = [("navigate", 1), ("confirm", 2), ("back", 3), ("error", 4)];
    let pools = crate::media::library::Pools::read(&workspace.extracted)?;
    let metadata = workspace.output.join("title-sounds.json");
    let bytes = fs::read(&bank_path)?;
    let bank = pools.bank(&bytes)?;
    let mut cues = BTreeMap::new();
    for (name, id) in ids {
        let (resources, score) = super::sound_library::sound(&bank, id, &pools.sustains)?;
        let package = super::sound_library::package(
            &workspace.output,
            &resources,
            score,
            super::synthesis_tables(&executable_bytes)?,
            auxiliary_reverbs,
        )?;
        let program = super::field_audio::write_package(
            &workspace.output,
            &format!("audio/menu-sound-{id}.json"),
            &package,
        )?;
        cues.insert(
            name.into(),
            Asset {
                program: Program {
                    path: program.path,
                    sha256: program.sha256,
                },
            },
        );
    }
    let package = Manifest {
        version: resonance_audio::cue::package::VERSION,
        sample_rate: PLAYBACK_RATE,
        reverbs: auxiliary_reverbs,
        cues,
    };
    let path = "audio/menu-cues.json";
    write_json(
        &workspace.output.join(path),
        &serde_json::to_value(package)?,
    )?;
    let sha256 = hash_file(&workspace.output.join(path))?;
    Manifest::load(&workspace.output, path, &sha256, |_, error| Err(error))?;
    write_json(
        &metadata,
        &json!({"version": 3, "path": path, "sha256":sha256}),
    )?;
    println!("Cooked {} menu cues for live synthesis", ids.len());
    Ok(())
}

use super::{
    PLAYBACK_RATE, SAMPLE_RATE, Workspace, hash_file, json_file, valid_asset, wav_frames,
    write_json,
};
use anyhow::Result;
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio::cue::package::{Asset, Manifest, Sample};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

/// Prepare menu cues in Rust, without an audio output device.
pub(crate) fn prepare_title_sounds(workspace: Workspace, coefficients: &Path) -> Result<()> {
    let _publications = crate::publication::Session::start_if_needed(&workspace.output)?;
    let executable = workspace.extracted.join("sys/main.dol");
    let executable_bytes = fs::read(&executable)?;
    let [_, bank] =
        crate::all_assets::roles::resident_banks(&workspace.extracted, &executable_bytes)?;
    let bank_path = workspace.extracted.join("files").join(bank);
    let auxiliary_reverbs = super::music::title_reverbs(&executable_bytes)?;
    let ids = [("navigate", 1), ("confirm", 2), ("back", 3), ("error", 4)];
    let pools = crate::media::library::Pools::read(&workspace.extracted)?;
    let recipe = json!({"version": 9, "bank_sha256": hash_file(&bank_path)?,
        "executable_sha256": hash_file(&executable)?, "bank_pool_sha256":pools.fingerprint()?, "auxiliary_reverbs": auxiliary_reverbs,
        "renderer": "resonance-audio-cook",
        "rust_renderer_sha256": hash_file(&std::env::current_exe()?)?,
        "coefficients_sha256":hash_file(coefficients)?,
        "ids": ids.into_iter().collect::<BTreeMap<_,_>>(), "synthesis_rate": SAMPLE_RATE, "sample_rate": PLAYBACK_RATE});
    let metadata = workspace.output.join("title-sounds.json");
    if let Some(previous) = json_file(&metadata)
        && previous["version"] == 3
        && previous["recipe"] == recipe
        && previous["path"] == "audio/menu-cues.json"
        && valid_asset(&workspace.output, &previous)
        && serde_json::from_value::<resonance_content::TitleSounds>(previous).is_ok_and(|sounds| {
            sounds.validate().is_ok()
                && Manifest::load(&workspace.output, &sounds.path, &sounds.sha256).is_ok()
        })
    {
        println!("Title sounds are current");
        return Ok(());
    }
    let bytes = fs::read(&bank_path)?;
    let bank = pools.bank(&bytes)?;
    let coefficients = fs::read(coefficients)?;
    let tables = super::sound_buses::tables(&executable_bytes)?;
    let mut previews = serde_json::Map::new();
    let mut cues = BTreeMap::new();
    for (name, id) in ids {
        let path = format!("audio/menu-{name}.wav");
        let destination = workspace.output.join(&path);
        let (resources, score) = super::sound_library::sound(&bank, id)?;
        let package = super::sound_library::package(
            &workspace.output,
            &resources,
            score,
            super::synthesis_tables(&executable_bytes, &coefficients)?,
            auxiliary_reverbs,
        )?;
        let program = super::field_audio::write_package(
            &workspace,
            &format!("audio/menu-sound-{id}.json"),
            &package,
        )?;
        let package = resonance_audio::package::Loaded {
            resources: resonance_audio::data::Resources {
                programs: package.programs,
                samples: resources.samples,
            },
            score: package.score,
            tables: package.tables,
            reverbs: package.reverbs,
        };
        let mut stream =
            resonance_audio::sequence::stream::Stream::new(std::sync::Arc::new(package), false)?;
        let mut buses = [Vec::new(), Vec::new(), Vec::new()];
        while let Some(block) = stream.block(resonance_audio::sequence::LiveControls {
            pan: Some(64),
            ..Default::default()
        })? {
            anyhow::ensure!(
                buses[0].len() / 2 + block.len() <= (SAMPLE_RATE * 10) as usize,
                "menu cue exceeds ten seconds"
            );
            for frame in block {
                for (bus, samples) in buses.iter_mut().zip(frame) {
                    bus.extend(samples);
                }
            }
        }
        let samples = resonance_audio::reverb::mix_studio(&buses, auxiliary_reverbs)?;
        let frames = write_pcm(&destination, &samples)?;
        cues.insert(
            name.into(),
            Asset {
                frames,
                sample: None,
                program: Some(Sample {
                    path: program.path,
                    sha256: program.sha256,
                }),
                controls: Vec::new(),
            },
        );
        previews.insert(
            name.into(),
            json!({"path": path, "sha256": hash_file(&destination)?,
            "frames": frames, "sample_rate": PLAYBACK_RATE, "channels": 2}),
        );
    }
    let package = Manifest {
        version: resonance_audio::cue::package::VERSION,
        sample_rate: PLAYBACK_RATE,
        reverbs: auxiliary_reverbs,
        tables,
        cues,
    };
    let path = "audio/menu-cues.json";
    write_json(
        &workspace.output.join(path),
        &serde_json::to_value(package)?,
    )?;
    let sha256 = hash_file(&workspace.output.join(path))?;
    Manifest::load(&workspace.output, path, &sha256)?;
    write_json(
        &metadata,
        &json!({"version": 3, "recipe": recipe, "path": path, "sha256":sha256, "previews": previews}),
    )?;
    println!(
        "Cooked {} menu cues with live controls and isolated previews",
        ids.len()
    );
    Ok(())
}

fn write_pcm(path: &Path, samples: &[i16]) -> Result<u32> {
    anyhow::ensure!(
        !samples.is_empty()
            && samples.len().is_multiple_of(2)
            && samples.len() <= SAMPLE_RATE as usize * 10 * 2,
        "invalid cue length"
    );
    let temporary = crate::temporary_path(path);
    write_pcm16(&temporary, 2, PLAYBACK_RATE, samples.iter().copied())?;
    let frames = wav_frames(&temporary, PLAYBACK_RATE, PLAYBACK_RATE * 10)?;
    crate::publication::install(&temporary, path, &hash_file(&temporary)?)?;
    Ok(frames)
}

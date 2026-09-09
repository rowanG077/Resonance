use super::{
    PLAYBACK_RATE, SAMPLE_RATE, Workspace, hash_file, json_file, valid_asset, wav_frames,
    write_json,
};
use anyhow::Result;
use resonance_audio::cue::package::{Asset, Manifest, Sample};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

/// Cook menu cues entirely in Rust, without an external renderer or audio device.
pub fn cook_title_sounds(extracted: &Path, output: &Path) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let bank_path = workspace.extracted.join("files/S/se.snd");
    let executable = workspace.extracted.join("sys/main.dol");
    let executable_bytes = fs::read(&executable)?;
    let auxiliary_reverbs = super::music::title_reverbs(&executable_bytes)?;
    // Title navigation and confirmation cues.
    let ids = [("navigate", 1), ("confirm", 2)];
    let recipe = json!({"version": 8, "bank_sha256": hash_file(&bank_path)?,
        "executable_sha256": hash_file(&executable)?, "auxiliary_reverbs": auxiliary_reverbs,
        "renderer": "resonance-audio-cook",
        "rust_renderer_sha256": hash_file(&std::env::current_exe()?)?,
        "ids": {"navigate": 1, "confirm": 2}, "synthesis_rate": SAMPLE_RATE, "sample_rate": PLAYBACK_RATE});
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
    let bank = resonance_audio_cook::bank::Bank::parse(&bytes)?;
    let tables = super::sound_buses::tables(&executable_bytes)?;
    let mut previews = serde_json::Map::new();
    let mut cues = BTreeMap::new();
    fs::create_dir_all(workspace.output.join("audio/cues"))?;
    for (name, id) in ids {
        let path = format!("audio/menu-{name}.wav");
        let destination = workspace.output.join(&path);
        let voice =
            resonance_audio_cook::render::render_voice_buses(&bank, id, &tables, SAMPLE_RATE * 10)?;
        let sample_path = format!("audio/cues/{name}.wav");
        let sample_destination = workspace.output.join(&sample_path);
        write_pcm(&sample_destination, &voice.pcm, 1, false)?;
        cues.insert(
            name.into(),
            Asset {
                frames: (voice.buses[0].len() / 2) as u32,
                sample: Sample {
                    path: sample_path,
                    sha256: hash_file(&sample_destination)?,
                },
                controls: voice.controls,
            },
        );
        // Keep independently comparable isolated previews. Playback reads controls.
        let samples = resonance_audio_cook::reverb::mix_studio(&voice.buses, auxiliary_reverbs)?;
        let frames = write_pcm(&destination, &samples, 2, true)?;
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
        "Cooked {} menu cues to envelope PCM, gain controls and isolated previews",
        ids.len()
    );
    Ok(())
}

fn write_pcm(path: &Path, samples: &[i16], channels: u16, require_signal: bool) -> Result<u32> {
    anyhow::ensure!(
        !samples.is_empty()
            && (1..=2).contains(&channels)
            && samples.len().is_multiple_of(usize::from(channels))
            && samples.len() <= SAMPLE_RATE as usize * 10 * usize::from(channels),
        "invalid cue length"
    );
    let temporary = path.with_extension("partial.wav");
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels,
            sample_rate: PLAYBACK_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    let frames = if require_signal {
        wav_frames(&temporary, PLAYBACK_RATE, PLAYBACK_RATE * 10)?
    } else {
        // Silence is valid in an envelope sample.
        hound::WavReader::open(&temporary)?.duration()
    };
    fs::rename(&temporary, path)?;
    Ok(frames)
}

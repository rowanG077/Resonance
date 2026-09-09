use super::{PLAYBACK_RATE, Workspace, hash_file, json_file, valid_asset, write_json};
use anyhow::Result;
use resonance_audio::{
    package::{Package, SampleAsset},
    sequence, volume,
};
use resonance_audio_cook::{bank::Bank, compile, song::Song};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

pub fn cook_title_audio(extracted: &Path, output: &Path, coefficients: &Path) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let bank_bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let song_bytes = fs::read(workspace.extracted.join("files/S/bgm_etc000.song"))?;
    let executable = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let coefficient_bytes = fs::read(coefficients)?;
    let recipe = json!({"version":5,"compiler":"resonance-audio-cook",
        "compiler_sha256":hash_file(&std::env::current_exe()?)?,
        "bank_sha256":crate::digest(&bank_bytes),"song_sha256":crate::digest(&song_bytes),
        "executable_sha256":crate::digest(&executable),"coefficients_sha256":crate::digest(&coefficient_bytes),
        "sample_rate":PLAYBACK_RATE,"setup":1,"package_version":resonance_audio::package::VERSION});
    let metadata = workspace.output.join("title-audio.json");
    if let Some(previous) = json_file(&metadata)
        && previous["recipe"] == recipe
        && previous["version"] == 3
        && previous["path"] == "audio/title-music.json"
        && valid_asset(&workspace.output, &previous)
        && Package::load(&workspace.output, "audio/title-music.json").is_ok()
    {
        println!("Title music package is current");
        return Ok(());
    }
    let bank = Bank::parse(&bank_bytes)?;
    let song = Song::parse(&song_bytes)?;
    let setup = bank.music_setup(0, 1)?;
    let (resources, score) = compile::music(&bank, &song, &setup)?;
    let tables = super::music_voice::tables(&executable, &coefficient_bytes)?;
    let reverbs = super::music::title_reverbs(&executable)?;
    fs::create_dir_all(workspace.output.join("audio/title-instruments"))?;
    let mut samples = BTreeMap::new();
    for (id, sample) in &resources.samples {
        let name = format!("audio/title-instruments/sample-{id}.wav");
        let path = workspace.output.join(&name);
        let temporary = path.with_extension("partial.wav");
        let mut writer = hound::WavWriter::create(
            &temporary,
            hound::WavSpec {
                channels: 1,
                sample_rate: u32::from(sample.rate),
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for &value in sample.pcm.iter().chain(&sample.loop_pcm) {
            writer.write_sample(value)?;
        }
        writer.finalize()?;
        fs::rename(temporary, &path)?;
        samples.insert(
            *id,
            SampleAsset {
                path: name,
                sha256: hash_file(&path)?,
                key: sample.key,
                rate: sample.rate,
                first_frames: sample.pcm.len() as u32,
                loop_start: sample.loop_start,
                loop_length: sample.loop_length,
            },
        );
    }
    let package = Package {
        version: resonance_audio::package::VERSION,
        programs: resources.programs,
        samples,
        score,
        tables,
        reverbs,
    };
    let path = workspace.output.join("audio/title-music.json");
    write_json(&path, &serde_json::to_value(&package)?)?;
    Package::load(&workspace.output, "audio/title-music.json")?;
    write_json(
        &metadata,
        &json!({"version":3,"path":"audio/title-music.json",
        "sha256":hash_file(&path)?,"sample_rate":PLAYBACK_RATE,"channels":2,
        "recipe_sha256":crate::digest(&serde_json::to_vec(&recipe)?),"recipe":recipe}),
    )?;
    println!(
        "Cooked {} instrument programs and {} decoded samples, without playback",
        package.programs.len(),
        package.samples.len()
    );
    Ok(())
}

pub fn render_cooked_title_audio(
    assets: &Path,
    output: &Path,
    frames: u32,
    lead: u16,
) -> Result<()> {
    let info: resonance_content::TitleAudio =
        serde_json::from_slice(&fs::read(assets.join("title-audio.json"))?)?;
    info.validate()?;
    let loaded = Package::load(assets, &info.path)?;
    let mut fade = volume::Startup::new(2000, 100, lead)?;
    let preview = sequence::render_preview_with_volume(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        frames,
        |frame| fade.value_at(u64::from(frame)),
    )?;
    fs::create_dir_all(output)?;
    let path = output.join("title-preview.wav");
    let temporary = path.with_extension("partial.wav");
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels: 2,
            sample_rate: PLAYBACK_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in preview.pcm {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    fs::rename(temporary, &path)?;
    write_json(
        &path.with_extension("json"),
        &json!({"version":1,"renderer":"resonance-audio",
        "audio_device":false,"inputs":"cooked_package_only","package_sha256":hash_file(&assets.join(info.path))?,
        "renderer_sha256":hash_file(&std::env::current_exe()?)?,"oracle_accepted":false,
        "master_fade_lead_ms":lead,"notes_started":preview.notes,"loop_start_frames":preview.loop_starts,
        "asset":{"path":"title-preview.wav","sha256":hash_file(&path)?,"frames":frames,"channels":2,"sample_rate":PLAYBACK_RATE}}),
    )?;
    println!("Recorded {frames} frames from cooked music data, without playback");
    Ok(())
}

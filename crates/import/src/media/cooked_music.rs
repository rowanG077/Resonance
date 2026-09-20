use super::{PLAYBACK_RATE, Workspace, hash_file, json_file, valid_asset, write_json, write_pcm16};
use anyhow::Result;
use resonance_audio::{package::Package, sequence, volume};
use serde_json::json;
use std::{fs, path::Path};

pub fn cook_title_audio(extracted: &Path, output: &Path) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let package = super::music_library::Library::open(output, workspace.disc)?.package(
        "S/bgm_etc000.song",
        1,
        None,
    )?;
    let recipe = json!({"version":6,"compiler":"resonance-audio-cook",
        "compiler_sha256":hash_file(&std::env::current_exe()?)?,
        "package_sha256":crate::digest(&serde_json::to_vec(&package)?),
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
    let path = workspace.output.join("audio/title-music.json");
    super::field_audio::write_package(&workspace, "audio/title-music.json", &package)?;
    write_json(
        &metadata,
        &json!({"version":3,"path":"audio/title-music.json",
        "sha256":hash_file(&path)?,"sample_rate":PLAYBACK_RATE,"channels":2,
        "recipe_sha256":crate::digest(&serde_json::to_vec(&recipe)?),"recipe":recipe}),
    )?;
    println!(
        "Bound {} instrument programs and {} shared samples, without playback",
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
    write_pcm16(&temporary, 2, PLAYBACK_RATE, preview.pcm)?;
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

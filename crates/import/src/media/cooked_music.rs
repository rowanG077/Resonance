use super::{PLAYBACK_RATE, Workspace, hash_file, write_json};
use anyhow::Result;
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio::{package::Package, sequence, volume};
use serde_json::json;
use std::{fs, path::Path};

pub(crate) fn prepare_title_audio(workspace: Workspace) -> Result<()> {
    let _publications = crate::publication::Session::start_if_needed(&workspace.output)?;
    let executable = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let pools = crate::media::library::Pools::read(&workspace.extracted)?;
    let package = super::music_library::package(&workspace, &executable, &pools, 1, None)?;
    let metadata = workspace.output.join("title-audio.json");
    let path = workspace.output.join("audio/title-music.json");
    super::field_audio::write_package(&workspace.output, "audio/title-music.json", &package)?;
    write_json(
        &metadata,
        &json!({"version":3,"path":"audio/title-music.json",
        "sha256":hash_file(&path)?}),
    )?;
    println!(
        "Bound {} instrument programs and {} shared samples, without playback",
        package.programs.len(),
        package.samples.len()
    );
    Ok(())
}

pub fn render_cooked_title_audio(assets: &Path, output: &Path, frames: u32) -> Result<()> {
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let info: resonance_content::TitleAudio =
        serde_json::from_slice(&fs::read(assets.join("title-audio.json"))?)?;
    info.validate()?;
    let loaded = Package::load_verified(assets, &info.path, &info.sha256, &mut Default::default())?;
    let reverbs = loaded.reverbs();
    let mut preview = sequence::render_preview(loaded.into(), reverbs, frames)?;
    let fade = volume::Fade::new(0., 1., volume::TITLE_STARTUP_MS)?;
    for (frame, stereo) in preview.pcm.chunks_exact_mut(2).enumerate() {
        for sample in stereo {
            *sample = (f32::from(*sample) * fade.value_at(frame as u64)) as i16;
        }
    }
    fs::create_dir_all(output)?;
    let path = output.join("title-preview.wav");
    let temporary = crate::temporary_path(&path);
    write_pcm16(&temporary, 2, PLAYBACK_RATE, preview.pcm)?;
    crate::publication::install(&temporary, &path, &hash_file(&temporary)?)?;
    write_json(
        &path.with_extension("json"),
        &json!({"version":1,"renderer":"resonance-audio",
        "audio_device":false,"inputs":"cooked_package_only","package_sha256":hash_file(&assets.join(info.path))?,
        "renderer_sha256":hash_file(&std::env::current_exe()?)?,"oracle_accepted":false,
        "startup_fade_ms":volume::TITLE_STARTUP_MS,"notes_started":preview.notes,"loop_start_frames":preview.loop_starts,
        "asset":{"path":"title-preview.wav","sha256":hash_file(&path)?,"frames":frames,"channels":2,"sample_rate":PLAYBACK_RATE}}),
    )?;
    println!("Recorded {frames} frames from cooked music data, without playback");
    Ok(())
}

#[test]
#[ignore = "requires prepared title music; renders to temporary files without a device"]
fn title_preview_requires_the_published_music_digest() -> Result<()> {
    let assets = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
        std::path::PathBuf::from,
    );
    let output = tempfile::tempdir()?;
    let frames = resonance_audio::SOURCE_RATE;
    render_cooked_title_audio(&assets, output.path(), frames)?;
    let mut wave = hound::WavReader::open(output.path().join("title-preview.wav"))?;
    assert_eq!(wave.duration(), frames);
    assert!(wave.samples::<i16>().any(|sample| sample.unwrap() != 0));

    let input = tempfile::tempdir()?;
    let metadata = fs::read(assets.join("title-audio.json"))?;
    let info: resonance_content::TitleAudio = serde_json::from_slice(&metadata)?;
    let mut package = fs::read(assets.join(&info.path))?;
    package.push(b'\n'); // Valid JSON still has to match its published digest.
    let path = input.path().join(&info.path);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, package)?;
    fs::write(input.path().join("title-audio.json"), metadata)?;
    let rejected = output.path().join("rejected");
    let error = render_cooked_title_audio(input.path(), &rejected, frames).unwrap_err();
    assert!(error.to_string().contains("digest differs"), "{error:#}");
    assert!(!rejected.join("title-preview.wav").exists());
    Ok(())
}

use super::*;
use anyhow::{Context, Result};
use resonance_content::{library::Mod, startup};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires a complete current named library; prepares day/night title and editable overrides"]
fn original_named_title_prepares_both_backgrounds_without_inventories() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to current named content")?;
    let work = tempfile::tempdir()?;
    let library = Library::open(&root, &[])?;
    let day = capture(&library, true, 12, Diagnostics::new(true), || false)?;
    let mut night = capture(&library, false, 23, Diagnostics::new(true), || false)?;
    assert_eq!(day.scene.assets.map_id, 474);
    assert_eq!(night.scene.assets.map_id, 531);
    assert!(day.boot.is_some() && night.boot.is_none());
    assert!(day.art.textures.len() >= 18);
    assert_eq!(day.art.textures.len(), night.art.textures.len());
    for (day_texture, night_texture) in day.art.textures.iter().zip(&night.art.textures) {
        assert_eq!(day_texture.path, night_texture.path);
        assert_eq!(
            day.files.read(&day_texture.path)?,
            night.files.read(&night_texture.path)?
        );
    }
    assert!(
        day.files
            .keys()
            .all(|path| !path.ends_with(".preload.json"))
    );
    assert!(
        day.files
            .keys()
            .all(|path| !path.starts_with("assets/") && !path.ends_with(".preload.json"))
    );
    assert!(!root.join("title.json").exists());
    assert!(!root.join("title-audio.json").exists());
    assert!(!root.join("title-sounds.json").exists());
    for (path, _) in day.files.iter() {
        if path.ends_with("sound.json")
            || path.ends_with("cues.json")
            || path == startup::PATH
            || path == startup::BOOT
        {
            let document = std::str::from_utf8(&day.files.read(path)?)?.to_owned();
            assert!(
                !document.contains("sha256"),
                "authoring file {path} contains a digest"
            );
        }
    }
    let mut day_events = day.events.fresh()?;
    for _ in 0..120 {
        day_events.step()?;
        night.events.step()?;
    }
    assert!(!day.audio.is_empty() && !night.audio.is_empty());
    let cues = render_audio(day.audio.cue_only())?;
    assert!(cues.iter().any(|sample| *sample != 0.));
    let actual = render_audio(day.audio.clone())?;
    let definition: startup::Definition =
        library.snapshot().json(startup::PATH, 16 * 1024 * 1024)?;
    let song =
        resonance_content::music::capture(&mut library.snapshot(), &definition.music, &|| false)?;
    let recording = resonance_audio::recording::Prepared::new(
        song.audio,
        song.definition.repeat.map(|r| r.start_frame..r.end_frame),
    )?;
    let mut player = recording.start(resonance_audio::SOURCE_RATE)?;
    let mut fade = resonance_audio::volume::Startup::new(2000, 100, 1185)?;
    let mut gain = 0.;
    for (index, (pcm, cue)) in actual.chunks_exact(2).zip(cues.chunks_exact(2)).enumerate() {
        let frame = player.next_frame()?.unwrap_or([0.; 2]);
        if index.is_multiple_of(resonance_audio::CONTROL_FRAMES) {
            gain = fade.value_at(index as u64);
        }
        for channel in 0..2 {
            assert_eq!(
                pcm[channel],
                (frame[channel] * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16 as f32
                    / 32768.
                    + cue[channel]
            );
        }
    }
    assert!(actual.iter().any(|sample| *sample != 0.));
    assert!(!root.join("audio/instruments/title").exists());
    assert!(!root.join("audio/music/title/sound.json").exists());

    let modification = work.path().join("night-menu");
    fs::create_dir_all(modification.join("ui/title"))?;
    let mut definition: startup::Definition =
        serde_json::from_slice(&fs::read(root.join(startup::PATH))?)?;
    definition.day_field = 531;
    fs::write(
        modification.join(startup::PATH),
        serde_json::to_vec(&definition)?,
    )?;
    let modified = Library::open(
        &root,
        &[Mod {
            name: "night-menu".into(),
            root: modification,
        }],
    )?;
    let changed = capture(&modified, false, 12, Diagnostics::new(true), || false)?;
    assert_eq!(changed.scene.assets.map_id, 531);
    assert!(
        changed
            .files
            .overrides()
            .iter()
            .any(|change| change.path == startup::PATH && change.replacement == "night-menu")
    );
    assert_eq!(
        day.scene.assets.map_id, 474,
        "retained title keeps its selected background"
    );
    assert!(capture(&library, true, 12, Diagnostics::new(false), || true).is_err());
    if let Some(output) = std::env::var_os("RESONANCE_NAMED_TITLE_CAPTURE") {
        let output = PathBuf::from(output);
        let mut options = crate::test_support::run_options(&root);
        options.capture = Some(output.clone());
        options.capture_at = Some(crate::CaptureAt::TitleTick(120));
        options.reveal = true;
        options.saves.directory = Some(work.path().join("saves"));
        let (app, _) = crate::build_app_with_display(options, Default::default())?;
        let resident = app.world().resource::<crate::loading::Resident>().clone();
        crate::playthrough::capture(app, crate::CaptureAt::TitleTick(120), &output)?;
        assert!(output.is_file());
        assert_eq!(
            resident
                .unprepared_reads
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert!(!resident.diagnostics.has_errors());
        println!(
            "Named title rendered at tick 120 with no unprepared reads: {}",
            output.display()
        );
    }
    println!(
        "Named title prepared both backgrounds, {} artwork textures, startup logos and live audio; scripts advanced 120 updates",
        day.art.textures.len()
    );
    Ok(())
}

fn render_audio(assets: crate::audio::PlaybackAssets) -> Result<Vec<f32>> {
    use resonance_playback::Decodable;
    let (source, control) = assets.session(false);
    let mut decoder = source.decoder();
    let mut pcm = Vec::new();
    for cue in ["navigate", "confirm", "back", "error"] {
        control.play(cue)?;
        pcm.extend(decoder.by_ref().take(resonance_audio::SOURCE_RATE as usize));
        control.check()?;
    }
    Ok(pcm)
}

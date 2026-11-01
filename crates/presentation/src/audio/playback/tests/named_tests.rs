use super::*;
use resonance_content::{
    library::{Library, Mod},
    startup,
};

pub(super) fn write_startup(root: &Path) -> Result<startup::Definition> {
    fs::write(
        root.join("content.json"),
        &serde_json::to_vec(&resonance_content::library::Descriptor {
            version: resonance_content::library::VERSION,
            game: resonance_content::library::GAME.into(),
        })?,
    )?;
    write_music(&root.join("music.wav"), 16, 32028, [4000, -2000])?;
    // Audio preparation intentionally does not load title images.
    let definition = startup::Definition {
        day_field: 474,
        night_field: 531,
        artwork: Vec::new(),
        music: "music.wav".into(),
        cues: "cues.json".into(),
    };
    fs::create_dir_all(root.join("ui/title"))?;
    fs::write(
        root.join(startup::PATH),
        serde_json::to_vec_pretty(&definition)?,
    )?;
    Ok(definition)
}

fn definition(root: &Path) -> Result<startup::Definition> {
    Ok(serde_json::from_slice(&fs::read(
        root.join(startup::PATH),
    )?)?)
}

fn write_music(path: &Path, frames: usize, rate: u32, pcm: [i16; 2]) -> Result<()> {
    write_music_loop(path, frames, rate, pcm, Some(0..frames as u32))
}

fn write_music_loop(
    path: &Path,
    frames: usize,
    rate: u32,
    pcm: [i16; 2],
    repeat: Option<std::ops::Range<u32>>,
) -> Result<()> {
    let mut wave = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..frames {
        for sample in pcm {
            wave.write_sample(sample)?;
        }
    }
    wave.finalize()?;
    if let Some(repeat) = repeat {
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        resonance_asset_writer::wav::append_loop(&mut file, rate, repeat)?;
    }
    Ok(())
}

fn render(assets: PlaybackAssets) -> Vec<f32> {
    let (source, control) = assets.cue_only().session(false);
    control.play("navigate").unwrap();
    let pcm = source.decoder().take(640).collect();
    control.check().unwrap();
    pcm
}

#[test]
fn title_recording_command_uses_selected_mod_music() -> Result<()> {
    let root = disk_assets();
    definition(root.path())?;
    let modification = tempfile::tempdir()?;
    write_music(
        &modification.path().join("music.wav"),
        16,
        32028,
        [-6000, 3000],
    )?;
    let output = tempfile::tempdir()?;
    let mut recordings = Vec::new();
    for (name, mods) in [("base", vec![]), ("mod", vec![modification.path().into()])] {
        let path = output.path().join(format!("{name}.wav"));
        crate::record_title_music(root.path(), &mods, &path, 6400, false, &[])?;
        let pcm = hound::WavReader::open(path)?
            .into_samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(pcm.len(), 12800);
        recordings.push(pcm);
    }
    assert!(recordings[0][12798] > 0);
    assert!(recordings[1][12798] < 0);
    assert!(recordings[0][12799] < 0);
    assert!(recordings[1][12799] > 0);
    Ok(())
}

#[test]
fn named_title_audio_preserves_cue_pcm_and_captures_current_mod_samples() -> Result<()> {
    let root = disk_assets();
    let definition = definition(root.path())?;
    let base = Library::open(root.path(), &[])?;
    let mut first = base.snapshot();
    let captured =
        PlaybackAssets::capture(&mut first, &definition, Diagnostics::new(true), &|| false)?;
    let expected = render(captured.clone());
    assert!(expected.iter().any(|sample| *sample != 0.));

    let modification = tempfile::tempdir()?;
    let mut wave = hound::WavWriter::create(
        modification.path().join("sample.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: resonance_audio::SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..16 {
        wave.write_sample(-2000i16)?;
    }
    wave.finalize()?;
    let library = Library::open(
        root.path(),
        &[Mod {
            name: "instruments".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut second = library.snapshot();
    let changed =
        PlaybackAssets::capture(&mut second, &definition, Diagnostics::new(true), &|| false)?;
    assert_ne!(render(changed), expected);
    assert_eq!(
        render(captured),
        expected,
        "retained audio must keep its admitted samples"
    );
    assert_eq!(
        render(PlaybackAssets::load(root.path(), Diagnostics::new(true))?),
        expected
    );
    assert_ne!(
        &*first.read("sample.wav", 1024)?,
        &*second.read("sample.wav", 1024)?
    );
    assert!(
        PlaybackAssets::capture(
            &mut base.snapshot(),
            &definition,
            Diagnostics::new(false),
            &|| true
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn named_title_audio_isolates_bad_cues_and_accepts_current_loop_edits() -> Result<()> {
    let root = disk_assets();
    let definition = definition(root.path())?;
    write_music_loop(
        &root.path().join("music.wav"),
        16,
        32028,
        [4000, -2000],
        Some(4..16),
    )?;
    assert!(PlaybackAssets::load(root.path(), Diagnostics::new(true)).is_ok());
    for (path, scope) in [
        ("music.wav", "title music preparation"),
        ("cues.json", "title cue preparation"),
        ("back.json", "title cue back"),
    ] {
        let original = fs::read(root.path().join(path))?;
        for missing in [false, true] {
            if missing {
                fs::remove_file(root.path().join(path))?;
            } else {
                fs::write(root.path().join(path), b"invalid edited document")?;
            }
            for paranoid in [false, true] {
                let policy = Diagnostics::new(paranoid);
                let result = PlaybackAssets::load(root.path(), policy.clone());
                assert_eq!(result.is_err(), paranoid, "{path}");
                assert_eq!(policy.entries().len(), 1);
                assert_eq!(policy.entries()[0].scope, scope);
                if let Ok(assets) = result {
                    assert_eq!(assets.music.is_none(), path == "music.wav");
                    assert_eq!(assets.cues.is_none(), path == "cues.json");
                    if let Some(cues) = assets.cues {
                        assert_eq!(cues.cues.contains_key("back"), path != "back.json");
                    }
                }
            }
            fs::write(root.path().join(path), &original)?;
        }
    }
    let library = Library::open(root.path(), &[])?;
    let count = std::cell::Cell::new(0);
    assert!(
        PlaybackAssets::capture(
            &mut library.snapshot(),
            &definition,
            Diagnostics::new(false),
            &|| {
                count.set(count.get() + 1);
                count.get() > 3
            }
        )
        .is_err(),
        "cancellation is never an optional audio diagnostic"
    );
    Ok(())
}

#[test]
fn named_title_music_streams_current_wav_with_fades_and_retained_replacements() -> Result<()> {
    use crate::audio::MusicFrames;
    use resonance_audio::{CONTROL_FRAMES, SOURCE_RATE, volume::Startup};
    let root = disk_assets();
    definition(root.path())?;
    let assets = PlaybackAssets::load(root.path(), Diagnostics::new(true))?;
    let retained = assets.music.unwrap();
    let mut fade = Startup::new(2000, 100, 1185)?;
    let mut original = MusicFrames::start(&retained)?;
    let mut audible = false;
    let mut gain = 0.;
    for index in 0..SOURCE_RATE as usize * 4 {
        if index.is_multiple_of(CONTROL_FRAMES) {
            gain = fade.value_at(index as u64);
        }
        let expected = [4000., -2000.].map(|v| (v * gain) as i16 as f32 / 32768.);
        let actual = original.next_frame()?;
        assert_eq!(actual, expected, "startup frame {index}");
        audible |= actual != [0.; 2];
    }
    assert!(audible);

    // Atomic replacement changes the next capture, including rate and length.
    // The existing recording retains its opened file and independent cursor.
    write_music(
        &root.path().join("replacement.wav"),
        48,
        48000,
        [-8000, 6000],
    )?;
    fs::rename(
        root.path().join("replacement.wav"),
        root.path().join("music.wav"),
    )?;
    let changed = PlaybackAssets::load(root.path(), Diagnostics::new(true))?
        .music
        .unwrap();
    let mut current = MusicFrames::start(&changed)?;
    let mut previous = MusicFrames::start(&retained)?;
    for _ in 0..SOURCE_RATE * 4 {
        current.next_frame()?;
        previous.next_frame()?;
    }
    assert_eq!(current.next_frame()?, [-8000., 6000.].map(|v| v / 32768.));
    assert_eq!(previous.next_frame()?, [4000., -2000.].map(|v| v / 32768.));

    // A one-shot replacement may finish; healthy menu cues remain available.
    write_music_loop(
        &root.path().join("music.wav"),
        16,
        32028,
        [-8000, 6000],
        None,
    )?;
    let one_shot = PlaybackAssets::load(root.path(), Diagnostics::new(true))?;
    assert!(one_shot.cues.is_some());
    let mut current = MusicFrames::start(&one_shot.music.unwrap())?;
    for _ in 0..SOURCE_RATE * 4 {
        current.next_frame()?;
    }
    assert_eq!(current.next_frame()?, [0.; 2]);
    Ok(())
}

#[test]
fn shared_program_mod_edits_reach_all_cues_and_keep_retained_playback() -> Result<()> {
    use resonance_audio::effect::{
        Definition,
        recipe::{ProgramSource, SharedProgram},
    };
    let root = disk_assets();
    let path = "audio/programs/menu.json";
    fs::create_dir_all(root.path().join("audio/programs"))?;
    for cue in ["cue.json", "back.json"] {
        let Definition::Synthesized(mut recipe) =
            serde_json::from_slice(&fs::read(root.path().join(cue))?)?
        else {
            unreachable!()
        };
        let ProgramSource::Inline(instructions) = recipe.programs.remove(&1).unwrap() else {
            unreachable!()
        };
        let shared = SharedProgram {
            instructions,
            samples: std::mem::take(&mut recipe.samples),
        };
        fs::write(root.path().join(path), serde_json::to_vec(&shared)?)?;
        recipe
            .programs
            .insert(1, ProgramSource::Shared(path.into()));
        fs::write(
            root.path().join(cue),
            serde_json::to_vec(&Definition::Synthesized(recipe))?,
        )?;
    }
    let play = |assets: PlaybackAssets, name: &str| -> Result<Vec<f32>> {
        let (source, control) = assets.cue_only().session(false);
        control.play(name)?;
        let pcm = source.decoder().take(640).collect();
        control.check()?;
        Ok(pcm)
    };
    let original = PlaybackAssets::load(root.path(), Diagnostics::new(true))?;
    let expected = play(original.clone(), "navigate")?;
    assert!(expected.iter().any(|&sample| sample != 0.));
    assert_eq!(play(original.clone(), "back")?, expected);
    let modification = tempfile::tempdir()?;
    fs::create_dir_all(modification.path().join("audio/programs"))?;
    let mut edited: serde_json::Value = serde_json::from_slice(&fs::read(root.path().join(path))?)?;
    edited["instructions"]
        .as_array_mut()
        .unwrap()
        .insert(1, serde_json::json!({"operation":"set_volume","factor":0}));
    fs::write(modification.path().join(path), serde_json::to_vec(&edited)?)?;
    let library = Library::open(
        root.path(),
        &[Mod {
            name: "shared program".into(),
            root: modification.path().into(),
        }],
    )?;
    let current = PlaybackAssets::load_in(&library, Diagnostics::new(true))?;
    for name in ["navigate", "back"] {
        assert!(
            play(current.clone(), name)?
                .iter()
                .all(|&sample| sample == 0.)
        );
        assert_eq!(play(original.clone(), name)?, expected);
    }
    Ok(())
}

use super::*;
use crate::{dls, mix, modulation, music_voice::Controls, pitch, resample};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (Fixture, serde_json::Value) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let root = Fixture(std::env::temp_dir().join(format!(
        "resonance-music-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&root.0).unwrap();
    let path = root.0.join("sample.wav");
    let mut wave = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 32000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for value in [10i16, 20, 30, 40, 50, 60] {
        wave.write_sample(value).unwrap();
    }
    wave.finalize().unwrap();
    let package = Package {
        version: VERSION,
        programs: BTreeMap::from([(1, vec![Command::StartSample { sample: 2 }, Command::End])]),
        samples: BTreeMap::from([(
            2,
            SampleAsset {
                path: "sample.wav".into(),
                sha256: format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                key: 60,
                rate: 32000,
                first_frames: 4,
                loop_start: 2,
                loop_length: 2,
            },
        )]),
        score: Score {
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 0,
            end_tick: 100,
            has_master_track: false,
            tempos: vec![],
            controls: [Controls::default(); 16],
            first_events: vec![],
            loop_events: vec![],
        },
        tables: Tables {
            mix: mix::Tables {
                volume: [1.; 129],
                alternate_volume: [1.; 129],
                pan: [1.; 4],
                volume_16_scale: 1.,
                controller_14_scale: 1.,
                pan_16_scale: 1.,
            },
            pitch: pitch::Tables {
                up: [1.; 128],
                down: [1.; 128],
                semitone: 1.05946,
            },
            dls: dls::Tables {
                attenuation: [0; 194],
                inverse: [0; 1024],
                sustain: [0.; 128],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        },
        reverbs: [[0., 0., 1., 0., 0.]; 2],
    };
    (root, serde_json::to_value(package).unwrap())
}

fn load(root: &Fixture, value: &serde_json::Value) -> Result<Loaded> {
    fs::write(
        root.0.join("music.json"),
        serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
    Package::load(&root.0, "music.json")
}

#[test]
fn banks_share_samples_only_when_tuning_and_loop_metadata_match() {
    let (root, value) = fixture();
    fs::write(
        root.0.join("bank.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let mut cache = SampleCache::default();
    let mut read = |path: &str, _: usize| Ok(fs::read(root.0.join(path))?);
    let a = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    let b = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &a.resources.samples[&2],
        &b.resources.samples[&2]
    ));
    let mut changed = value;
    changed["samples"]["2"]["key"] = 61.into();
    fs::write(
        root.0.join("bank.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let c = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    assert!(!std::sync::Arc::ptr_eq(
        &a.resources.samples[&2],
        &c.resources.samples[&2]
    ));
    assert_eq!(a.resources.samples[&2].pcm, c.resources.samples[&2].pcm);
}

#[test]
fn package_preserves_independent_loop_pcm_and_rejects_corruption() {
    let (root, value) = fixture();
    let loaded = load(&root, &value).unwrap();
    let sample = &loaded.resources.samples[&2];
    assert_eq!(sample.pcm, [10, 20, 30, 40]);
    assert_eq!(sample.loop_pcm, [50, 60]);
    let cases = [
        ("/samples/2/sha256", serde_json::json!("incorrect")),
        ("/samples/2/first_frames", serde_json::json!(5)),
        ("/samples/2/loop_start", serde_json::json!(4)),
        ("/samples/2/path", serde_json::json!("../sample.wav")),
        ("/programs/1/0/sample", serde_json::json!(3)),
        ("/tables/pitch/up", serde_json::json!([1.])),
        ("/tables/coefficients", serde_json::json!([0])),
        ("/score/controls/0/pan", serde_json::json!(128)),
        ("/score/end_tick", serde_json::json!(0)),
    ];
    for (pointer, replacement) in cases {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        assert!(load(&root, &invalid).is_err(), "accepted invalid {pointer}");
    }
    assert!(Package::load(&root.0, "../music.json").is_err());
}

#[test]
fn field_loops_keep_note_onsets_and_held_note_releases_on_time() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        reverb::Studio,
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::End,
        ],
    );
    // Exactly one score tick per millisecond. Each ten-tick note spans the
    // eight-tick loop, exercising both the new and the retiring clock.
    loaded.score.initial_bpm_1024 = 160_000;
    let event = |tick| Event {
        tick,
        channel: 0,
        kind: EventKind::Notes {
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 64,
                max_voices: 255,
            }],
            length: 10,
        },
    };
    // An explicitly written-out passage supplies an independent timing
    // expectation, without taking a loop or sharing its clock handoff.
    loaded.score.first_events = [1, 9, 17].map(event).into();
    let expected = sequence::render_preview(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        800,
    )
    .unwrap();
    assert!(expected.pcm.iter().any(|sample| *sample != 0));
    assert_eq!(
        expected
            .voice_lifetimes
            .iter()
            .map(|voice| voice.start_frame)
            .collect::<Vec<_>>(),
        [32, 288, 544]
    );
    loaded.score.end_tick = 8;
    loaded.score.first_events = vec![event(1)];
    loaded.score.loop_events = vec![event(1)];
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    let mut stream = Stream::new(Arc::new(loaded), true).unwrap();
    let mut actual = Vec::new();
    for _ in 0..5 {
        for buses in stream.block(LiveControls::default()).unwrap().unwrap() {
            actual.extend(
                studio
                    .process(buses)
                    .map(|sample| sample.clamp(-32768, 32767) as i16),
            );
        }
    }
    assert_eq!(actual, expected.pcm);
    stream.stop().unwrap();
}

#[test]
fn worker_preserves_millisecond_controls_from_the_offline_renderer() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        reverb::Studio,
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.tables.mix.volume = std::array::from_fn(|i| i as f32 / 128.);
    loaded.tables.mix.volume_16_scale = 1. / (127. * 65536.);
    loaded.tables.mix.controller_14_scale = 1. / 16383.;
    let sample = Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
    sample.pcm.fill(12000);
    sample.loop_pcm.fill(16000);
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::End,
        ],
    );
    loaded.score.first_events.push(Event {
        // Birth inside the block observes the current group value even
        // though existing voices normally update on five-ms boundaries.
        tick: 1,
        channel: 0,
        kind: EventKind::Notes {
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 64,
                max_voices: 255,
            }],
            length: 90,
        },
    });
    let volume = |frame: u32| (frame / 32 % 5) as f32 / 4.;
    let expected = sequence::render_preview_with_volume(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        1600,
        volume,
    )
    .unwrap();
    assert!(
        expected.pcm.iter().any(|sample| *sample != 0),
        "silent fixture cannot test gain changes"
    );
    let held = sequence::render_preview_with_volume(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        1600,
        |frame| volume(frame / 160 * 160),
    )
    .unwrap();
    assert_ne!(
        expected.pcm, held.pcm,
        "fixture must detect the previous block-wide gain hold"
    );
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    let mut stream = Stream::new(Arc::new(loaded), false).unwrap();
    let mut actual = Vec::new();
    for block in 0..10 {
        let input = std::array::from_fn(|i| LiveControls {
            volume: volume(block * 160 + i as u32 * 32),
            ..Default::default()
        });
        for buses in stream.block_envelope(input).unwrap().unwrap() {
            actual.extend(
                studio
                    .process(buses)
                    .map(|sample| sample.clamp(-32768, 32767) as i16),
            );
        }
    }
    assert_eq!(actual, expected.pcm);
}

#[test]
fn exclusive_group_ends_the_previous_voice_and_cue_release_finishes() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::ExclusiveGroup {
                group: 5,
                kill: true,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::StopSample,
            Command::End,
        ],
    );
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 127,
        pan: 64,
        priority: 64,
        max_voices: 255,
    };
    loaded.score.first_events = [0, 20]
        .map(|tick| Event {
            tick,
            channel: 0,
            kind: EventKind::Notes {
                voices: vec![note],
                length: 90,
            },
        })
        .to_vec();
    let preview = sequence::render_preview(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        2000,
    )
    .unwrap();
    assert_eq!(preview.voice_lifetimes.len(), 2);
    assert_eq!(
        preview.voice_lifetimes[0].end_frame,
        Some(preview.voice_lifetimes[1].start_frame)
    );
    assert!(preview.voice_lifetimes[1].end_frame.is_none());
    loaded.score.first_events.truncate(1);
    let loaded = Arc::new(loaded);
    let mut stream = Stream::new(loaded.clone(), false).unwrap();
    assert_eq!(
        stream
            .block(LiveControls::default())
            .unwrap()
            .unwrap()
            .len(),
        160 // Unclipped stereo bus frames, before shared effects and output conversion.
    );
    assert!(
        stream
            .block(LiveControls {
                release: true,
                ..Default::default()
            })
            .unwrap()
            .is_some()
    );
    let mut completed = false;
    for _ in 0..850 {
        if stream.block(LiveControls::default()).unwrap().is_none() {
            completed = true;
            break;
        }
    }
    assert!(completed, "released cue did not finish its effects tail");
    // A worker waiting for its first control request must also cancel cleanly.
    let mut paused = Stream::new(loaded, true).unwrap();
    paused.stop().unwrap();
    assert!(paused.block(LiveControls::default()).unwrap().is_none());
}

//! Exercise the actual movie systems and native mixer without an output device.
use super::*;
use crate::{Art, Clock, FieldAssets, Menu, Replay};
use resonance_game::TitleState;
use std::{path::PathBuf, thread};

#[test]
fn subtitles_follow_media_time_when_video_is_held() {
    let mut movie = Playback {
        active: true,
        presented_frame: Some(5),
        asset: Some(MovieAsset {
            audio_track: 0,
            version: 2,
            path: "movies/test.mkv".into(),
            sha256: "0".repeat(64),
            width: 640,
            height: 480,
            frames: 100,
            frame_micros: 33367,
            sample_rate: 32028,
            channels: 2,
            audio_frames: 106868,
        }),
        ..Default::default()
    };
    assert_eq!(movie.timeline_frame(None), Some(5)); // Static capture.
    assert_eq!(
        movie.timeline_frame(Some(Duration::from_micros(333670))),
        Some(10)
    );
    assert_eq!(
        movie.timeline_frame(Some(Duration::from_secs(20))),
        Some(99)
    );
    movie.active = false;
    assert_eq!(movie.timeline_frame(Some(Duration::ZERO)), None);
}

fn fixture() -> App {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        PathBuf::from,
    );
    let options = RunOptions {
        script_root: None,
        saves: Default::default(),
        assets: root.clone(),
        tick: None,
        presentation_start: None,
        capture: None,
        reveal: false,
        selected: 0,
        silent: true,
        paranoid: true,
        replay: None,
        movie_frame: None,
        boot_frame: None,
        skip_intro: false,
        record_playthrough: None,
        record_title_ticks: 1000,
    };
    let mut movie = Playback::load(&root, &options).expect("cook-all first");
    let asset = movie.asset.as_ref().unwrap();
    let mut images = Assets::<Image>::default();
    movie.texture = images.add(Image::new(
        Extent3d {
            width: asset.width,
            height: asset.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; asset.width as usize * asset.height as usize * 4],
        TextureFormat::Rgba8Unorm,
        default(),
    ));
    let manifest = serde_json::from_slice(&fs::read(root.join("title.json")).unwrap()).unwrap();
    let mut field = FieldAssets::default();
    field.ready = true;
    let audio = crate::audio::PlaybackAssets::load(&root).unwrap();
    let mut app = App::new();
    // MinimalPlugins and AssetPlugin have no window, renderer, or audio device.
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(images)
        .insert_resource(crate::diagnostics::Diagnostics(
            resonance_content::diagnostics::Diagnostics::new(true),
        ))
        .add_message::<AppExit>()
        .init_resource::<Assets<MovieAudio>>()
        .init_resource::<Assets<crate::audio::GameAudio>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<PendingInput>()
        .init_resource::<Clock>()
        .init_resource::<crate::boot::Playback>()
        .init_resource::<crate::loading::Resident>()
        .insert_resource(crate::timing::Ready(true))
        .init_resource::<crate::audio::MenuSounds>()
        .insert_resource(options)
        .insert_resource(movie)
        .insert_resource(Menu(TitleState::default()))
        .insert_resource(Replay(None))
        .insert_resource(field)
        .insert_resource(crate::PendingAudio(Some(audio)))
        .insert_resource(Art {
            manifest,
            images: Vec::new(),
        })
        .add_systems(
            Update,
            // One manual update represents a fixed title step followed by the
            // real Update ordering: movie completion, then title audio startup.
            (
                crate::timing::advance_clock,
                crate::advance,
                update,
                crate::start_audio,
            )
                .chain(),
        );
    app.world_mut().spawn((Camera::default(), MovieCamera));
    crate::audio::validate_startup(&app, true, true).unwrap();
    app
}

#[test]
#[ignore = "requires locally cooked field/movie assets; no window or audio device"]
fn movie_play_time_excludes_preparation_and_pause() {
    use bevy::ecs::system::RunSystemOnce;
    let mut app = fixture();
    let root = app.world().resource::<RunOptions>().assets.clone();
    app.insert_resource(crate::new_game::Session::load(&root).unwrap());
    let started = app
        .world()
        .resource::<crate::new_game::Session>()
        .field
        .events
        .tick();
    for (ready, resident, active, presenting, paused, expected) in [
        (false, true, true, true, false, 0),
        (true, false, true, true, false, 0),
        (true, true, true, false, false, 0),
        (true, true, true, true, false, 60),
        (true, true, true, true, true, 60),
        (true, true, false, false, false, 60),
    ] {
        app.world_mut().resource_mut::<crate::timing::Ready>().0 = ready;
        app.world()
            .resource::<crate::loading::Resident>()
            .active
            .store(resident, std::sync::atomic::Ordering::Release);
        let mut movie = app.world_mut().resource_mut::<Playback>();
        movie.active = active;
        movie.started = presenting.then(Instant::now);
        movie.paused = paused;
        for _ in 0..60 {
            app.world_mut()
                .run_system_once(crate::timing::advance_clock)
                .unwrap();
        }
        let field = &app.world().resource::<crate::new_game::Session>().field;
        assert_eq!(field.play_time.total(), expected);
        assert_eq!(field.events.tick(), started);
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF opening/title; runs a full movie without a device"]
fn completes_opening_and_advances_title_without_an_audio_device() {
    let mut app = fixture();
    let asset = app.world().resource::<Playback>().asset.clone().unwrap();
    let root = app.world().resource::<RunOptions>().assets.clone();
    // This explicitly diagnostic Dolphin run removes the original decoder
    // starvation gaps. Its source content is exact; it is not the normal-speed
    // movie timing acceptance fixture.
    let reference_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../local/oracle/intro-cpu2-complete-diagnostic-silent/user/Dump/Audio/GQSEAF_2026-09-07_13-40-07_dspdump.wav",
    );
    let reference_bytes = fs::read(reference_path).unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(&reference_bytes)),
        "4be2cb5856f5ec753cf016c69a606ed87875c54460027cdf8bb2a84a3fec230c"
    );
    let mut reference = hound::WavReader::new(std::io::Cursor::new(reference_bytes)).unwrap();
    reference.seek(664704).unwrap();
    let mut expected_audio = reference.samples::<i16>().take(3_879_328 * 2);
    let last =
        MovieDecoder::frame(&root.join(&asset.path), asset.clone(), asset.frames - 1).unwrap();
    // The independent final-frame decode above must not spend the player's
    // prebuffer deadline. The streaming worker has simply waited on its queue.
    app.world_mut().resource_mut::<Playback>().prebuffer_started = Some(Instant::now());
    let started = Instant::now();
    let (mixer, stream) = resonance_playback::Offline::new();
    let mut output = Some(stream);
    let mut samples = 0u64;
    let mut audio_updates = 0u64;
    let mut next_update = Instant::now();
    let mut high_water = Duration::ZERO;
    let mut presentation_updates = 0;
    while app.world().resource::<Playback>().active {
        assert!(
            started.elapsed() < Duration::from_secs(150),
            "movie stalled"
        );
        assert_eq!(
            app.world().resource::<Menu>().0.tick,
            0,
            "title advanced during movie"
        );
        presentation_updates += u32::from(app.world().resource::<Playback>().is_presenting());
        app.update();
        assert_eq!(
            app.world().resource::<Clock>().0.tick(),
            presentation_updates,
            "presentation clock did not follow active movie playback"
        );
        assert!(
            app.should_exit().is_none(),
            "movie system reported a playback failure"
        );
        if let Some(entity) = app.world().resource::<Playback>().audio_entity {
            crate::playthrough::attach::<MovieAudio>(app.world_mut(), &mixer).unwrap();
            let sink = app.world().get::<AudioSink>(entity).unwrap();
            let position = sink.position();
            if !sink.empty() {
                assert!(position >= high_water, "movie clock moved backwards");
                high_water = position;
            }
            audio_updates += 1;
            let end = audio_updates * u64::from(asset.sample_rate) * 2 / 60;
            for sample_index in samples..end {
                let actual = output.as_mut().unwrap().next().unwrap();
                if let Some(expected) = expected_audio.next() {
                    assert_eq!(
                        (actual * 32768.).round() as i16,
                        expected.unwrap(),
                        "movie source PCM at sample {sample_index}"
                    );
                } else {
                    assert_eq!(actual, 0., "movie source produced audio after its end");
                }
            }
            samples = end;
        }
        next_update += Duration::from_secs_f64(1. / 60.);
        thread::sleep(next_update.saturating_duration_since(Instant::now()));
    }
    let movie = app.world().resource::<Playback>();
    assert!(movie.ended, "movie exited before the decoder completed");
    assert!(
        movie.completed_naturally,
        "title music selected the skip fade"
    );
    assert!(movie.decoder.is_none() && movie.audio_entity.is_none());
    assert!(
        app.world()
            .resource::<crate::audio::MenuSounds>()
            .control
            .is_some(),
        "title audio was deferred past movie completion"
    );
    assert_eq!(movie.buffer.underruns(), 0);
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&movie.texture)
            .unwrap()
            .data
            .as_ref(),
        Some(&last.rgba)
    );
    assert_eq!(
        app.world().resource::<Menu>().0.tick,
        0,
        "title advanced in the movie's final update"
    );
    assert!(high_water > Duration::from_secs(120));
    assert!(
        expected_audio.next().is_none(),
        "movie dropped its final audio samples"
    );
    assert!(presentation_updates > 7200);
    assert!(
        app.world_mut()
            .query::<&AudioSink>()
            .iter(app.world())
            .next()
            .is_none()
    );
    assert!(
        app.world_mut()
            .query::<&Camera>()
            .iter(app.world())
            .all(|camera| !camera.is_active)
    );
    app.update();
    assert_eq!(app.world().resource::<Menu>().0.tick, 1);
    verify_title_source(&mut app, true);
    println!(
        "Device-free opening complete: {} frames, clock {high_water:?}, title tick 1",
        asset.frames
    );
}

#[test]
#[ignore = "requires locally cooked GQSEAF opening/title; never opens a device"]
fn skip_cancels_the_decoder_and_does_not_reveal_the_next_menu() {
    let mut app = fixture();
    app.world_mut().resource_mut::<Clock>().0 = resonance_game::clock::PresentationClock::new(2364);
    let mut pending = app.world_mut().resource_mut::<PendingInput>();
    pending.held.accept = true;
    pending.pressed.accept = true;
    pending.pressed.reveal = true;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.update();
    let movie = app.world().resource::<Playback>();
    assert!(!movie.active && movie.decoder.is_none());
    assert!(
        app.world()
            .resource::<crate::audio::MenuSounds>()
            .control
            .is_some(),
        "title audio was deferred past movie skip"
    );
    assert!(
        !movie.completed_naturally,
        "skip selected the full-intro fade"
    );
    let menu = &app.world().resource::<Menu>().0;
    assert_eq!(menu.tick, 0);
    assert_eq!(app.world().resource::<Clock>().0.tick(), 2364);
    assert!(
        !menu.revealed,
        "movie skip leaked into the title controller"
    );
    let pending = app.world().resource::<PendingInput>();
    assert!(pending.held.accept && !pending.pressed.accept);
    assert!(app.should_exit().is_none());
    app.update();
    assert_eq!(app.world().resource::<Menu>().0.tick, 1);
    assert!(!app.world().resource::<Menu>().0.revealed);
    verify_title_source(&mut app, false);
}

/// Inspect the source spawned by the actual handoff system. Decode directly;
/// neither this helper nor the fixture installs an audio-device plugin.
fn verify_title_source(app: &mut App, full_intro: bool) {
    let handle = app
        .world_mut()
        .query::<&AudioPlayer<crate::audio::GameAudio>>()
        .single(app.world())
        .unwrap()
        .0
        .clone();
    let source = app
        .world()
        .resource::<Assets<crate::audio::GameAudio>>()
        .get(&handle)
        .unwrap()
        .clone();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (path, offset) = if full_intro {
        (
            "local/oracle/intro-title-silent/user/Dump/Audio/GQSEAF_2026-09-07_07-28-34_dspdump.wav",
            4739200,
        )
    } else {
        (
            "local/oracle/title-music-loop-complete-silent/user/Dump/Audio/GQSEAF_2026-09-07_12-05-38_dspdump.wav",
            1383592,
        )
    };
    let mut reference = hound::WavReader::open(root.join(path)).unwrap();
    reference.seek(offset).unwrap();
    let mut output = source.decoder();
    for (index, expected) in reference.samples::<i16>().take(16000 * 2).enumerate() {
        let actual = (output.next().expect("handoff audio stopped") * 32768.).round() as i16;
        assert_eq!(
            actual,
            expected.unwrap(),
            "handoff sample {index}, full_intro={full_intro}"
        );
    }
    let control = app
        .world()
        .resource::<crate::audio::MenuSounds>()
        .control
        .as_ref()
        .unwrap();
    assert_eq!(control.rendered_frames(), 16000);
    output.stop().unwrap();
    assert!(
        control.play("navigate").is_err(),
        "handoff source did not close requests"
    );
}

#[test]
#[ignore = "requires locally cooked classroom/story movie; never opens an audio device"]
fn new_game_confirm_opens_script_movie_and_preserves_the_field_session() {
    use crate::new_game;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = fixture();
    app.add_plugins(bevy::log::LogPlugin::default());
    // An already completed startup movie leaves its reusable surface behind.
    app.world_mut().resource_mut::<Playback>().active = false;
    {
        let mut menu = app.world_mut().resource_mut::<Menu>();
        menu.0.revealed = true;
        menu.0.opacity = 255;
    }
    app.world_mut()
        .resource_mut::<PendingInput>()
        .pressed
        .accept = true;
    app.world_mut().run_system_once(crate::advance).unwrap();
    assert!(app.world().contains_resource::<new_game::Request>());
    let prepared = Instant::now();
    while !app.world().contains_resource::<new_game::Session>() {
        new_game::enter(app.world_mut());
        assert!(
            prepared.elapsed() < Duration::from_secs(30),
            "field preparation timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.world().resource::<new_game::Session>().assets.map_id, 5);
    for _ in 0..1000 {
        {
            let mut session = app.world_mut().resource_mut::<new_game::Session>();
            let choose = session
                .field
                .events
                .world
                .choices
                .get(&1)
                .is_some_and(|choice| {
                    choice.operation.is_pending()
                        && session
                            .field
                            .dialogue
                            .get(&1)
                            .is_some_and(|d| d.fully_revealed())
                });
            let selected = session
                .field
                .events
                .world
                .choices
                .get(&1)
                .map(|c| c.selected_line);
            session
                .field
                .step(resonance_game::field::FieldInput {
                    direction: if choose && selected == Some(0) {
                        [0., -1.]
                    } else {
                        [0.; 2]
                    },
                    interact: choose && selected == Some(1),
                    ..Default::default()
                })
                .unwrap();
        }
        app.world_mut()
            .run_system_once(new_game::transition)
            .unwrap();
        app.world_mut().run_system_once(new_game::advance).unwrap();
        if app.world().resource::<Playback>().active {
            break;
        }
    }
    let session = app.world().resource::<new_game::Session>();
    assert_eq!(session.assets.map_id, 340);
    assert!(!session.ready_for_field);
    assert!(session.field.events.world.blocked_by_movie());
    let completion = session
        .field
        .events
        .world
        .movie
        .as_ref()
        .unwrap()
        .operation
        .clone();
    let movie = app.world().resource::<Playback>();
    assert!(movie.active);
    assert_eq!(movie.resource, Some(1));
    assert!(app.world().resource::<crate::PendingAudio>().0.is_none());

    // The same Enter that confirmed New Game must not immediately skip it.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.world_mut().run_system_once(update).unwrap();
    assert!(app.world().resource::<Playback>().active);
    assert!(completion.is_pending());

    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(KeyCode::Enter);
        input.clear();
        input.press(KeyCode::Enter);
    }
    app.world_mut().run_system_once(update).unwrap();
    assert!(!app.world().resource::<Playback>().active);
    assert_eq!(
        completion.progress().outcome,
        Some(resonance_events::Outcome::Completed(None))
    );
    app.world_mut()
        .run_system_once(new_game::movie_handoff)
        .unwrap();
    assert!(app.world().resource::<new_game::Session>().ready_for_field);
    app.world_mut().run_system_once(crate::start_audio).unwrap();
    assert_eq!(
        app.world_mut()
            .query::<&AudioPlayer<crate::audio::GameAudio>>()
            .iter(app.world())
            .count(),
        0
    );
}

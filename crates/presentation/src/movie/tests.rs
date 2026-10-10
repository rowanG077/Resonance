//! Exercise the actual movie systems and native mixer without an output device.
use super::*;
use crate::{Art, Clock, FieldAssets, Menu};
use resonance_events::input::{Button, Buttons};
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

pub(crate) fn fixture() -> App {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        PathBuf::from,
    );
    let options = RunOptions {
        script_root: None,
        saves: Default::default(),
        assets: root.clone(),
        capture_at: None,
        capture: None,
        reveal: false,
        selected: 0,
        silent: true,
        paranoid: true,
        skip_intro: false,
        skip_battles: false,
        allow_incomplete_scripts: false,
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
    let audio = crate::audio::PlaybackAssets::load(
        &root,
        resonance_content::diagnostics::Diagnostics::new(true),
    )
    .unwrap();
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
                controls,
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
#[ignore = "requires locally cooked fields; no window or audio device"]
fn movie_play_time_excludes_preparation_and_pause() {
    use bevy::ecs::system::RunSystemOnce;
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        PathBuf::from,
    );
    let mut app = App::new();
    app.init_resource::<Clock>()
        .init_resource::<Playback>()
        .init_resource::<crate::boot::Playback>()
        .init_resource::<crate::loading::Resident>()
        // Field movies use field residency, independently of title readiness.
        .insert_resource(crate::timing::Ready(false))
        .insert_resource(crate::new_game::Session::load(&root).unwrap());
    let started = app
        .world()
        .resource::<crate::new_game::Session>()
        .field
        .events
        .tick();
    for (resident, active, presenting, paused, expected) in [
        (false, true, true, false, 0),
        (true, true, false, false, 0),
        (true, true, true, false, 60),
        (true, true, true, true, 60),
        (true, false, false, false, 60),
    ] {
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
        "skip was reported as natural movie completion"
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
    verify_title_source(&mut app);
}

/// Inspect the source spawned by the actual handoff system. Decode directly;
/// neither this helper nor the fixture installs an audio-device plugin.
fn verify_title_source(app: &mut App) {
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
    let root = &app.world().resource::<RunOptions>().assets;
    let prepared = crate::audio::PlaybackAssets::load(
        root,
        resonance_content::diagnostics::Diagnostics::new(true),
    )
    .unwrap();
    let (expected, _expected_control) = prepared.session();
    let mut expected = expected.decoder();
    let mut output = source.decoder();
    let mut audible = false;
    for index in 0..16000 * 2 {
        let actual = output.next().expect("handoff audio stopped");
        let reference = expected.next().expect("prepared title audio stopped");
        assert!(actual.is_finite());
        assert_eq!(actual, reference, "handoff sample {index}");
        audible |= actual != 0.;
    }
    assert!(audible, "handoff title source remained silent");
    let control = app
        .world()
        .resource::<crate::audio::MenuSounds>()
        .control
        .as_ref()
        .unwrap();
    assert_eq!(control.rendered_frames(), 16000);
    output.stop();
    assert!(
        control.play("navigate").is_err(),
        "handoff source did not close requests"
    );
}

fn script_movie_fixture() -> (App, resonance_events::Operation) {
    use crate::new_game;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = fixture();
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
                .map(|c| c.selection.lines().unwrap().selected_line);
            session
                .field
                .step(resonance_game::field::FieldInput {
                    direction: if choose && selected == Some(0) {
                        [0., -1.]
                    } else {
                        [0.; 2]
                    },
                    pressed_buttons: Buttons::default()
                        .with(Button::Accept, choose && selected == Some(1)),
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
    (app, completion)
}

#[test]
#[ignore = "requires locally cooked classroom/story movie; never opens an audio device"]
fn new_game_confirm_opens_script_movie_and_preserves_the_field_session() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, completion) = script_movie_fixture();
    // The same Enter that confirmed New Game must not immediately skip it.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.world_mut().run_system_once(controls).unwrap();
    app.world_mut().run_system_once(update).unwrap();
    assert!(app.world().resource::<Playback>().active);
    assert!(completion.is_pending());

    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(KeyCode::Enter);
        input.clear();
        input.press(KeyCode::Enter);
    }
    app.world_mut().run_system_once(controls).unwrap();
    app.world_mut().run_system_once(update).unwrap();
    assert!(!app.world().resource::<Playback>().active);
    assert_eq!(
        completion.progress().outcome,
        Some(resonance_events::Outcome::Completed(None))
    );
    app.world_mut()
        .run_system_once(crate::new_game::movie_handoff)
        .unwrap();
    assert!(
        app.world()
            .resource::<crate::new_game::Session>()
            .ready_for_field
    );
    app.world_mut().run_system_once(crate::start_audio).unwrap();
    assert_eq!(
        app.world_mut()
            .query::<&AudioPlayer<crate::audio::GameAudio>>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
#[ignore = "requires locally cooked classroom/story movie; never opens an audio device"]
fn asynchronous_movie_failure_completes_field_handoff_only_in_tolerant_mode() {
    use bevy::ecs::system::RunSystemOnce;
    for paranoid in [false, true] {
        let (mut app, completion) = script_movie_fixture();
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
        app.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
        {
            let mut movie = app.world_mut().resource_mut::<Playback>();
            assert!(movie.decoder.is_some(), "movie did not open successfully");
            // Startup already decoded valid video and audio. A malformed PCM
            // packet now fails on the real asynchronous MovieStream worker.
            let chunk = movie
                .pending_events
                .iter_mut()
                .find_map(|event| match event {
                    MovieEvent::Audio(chunk) => Some(chunk),
                    _ => None,
                })
                .expect("prepared movie has no decoded audio");
            chunk.samples.truncate(1);
        }
        let started = Instant::now();
        while app.world().resource::<Playback>().active {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "asynchronous movie failure was not observed"
            );
            app.world_mut().run_system_once(update).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        let errors = diagnostics.entries();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].scope, "movie playback");
        assert_eq!(
            errors[0].message,
            "movie feed failed: invalid decoded PCM block"
        );
        assert_eq!(
            app.world().resource::<Messages<AppExit>>().is_empty(),
            !paranoid
        );
        assert_eq!(
            completion.progress().outcome,
            Some(if paranoid {
                resonance_events::Outcome::Cancelled
            } else {
                resonance_events::Outcome::Completed(None)
            })
        );
        let movie = app.world().resource::<Playback>();
        assert!(!movie.completed_naturally);
        assert!(movie.decoder.is_none() && movie.stream.is_none() && movie.audio_entity.is_none());
        assert!(movie.pending_events.is_empty() && movie.frames.is_empty());
        assert!(movie.buffer.finished());
        assert!(
            app.world_mut()
                .query_filtered::<&Camera, With<MovieCamera>>()
                .iter(app.world())
                .all(|camera| !camera.is_active)
        );
        app.world_mut()
            .run_system_once(crate::new_game::movie_handoff)
            .unwrap();
        let session = app.world().resource::<crate::new_game::Session>();
        assert_eq!(session.ready_for_field, !paranoid);
        assert!(!session.field.events.world.blocked_by_movie());
    }
}

#[test]
fn short_movie_plays_to_completion_and_rejects_missing_streams() -> Result<()> {
    use bevy::ecs::system::RunSystemOnce;
    use resonance_media::encode::MovieWriter;

    struct Clip(PathBuf);
    impl Drop for Clip {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let root = std::env::temp_dir();
    let name = format!(
        "resonance-short-movie-{}-{}.mkv",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let clip = Clip(root.join(&name));
    let asset = MovieAsset {
        version: 2,
        path: name,
        sha256: "0".repeat(64),
        width: 16,
        height: 16,
        frames: 1,
        frame_micros: 40_000,
        sample_rate: resonance_playback::SOURCE_RATE,
        channels: 2,
        audio_frames: 1024,
        audio_track: 0,
    };
    let pcm: Vec<i16> = [4096, -8192].repeat(asset.audio_frames as usize);
    for (video, audio) in [(true, true), (true, false), (false, true), (false, false)] {
        let mut writer = MovieWriter::new(
            fs::File::create(&clip.0)?,
            asset.width,
            asset.height,
            asset.frame_micros,
            asset.sample_rate,
        )?;
        if video {
            writer.video(&[32; 16 * 16 * 3], Duration::ZERO)?;
        }
        if audio {
            writer.audio(&pcm)?;
        }
        writer.finish()?;
        let prepared = Prepared::load(&root, &asset, || false);
        if !(video && audio) {
            assert!(
                prepared.is_err(),
                "accepted movie with video={video}, audio={audio}"
            );
            continue;
        }
        let prepared = prepared?;
        assert!(matches!(prepared.events.back(), Some(MovieEvent::End)));
        assert!(
            Prepared::load(&root, &asset, || true)
                .err()
                .unwrap()
                .to_string()
                .contains("cancelled")
        );
        let direct = Prepared {
            decoder: MovieDecoder::open(&clip.0, asset.clone())?,
            events: VecDeque::new(),
        };
        for prepared in [prepared, direct] {
            let mut app = App::new();
            app.init_resource::<Assets<Image>>()
                .init_resource::<Assets<MovieAudio>>()
                .init_resource::<PendingInput>()
                .init_resource::<crate::boot::Playback>()
                .insert_resource(crate::timing::Ready(true))
                .insert_resource(crate::diagnostics::Diagnostics(
                    resonance_content::diagnostics::Diagnostics::new(true),
                ))
                .add_message::<AppExit>()
                .insert_resource(Playback {
                    active: true,
                    asset: Some(asset.clone()),
                    decoder: Some(prepared.decoder),
                    pending_events: prepared.events,
                    ..Default::default()
                });
            let texture = app
                .world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new(
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
            app.world_mut().resource_mut::<Playback>().texture = texture.clone();
            let started = Instant::now();
            while !app.world().resource::<Playback>().is_presenting() {
                app.world_mut().run_system_once(update).unwrap();
                ensure!(app.should_exit().is_none(), "short movie admission failed");
                ensure!(
                    started.elapsed() < Duration::from_secs(5),
                    "short movie did not start"
                );
                thread::sleep(Duration::from_millis(1));
            }
            let movie = app.world().resource::<Playback>();
            assert!(movie.ended);
            assert_eq!(movie.frames.len(), 1);
            assert_eq!(movie.buffer.buffered(), asset.audio_frames);
            let (mixer, mut output) = resonance_playback::Offline::new();
            crate::playthrough::attach::<MovieAudio>(app.world_mut(), &mixer)?;
            assert_eq!(
                output.by_ref().take(pcm.len()).collect::<Vec<_>>(),
                [0.125, -0.25].repeat(asset.audio_frames as usize)
            );
            assert_eq!(output.next(), Some(0.));
            app.world_mut().run_system_once(update).unwrap();
            let movie = app.world().resource::<Playback>();
            assert!(!movie.active && movie.completed_naturally);
            assert!(movie.audio_entity.is_none() && movie.stream.is_none());
            assert_eq!(movie.buffer.underruns(), 0);
            assert_eq!(
                app.world()
                    .resource::<Assets<Image>>()
                    .get(&texture)
                    .unwrap()
                    .data
                    .as_ref()
                    .unwrap(),
                &[32, 32, 32, 255].repeat(asset.width as usize * asset.height as usize),
            );
        }
    }
    Ok(())
}

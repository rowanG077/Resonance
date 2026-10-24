//! Exercise the actual movie systems and native mixer without an output device.
use super::*;
use crate::{Art, Clock, FieldAssets, Menu};
use resonance_events::input::{Button, Buttons};
use resonance_game::TitleState;
use std::{fs, path::PathBuf, thread};

#[test]
#[ignore = "requires cooked opening/classroom movies; no window or audio device"]
fn movie_soundtrack_respects_music_volume_and_stereo() {
    use bevy::ecs::system::RunSystemOnce;
    for (in_game, volume, stereo) in [(false, 127, true), (false, 64, false), (true, 0, true)] {
        let mut app = if in_game {
            script_movie_fixture().0
        } else {
            fixture()
        };
        let mut preferences = crate::config::Preferences::default();
        preferences.settings.volumes.music = volume;
        preferences.settings.stereo = stereo;
        if in_game {
            // The active party takes precedence over title preferences.
            let mut session = app.world_mut().resource_mut::<crate::new_game::Session>();
            let settings = &mut session
                .events_mut()
                .world
                .party
                .as_mut()
                .unwrap()
                .settings
                .preferences;
            settings.volumes.music = volume;
            settings.stereo = stereo;
            preferences.settings.volumes.music = 127;
        }
        app.insert_resource(preferences);
        let began = Instant::now();
        let entity = loop {
            app.world_mut().run_system_once(update).unwrap();
            if let Some(entity) = app.world().resource::<Playback>().audio_entity {
                break entity;
            }
            assert!(began.elapsed() < Duration::from_secs(30));
            thread::sleep(Duration::from_millis(1));
        };
        let handle = &app
            .world()
            .get::<AudioPlayer<MovieAudio>>(entity)
            .unwrap()
            .0;
        let mut audio = app
            .world()
            .resource::<Assets<MovieAudio>>()
            .get(handle)
            .unwrap()
            .clone();
        // Known PCM exposes attenuation even when the movie begins with silence.
        audio.buffer = Arc::new(AudioBuffer::default());
        audio.buffer.push(0, vec![0.75, -0.25, -0.5, 0.25]).unwrap();
        audio.buffer.finish();
        let samples: Vec<_> = audio.decoder().collect();
        let gain = f32::from(volume) / 127.;
        let expected = if stereo {
            [0.75, -0.25, -0.5, 0.25]
        } else {
            [0.25, 0.25, -0.125, -0.125]
        };
        assert_eq!(samples, expected.map(|sample| sample * gain));
        assert_eq!(
            audio.buffer.buffered(),
            0,
            "muting must still consume audio"
        );
    }
}

#[test]
fn subtitles_follow_media_time_when_video_is_held() {
    let mut movie = Playback {
        active: true,
        presented_frame: Some(5),
        final_frame: Some(99),
        asset: Some(Format {
            width: 640,
            height: 480,
            frame_duration: Duration::from_micros(33367),
            sample_rate: 32028,
            audio_frames: Some(106868),
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
        skip_intro: false,
        record_title_ticks: 1000,
        ..crate::test_support::run_options(&root)
    };
    let library = resonance_content::library::Library::open(&root, &[]).expect("cook-all first");
    let mut movie = Playback::load(&library, &options).expect("cook-all first");
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
    let mut materials = Assets::<TitleText>::default();
    let material = materials.add(TitleText {
        source: movie.texture.clone(),
        opacity_pulse: Vec4::X,
    });
    let library = resonance_content::library::Library::open(&root, &[]).unwrap();
    let (_, manifest, _) =
        resonance_content::startup::capture(&mut library.snapshot(), false, &|| false).unwrap();
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
        .insert_resource(materials)
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
    app.world_mut()
        .spawn((Transform::default(), MeshMaterial2d(material), MovieSurface));
    crate::audio::validate_startup(&app, true, true).unwrap();
    app
}

pub(crate) fn verify_surface(app: &mut App) {
    let movie = app.world().resource::<Playback>();
    let asset = movie.asset.as_ref().unwrap();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&movie.texture)
        .unwrap();
    let size = Vec3::new(asset.width as f32, asset.height as f32, 1.);
    let texture = movie.texture.clone();
    assert_eq!(image.size(), UVec2::new(asset.width, asset.height));
    assert_eq!(
        image.data.as_ref().unwrap().len(),
        asset.width as usize * asset.height as usize * 4
    );
    let (surface, material) = app
        .world_mut()
        .query_filtered::<(&Transform, &MeshMaterial2d<TitleText>), With<MovieSurface>>()
        .single(app.world())
        .unwrap();
    assert_eq!(surface.scale, size);
    assert_eq!(
        app.world()
            .resource::<Assets<TitleText>>()
            .get(&material.0)
            .unwrap()
            .source,
        texture,
        "movie surface must bind the image receiving decoded frames"
    );
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
        .field()
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
        let field = &app.world().resource::<crate::new_game::Session>().field();
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
    let (expected, _expected_control) =
        prepared.session(!app.world().resource::<RunOptions>().skip_intro);
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
    assert_eq!(app.world().resource::<new_game::Session>().map_id(), 5);
    for _ in 0..1000 {
        {
            let mut session = app.world_mut().resource_mut::<new_game::Session>();
            let choose = session
                .field()
                .events
                .world
                .choices
                .get(&1)
                .is_some_and(|choice| {
                    choice.operation.is_pending()
                        && session
                            .field()
                            .dialogue
                            .get(&1)
                            .is_some_and(|d| d.fully_revealed())
                });
            let selected = session
                .field()
                .events
                .world
                .choices
                .get(&1)
                .map(|c| c.selection.lines().unwrap().selected_line);
            session
                .field_mut()
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
        let began = Instant::now();
        while app
            .world()
            .contains_resource::<crate::loading::FieldPending>()
        {
            assert!(began.elapsed().as_secs() < 30, "field preparation stalled");
            app.world_mut()
                .run_system_once(new_game::transition)
                .unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        app.world_mut().run_system_once(new_game::advance).unwrap();
        if app.world().resource::<Playback>().active {
            break;
        }
    }
    let session = app.world().resource::<new_game::Session>();
    assert_eq!(session.map_id(), 340);
    assert!(!session.ready_for_field);
    assert!(session.field().events.world.blocked_by_movie());
    let completion = session
        .field()
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
fn named_opening_prepares_short_unknown_lengths_and_preserves_cancellation() -> Result<()> {
    use resonance_content::library::Library;
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("movies/short"))?;
    fs::write(
        root.path().join("content.json"),
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    fs::write(
        root.path().join("movies/mappings.json"),
        br#"{"0":"short"}"#,
    )?;
    fs::write(
        root.path().join("movies/short/movie.json"),
        br#"{"version":1,"path":"movies/short/video.mkv"}"#,
    )?;
    let mut writer = resonance_media::encode::MovieWriter::new(
        fs::File::create(root.path().join("movies/short/video.mkv"))?,
        16,
        16,
        40000,
        8000,
    )?;
    writer.video(&vec![19; 16 * 16 * 3], Duration::ZERO)?;
    writer.audio(&vec![3700; 640])?;
    writer.finish()?;
    let library = Library::open(root.path(), &[])?;
    assert!(Prepared::capture(&mut library.snapshot(), 0, || true).is_err());
    let calls = std::cell::Cell::new(0);
    let error = Prepared::capture(&mut library.snapshot(), 0, || {
        calls.set(calls.get() + 1);
        calls.get() > 1
    })
    .err()
    .context("cancellation after stream admission was ignored")?;
    assert!(error.to_string().contains("cancelled"));
    let prepared = Prepared::capture(&mut library.snapshot(), 0, || false)?;
    assert_eq!(prepared.decoder.format().audio_frames, None);
    assert_eq!(
        prepared
            .events
            .iter()
            .filter(|event| matches!(event, MovieEvent::Video(_)))
            .count(),
        1
    );
    let samples: usize = prepared
        .events
        .iter()
        .filter_map(|event| match event {
            MovieEvent::Audio(chunk) => Some(chunk.samples.len()),
            _ => None,
        })
        .sum();
    assert_eq!(samples, 640);
    let options = RunOptions {
        skip_intro: false,
        ..crate::test_support::run_options(root.path())
    };
    let movie = Playback::load(&library, &options)?;
    assert!(movie.active);
    assert_eq!(movie.asset.unwrap().sample_rate, 8000);
    assert_eq!(movie.video_timestamps()?, [Duration::ZERO]);
    // The public title loader uses only named entry points and current stream
    // facts. No movies/0.json, recorded hash or frame count exists in this fixture.
    Ok(())
}

//! Exercise the ending's real script request and GPU movie surface without audio output.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
#[ignore = "requires full cooked assets, RESONANCE_TEST_SAVE and a GPU; silent"]
fn ending_movie_draws_its_picture_and_returns_to_the_following_field() -> Result<()> {
    let root = asset_root();
    let save = std::env::var_os("RESONANCE_TEST_SAVE").context("set RESONANCE_TEST_SAVE")?;
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ARTIFACTS").context("set RESONANCE_TEST_ARTIFACTS")?,
    );
    std::fs::create_dir_all(&output)?;
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(save)?)?;
    let progress: resonance_events::SavedProgress =
        serde_json::from_value(saved["state"]["field"]["progress"].clone())?;
    let mut session = Session::load(&root)?;
    let package = Arc::new(FieldPackage::prepare(
        &root,
        136,
        &mut Default::default(),
        || false,
    )?);
    // Read-only progress fixture: admission of a complete saved checkpoint is a
    // separate acceptance check. Complete battles through the existing fixture API.
    let entry = FieldEntry {
        persistent: progress.into_state(&package.data)?,
        available_fields: session.available_fields.clone(),
        ..Default::default()
    };
    session.activate(package.enter(entry)?, package, false);
    session.field_mut().events.world.voice_durations = Default::default();
    let mut triggered = false;
    for _ in 0..30000 {
        let field = session.field_mut();
        if field.player_has_control() && !triggered {
            ensure!(
                field.events.trigger(100, true)?,
                "ending scene did not start"
            );
            triggered = true;
        }
        field.step(FieldInput {
            skip_dialogue: true,
            pressed_buttons: [Button::Accept].into(),
            ..Default::default()
        })?;
        if field.events.battle_pending() {
            field
                .events
                .world
                .skip_battle_as_victory()
                .map_err(anyhow::Error::msg)?;
        }
        if field
            .events
            .world
            .movie
            .as_ref()
            .is_some_and(|movie| movie.operation.is_pending())
        {
            break;
        }
    }
    ensure!(triggered, "ending scene did not run");
    ensure!(
        session
            .events()
            .world
            .movie
            .as_ref()
            .is_some_and(|movie| movie.resource == 7 && movie.operation.is_pending()),
        "ending movie request missing"
    );
    session.events_mut().world.audio_commands.clear();
    let mut movie_request = session.events_mut().world.movie.take();
    let mut options = crate::test_support::run_options(&root);
    assert!(options.silent);
    options.capture = Some(output.join("unused.png"));
    let (mut app, _) = crate::build_app_with_display(options, crate::Resolution::default())?;
    let mut candidate = Some(session);
    app.add_systems(Update, move |world: &mut World| {
        if !world.resource::<crate::timing::Ready>().0 {
            return;
        }
        if let Some(session) = candidate.take() {
            crate::new_game::activate(world, session);
        } else if crate::new_game::scene_ready(world) && movie_request.is_some() {
            world.resource_mut::<Session>().events_mut().world.movie = movie_request.take();
        }
    });
    let captured = Arc::new(AtomicBool::new(false));
    let captured_result = captured.clone();
    let mut requested = false;
    let movie_output = output.clone();
    app.add_systems(Update, move |world: &mut World| {
        let playback = world.resource::<movie::Playback>();
        if requested || !playback.presented_frame.is_some_and(|frame| frame >= 600) { return; }
        requested = true;
        assert!(playback.active);
        let frame = playback.presented_frame.unwrap();
        let texture = world.query_filtered::<&MeshMaterial2d<crate::materials::TitleText>,
            With<movie::MovieSurface>>().iter(world).map(|handle|
                world.resource::<Assets<crate::materials::TitleText>>().get(&handle.0).unwrap().source.clone())
            .next().unwrap();
        let decoded = world.resource::<Assets<Image>>().get(&texture).unwrap();
        let size = decoded.texture_descriptor.size;
        let decoded_nonblack = decoded.data.as_ref().unwrap().chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|&value| value > 32)).count();
        assert!(decoded_nonblack > 1000, "ending decoder has no picture");
        let output = movie_output.clone();
        let captured = captured_result.clone();
        let framebuffer = world.resource::<crate::Framebuffer>().0.clone();
        world.spawn(bevy::render::view::screenshot::Screenshot(framebuffer)).observe(
            move |event: On<bevy::render::view::screenshot::ScreenshotCaptured>| {
                let rendered_nonblack = event.image.data.as_ref().unwrap().chunks_exact(4)
                    .filter(|pixel| pixel[..3].iter().any(|&value| value > 32)).count();
                crate::screenshot::write(&event.image, &output.join("movie-playing.png"), None).unwrap();
                let evidence = serde_json::json!({"frame":frame,"width":size.width,"height":size.height,
                    "decoded_nonblack":decoded_nonblack,"rendered_nonblack":rendered_nonblack,"silent":true});
                std::fs::write(output.join("movie-picture.json"), serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
                assert!(rendered_nonblack > 1000, "ending movie framebuffer stayed black");
                captured.store(true, Ordering::Release);
            });
    });
    let replay = serde_json::from_value(serde_json::json!({"version":2,"steps":[
        {"do":"wait","until":{"kind":"field","map_id":152},"max_updates":6000},
        {"do":"hold","keys":[],"updates":120},
        {"do":"capture","name":"after-movie"}
    ]}))?;
    crate::saves::record_app(app, &output, &replay, false)?;
    ensure!(
        captured.load(Ordering::Acquire),
        "ending movie was never captured"
    );
    Ok(())
}

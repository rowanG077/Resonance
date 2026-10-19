//! Return failed sessions to the prepared title scene.
use super::{audio, loading, new_game, saves};
use anyhow::Result;
use bevy::prelude::*;
use std::sync::atomic::Ordering;

#[derive(Resource)]
pub(super) struct Returning(Option<audio::PlaybackAssets>);
/// Reuse the retained title resources when recovering a session.
pub(super) fn install(app: &mut App) -> Result<()> {
    app.add_systems(
        Update,
        finish_return
            .after(super::timing::prepare)
            .before(super::loading::black_hold),
    );
    Ok(())
}

fn discard_field(world: &mut World) {
    if let Some(mut session) = world.remove_resource::<new_game::Session>() {
        session.events_mut().cancel();
    }
    super::field_view::retire_live(world);
}
fn retire(world: &mut World) {
    saves::title::retire(world);
    // Title/load replacement retires the entire mixer, with no field playback
    // left to resume and no consumer available for a queued End request.
    super::field_audio::retire(world);
}
fn restore_cameras(world: &mut World) {
    for mut camera in world
        .query_filtered::<&mut Camera, With<super::FieldCamera>>()
        .iter_mut(world)
    {
        camera.is_active = true;
    }
    for mut camera in world
        .query_filtered::<&mut Camera, With<super::FieldOverlayCamera>>()
        .iter_mut(world)
    {
        camera.is_active = true;
        camera.clear_color = ClearColorConfig::None;
    }
}
pub(super) fn return_to_title(world: &mut World) -> Result<()> {
    // Build the new event lifetime before cancelling the failed field.
    let events = world
        .resource::<super::session_screen::Title>()
        .events
        .as_ref()
        .map(resonance_events::EventRuntime::fresh)
        .transpose();
    let events = super::diagnostics::policy(world)
        .attempt("title return script", events)?
        .flatten();
    let audio = world
        .resource::<super::session_screen::Title>()
        .audio
        .clone();
    discard_field(world);
    retire(world);
    let resident = world.resource::<loading::Resident>();
    *resident.files.write().unwrap() = None;
    resident.active.store(false, Ordering::Release);
    if let Some(mut old) = world.remove_resource::<super::Events>() {
        old.0.cancel();
    }
    if let Some(events) = events {
        world.insert_resource(super::Events(events));
    }
    let old = &world.resource::<super::Menu>().0;
    world.insert_resource(super::Menu(resonance_game::TitleState {
        selected: 1,
        opacity: old.opacity,
        sound_test: old.sound_test,
        ..Default::default()
    }));
    world.insert_resource(super::TitleActive);
    world.resource_mut::<super::timing::Ready>().0 = false;
    let projection =
        super::title_field::projection(world.resource::<super::display::Display>().0.aspect());
    for (mut camera, mut fog) in world
        .query_filtered::<(&mut Projection, &mut DistanceFog), With<super::FieldCamera>>()
        .iter_mut(world)
    {
        *camera = projection.clone();
        *fog = DistanceFog::default();
    }
    for mut visible in world
        .query_filtered::<&mut Visibility, Or<(
            With<super::TitleQuad>,
            With<super::glow::GlowMesh>,
            With<super::scene::PartRoot>,
        )>>()
        .iter_mut(world)
    {
        *visible = Visibility::Inherited;
    }
    restore_cameras(world);
    super::materials::TitleOutput::update(&mut world.resource_mut(), |b| {
        *b = Vec4::new(1., 0., 0., 0.)
    });
    clear_title_input(world);
    world.insert_resource(Returning(audio));
    Ok(())
}
fn finish_return(world: &mut World) {
    if !world.resource::<super::timing::Ready>().0 {
        return;
    }
    let Some(Returning(audio)) = world.remove_resource::<Returning>() else {
        return;
    };
    if let Some(audio) = audio.filter(|a| !a.is_empty()) {
        let (source, control) = audio.session(false);
        world.resource_mut::<audio::MenuSounds>().control = Some(control);
        let source = world.resource_mut::<Assets<audio::GameAudio>>().add(source);
        world.spawn(super::audio_output::Player(source));
    }
}

fn clear_title_input(world: &mut World) {
    let held = world.resource::<super::PendingInput>().held;
    world.insert_resource(super::PendingInput {
        held,
        ..Default::default()
    });
    world
        .resource_mut::<super::field_view::Controls>()
        .clear_actions();
}

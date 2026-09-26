//! Fatal defeat owns the retained caller until load or the original title route.
use super::{audio, battle_audio, field_ui::GameOverArt, loading, new_game, saves};
use anyhow::{Result, ensure};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use resonance_game::game_over::{Cue, Destination, Screen};
use std::sync::atomic::Ordering;

#[derive(Resource)]
pub(super) struct Active {
    screen: Screen,
    art: GameOverArt,
    loading: bool,
}
impl Active {
    pub(super) fn loading(&self) -> bool {
        self.loading
    }

    pub(super) fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
            "selected": self.screen.selected,
            "alpha": self.screen.alpha,
            "closing": self.screen.closing(),
            "loading": self.loading,
            "destination": self.screen.destination().map(|destination| match destination {
                Destination::Load => "load",
                Destination::Title => "title",
            }),
        })
    }
}
#[derive(Resource)]
pub(super) struct Returning(Option<audio::PlaybackAssets>);
#[derive(Resource)]
struct Title {
    events: Option<resonance_game::title_events::Prepared>,
    audio: Option<audio::PlaybackAssets>,
}

/// Keep immutable title resources available for every later fatal-defeat route.
pub(super) fn install(app: &mut App) -> Result<()> {
    use sha2::{Digest, Sha256};
    let world = app.world_mut();
    let root = world.resource::<super::RunOptions>().assets.clone();
    let scene = world.resource::<super::Art>().manifest.scene.clone();
    let events = scene
        .as_ref()
        .map(|scene| -> Result<_> {
            let bytes = std::fs::read(root.join(&scene.script.path))?;
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == scene.script.sha256,
                "title-return script digest mismatch"
            );
            let mut clips = world.resource_mut::<super::sparse_animation::Prepared>();
            resonance_game::title_events::Prepared::new(&bytes, scene, |path| {
                clips.load(&root, path)
            })
        })
        .transpose();
    let events = super::diagnostics::policy(world)
        .attempt("title return preparation", events)?
        .flatten();
    let audio = world.resource::<super::PendingAudio>().0.clone();
    world.insert_resource(Title { events, audio });
    app.add_systems(FixedUpdate, advance.after(super::timing::advance_clock))
        .add_systems(Update, (transition, render).chain().after(super::layout))
        .add_systems(
            Update,
            finish_return
                .after(super::timing::prepare)
                .before(super::loading::black_hold),
        );
    Ok(())
}

/// Called only with battle-warmed surfaces, after its cameras have retired.
pub(super) fn enter(world: &mut World, art: GameOverArt) -> Result<()> {
    ensure!(
        !world.contains_resource::<Active>(),
        "game-over screen already active"
    );
    ensure!(
        art.ready(world.resource::<Assets<Image>>()),
        "game-over artwork is not ready"
    );
    ensure!(
        world.contains_resource::<new_game::Session>(),
        "fatal defeat has no retained caller"
    );
    for entity in art.entities() {
        world.entity_mut(entity).insert(RenderLayers::default());
    }
    for mut camera in world
        .query_filtered::<&mut Camera, With<super::FieldCamera>>()
        .iter_mut(world)
    {
        camera.is_active = false;
    }
    for mut camera in world
        .query_filtered::<&mut Camera, With<super::FieldOverlayCamera>>()
        .iter_mut(world)
    {
        camera.is_active = true;
        camera.clear_color = ClearColorConfig::Custom(Color::BLACK);
    }
    super::materials::TitleOutput::update(&mut world.resource_mut(), |b| {
        *b = Vec4::new(1., 0., 0., 0.)
    });
    world
        .resource_mut::<super::field_view::Controls>()
        .clear_actions();
    clear_title_input(world);
    world.insert_resource(Active {
        screen: Screen::default(),
        art,
        loading: false,
    });
    Ok(())
}

fn advance(world: &mut World) {
    if world.resource::<super::PresentationPause>().0 {
        return;
    }
    let Some(mut active) = world.remove_resource::<Active>() else {
        return;
    };
    if !active.loading {
        let input = world
            .resource_mut::<super::field_view::Controls>()
            .consume();
        let result = active
            .screen
            .step(input, world.resource::<super::Clock>().0.tick());
        if let Some(cue) = result.cue
            && let Some(audio) = world.get_resource::<battle_audio::Playback>()
        {
            let result = (|| -> Result<()> {
                if cue == Cue::Confirm {
                    audio.music(None, 0)?;
                }
                audio.system_cue(if cue == Cue::Navigate { 1 } else { 2 })
            })();
            if let Err(error) = result {
                error!("Game-over sound failed: {error:#}");
            }
        }
        match result.destination {
            Some(Destination::Load) => {
                // Never complete the suspended battle operation with a success code.
                discard_field(world);
                active.art.hide(&mut world.commands());
                if let Some(menu) = active.art.take_menu() {
                    world.insert_resource(menu);
                }
                world.insert_resource(saves::title::LoadMenu::new());
                active.loading = true;
                clear_title_input(world);
            }
            Some(Destination::Title) => {
                world.insert_resource(active);
                if let Err(error) = return_to_title(world) {
                    fail(world, error);
                }
                return;
            }
            None => {}
        }
    }
    world.insert_resource(active);
}
fn transition(world: &mut World) {
    if world
        .get_resource::<Active>()
        .is_some_and(|active| active.loading)
        && !world.contains_resource::<saves::title::LoadMenu>()
        && let Err(error) = return_to_title(world)
    {
        fail(world, error);
    }
}
fn render(world: &mut World) {
    let Some(mut active) = world.remove_resource::<Active>() else {
        return;
    };
    if !active.loading {
        let result = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            active
                .art
                .render(&active.screen, &mut world.commands(), &mut meshes)
        });
        if let Err(error) = result {
            fail(world, error);
        }
    }
    world.insert_resource(active);
}

/// The save loader calls this only after its complete replacement is validated.
pub(super) fn loaded(world: &mut World) {
    if !world.contains_resource::<Active>() {
        return;
    }
    retire(world);
    restore_cameras(world);
    world
        .resource::<loading::Resident>()
        .battle
        .store(false, Ordering::Release);
}
fn discard_field(world: &mut World) {
    if let Some(mut session) = world.remove_resource::<new_game::Session>() {
        session.field.events.cancel();
    }
    super::field_view::retire_live(world);
}
fn retire(world: &mut World) {
    if let Some(active) = world.remove_resource::<Active>() {
        active.art.despawn(world);
    }
    saves::title::retire(world);
    if let Some(playback) = world.remove_resource::<battle_audio::Playback>()
        && let Some(mut control) = world.get_resource_mut::<super::field_audio::Control>()
        && let Err(error) = playback.finish(&mut control, false)
    {
        error!("Game-over audio retirement failed: {error:#}");
    }
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
        .resource::<Title>()
        .events
        .as_ref()
        .map(|events| events.enter())
        .transpose();
    let events = super::diagnostics::policy(world)
        .attempt("title return script", events)?
        .flatten();
    let audio = world.resource::<Title>().audio.clone();
    let scene = world.resource::<super::Art>().manifest.scene.clone();
    discard_field(world);
    retire(world);
    let resident = world.resource::<loading::Resident>();
    *resident.files.write().unwrap() = None;
    resident.active.store(false, Ordering::Release);
    resident.battle.store(false, Ordering::Release);
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
    world.insert_resource(super::scene::FieldAssets::default());
    world.resource_mut::<super::timing::Ready>().0 = false;
    let server = world.resource::<AssetServer>().clone();
    if let Some(scene) = scene {
        let aspect = world.resource::<super::display::Display>().0.aspect();
        for mut projection in world
            .query_filtered::<&mut Projection, With<super::FieldCamera>>()
            .iter_mut(world)
        {
            *projection =
                Projection::custom(super::camera::TitleProjection(PerspectiveProjection {
                    fov: scene.fov_degrees.to_radians(),
                    aspect_ratio: aspect,
                    near: 100.,
                    far: 40000.,
                    ..Default::default()
                }));
        }
        world.resource_scope(|world, mut field: Mut<super::scene::FieldAssets>| {
            field.load(&scene, &server, &mut world.commands())
        });
    }
    for mut visible in world.query_filtered::<&mut Visibility, Or<(With<super::TitleQuad>, With<super::glow::GlowMesh>)>>().iter_mut(world) { *visible = Visibility::Inherited; }
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
    if let Some(audio) = audio.filter(|a| !a.is_empty())
        && world.resource::<super::RunOptions>().capture.is_none()
    {
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
fn fail(world: &mut World, error: anyhow::Error) {
    let diagnostics = super::diagnostics::policy(world);
    if diagnostics.report("game-over transition", error).is_err() {
        world.write_message(AppExit::error());
    } else if let Err(error) = return_to_title(world) {
        let _ = diagnostics.report("game-over recovery", error);
    }
}

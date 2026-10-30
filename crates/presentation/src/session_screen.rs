//! Terminal session screens use captured artwork and the shared save browser.
use super::*;
use crate::materials::TitleOutput;
use crate::{Events, Menu, PendingAudio, PendingInput, audio, new_game, saves};
use resonance_events::session_screen::Target;

#[derive(Resource)]
pub(crate) struct Title {
    pub events: Option<resonance_events::EventRuntime>,
    pub audio: Option<audio::PlaybackAssets>,
    pub files: Option<std::sync::Arc<resonance_content::prepared::Files>>,
}

#[derive(Resource)]
pub(crate) struct GameOver {
    selected: usize,
    font: BitmapFont,
    art: resonance_content::game_over::Art,
    mesh: Handle<Mesh>,
    entities: [Entity; 2],
}

pub(crate) fn install(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        advance
            .run_if(crate::dungeons::running)
            .before(crate::advance)
            .before(crate::field_view::advance_live),
    );
}

pub(crate) fn update(world: &mut World) {
    super::credits::retire_cancelled(world);
    let Some(target) = world
        .get_resource::<new_game::Session>()
        .and_then(|session| session.events().world.screen_request.as_ref())
        .map(|request| request.target)
    else {
        return;
    };
    if target != Target::Title && !new_game::scene_ready(world) {
        return;
    }
    let result = match target {
        Target::Title => return_to_title(world, false),
        Target::GameOver => show_game_over(world),
        Target::Credits => super::credits::start(world),
    };
    if let Err(error) = result {
        error!("Session screen failed: {error:#}");
        world.write_message(AppExit::error());
    }
}

fn reset_input(world: &mut World) {
    let held = world.resource::<PendingInput>().held;
    world.insert_resource(PendingInput { held, ..default() });
    world
        .resource_mut::<crate::field_view::Controls>()
        .consume();
}

fn retire_session(world: &mut World) {
    if let Some(mut session) = world.remove_resource::<new_game::Session>() {
        session.events_mut().cancel();
    }
    world.remove_resource::<crate::loading::Pending>();
    world.remove_resource::<crate::loading::FieldPending>();
    world.remove_resource::<crate::loading::WorldPending>();
    world.remove_resource::<new_game::Request>();
    world.remove_resource::<new_game::TransitionFailure>();
    saves::title::retire(world);
    crate::field_audio::retire(world);
    TitleOutput::update(
        &mut world.resource_mut::<Assets<TitleOutput>>(),
        |brightness| *brightness = Vec4::X,
    );
    let resident = world.resource::<crate::loading::Resident>();
    resident
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    *resident.files.write().unwrap() = None;
    if let Some(mut controls) = world.get_resource_mut::<crate::testing::Controls>() {
        *controls = default();
        let mut time = world.resource_mut::<Time<Virtual>>();
        time.unpause();
        time.set_relative_speed(1.);
    }
    reset_input(world);
}

pub(crate) fn retire(world: &mut World) {
    super::credits::retire(world);
    if let Some(game_over) = world.remove_resource::<GameOver>() {
        for entity in game_over.entities {
            world.despawn(entity);
        }
    }
}

fn return_to_title(world: &mut World, load: bool) -> Result<()> {
    let title = world.resource::<Title>();
    let events = title
        .events
        .as_ref()
        .map(resonance_events::EventRuntime::fresh)
        .transpose()?;
    let audio = title.audio.clone();
    let files = title.files.clone();
    retire_session(world);
    *world
        .resource::<crate::loading::Resident>()
        .files
        .write()
        .unwrap() = files;
    retire(world);
    world.insert_resource(Menu(resonance_game::TitleState {
        revealed: true,
        ..default()
    }));
    world.insert_resource(PendingAudio(audio));
    if let Some(events) = events {
        world.insert_resource(Events(events));
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, Or<(With<crate::TitleQuad>, With<crate::scene::PartRoot>)>>()
        .iter(world)
        .collect();
    for entity in entities {
        world.entity_mut(entity).insert(Visibility::Inherited);
    }
    if world.contains_resource::<crate::title_field::Scene>() {
        let projection =
            crate::title_field::projection(world.resource::<crate::display::Display>().0.aspect());
        for (mut camera, mut fog) in world
            .query_filtered::<(&mut Projection, &mut DistanceFog), With<crate::FieldCamera>>()
            .iter_mut(world)
        {
            *camera = projection.clone();
            *fog = DistanceFog::default();
        }
    }
    if load {
        world.insert_resource(saves::title::TitleMenu::load());
    }
    Ok(())
}

fn game_over_text(
    font: &BitmapFont,
    art: &resonance_content::game_over::Art,
    selected: usize,
) -> Result<Mesh> {
    let mut batch = Batch::default();
    for (i, text) in art.choices.iter().enumerate() {
        skit::centered(
            &mut batch,
            font,
            text,
            240. + i as f32 * 36.,
            24.,
            if i == selected { 1. } else { 0.35 },
            false,
        )?;
    }
    skit::centered(&mut batch, font, &art.caption, 416., 20., 1., false)?;
    Ok(batch.mesh([font.width, font.height]))
}

fn show_game_over(world: &mut World) -> Result<()> {
    let files = world
        .resource::<crate::loading::Resident>()
        .files
        .read()
        .unwrap()
        .as_ref()
        .context("game-over needs prepared scene resources")?
        .as_ref()
        .clone();
    let data = resonance_game::game_over::Assets::from_prepared(files)?;
    let font = data.font;
    let art = data.art;
    let image = world
        .resource::<AssetServer>()
        .get_handle::<Image>(&font.texture)
        .context("scene font is not resident")?;
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(game_over_text(&font, &art, 0)?);
    let material = world.resource_mut::<Assets<Surface>>().add(Surface {
        source: image.clone(),
        sampling: image.clone(),
        frame_mask: image.clone(),
        color_mask: image,
        coverage: Coverage::default(),
        sample_coverage: false,
        opaque: false,
        additive: false,
        texture_effect: Default::default(),
    });
    let background = if data.files.contains_key(&art.background.path) {
        world
            .resource::<AssetServer>()
            .get_handle::<Image>(&art.background.path)
            .context("game-over background was not prepared")?
    } else {
        // The optional-art diagnostic was recorded during capture. A black
        // surface preserves the screen's text and choices without stale artwork.
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        world.resource_mut::<Assets<Image>>().add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
            bevy::asset::RenderAssetUsages::MAIN_WORLD
                | bevy::asset::RenderAssetUsages::RENDER_WORLD,
        ))
    };
    let quad = world.resource_mut::<Assets<Mesh>>().add(Rectangle::new(
        resonance_content::WIDTH as f32,
        resonance_content::HEIGHT as f32,
    ));
    let background = world
        .resource_mut::<Assets<crate::materials::TitleText>>()
        .add(crate::materials::TitleText {
            source: background,
            opacity_pulse: Vec4::X,
        });
    retire_session(world);
    let background = world
        .spawn((
            Mesh2d(quad),
            MeshMaterial2d(background),
            Transform::from_xyz(0., 0., 18.),
        ))
        .id();
    let text = world
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0., 0., 19.),
        ))
        .id();
    world.insert_resource(GameOver {
        selected: 0,
        font,
        art,
        mesh,
        entities: [background, text],
    });
    Ok(())
}

fn advance(world: &mut World) {
    if !world.contains_resource::<GameOver>() {
        return;
    }
    let clock = world.resource::<crate::Clock>().0;
    let input = world.resource_mut::<PendingInput>().consume(clock);
    world
        .resource_mut::<crate::field_view::Controls>()
        .consume();
    if input.up || input.down {
        let mut game_over = world.resource_mut::<GameOver>();
        game_over.selected ^= 1;
        match game_over_text(&game_over.font, &game_over.art, game_over.selected) {
            Ok(mesh) => {
                let handle = game_over.mesh.clone();
                *world
                    .resource_mut::<Assets<Mesh>>()
                    .get_mut(&handle)
                    .unwrap() = mesh;
            }
            Err(error) => {
                error!("Game over text failed: {error:#}");
                world.write_message(AppExit::error());
            }
        }
    }
    if input.accept {
        let load = world.resource::<GameOver>().selected == 0;
        if let Err(error) = return_to_title(world, load) {
            error!("Title return failed: {error:#}");
            world.write_message(AppExit::error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS and extracted discs; publishes a named library and uses no window or audio device"]
    fn game_over_load_and_quit_restore_the_title_and_release_the_field() -> Result<()> {
        use resonance_content::{diagnostics::Diagnostics, game_over::document};
        let work = crate::test_support::named_opening_fixture()?;
        let root = work.path();
        let path = root.join(document::PATH);
        let mut edited: resonance_content::game_over::Art =
            serde_json::from_slice(&fs::read(&path)?)?;
        edited.caption = "Try again?".into();
        edited.choices = ["Load a save".into(), "Back to title".into()];
        fs::write(path, serde_json::to_vec_pretty(&edited)?)?;
        let mut snapshot = resonance_content::library::Library::open(root, &[])?.snapshot();
        let (title, _) = resonance_content::startup::capture(&mut snapshot, false, &|| false)?;
        let title_art = title.artwork;
        resonance_content::preparation::ui(&mut snapshot, &Diagnostics::new(true), &|| false)?;
        let title_files = Arc::new(resonance_content::prepared::Files::from_snapshot(
            snapshot,
            || false,
            Diagnostics::new(true),
        )?);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..default()
            },
        ))
        .init_asset::<Image>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Surface>>()
        .init_resource::<Assets<crate::materials::TitleText>>()
        .init_resource::<Assets<TitleOutput>>()
        .init_resource::<crate::loading::Resident>()
        .init_resource::<crate::field_view::Controls>()
        .init_resource::<crate::audio::MenuSounds>()
        .init_resource::<crate::display::Display>()
        .init_resource::<PendingInput>()
        .init_resource::<crate::Clock>()
        .insert_resource(PendingAudio(None))
        .insert_resource(Title {
            events: None,
            audio: None,
            files: Some(title_files.clone()),
        })
        .insert_resource(crate::Art {
            manifest: title_art,
            images: vec![Handle::default(); 17],
        });
        let world = app.world_mut();
        let output = world
            .resource_mut::<Assets<TitleOutput>>()
            .add(TitleOutput {
                source: Handle::default(),
                brightness: Vec4::X,
            });
        let title = world
            .spawn((
                crate::scene::PartRoot,
                WorldAssetRoot(Handle::default()),
                Visibility::Inherited,
            ))
            .id();
        for selected in [0, 1] {
            let session = new_game::Session::load(root)?;
            let data =
                resonance_game::game_over::Assets::from_prepared(session.files().as_ref().clone())?;
            let _prepared = (
                world
                    .resource::<AssetServer>()
                    .load::<Image>(data.font.texture),
                world
                    .resource::<AssetServer>()
                    .load::<Image>(data.art.background.path),
            );
            new_game::activate(world, session);
            world
                .resource_mut::<Assets<TitleOutput>>()
                .get_mut(&output)
                .unwrap()
                .brightness = Vec4::Y;
            assert_eq!(*world.get::<Visibility>(title).unwrap(), Visibility::Hidden);
            show_game_over(world)?;
            assert_eq!(world.resource::<GameOver>().art.caption, edited.caption);
            assert_eq!(world.resource::<GameOver>().art.choices, edited.choices);
            assert_eq!(
                world
                    .resource::<Assets<TitleOutput>>()
                    .get(&output)
                    .unwrap()
                    .brightness,
                Vec4::X
            );
            assert!(!world.contains_resource::<new_game::Session>());
            assert!(
                world
                    .resource::<crate::loading::Resident>()
                    .files
                    .read()
                    .unwrap()
                    .is_none()
            );
            let entities = world.resource::<GameOver>().entities;
            world.resource_mut::<GameOver>().selected = selected;
            world.resource_mut::<PendingInput>().pressed.accept = true;
            world
                .resource_mut::<Assets<TitleOutput>>()
                .get_mut(&output)
                .unwrap()
                .brightness = Vec4::Y;
            advance(world);
            assert_eq!(
                world
                    .resource::<Assets<TitleOutput>>()
                    .get(&output)
                    .unwrap()
                    .brightness,
                Vec4::X
            );
            assert!(!world.contains_resource::<GameOver>());
            assert!(Arc::ptr_eq(
                world
                    .resource::<crate::loading::Resident>()
                    .files
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap(),
                &title_files,
            ));
            assert!(
                entities
                    .iter()
                    .all(|&entity| world.get_entity(entity).is_err())
            );
            assert_eq!(
                *world.get::<Visibility>(title).unwrap(),
                Visibility::Inherited
            );
            assert_eq!(
                world.contains_resource::<saves::title::TitleMenu>(),
                selected == 0
            );
            assert!(!world.resource::<PendingInput>().pressed.accept);
            assert!(world.resource::<Menu>().0.revealed);
        }
        Ok(())
    }
}

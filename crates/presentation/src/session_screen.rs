//! Terminal session screens reuse the title artwork and shared save browser.
use super::*;
use crate::materials::TitleOutput;
use crate::{Events, Menu, PendingAudio, PendingInput, audio, new_game, saves};
use resonance_events::session_screen::Target;

#[derive(Resource)]
pub(crate) struct Title {
    pub events: Option<resonance_events::EventRuntime>,
    pub audio: Option<audio::PlaybackAssets>,
}

#[derive(Resource)]
pub(crate) struct GameOver {
    selected: usize,
    font: BitmapFont,
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
    if target != Target::Title
        && !(if world.resource::<new_game::Session>().overworld.is_some() {
            crate::overworld::ready(world)
        } else {
            crate::field_view::ready(world)
        })
    {
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
    retire_session(world);
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
    if let Some(scene) = &world.resource::<crate::Art>().manifest.scene {
        let projection = Projection::custom(crate::TitleProjection(PerspectiveProjection {
            fov: scene.fov_degrees.to_radians(),
            aspect_ratio: world.resource::<crate::display::Display>().0.aspect(),
            near: 100.,
            far: 40000.,
            ..default()
        }));
        for (mut camera, mut fog) in world
            .query_filtered::<(&mut Projection, &mut DistanceFog), With<crate::FieldCamera>>()
            .iter_mut(world)
        {
            *camera = projection.clone();
            *fog = DistanceFog::default();
        }
    }
    if load {
        world.insert_resource(saves::title::LoadMenu::new());
    }
    Ok(())
}

fn game_over_text(font: &BitmapFont, selected: usize) -> Result<Mesh> {
    let mut batch = Batch::default();
    for (i, text) in ["Load data", "Quit game"].iter().enumerate() {
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
    skit::centered(
        &mut batch,
        font,
        "No one ever heard from them again.",
        416.,
        20.,
        1.,
        false,
    )?;
    Ok(batch.mesh([font.width, font.height]))
}

fn show_game_over(world: &mut World) -> Result<()> {
    let font: BitmapFont = world
        .resource::<crate::loading::Resident>()
        .files
        .read()
        .unwrap()
        .as_ref()
        .context("scene font inventory is missing")?
        .json("fonts/dialogue.json")?;
    let image = world
        .resource::<AssetServer>()
        .get_handle::<Image>(&font.texture)
        .context("scene font is not resident")?;
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(game_over_text(&font, 0)?);
    let material = world.resource_mut::<Assets<Surface>>().add(Surface {
        source: image.clone(),
        sampling: image.clone(),
        frame_mask: image.clone(),
        color_mask: image,
        coverage: Coverage::default(),
        opaque: false,
        additive: false,
        red_channel: false,
    });
    let background = world.resource::<crate::Art>().images[14].clone();
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
        match game_over_text(&game_over.font, game_over.selected) {
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

    #[test]
    #[ignore = "requires cooked field/title assets; no window or audio device"]
    fn game_over_load_and_quit_restore_the_title_and_release_the_field() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            std::path::PathBuf::from,
        );
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
        })
        .insert_resource(crate::Art {
            manifest: serde_json::from_slice(&fs::read(root.join("title.json"))?)?,
            images: vec![Handle::default(); 17],
        });
        let world = app.world_mut();
        let output = world
            .resource_mut::<Assets<TitleOutput>>()
            .add(TitleOutput {
                source: Handle::default(),
                brightness: Vec4::X,
                screen_offset: Vec2::ZERO,
            });
        let title = world
            .spawn((
                crate::scene::PartRoot,
                WorldAssetRoot(Handle::default()),
                Visibility::Inherited,
            ))
            .id();
        for selected in [0, 1] {
            let session = new_game::Session::load(&root)?;
            let font: BitmapFont = session.files().json("fonts/dialogue.json")?;
            let _font = world.resource::<AssetServer>().load::<Image>(font.texture);
            new_game::activate(world, session);
            world
                .resource_mut::<Assets<TitleOutput>>()
                .get_mut(&output)
                .unwrap()
                .brightness = Vec4::Y;
            assert_eq!(*world.get::<Visibility>(title).unwrap(), Visibility::Hidden);
            show_game_over(world)?;
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
                world.contains_resource::<saves::title::LoadMenu>(),
                selected == 0
            );
            assert!(!world.resource::<PendingInput>().pressed.accept);
            assert!(world.resource::<Menu>().0.revealed);
        }
        Ok(())
    }
}

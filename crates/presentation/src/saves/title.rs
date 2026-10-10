//! Title loading owns the shared slot menu until it closes or a field is ready.
use super::*;
use crate::{
    PendingInput, audio,
    field_ui::{MenuOverlay, Surface},
};
use resonance_game::menu::{Menu, Mode, Page};

#[derive(Resource)]
pub(crate) struct LoadMenu(pub Menu);
impl LoadMenu {
    pub fn new() -> Self {
        Self(Menu::new(Page::Slots(Mode::Load), None, false))
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        advance
            .run_if(crate::dungeons::running)
            .before(field_view::advance_live)
            .before(crate::advance),
    )
    .add_systems(Update, (prepare, render).chain().after(crate::layout));
}
fn prepare(world: &mut World) {
    if !world.contains_resource::<LoadMenu>() {
        return;
    }
    if world.contains_resource::<MenuOverlay>() {
        return;
    }
    let server = world.resource::<AssetServer>().clone();
    let root = world.resource::<crate::RunOptions>().assets.clone();
    let diagnostics = crate::diagnostics::policy(world);
    match MenuOverlay::load(
        &root,
        &server,
        &mut world.resource_mut::<Assets<Surface>>(),
        &diagnostics,
    ) {
        Ok(overlay) => {
            world.insert_resource(overlay);
        }
        Err(error) => unavailable(world, error),
    }
}
#[allow(clippy::too_many_arguments)] // Bevy queries the existing input, menu and audio owners directly.
fn advance(
    mut commands: Commands,
    mut menu: Option<ResMut<LoadMenu>>,
    art: Option<Res<MenuOverlay>>,
    images: Res<Assets<Image>>,
    mut controls: ResMut<field_view::Controls>,
    mut pending: ResMut<PendingInput>,
    sounds: Res<audio::MenuSounds>,
    game_over: Option<Res<crate::game_over::Active>>,
    battle_audio: Option<Res<crate::battle_audio::Playback>>,
    mut failures: field_view::Failures,
    scenario: Option<ResMut<ScenarioInput>>,
) {
    let Some(menu) = &mut menu else {
        return;
    };
    let input = controls.consume();
    if art.is_none_or(|art| !art.ready(&images)) {
        return;
    }
    if let Some(mut scenario) = scenario {
        scenario.acknowledge_input();
    }
    if let Some(cue) = menu.0.step(input) {
        if game_over.is_some() {
            if let Some(audio) = &battle_audio {
                let result = u16::try_from(cue)
                    .context("invalid load-menu sound ID")
                    .and_then(|cue| audio.system_cue(cue));
                if let Err(error) = result
                    && !failures.skip("load menu cue", error)
                {
                    return;
                }
            }
        } else if let Some(control) = &sounds.control {
            let name = match cue {
                1 => "navigate",
                2 => "confirm",
                3 => "back",
                _ => "error",
            };
            if let Err(error) = control.play(name)
                && !failures.skip("load menu cue", error)
            {
                return;
            }
        }
    }
    if menu.0.closed {
        commands.remove_resource::<LoadMenu>();
        *pending = PendingInput {
            held: pending.held,
            ..Default::default()
        };
    }
}
#[allow(clippy::too_many_arguments)] // Menu state, prepared drawing resources, and the presentation clock.
fn render(
    mut commands: Commands,
    display: Res<crate::display::Display>,
    menu: Option<Res<LoadMenu>>,
    art: Option<ResMut<MenuOverlay>>,
    images: Res<Assets<Image>>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    clock: Res<crate::Clock>,
) {
    let Some(mut art) = art else {
        return;
    };
    let ready = art
        .render(
            menu.as_ref().map(|m| &m.0),
            clock.0.tick(),
            display.0,
            &mut commands,
            &mut meshes,
        )
        .and_then(|()| art.drawn_images_ready(&images, &server));
    if let Err(error) = ready {
        art.hide(&mut commands);
        commands.queue(move |world: &mut World| unavailable(world, error));
    }
}
fn unavailable(world: &mut World, error: anyhow::Error) {
    retire(world);
    let mut pending = world.resource_mut::<PendingInput>();
    *pending = PendingInput {
        held: pending.held,
        ..Default::default()
    };
    if crate::diagnostics::policy(world)
        .report("load menu artwork", error)
        .is_err()
    {
        world.write_message(AppExit::error());
    }
}
pub(crate) fn retire(world: &mut World) {
    world.remove_resource::<LoadMenu>();
    if let Some(overlay) = world.remove_resource::<MenuOverlay>() {
        overlay.despawn(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires prepared menu artwork and title audio; no device"]
    fn stopped_audio_preserves_tolerant_load_menu_navigation_and_cancel() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            PathBuf::from,
        );
        for paranoid in [false, true] {
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
            let (source, control) =
                audio::PlaybackAssets::load(&root, diagnostics.clone())?.session();
            drop(source);
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                AssetPlugin {
                    file_path: root.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            ))
            .register_asset_loader(bevy::image::ImageLoader::new(
                bevy::image::CompressedImageFormats::NONE,
            ))
            .init_asset::<Image>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<field_view::Controls>()
            .init_resource::<PendingInput>()
            .add_message::<AppExit>()
            .insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()))
            .insert_resource(audio::MenuSounds {
                control: Some(control),
            })
            .insert_resource(crate::TitleActive)
            .insert_resource(LoadMenu::new())
            .add_systems(Update, (field_view::gather_controls, advance).chain());
            let mut materials = Assets::<Surface>::default();
            let mut overlay = MenuOverlay::load_with(
                |path| {
                    ensure!(
                        !path.starts_with("game/"),
                        "Load must not admit gameplay definitions"
                    );
                    let bytes = std::fs::read(root.join(path))?;
                    if path == "ui/menu.json" {
                        let mut document: serde_json::Value = serde_json::from_slice(&bytes)?;
                        document["sprites"]["rects"]
                            .as_object_mut()
                            .unwrap()
                            .remove("recipes");
                        for group in ["names", "number_colors", "bar_colors"] {
                            document["sprites"].as_object_mut().unwrap().remove(group);
                        }
                        return Ok(serde_json::to_vec(&document)?);
                    }
                    Ok(bytes)
                },
                app.world().resource::<AssetServer>(),
                &mut materials,
                &diagnostics,
            )?;
            app.insert_resource(materials);
            app.world_mut().resource_mut::<LoadMenu>().0.take_command();
            app.world_mut().resource_mut::<LoadMenu>().0.finish(None);
            let mut queue = bevy::ecs::world::CommandQueue::default();
            let mut meshes = Assets::<Mesh>::default();
            overlay.render(
                Some(&app.world().resource::<LoadMenu>().0),
                0,
                crate::Resolution::default(),
                &mut Commands::new(&mut queue, app.world()),
                &mut meshes,
            )?;
            queue.apply(app.world_mut());
            // Admit only images used by the actual Load drawing. Other menu
            // textures are still absent when navigation and cancel execute.
            assert!(!overlay.ready(app.world().resource::<Assets<Image>>()));
            for image in overlay.drawn_images() {
                app.world_mut()
                    .resource_mut::<Assets<Image>>()
                    .insert(image.id(), Image::default())?;
            }
            assert!(
                !overlay.ready(app.world().resource::<Assets<Image>>()),
                "decoded images must still await a submitted menu draw"
            );
            app.world()
                .resource::<crate::field_ui::MenuDraws>()
                .0
                .lock()
                .unwrap()
                .completed
                .store(true, std::sync::atomic::Ordering::Release);
            assert!(overlay.ready(app.world().resource::<Assets<Image>>()));
            assert!(
                overlay
                    .images()
                    .any(|image| !app.world().resource::<Assets<Image>>().contains(image.id()))
            );
            app.insert_resource(overlay);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ArrowRight);
            app.update();
            assert!(diagnostics.entries().iter().any(
                |entry| entry.scope == "title cue request" && entry.message.contains("stopped")
            ));
            assert_eq!(
                app.world().resource::<Messages<AppExit>>().is_empty(),
                !paranoid
            );
            assert!(app.world().contains_resource::<LoadMenu>());
            if paranoid {
                continue;
            }
            assert_eq!(app.world().resource::<LoadMenu>().0.bank, 1);
            let mut pending = app.world_mut().resource_mut::<PendingInput>();
            pending.held.accept = true;
            pending.pressed.accept = true;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.update();
            assert!(!app.world().contains_resource::<LoadMenu>());
            assert!(app.world().contains_resource::<crate::TitleActive>());
            let pending = app.world().resource::<PendingInput>();
            assert!(pending.held.accept);
            assert!(!pending.pressed.accept);
            assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires prepared menu artwork"]
    fn missing_and_corrupt_menu_images_retire_load_menu_and_honor_policy() -> Result<()> {
        use std::{fs, time::Duration};

        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            PathBuf::from,
        );
        let dialogue: resonance_content::font::DialogueArt =
            serde_json::from_slice(&fs::read(root.join("ui/dialogue.json"))?)?;
        let directory = std::env::temp_dir().join(format!(
            "resonance-menu-images-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        ));
        fs::create_dir(&directory)?;
        let ready = format!(
            "ready.{}",
            std::path::Path::new(&dialogue.textures[0].path)
                .extension()
                .and_then(|value| value.to_str())
                .context("dialogue image has no suffix")?
        );
        fs::copy(
            root.join(&dialogue.textures[0].path),
            directory.join(&ready),
        )?;
        fs::write(directory.join("broken.png"), b"not a PNG")?;
        for paranoid in [false, true] {
            for image_path in ["missing.png", "broken.png"] {
                let mut app = App::new();
                let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
                app.add_plugins((
                    MinimalPlugins,
                    AssetPlugin {
                        file_path: directory.to_string_lossy().into_owned(),
                        ..Default::default()
                    },
                ))
                .register_asset_loader(bevy::image::ImageLoader::new(
                    bevy::image::CompressedImageFormats::NONE,
                ))
                .init_asset::<Image>()
                .init_asset::<Mesh>()
                .init_resource::<crate::display::Display>()
                .init_resource::<crate::Clock>()
                .init_resource::<PendingInput>()
                .add_message::<AppExit>()
                .insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()))
                .insert_resource(crate::TitleActive)
                .insert_resource(LoadMenu::new())
                .add_systems(Update, render);
                let server = app.world().resource::<AssetServer>().clone();
                let mut materials = Assets::<Surface>::default();
                let overlay = MenuOverlay::load_with(
                    |path| {
                        let mut metadata: serde_json::Value =
                            serde_json::from_slice(&fs::read(root.join(path))?)?;
                        if path == "ui/menu.json" {
                            for texture in
                                metadata["textures"].as_object_mut().unwrap().values_mut()
                            {
                                texture["path"] = image_path.into();
                            }
                        } else if path == "ui/dialogue.json" {
                            metadata["cursor"]["path"] = ready.clone().into();
                        } else if path == dialogue.font {
                            metadata["texture"] = ready.clone().into();
                        }
                        Ok(serde_json::to_vec(&metadata)?)
                    },
                    &server,
                    &mut materials,
                    &diagnostics,
                )?;
                app.insert_resource(materials);
                let image: Handle<Image> = server.load(image_path.to_owned());
                let healthy = server.load::<Image>(ready.clone());
                app.insert_resource(overlay);
                let began = Instant::now();
                while !diagnostics.has_errors()
                    || !app
                        .world()
                        .resource::<Assets<Image>>()
                        .contains(healthy.id())
                {
                    ensure!(
                        began.elapsed() < Duration::from_secs(5),
                        "menu image failure was not observed"
                    );
                    app.update();
                    std::thread::sleep(Duration::from_millis(1));
                }
                let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(image.id())
                else {
                    panic!("intended menu image did not fail: {image_path}");
                };
                let entries = diagnostics.entries();
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].scope, "load menu artwork");
                assert!(entries[0].message.contains(&error.to_string()));
                assert!(!app.world().contains_resource::<LoadMenu>());
                assert!(!app.world().contains_resource::<MenuOverlay>());
                assert!(app.world().contains_resource::<crate::TitleActive>());
                assert_eq!(
                    app.world().resource::<Messages<AppExit>>().is_empty(),
                    !paranoid
                );
            }
        }
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn unavailable_load_menu_returns_to_its_caller_and_honors_error_policy() {
        for paranoid in [false, true] {
            let mut world = World::new();
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
            world.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
            world.init_resource::<Messages<AppExit>>();
            world.init_resource::<PendingInput>();
            world.resource_mut::<PendingInput>().held.accept = true;
            world.resource_mut::<PendingInput>().pressed.accept = true;
            world.insert_resource(LoadMenu::new());
            unavailable(&mut world, anyhow::anyhow!("missing menu glyph"));
            assert!(!world.contains_resource::<LoadMenu>());
            assert!(world.resource::<PendingInput>().held.accept);
            assert!(!world.resource::<PendingInput>().pressed.accept);
            assert!(diagnostics.has_errors());
            assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
        }
    }
}

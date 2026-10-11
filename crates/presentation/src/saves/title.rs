//! Title menus share the field menu controller and artwork without a live field.
use super::*;
use crate::{
    PendingInput, audio,
    config::Preferences,
    field_ui::{MenuOverlay, Surface},
};
use resonance_content::menu_data::CustomizeSettings;
use resonance_game::menu::{Menu, Mode, Page};
use std::{fs, sync::Arc};

#[derive(Resource)]
pub(crate) struct TitleMenu(pub Menu);
impl TitleMenu {
    pub fn load() -> Self {
        Self(Menu::new(Page::Slots(Mode::Load), None, false))
    }

    pub fn customize(preferences: &CustomizeSettings) -> Self {
        Self(Menu::title_customize(preferences.clone()))
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Preferences>()
        .add_systems(
            FixedUpdate,
            advance
                .run_if(crate::dungeons::running)
                .before(field_view::advance_live)
                .before(crate::advance),
        )
        .add_systems(Update, (prepare, render).chain().after(crate::layout))
        .add_systems(
            Update,
            preview.after(crate::layout).after(crate::start_audio),
        );
}
fn prepare(world: &mut World) {
    let Some(menu) = world.get_resource::<TitleMenu>() else {
        return;
    };
    let needs_resources = menu.0.page == Page::Customize && menu.0.resources.is_none();
    if !needs_resources && world.contains_resource::<MenuOverlay>() {
        return;
    }
    let root = world.resource::<crate::RunOptions>().assets.clone();
    let result = (|| -> Result<()> {
        if needs_resources {
            let data: resonance_content::menu_data::MenuData =
                serde_json::from_slice(&fs::read(root.join("game/menu-data.json"))?)?;
            data.validate()?;
            let session: resonance_content::session::SessionData =
                serde_json::from_slice(&fs::read(root.join("game/session-data.json"))?)?;
            session.validate()?;
            world.resource_mut::<TitleMenu>().0.resources =
                Some(Arc::new(resonance_game::menu::Resources {
                    session: Arc::new(session),
                    data: Arc::new(data),
                }));
        }
        if !world.contains_resource::<MenuOverlay>() {
            let server = world.resource::<AssetServer>().clone();
            let overlay =
                MenuOverlay::load(&root, &server, &mut world.resource_mut::<Assets<Surface>>())?;
            world.insert_resource(overlay);
        }
        Ok(())
    })();
    if let Err(error) = result {
        world.remove_resource::<TitleMenu>();
        report(world, Err(error.context("Title menu is unavailable")));
        let mut pending = world.resource_mut::<PendingInput>();
        *pending = PendingInput {
            held: pending.held,
            ..Default::default()
        };
    }
}
#[allow(clippy::too_many_arguments)] // Shared input, artwork readiness, and title preferences.
fn advance(
    mut commands: Commands,
    mut menu: Option<ResMut<TitleMenu>>,
    art: Option<Res<MenuOverlay>>,
    images: Res<Assets<Image>>,
    mut controls: ResMut<field_view::Controls>,
    mut pending: ResMut<PendingInput>,
    sounds: Res<audio::MenuSounds>,
    mut preferences: ResMut<Preferences>,
) {
    let Some(menu) = &mut menu else {
        return;
    };
    let input = controls.consume();
    if art.is_none_or(|art| !art.ready(&images))
        || menu.0.page == Page::Customize && menu.0.resources.is_none()
    {
        return;
    }
    let closing = menu.0.customize.transition.page_closing;
    let cue = menu.0.step(input);
    if !closing && menu.0.page == Page::Customize && menu.0.customize.transition.page_closing {
        if let Err(error) = preferences.commit(menu.0.customize.draft.clone()) {
            commands.queue(move |world: &mut World| {
                report(
                    world,
                    Err(error.context("Settings applied, but config.toml could not be saved")),
                );
            });
        }
        if let Some(control) = &sounds.control {
            control.settings(
                preferences.settings.volumes.music,
                preferences.settings.volumes.effects,
                preferences.settings.stereo,
            );
        }
    }
    if let Some(cue) = cue
        && let Some(control) = &sounds.control
    {
        let name = match cue {
            1 | 38 => "navigate",
            2 => "confirm",
            3 => "back",
            _ => "error",
        };
        if let Err(error) = control.play(name) {
            error!("Menu cue failed: {error:#}");
        }
    }
    if menu.0.closed {
        commands.remove_resource::<TitleMenu>();
        *pending = PendingInput {
            held: pending.held,
            ..Default::default()
        };
    }
}
#[allow(clippy::too_many_arguments)] // Scene endpoints and independent Bevy presentation resources.
fn render(
    mut commands: Commands,
    mut prediction: ResMut<crate::field_view::Prediction>,
    display: Res<crate::display::Display>,
    menu: Option<Res<TitleMenu>>,
    art: Option<ResMut<MenuOverlay>>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut art) = art else {
        return;
    };
    art.prepare(&mut commands, &mut meshes);
    if art.ready(&images)
        && let Err(error) = art.render(
            menu.as_ref().map(|m| &m.0),
            display.0,
            Some(&mut prediction),
            &mut commands,
            &mut meshes,
        )
    {
        error!("Title menu rendering failed: {error:#}");
        exit.write(AppExit::error());
    }
}

fn preview(
    menu: Option<Res<TitleMenu>>,
    session: Option<Res<new_game::Session>>,
    preferences: Res<Preferences>,
    sounds: Res<audio::MenuSounds>,
    stage: Res<crate::display::OutputStage>,
    mut outputs: ResMut<Assets<crate::materials::TitleOutput>>,
) {
    if session.is_some() {
        return;
    }
    let draft = menu
        .as_ref()
        .and_then(|m| m.0.preferences())
        .unwrap_or(&preferences.settings);
    crate::materials::TitleOutput::position(&mut outputs, draft.screen_position, *stage);
    if let Some(control) = &sounds.control {
        control.settings(
            draft.volumes.music,
            preferences.settings.volumes.effects,
            preferences.settings.stereo,
        );
    }
}
pub(crate) fn retire(world: &mut World) {
    world.remove_resource::<TitleMenu>();
    if let Some(overlay) = world.remove_resource::<MenuOverlay>() {
        overlay.despawn(world);
    }
}

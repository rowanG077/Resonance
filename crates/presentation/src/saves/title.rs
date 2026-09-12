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
            .before(field_view::advance_live)
            .before(crate::advance),
    )
    .add_systems(Update, (prepare, render).chain().after(crate::layout));
}
fn prepare(world: &mut World) {
    if !world.contains_resource::<LoadMenu>() || world.contains_resource::<MenuOverlay>() {
        return;
    }
    let root = world.resource::<crate::RunOptions>().assets.clone();
    let server = world.resource::<AssetServer>().clone();
    match MenuOverlay::load(&root, &server, &mut world.resource_mut::<Assets<Surface>>()) {
        Ok(overlay) => {
            world.insert_resource(overlay);
        }
        Err(error) => {
            world.remove_resource::<LoadMenu>();
            report(
                world,
                Err(error.context("Load menu artwork is unavailable")),
            );
            let mut pending = world.resource_mut::<PendingInput>();
            *pending = PendingInput {
                held: pending.held,
                ..Default::default()
            };
        }
    }
}
fn advance(
    mut commands: Commands,
    mut menu: Option<ResMut<LoadMenu>>,
    art: Option<Res<MenuOverlay>>,
    images: Res<Assets<Image>>,
    mut controls: ResMut<field_view::Controls>,
    mut pending: ResMut<PendingInput>,
    sounds: Res<audio::MenuSounds>,
) {
    let Some(menu) = &mut menu else {
        return;
    };
    let input = controls.consume();
    if art.is_none_or(|art| !art.ready(&images)) {
        return;
    }
    if let Some(cue) = menu.0.step(input)
        && let Some(control) = &sounds.control
    {
        let name = match cue {
            1 => "navigate",
            2 => "confirm",
            3 => "back",
            _ => "error",
        };
        if let Err(error) = control.play(name) {
            error!("Menu cue failed: {error:#}");
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
fn render(
    mut commands: Commands,
    display: Res<crate::display::Display>,
    menu: Option<Res<LoadMenu>>,
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
            &mut commands,
            &mut meshes,
        )
    {
        error!("Load menu rendering failed: {error:#}");
        exit.write(AppExit::error());
    }
}
pub(crate) fn retire(world: &mut World) {
    world.remove_resource::<LoadMenu>();
    if let Some(overlay) = world.remove_resource::<MenuOverlay>() {
        overlay.despawn(world);
    }
}

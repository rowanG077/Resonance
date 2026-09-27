mod destinations;
mod overlay;
use super::{loading, new_game};
use bevy::prelude::*;
use destinations::DESTINATIONS;
pub(super) use destinations::Destination;

#[derive(Resource, Default)]
pub(super) struct Menu {
    state: State,
    selected: usize,
    pub testing: bool,
}

#[derive(Default)]
enum State {
    #[default]
    Closed,
    AwaitRelease,
    Selecting,
    Loading(loading::Pending),
    Failed(String),
}

pub(super) fn running(menu: Option<Res<Menu>>) -> bool {
    menu.is_none_or(|menu| !menu.blocked())
}

impl Menu {
    pub(super) fn blocked(&self) -> bool {
        !matches!(self.state, State::Closed)
    }

    fn open(&self) -> bool {
        matches!(
            self.state,
            State::Selecting | State::Loading(_) | State::Failed(_)
        )
    }
}

pub(super) fn install(app: &mut App, headless: bool) {
    app.init_resource::<Menu>().add_systems(
        PreUpdate,
        controls
            .after(bevy::input::InputSystems)
            .before(super::gather_input)
            .before(super::field_view::gather_controls)
            .before(super::overworld::controls),
    );
    if !headless {
        app.add_systems(Startup, overlay::setup)
            .add_systems(Update, overlay::update);
    }
}

fn available(world: &World) -> bool {
    !world
        .get_resource::<super::movie::Playback>()
        .is_some_and(|m| m.active)
        && !world
            .get_resource::<super::boot::Playback>()
            .is_some_and(|b| b.active())
        && !world.contains_resource::<loading::Pending>()
        && !world.contains_resource::<new_game::Request>()
        && !world.contains_resource::<super::saves::WorldLoad>()
}

fn controls(world: &mut World) {
    let keys = world.resource::<ButtonInput<KeyCode>>().clone();
    let mouse = world.get_resource::<ButtonInput<MouseButton>>();
    let clicked = mouse.is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    let mouse_held = mouse.is_some_and(|buttons| buttons.pressed(MouseButton::Left));
    let row = world
        .query::<&Window>()
        .iter(world)
        .next()
        .and_then(|window| {
            overlay::row_at(
                window.cursor_position()?,
                Vec2::new(window.width(), window.height()),
            )
        });
    let available = available(world);
    world.resource_scope(|world, mut menu: Mut<Menu>| {
        if matches!(menu.state, State::AwaitRelease)
            && keys.get_pressed().next().is_none()
            && !mouse_held
        {
            menu.state = State::Closed;
        }
        if let State::Loading(pending) = &menu.state {
            let result = match pending.poll() {
                Ok(None) => None,
                Ok(Some(result)) => Some(result),
                Err(error) => Some(Err(error)),
            };
            if let Some(result) = result {
                menu.state = match result {
                    Ok(candidate) => {
                        activate(world, candidate);
                        menu.testing = true;
                        State::AwaitRelease
                    }
                    Err(error) => {
                        warn!("Dungeon preparation failed: {error:#}");
                        State::Failed(format!("{error:#}"))
                    }
                };
                return;
            }
        }
        let toggle = keys.just_pressed(KeyCode::Tab)
            && (menu.open() || keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
        if toggle && (menu.open() || available) {
            menu.state = if menu.open() {
                State::AwaitRelease
            } else {
                State::Selecting
            };
            return;
        }
        if !menu.open() {
            return;
        }
        if keys.just_pressed(KeyCode::Escape) {
            menu.state = State::AwaitRelease;
            return;
        }
        if matches!(menu.state, State::Loading(_)) {
            return;
        }
        if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
            menu.selected = (menu.selected + DESTINATIONS.len() - 1) % DESTINATIONS.len();
            menu.state = State::Selecting;
        }
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
            menu.selected = (menu.selected + 1) % DESTINATIONS.len();
            menu.state = State::Selecting;
        }
        let mut choose = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
        if clicked && let Some(row) = row {
            menu.selected = row;
            choose = true;
        }
        if let Some(index) = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::Digit0,
        ]
        .iter()
        .position(|key| keys.just_pressed(*key))
        {
            menu.selected = index;
            choose = true;
        }
        if choose {
            let options = world.resource::<super::RunOptions>();
            let result = loading::Pending::dungeon(
                options.assets.clone(),
                options.script_root.clone(),
                DESTINATIONS[menu.selected],
                world.resource::<loading::Resident>(),
            );
            menu.state = match result {
                Ok(pending) => State::Loading(pending),
                Err(error) => State::Failed(format!("{error:#}")),
            };
        }
    });
}

fn activate(world: &mut World, candidate: new_game::Session) {
    // A pending native exit belongs to the old run, including a failed exit.
    world.remove_resource::<loading::FieldPending>();
    world.remove_resource::<loading::WorldPending>();
    world.remove_resource::<new_game::TransitionFailure>();
    if let Some(mut current) = world.get_resource_mut::<new_game::Session>() {
        let changing_field =
            current.overworld.is_some() || current.assets.map_id != candidate.assets.map_id;
        current.replace_loaded(candidate);
        // Cold rooms must submit their UI draws during GPU preparation. Holding
        // the old scene cameras (as for a warm quickload) prevents that submission.
        super::saves::release_retained_frame(world);
        super::saves::reset_scene(world, changing_field);
    } else {
        new_game::activate(world, candidate);
    }
    let map = world.resource::<new_game::Session>().assets.map_id;
    for mut window in world.query::<&mut Window>().iter_mut(world) {
        window.title =
            format!("Resonance — Dungeon test / map {map} — Shift+Tab: Dungeons / Tab: Party");
    }
}

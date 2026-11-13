//! Translate physical devices once, then apply the user's standard action bindings.
use bevy::{
    ecs::system::SystemParam,
    input::{ButtonState, keyboard::KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};
use resonance_content::input::{AxisDirection, Keymap, PhysicalInput, StandardAction};
use std::collections::{BTreeMap, BTreeSet};

const STICK_DEAD_ZONE: f32 = 0.2;

fn radial_dead_zone(stick: Vec2) -> Vec2 {
    let length = stick.length();
    if length <= STICK_DEAD_ZONE {
        return Vec2::ZERO;
    }
    stick * (((length - STICK_DEAD_ZONE) / (1. - STICK_DEAD_ZONE)).min(1.) / length)
}

#[derive(Clone, Default)]
pub(crate) struct PhysicalState {
    values: BTreeMap<PhysicalInput, f32>,
    pub pressed: BTreeSet<PhysicalInput>,
}
impl PhysicalState {
    pub fn keyboard(keys: &ButtonInput<KeyCode>) -> Self {
        let mut state = Self::default();
        for key in keys.get_pressed() {
            state
                .values
                .insert(PhysicalInput::Key(format!("{key:?}")), 1.);
        }
        for key in keys.get_just_pressed() {
            state.pressed.insert(PhysicalInput::Key(format!("{key:?}")));
        }
        state
    }
    pub fn gamepad(pad: &Gamepad) -> Self {
        let mut state = Self::default();
        for button in pad.get_pressed() {
            state
                .values
                .insert(PhysicalInput::Button(format!("{button:?}")), 1.);
        }
        for button in pad.get_just_pressed() {
            state
                .pressed
                .insert(PhysicalInput::Button(format!("{button:?}")));
        }
        use AxisDirection::*;
        for (stick, [negative_x, positive_x, negative_y, positive_y]) in [
            (pad.left_stick(), [LeftLeft, LeftRight, LeftDown, LeftUp]),
            (
                pad.right_stick(),
                [RightLeft, RightRight, RightDown, RightUp],
            ),
        ] {
            let stick = radial_dead_zone(stick);
            for (axis, value) in [
                (negative_x, -stick.x),
                (positive_x, stick.x),
                (negative_y, -stick.y),
                (positive_y, stick.y),
            ] {
                if value > 0. {
                    state.values.insert(PhysicalInput::Axis(axis), value);
                }
            }
        }
        state
    }
    pub fn merge(&mut self, other: &Self) {
        for (input, value) in &other.values {
            let entry = self.values.entry(input.clone()).or_default();
            *entry = entry.max(*value);
        }
        self.pressed.extend(other.pressed.iter().cloned());
    }
    pub fn held(&self) -> BTreeSet<PhysicalInput> {
        self.values
            .iter()
            .filter(|(_, value)| **value > 0.65)
            .map(|(input, _)| input.clone())
            .collect()
    }
    pub fn capture_inputs(&self, keymap: &Keymap) -> BTreeSet<PhysicalInput> {
        // Moderate tilts count, but each stick contributes only its strongest
        // direction. Vertical wins ties, giving bindings four cardinal directions.
        let mut inputs: BTreeSet<_> = self
            .values
            .iter()
            .filter(|(input, value)| matches!(input, PhysicalInput::Axis(_)) || **value > 0.65)
            .map(|(input, _)| input.clone())
            .chain(self.pressed.iter().cloned())
            .collect();
        for source in resonance_content::input::DirectionSource::ALL {
            let strongest = inputs
                .iter()
                .filter_map(|input| {
                    let direction = keymap.direction(input)?;
                    (direction.source == source).then_some((input, direction.slot))
                })
                .max_by(|(a, a_slot), (b, b_slot)| {
                    let strength = |input| self.values.get(input).copied().unwrap_or(0.);
                    strength(a)
                        .total_cmp(&strength(b))
                        .then_with(|| a_slot.vertical().cmp(&b_slot.vertical()))
                })
                .map(|(input, _)| input.clone());
            inputs.retain(|input| {
                keymap.direction(input).is_none_or(|direction| {
                    direction.source != source || Some(input) == strongest.as_ref()
                })
            });
        }
        inputs
    }
    pub fn without(&self, consumed: &BTreeSet<PhysicalInput>) -> Self {
        Self {
            values: self
                .values
                .iter()
                .filter(|(input, _)| !consumed.contains(*input))
                .map(|(input, value)| (input.clone(), *value))
                .collect(),
            pressed: self.pressed.difference(consumed).cloned().collect(),
        }
    }
    pub fn restore_input(&mut self, source: &Self, input: &PhysicalInput) {
        if let Some(value) = source.values.get(input) {
            self.values.insert(input.clone(), *value);
        }
        if source.pressed.contains(input) {
            self.pressed.insert(input.clone());
        }
    }
    fn standard_input(&self, input: &PhysicalInput) -> bool {
        // Preserve the platform overlay shortcut without opening the game menu.
        !matches!(input, PhysicalInput::Key(key) if key == "Tab"
            && ["ShiftLeft", "ShiftRight"].into_iter().any(|shift| self.values.contains_key(&PhysicalInput::Key(shift.into()))))
    }
    pub fn value(&self, map: &Keymap, action: StandardAction) -> f32 {
        map.get(action)
            .iter()
            .filter(|i| self.standard_input(i))
            .filter_map(|i| self.values.get(i))
            .copied()
            .fold(0., f32::max)
    }
    pub fn down(&self, map: &Keymap, action: StandardAction) -> bool {
        self.value(map, action) > 0.5
    }
    pub fn edge(&self, map: &Keymap, action: StandardAction) -> bool {
        map.get(action)
            .iter()
            .any(|i| self.standard_input(i) && self.pressed.contains(i))
    }
    pub fn track_edges(&mut self, previous: &mut BTreeSet<PhysicalInput>) {
        let held = self.held();
        self.pressed.extend(
            held.difference(previous)
                .filter(|input| matches!(input, PhysicalInput::Axis(_)))
                .cloned(),
        );
        *previous = held;
    }
    pub fn movement(&self, map: &Keymap) -> Vec2 {
        use StandardAction::*;
        Vec2::new(
            self.value(map, MoveRight) - self.value(map, MoveLeft),
            self.value(map, MoveUp) - self.value(map, MoveDown),
        )
    }
    pub fn camera(&self, map: &Keymap) -> Vec2 {
        use StandardAction::*;
        Vec2::new(
            self.value(map, CameraRight) - self.value(map, CameraLeft),
            self.value(map, CameraUp) - self.value(map, CameraDown),
        )
    }
}

/// Host shortcuts share physical bindings with gameplay, including controllers,
/// while keyboard repeats and events for other windows never create new edges.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct HostActions;

/// Resolve host shortcuts once, before any input recorder can finish this frame.
#[derive(Resource, Default)]
pub(super) struct HostInput(Vec<StandardAction>);

impl HostInput {
    pub fn pressed(&self, action: StandardAction) -> bool {
        self.0.contains(&action)
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<HostInput>()
        // Every host action runs before a field recorder can finish this frame.
        .configure_sets(
            PreUpdate,
            HostActions.before(crate::field_view::gather_controls),
        )
        .add_systems(
            PreUpdate,
            gather_host_input
                .after(bevy::input::InputSystems)
                .before(HostActions),
        );
}

/// Stick shortcuts latch until the direction is released well below the press
/// threshold. Each controller owns its latches before its edges are merged.
#[derive(Default)]
struct HostAxes(BTreeSet<PhysicalInput>);

impl HostAxes {
    fn sample(&mut self, physical: &mut PhysicalState) {
        const PRESS: f32 = 0.65;
        const RELEASE: f32 = 0.35;
        self.0.retain(|input| {
            physical
                .values
                .get(input)
                .is_some_and(|value| *value > RELEASE)
        });
        for (input, value) in &physical.values {
            if matches!(input, PhysicalInput::Axis(_))
                && *value > PRESS
                && self.0.insert(input.clone())
            {
                physical.pressed.insert(input.clone());
            }
        }
    }
}

#[derive(SystemParam)]
struct HostDevices<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    keyboard: MessageReader<'w, 's, KeyboardInput>,
    windows: Query<'w, 's, Entity, With<PrimaryWindow>>,
    pads: Query<'w, 's, (Entity, &'static Gamepad)>,
    axes: Local<'s, BTreeMap<Entity, HostAxes>>,
}
impl HostDevices<'_, '_> {
    fn sample(&mut self) -> PhysicalState {
        let mut physical = PhysicalState::keyboard(&self.keys);
        let primary = self.windows.single().ok();
        let presses: BTreeSet<_> = self
            .keyboard
            .read()
            .filter(|event| {
                Some(event.window) == primary
                    && event.state == ButtonState::Pressed
                    && !event.repeat
            })
            .map(|event| PhysicalInput::Key(format!("{:?}", event.key_code)))
            .collect();
        physical.pressed.retain(|input| presses.contains(input));
        self.axes.retain(|entity, _| self.pads.contains(*entity));
        for (entity, pad) in &self.pads {
            let mut state = PhysicalState::gamepad(pad);
            self.axes.entry(entity).or_default().sample(&mut state);
            physical.merge(&state);
        }
        physical
    }
}

fn gather_host_input(
    mut devices: HostDevices,
    session: Option<Res<crate::new_game::Session>>,
    title: Option<Res<crate::saves::title::TitleMenu>>,
    preferences: Option<Res<crate::config::Preferences>>,
    battle: Option<Res<crate::battle::Owner>>,
    mut input: ResMut<HostInput>,
) {
    // Consume device edges even while recording so held inputs cannot leak on exit.
    let physical = devices.sample();
    input.0.clear();
    if battle.is_some_and(|battle| battle.capturing_arte_binding()) {
        return;
    }
    let menu = active_menu(
        session.as_ref().and_then(|session| session.menu()),
        title.as_ref().map(|title| &title.0),
    )
    .map(|(_, menu)| menu);
    if menu.is_some_and(|menu| {
        menu.page == resonance_game::menu::Page::Customize
            && (menu.customize.focus == resonance_game::menu::customize::Focus::Controls
                || menu.customize.capture.is_some())
            || menu.tech.capture.is_some()
    }) {
        return;
    }
    let party = session
        .as_ref()
        .and_then(|session| session.events().world.party.as_ref());
    let map = active_keymap(menu, party, preferences.as_deref());
    input.0.extend(
        StandardAction::HOST
            .into_iter()
            .filter(|&action| physical.edge(&map, action)),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuOwner {
    Title,
    Session,
}

/// The visible title/load menu owns input and previews over a retained session
/// menu. Accept shared or mutable references so both paths use the same policy.
pub(crate) fn active_menu<M>(session: Option<M>, title: Option<M>) -> Option<(MenuOwner, M)>
where
    M: std::ops::Deref<Target = resonance_game::menu::Menu>,
{
    title
        .filter(|menu| !menu.closed)
        .map(|menu| (MenuOwner::Title, menu))
        .or_else(|| {
            session
                .filter(|menu| !menu.closed)
                .map(|menu| (MenuOwner::Session, menu))
        })
}

/// Navigation and prompt eligibility must resolve the same map, including the
/// title's unsaved draft and the keymap editor's fixed escape/navigation keys.
pub(crate) fn active_keymap(
    menu: Option<&resonance_game::menu::Menu>,
    party: Option<&resonance_events::party::Party>,
    preferences: Option<&crate::config::Preferences>,
) -> Keymap {
    if menu.is_some_and(|menu| {
        menu.page == resonance_game::menu::Page::Customize
            && menu.customize.focus == resonance_game::menu::customize::Focus::Controls
    }) {
        return Keymap::default();
    }
    menu.and_then(|menu| menu.preferences())
        .map(|settings| &settings.keymap)
        .or_else(|| party.map(|party| &party.settings.preferences.keymap))
        .or_else(|| preferences.map(|preferences| &preferences.settings.keymap))
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn settings(
    session: Option<&crate::new_game::Session>,
    preferences: Option<&crate::config::Preferences>,
) -> resonance_content::menu_data::CustomizeSettings {
    session
        .and_then(|s| {
            s.menu().and_then(|m| m.preferences()).or_else(|| {
                s.events()
                    .world
                    .party
                    .as_ref()
                    .map(|p| &p.settings.preferences)
            })
        })
        .or_else(|| preferences.map(|p| &p.settings))
        .cloned()
        .unwrap_or_default()
}

/// Resolve each actor's most specific chord on a port. Auto actors
/// listen to port one while manual actors retain their independent controller.
pub(crate) fn resolve_arte_bindings<'a, 'b>(
    port: usize,
    held: &BTreeSet<PhysicalInput>,
    map: &Keymap,
    actors: impl IntoIterator<
        Item = (
            resonance_battle::ActorId,
            &'b resonance_battle::Actor,
            usize,
            &'a [resonance_content::input::ArteBinding],
        ),
    >,
) -> ArteResolution<'a> {
    let mut matches = Vec::new();
    let mut active = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    for (actor_id, actor, member, bindings) in actors {
        let owner = if actor.control == resonance_battle::Control::Auto {
            0
        } else {
            actor.control_slot.index()
        };
        if actor.side != resonance_battle::Side::Party || owner != port {
            continue;
        }
        for (index, binding) in bindings.iter().enumerate() {
            if binding.chord.validate(map).is_ok() && binding.chord.matches(held, map) {
                active.insert((member, index));
                if !binding.chord.matches_directions(held, map) {
                    continue;
                }
                let inputs = binding.chord.held_inputs(held, map);
                consumed.extend(inputs.iter().cloned());
                matches.push(ArteMatch {
                    actor: actor_id,
                    member,
                    index,
                    binding,
                    inputs,
                });
            }
        }
    }
    matches.sort_by_key(|m| {
        (
            std::cmp::Reverse(m.binding.chord.0.len()),
            m.member,
            m.index,
        )
    });
    let mut selected_actors = BTreeSet::new();
    let mut selected_inputs = BTreeSet::new();
    let selected = matches
        .into_iter()
        .filter(|m| {
            if selected_actors.contains(&m.actor.index()) || !m.inputs.is_disjoint(&selected_inputs)
            {
                return false;
            }
            selected_actors.insert(m.actor.index());
            selected_inputs.extend(m.inputs.iter().cloned());
            true
        })
        .collect();
    ArteResolution {
        selected,
        active,
        consumed,
    }
}
pub(crate) struct ArteResolution<'a> {
    pub selected: Vec<ArteMatch<'a>>,
    pub active: BTreeSet<(usize, usize)>,
    pub consumed: BTreeSet<PhysicalInput>,
}
pub(crate) struct ArteMatch<'a> {
    pub actor: resonance_battle::ActorId,
    pub member: usize,
    pub index: usize,
    pub binding: &'a resonance_content::input::ArteBinding,
    inputs: BTreeSet<PhysicalInput>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{ActorId, Control, ControllerPort, Side};
    use resonance_content::input::{ArteBinding, InputChord};

    #[test]
    fn title_menu_owns_bindings_and_edits_until_it_closes() {
        use resonance_game::menu::{Menu, Page};
        let mut session = Menu::new(Page::Customize, None, false);
        let mut title = Menu::new(Page::Customize, None, false);
        session.customize.draft.keymap.assign(
            StandardAction::PerformanceOverlay,
            PhysicalInput::Key("F2".into()),
        );
        title.customize.draft.keymap.assign(
            StandardAction::PerformanceOverlay,
            PhysicalInput::Key("F10".into()),
        );
        let (owner, menu) = active_menu(Some(&session), Some(&title)).unwrap();
        assert_eq!(owner, MenuOwner::Title);
        let map = active_keymap(Some(menu), None, None);
        assert_eq!(
            map.get(StandardAction::PerformanceOverlay),
            &[PhysicalInput::Key("F10".into())]
        );
        active_menu(Some(&mut session), Some(&mut title))
            .unwrap()
            .1
            .customize
            .draft
            .display_mode = resonance_content::menu_data::DisplayMode::Borderless;
        assert_eq!(
            session.customize.draft.display_mode,
            resonance_content::menu_data::DisplayMode::Windowed
        );
        assert_eq!(
            title.customize.draft.display_mode,
            resonance_content::menu_data::DisplayMode::Borderless
        );

        // A load menu still owns the screen over a retained Customize page.
        title.page = Page::Slots(resonance_game::menu::Mode::Load);
        assert_eq!(
            active_menu(Some(&session), Some(&title)).unwrap().0,
            MenuOwner::Title
        );
        title.closed = true;
        let (owner, menu) = active_menu(Some(&session), Some(&title)).unwrap();
        assert_eq!(owner, MenuOwner::Session);
        assert_eq!(
            active_keymap(Some(menu), None, None).get(StandardAction::PerformanceOverlay),
            &[PhysicalInput::Key("F2".into())]
        );
    }

    #[test]
    fn host_shortcuts_sample_primary_nonrepeat_keys_controller_buttons_and_axes_once() {
        use bevy::input::gamepad::{
            RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
        };
        #[derive(Resource, Default)]
        struct Edges([usize; 3]);
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .init_resource::<crate::config::Preferences>()
            .init_resource::<Edges>()
            .add_systems(Update, |input: Res<HostInput>, mut edges: ResMut<Edges>| {
                for (index, action) in [
                    StandardAction::PerformanceOverlay,
                    StandardAction::Pause,
                    StandardAction::SpeedToggle,
                ]
                .into_iter()
                .enumerate()
                {
                    edges.0[index] += usize::from(input.pressed(action));
                }
            });
        install(&mut app);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let other = app.world_mut().spawn_empty().id();
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        {
            let mut preferences = app.world_mut().resource_mut::<crate::config::Preferences>();
            preferences.settings.keymap.assign(
                StandardAction::PerformanceOverlay,
                PhysicalInput::Key("F2".into()),
            );
            preferences.settings.keymap.assign(
                StandardAction::Pause,
                PhysicalInput::Button("Other(9)".into()),
            );
            preferences.settings.keymap.assign(
                StandardAction::SpeedToggle,
                PhysicalInput::Axis(AxisDirection::RightRight),
            );
        }
        for (target, state, repeat) in [
            (window, ButtonState::Pressed, false),
            (window, ButtonState::Released, false),
            (window, ButtonState::Pressed, true),
            (window, ButtonState::Released, false),
            (other, ButtonState::Pressed, false),
        ] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::F2,
                logical_key: bevy::input::keyboard::Key::F2,
                state,
                text: None,
                repeat,
                window: target,
            });
            app.update();
        }
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad,
                GamepadButton::Other(9),
                1.,
            )));
        app.update();
        app.update();
        for value in [0.9, 0.9, 0., 0.9] {
            app.world_mut()
                .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                    pad,
                    GamepadAxis::RightStickX,
                    value,
                )));
            app.update();
        }
        assert_eq!(app.world().resource::<Edges>().0, [1, 1, 2]);
    }

    fn host_axis_app(direction: AxisDirection) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .init_resource::<crate::config::Preferences>();
        install(&mut app);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .keymap
            .assign(StandardAction::Pause, PhysicalInput::Axis(direction));
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        (app, pad)
    }

    fn pause_axis_frame(app: &mut App, changes: &[(Entity, GamepadAxis, f32)]) -> bool {
        use bevy::input::gamepad::{RawGamepadAxisChangedEvent, RawGamepadEvent};
        for &(pad, axis, value) in changes {
            app.world_mut()
                .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                    pad, axis, value,
                )));
        }
        app.update();
        app.world()
            .resource::<HostInput>()
            .pressed(StandardAction::Pause)
    }

    #[test]
    fn host_axis_shortcuts_require_release_before_repeating() {
        use AxisDirection::*;
        for (axis, direction, sign) in [
            (GamepadAxis::LeftStickX, LeftLeft, -1.),
            (GamepadAxis::LeftStickX, LeftRight, 1.),
            (GamepadAxis::LeftStickY, LeftDown, -1.),
            (GamepadAxis::LeftStickY, LeftUp, 1.),
            (GamepadAxis::RightStickX, RightLeft, -1.),
            (GamepadAxis::RightStickX, RightRight, 1.),
            (GamepadAxis::RightStickY, RightDown, -1.),
            (GamepadAxis::RightStickY, RightUp, 1.),
        ] {
            let (mut app, pad) = host_axis_app(direction);
            for (value, pressed) in [
                (0.9, true),
                (0.71, false),
                (0.73, false),
                (0.71, false),
                (0.73, false),
                (0.55, false), // Still tilted: stay latched even well below the press threshold.
                (0.9, false),
                (0.4, false), // Release enough to rearm without requiring exact zero.
                (0.71, false),
                (0.9, true),
                (0., false),
                (0.9, true),
            ] {
                assert_eq!(
                    pause_axis_frame(&mut app, &[(pad, axis, value * sign)]),
                    pressed,
                    "{direction:?} at {value}"
                );
            }
        }
    }

    #[test]
    fn host_axis_shortcuts_keep_controllers_independent() {
        let (mut app, a) = host_axis_app(AxisDirection::RightRight);
        let b = app.world_mut().spawn(Gamepad::default()).id();
        let axis = GamepadAxis::RightStickX;
        assert!(pause_axis_frame(&mut app, &[(a, axis, 0.9)]));
        assert!(pause_axis_frame(&mut app, &[(b, axis, 0.9)]));
        assert!(!pause_axis_frame(&mut app, &[]));
        for pad in [b, a] {
            assert!(!pause_axis_frame(&mut app, &[(pad, axis, 0.)]));
            assert!(pause_axis_frame(&mut app, &[(pad, axis, 0.9)]));
        }
        assert!(!pause_axis_frame(&mut app, &[(a, axis, 0.), (b, axis, 0.)]));
        // Simultaneous presses still resolve to one host action in this frame.
        assert!(pause_axis_frame(
            &mut app,
            &[(a, axis, 0.9), (b, axis, 0.9)]
        ));
        assert!(!pause_axis_frame(&mut app, &[]));
    }

    #[test]
    fn host_axis_shortcuts_consume_tilts_while_controls_owns_input() {
        use resonance_game::menu::{Menu, Page, customize::Focus};
        let (mut app, pad) = host_axis_app(AxisDirection::RightRight);
        let mut menu = Menu::new(Page::Customize, None, false);
        menu.customize.draft = app
            .world()
            .resource::<crate::config::Preferences>()
            .settings
            .clone();
        menu.customize.focus = Focus::Controls;
        app.insert_resource(crate::saves::title::TitleMenu(menu));
        let axis = GamepadAxis::RightStickX;
        assert!(!pause_axis_frame(&mut app, &[(pad, axis, 0.9)]));
        app.world_mut()
            .resource_mut::<crate::saves::title::TitleMenu>()
            .0
            .customize
            .focus = Focus::Options;
        for value in [0.71, 0.73, 0.9, 0.] {
            assert!(!pause_axis_frame(&mut app, &[(pad, axis, value)]));
        }
        assert!(pause_axis_frame(&mut app, &[(pad, axis, 0.9)]));
    }

    #[test]
    fn host_axis_shortcuts_forget_disconnected_controllers() {
        let (mut app, pad) = host_axis_app(AxisDirection::RightRight);
        let axis = GamepadAxis::RightStickX;
        assert!(pause_axis_frame(&mut app, &[(pad, axis, 0.9)]));
        app.world_mut().entity_mut(pad).remove::<Gamepad>();
        assert!(!pause_axis_frame(&mut app, &[]));
        app.world_mut().entity_mut(pad).insert(Gamepad::default());
        assert!(pause_axis_frame(&mut app, &[(pad, axis, 0.9)]));
    }

    #[test]
    fn digital_directions_use_vertical_priority_and_follow_remapped_movement() {
        let mut map = Keymap::default();
        map.assign(StandardAction::MoveRight, PhysicalInput::Key("KeyL".into()));
        let mut keys = ButtonInput::default();
        for (horizontal, vertical) in [
            (KeyCode::ArrowRight, KeyCode::ArrowUp),
            (KeyCode::KeyL, KeyCode::KeyW),
        ] {
            keys.reset_all();
            keys.press(horizontal);
            keys.press(vertical);
            let state = PhysicalState::keyboard(&keys);
            assert_eq!(
                state.capture_inputs(&map),
                [PhysicalInput::Key(format!("{vertical:?}"))].into()
            );
            assert!(state.down(&map, StandardAction::MoveUp));
        }
        let mut pad = Gamepad::default();
        pad.digital_mut().press(GamepadButton::DPadRight);
        pad.digital_mut().press(GamepadButton::DPadUp);
        let state = PhysicalState::gamepad(&pad);
        assert_eq!(
            state.capture_inputs(&map),
            [PhysicalInput::Button("DPadUp".into())].into()
        );
        assert!(state.down(&map, StandardAction::OrderRight));
        assert!(state.down(&map, StandardAction::OrderUp));
    }

    #[test]
    fn each_stick_contributes_one_cardinal_binding_direction() {
        use AxisDirection::*;
        let map = Keymap::default();
        for (axes, directions) in [
            (
                [GamepadAxis::LeftStickX, GamepadAxis::LeftStickY],
                [LeftLeft, LeftRight, LeftDown, LeftUp],
            ),
            (
                [GamepadAxis::RightStickX, GamepadAxis::RightStickY],
                [RightLeft, RightRight, RightDown, RightUp],
            ),
        ] {
            for (x, y, direction) in [
                (0.9, 0.8, directions[1]),
                (-0.9, 0.8, directions[0]),
                (0.8, 0.9, directions[3]),
                (0.8, -0.9, directions[2]),
                (0.8, 0.8, directions[3]),
                (-0.8, -0.8, directions[2]),
                (0.3, 0.4, directions[3]),
            ] {
                let mut pad = Gamepad::default();
                pad.analog_mut().set(axes[0], x);
                pad.analog_mut().set(axes[1], y);
                let mut state = PhysicalState::gamepad(&pad);
                state.track_edges(&mut BTreeSet::new());
                assert_eq!(
                    state.capture_inputs(&map),
                    [PhysicalInput::Axis(direction)].into()
                );
                let movement = if axes[0] == GamepadAxis::LeftStickX {
                    state.movement(&map)
                } else {
                    state.camera(&map)
                };
                assert!(movement.distance(radial_dead_zone(Vec2::new(x, y))) < 1e-6);
            }
        }
    }

    #[test]
    fn moderate_stick_tilts_record_and_resolve_combos_with_every_standard_button() {
        use AxisDirection::*;
        use resonance_game::menu::input_capture::Capture;
        let map = Keymap::default();
        let mut actor = crate::test_support::actor(Side::Party, 100, 20);
        actor.control = Control::Manual;
        actor.control_slot = ControllerPort::One;
        for (axis, value, direction) in [
            (GamepadAxis::LeftStickX, -0.35, LeftLeft),
            (GamepadAxis::LeftStickX, 0.35, LeftRight),
            (GamepadAxis::LeftStickY, -0.35, LeftDown),
            (GamepadAxis::LeftStickY, 0.35, LeftUp),
            (GamepadAxis::RightStickX, -0.35, RightLeft),
            (GamepadAxis::RightStickX, 0.35, RightRight),
            (GamepadAxis::RightStickY, -0.35, RightDown),
            (GamepadAxis::RightStickY, 0.35, RightUp),
        ] {
            for button in [
                GamepadButton::South,
                GamepadButton::East,
                GamepadButton::West,
                GamepadButton::North,
                GamepadButton::LeftTrigger,
                GamepadButton::RightTrigger,
                GamepadButton::Z,
                GamepadButton::Start,
            ] {
                let mut pad = Gamepad::default();
                pad.analog_mut().set(axis, value);
                pad.digital_mut().press(GamepadButton::South);
                let mut capture = Capture::chord(map.get(StandardAction::Confirm).iter().cloned());
                let sample = |pad: &Gamepad| PhysicalState::gamepad(pad).capture_inputs(&map);
                assert!(capture.sample(&sample(&pad)).is_none());
                pad.digital_mut().release(GamepadButton::South);
                pad.digital_mut().clear();
                // The direction stays tilted while the opener is released.
                assert!(capture.sample(&sample(&pad)).is_none());
                pad.digital_mut().press(button);
                let held_combo = sample(&pad);
                assert!(capture.sample(&held_combo).is_none());
                pad.digital_mut().release(button);
                pad.digital_mut().clear();
                assert!(capture.sample(&sample(&pad)).is_none());
                pad.analog_mut().set(axis, 0.);
                let chord = capture.sample(&sample(&pad)).unwrap();
                assert_eq!(
                    chord.0.iter().cloned().collect::<BTreeSet<_>>(),
                    [
                        PhysicalInput::Axis(direction),
                        PhysicalInput::Button(format!("{button:?}"))
                    ]
                    .into()
                );
                chord.validate(&map).unwrap();
                let binding = [ArteBinding {
                    technique: 66,
                    chord,
                }];
                let resolution = resolve_arte_bindings(
                    0,
                    &held_combo,
                    &map,
                    [(
                        ActorId::from_index(0).unwrap(),
                        &actor,
                        0,
                        binding.as_slice(),
                    )],
                );
                assert_eq!(resolution.selected.len(), 1);
                assert_eq!(resolution.selected[0].binding.technique, 66);
            }
        }
        assert!(
            InputChord(vec![PhysicalInput::Button("South".into())])
                .validate(&map)
                .is_err()
        );
    }
    #[test]
    fn longest_overlapping_chord_wins_across_characters_and_ports_are_independent() {
        let mut manual = crate::test_support::actor(Side::Party, 100, 20);
        manual.control = Control::Manual;
        manual.control_slot = ControllerPort::Three;
        let mut auto = manual.clone();
        auto.control = Control::Auto;
        let cancel = PhysicalInput::Button("East".into());
        let axis = PhysicalInput::Axis(AxisDirection::LeftRight);
        let bindings = [
            [ArteBinding {
                technique: 1,
                chord: InputChord(vec![cancel.clone()]),
            }],
            [ArteBinding {
                technique: 35,
                chord: InputChord(vec![cancel.clone(), axis.clone()]),
            }],
        ];
        let held = [cancel.clone(), axis].into();
        let sample = |port, actor| {
            resolve_arte_bindings(
                port,
                &held,
                &Keymap::default(),
                [
                    (
                        ActorId::from_index(0).unwrap(),
                        actor,
                        0,
                        bindings[0].as_slice(),
                    ),
                    (
                        ActorId::from_index(1).unwrap(),
                        actor,
                        1,
                        bindings[1].as_slice(),
                    ),
                ],
            )
        };
        assert!(sample(0, &manual).selected.is_empty());
        let resolution = sample(2, &manual);
        assert_eq!(resolution.selected.len(), 1);
        assert_eq!(resolution.selected[0].binding.technique, 35);
        assert_eq!(resolution.active.len(), 2);
        assert_eq!(resolution.consumed, held);
        assert_eq!(sample(0, &auto).selected[0].binding.technique, 35);
        assert!(sample(2, &auto).selected.is_empty());
        let single = resolve_arte_bindings(
            0,
            &[cancel].into(),
            &Keymap::default(),
            [(
                ActorId::from_index(0).unwrap(),
                &auto,
                0,
                bindings[0].as_slice(),
            )],
        );
        assert_eq!(single.selected[0].binding.technique, 1);
    }
    #[test]
    fn independent_ai_character_hotkeys_are_not_limited_to_one_per_controller() {
        let mut actor = crate::test_support::actor(Side::Party, 100, 20);
        actor.control = Control::Auto;
        actor.control_slot = ControllerPort::Four;
        let keys = [
            PhysicalInput::Key("F11".into()),
            PhysicalInput::Key("F12".into()),
        ];
        let bindings: [_; 2] = std::array::from_fn(|i| {
            [ArteBinding {
                technique: 66 + i as u16,
                chord: InputChord(vec![keys[i].clone()]),
            }]
        });
        let actors = [
            (
                ActorId::from_index(0).unwrap(),
                &actor,
                0,
                bindings[0].as_slice(),
            ),
            (
                ActorId::from_index(1).unwrap(),
                &actor,
                1,
                bindings[1].as_slice(),
            ),
        ];
        let ArteResolution {
            selected, active, ..
        } = resolve_arte_bindings(0, &keys.into(), &Keymap::default(), actors);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].binding.technique, 66);
        assert_eq!(selected[1].binding.technique, 67);
        assert_eq!(active.len(), 2);
    }
}

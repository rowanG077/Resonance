//! Device edges are latched between renders and sampled once per fixed update.
use bevy::prelude::*;
use resonance_battle::ControllerPort;
use resonance_battle::{Actor, ActorId, ButtonInput as BattleButton, Control, ControlInput, Side};
use resonance_content::{input::StandardAction, menu_data::CustomizeSettings};
use resonance_game::battle::command::{Input as CommandInput, InputKind};
use resonance_game::battle::lifecycle::QuickOrder;
use resonance_game::menu::{Input as SharedMenuInput, MenuAction};

// Battle movement uses signed controller units; navigation reads the same range.
const STICK_RANGE: f32 = 70.;
const STICK_CENTER_TOLERANCE: i8 = 16;
#[derive(Clone, Copy)]
#[repr(u8)]
enum Action {
    Attack,
    Technique,
    Guard,
    Menu,
    DelaySpell,
    Target,
    Unison,
}
const NAVIGATION: NavigationPolicy = NavigationPolicy {
    dead_zone: 40,
    initial_delay: 24,
    repeat_interval: 6,
};
struct NavigationPolicy {
    dead_zone: i8,
    initial_delay: u8,
    repeat_interval: u8,
}

#[derive(Clone, Copy, Default)]
struct DirectionRepeat {
    held: i8,
    remaining: u8,
}
#[derive(Clone, Copy, Default)]
struct Direction {
    held: i8,
    pressed: i8,
    released: i8,
    step: i8,
}
impl DirectionRepeat {
    fn sample(&mut self, held: i8) -> Direction {
        let changed = held != self.held;
        let mut sample = Direction {
            held,
            pressed: if changed { held } else { 0 },
            released: if changed { self.held } else { 0 },
            step: 0,
        };
        self.held = held;
        if changed {
            self.remaining = NAVIGATION.initial_delay;
            sample.step = held;
        } else if held != 0 {
            self.remaining = self.remaining.saturating_sub(1);
            if self.remaining == 0 {
                sample.step = held;
                self.remaining = NAVIGATION.repeat_interval;
            }
        }
        sample
    }
}
fn direction(value: i8) -> i8 {
    if value.abs() > NAVIGATION.dead_zone {
        value.signum()
    } else {
        0
    }
}

#[derive(Clone, Default)]
struct LocalControls {
    buttons: [BattleButton; 7],
    // D-pad left/right/down/up and Start are independent of battle remapping.
    extra: [BattleButton; 5],
    stick: [i8; 2],
    c_stick: [i8; 2],
    navigation: [DirectionRepeat; 6],
}
impl LocalControls {
    fn clear(&mut self) {
        for button in self.buttons.iter_mut().chain(&mut self.extra) {
            button.pressed = false;
            button.released = false;
        }
    }

    fn directions(&mut self) -> [Direction; 6] {
        let digital = [
            i8::from(self.extra[1].held) - i8::from(self.extra[0].held),
            i8::from(self.extra[3].held) - i8::from(self.extra[2].held),
        ];
        // Battle reads sticks only; shared menu navigation also accepts the D-pad.
        let held = [
            direction(self.stick[0]),
            direction(self.stick[1]),
            direction(self.c_stick[0]),
            direction(self.c_stick[1]),
            (direction(self.stick[0]) + digital[0]).signum(),
            (direction(self.stick[1]) + digital[1]).signum(),
        ];
        std::array::from_fn(|axis| self.navigation[axis].sample(held[axis]))
    }

    fn sample(&mut self, settings: &CustomizeSettings) -> ControllerSample {
        let directions = self.directions();
        let [confirm, cancel, cook, cancel_selector, ..] = self.buttons;
        let action = |logical: Action| {
            settings
                .button_map
                .iter()
                .position(|&mapped| mapped == logical as u8)
                .map_or_else(BattleButton::default, |physical| self.buttons[physical])
        };
        let sample = ControllerSample {
            attack: action(Action::Attack),
            technique: action(Action::Technique),
            guard: action(Action::Guard),
            open: action(Action::Menu),
            delay_spell: action(Action::DelaySpell),
            target: action(Action::Target),
            unison: action(Action::Unison),
            selection: self.buttons[..4].try_into().unwrap(),
            confirm,
            cancel,
            cancel_selector,
            cook: cook.pressed,
            order: [
                (1, QuickOrder::RightPreset),
                (0, QuickOrder::LeftPreset),
                (3, QuickOrder::UpperPreset),
                (2, QuickOrder::Cancel),
            ]
            .into_iter()
            .find_map(|(button, order)| self.extra[button].pressed.then_some(order)),
            menu: menu_input(&self.buttons, &self.extra, directions),
            stick: self.stick,
            pressed_direction: [directions[0].pressed, directions[1].pressed],
            step: directions[0].step,
            assist: [1, -1].map(|sign| BattleButton {
                held: directions[3].held == sign,
                pressed: directions[3].pressed == sign,
                released: directions[3].released == sign,
            }),
        };
        self.clear();
        sample
    }

    fn discard(&mut self) {
        self.directions();
        self.clear();
    }

    fn disconnected(&mut self, keyboard: Option<&Self>) {
        let previous = self.buttons;
        let previous_extra = self.extra;
        *self = keyboard.cloned().unwrap_or_default();
        for (button, previous) in self.buttons.iter_mut().zip(previous) {
            button.released |= previous.held && !button.held;
        }
        for (button, previous) in self.extra.iter_mut().zip(previous_extra) {
            button.released |= previous.held && !button.held;
        }
    }

    fn gather(&mut self, sample: Sample) {
        for (button, (down, pressed)) in self
            .buttons
            .iter_mut()
            .zip(sample.held.into_iter().zip(sample.pressed))
            .chain(
                self.extra
                    .iter_mut()
                    .zip(sample.extra_held.into_iter().zip(sample.extra_pressed)),
            )
        {
            button.pressed |= down && !button.held || pressed;
            button.released |= !down && button.held;
            button.held = down;
        }
        self.stick = quantize(sample.stick);
        self.c_stick = quantize(sample.c_stick);
    }
}

/// Resolve latched edges and repeated directions into one shared menu action.
fn menu_input(
    buttons: &[BattleButton; 7],
    extra: &[BattleButton; 5],
    directions: [Direction; 6],
) -> SharedMenuInput {
    use MenuAction::*;
    let [
        confirm,
        cancel,
        secondary,
        menu,
        left_shoulder,
        right_shoulder,
        _,
    ] = buttons;
    // Cancel wins; battle Details has priority over confirmation and navigation.
    [
        (cancel.pressed, Cancel),
        (extra[4].pressed, Details),
        (confirm.pressed, Confirm),
        (secondary.pressed, Alternate),
        (menu.pressed, Menu),
        (left_shoulder.pressed, PreviousTab),
        (right_shoulder.pressed, NextTab),
        (directions[2].step < 0, PreviousPosition),
        (directions[2].step > 0, NextPosition),
        (directions[3].step > 0, PageUp),
        (directions[3].step < 0, PageDown),
        (directions[5].step > 0, Up),
        (directions[5].step < 0, Down),
        (directions[4].step < 0, Left),
        (directions[4].step > 0, Right),
    ]
    .into_iter()
    .find_map(|(active, action)| active.then_some(action))
}

#[derive(Clone, Copy, Default)]
struct ControllerSample {
    attack: BattleButton,
    technique: BattleButton,
    guard: BattleButton,
    open: BattleButton,
    delay_spell: BattleButton,
    target: BattleButton,
    unison: BattleButton,
    selection: [BattleButton; 4],
    confirm: BattleButton,
    cancel: BattleButton,
    cancel_selector: BattleButton,
    cook: bool,
    order: Option<QuickOrder>,
    menu: SharedMenuInput,
    stick: [i8; 2],
    pressed_direction: [i8; 2],
    assist: [BattleButton; 2],
    step: i8,
}
impl ControllerSample {
    fn actor_input(
        &self,
        id: ActorId,
        actor: &Actor,
        activity: resonance_battle::Activity,
    ) -> Option<ControlInput> {
        if actor.side != Side::Party || actor.control == Control::Enemy {
            return None;
        }
        if actor.control != Control::Auto {
            return Some(ControlInput {
                actor: id,
                stick: self.stick,
                horizontal_pressed: self.pressed_direction[0],
                vertical_pressed: self.pressed_direction[1],
                attack: self.attack,
                technique: self.technique,
                assigned_arte: None,
                delay_spell: self.delay_spell,
                // Taunt is the same command when its EX skill is equipped and
                // the gauge is not full; the core applies those conditions.
                taunt: self.unison,
                guard: self.guard,
                target: self.target,
                assist: self.assist,
                target_step: self.step,
            });
        }
        let rhythm = actor.equipment.casting.rhythm
            && matches!(activity, resonance_battle::Activity::Casting { .. });
        let charge = actor.equipment.control_ex.charge;
        let jump = actor.equipment.control_ex.double_jump
            && activity == resonance_battle::Activity::Jumping
            && self.pressed_direction[1] > 0;
        if self.delay_spell == BattleButton::default()
            && !(rhythm && self.attack.pressed || charge || jump)
        {
            return None;
        }
        let mut input = ControlInput::neutral(id);
        input.delay_spell = self.delay_spell;
        input.attack.pressed = rhythm && self.attack.pressed;
        input.attack.held = charge && self.attack.held;
        if jump {
            input.vertical_pressed = self.pressed_direction[1];
        }
        Some(input)
    }
}

/// One immutable sample is shared by command, results and actor readers.
pub(super) struct Frame {
    slots: [ControllerSample; 4],
    connected: [bool; 4],
    assigned_artes: Vec<(ActorId, resonance_battle::TechniqueId)>,
}
impl Frame {
    pub fn unison_input(
        &self,
        actors: &[Actor],
        frame: Option<&resonance_battle::UnisonFrame>,
    ) -> Option<resonance_battle::UnisonInput> {
        if let Some(frame) = frame {
            let owner = actors.get(frame.initiator.index())?;
            let sample = &self.slots[owner.control_slot.index()];
            let pressed_slots = sample
                .selection
                .iter()
                .enumerate()
                .fold(0, |bits, (slot, button)| {
                    bits | (u8::from(button.pressed) << slot)
                });
            return (pressed_slots != 0)
                .then_some(resonance_battle::UnisonInput::Select { pressed_slots });
        }
        actors.iter().enumerate().find_map(|(index, actor)| {
            (actor.side == Side::Party
                && matches!(actor.control, Control::Manual | Control::SemiAuto)
                && self.slots[actor.control_slot.index()].unison.pressed)
                .then(|| {
                    resonance_battle::UnisonInput::Activate(ActorId::from_index(index).unwrap())
                })
        })
    }

    pub fn confirm(&self) -> bool {
        self.slots[0].confirm.pressed
    }

    pub fn cook(&self) -> bool {
        self.slots[0].cook
    }

    pub fn order(&self) -> Option<QuickOrder> {
        self.slots.iter().find_map(|sample| sample.order)
    }

    pub fn command_input(
        &self,
        owner: Option<ControllerPort>,
        actors: &[Actor],
        kind: InputKind,
    ) -> CommandInput {
        let controller = owner.or_else(|| {
            actors.iter().find_map(|actor| {
                let slot = actor.control_slot;
                (actor.side == Side::Party
                    && (matches!(actor.control, Control::Manual | Control::SemiAuto)
                        || slot == ControllerPort::One)
                    && self.slots[slot.index()].open.pressed)
                    .then_some(slot)
            })
        });
        let Some(controller) = controller else {
            return CommandInput {
                connected: self.connected,
                ..Default::default()
            };
        };
        let sample = self.slots[controller.index()];
        CommandInput {
            controller,
            connected: self.connected,
            open: sample.open,
            confirm_a: sample.confirm,
            cancel_b: sample.cancel,
            cancel_y: sample.cancel_selector,
            picker_directions: match sample.step {
                -1 => 1,
                1 => 2,
                _ => 0,
            },
            shared_menu: if kind == InputKind::SharedMenu {
                sample.menu
            } else {
                SharedMenuInput::default()
            },
            step: sample.step,
        }
    }

    pub fn actor_inputs<'a>(
        &self,
        actors: impl IntoIterator<Item = (ActorId, &'a Actor, resonance_battle::Activity)>,
    ) -> Vec<ControlInput> {
        actors
            .into_iter()
            .filter_map(|(id, actor, activity)| {
                let assigned = self
                    .assigned_artes
                    .iter()
                    .find(|(target, _)| *target == id)
                    .map(|(_, arte)| *arte);
                let mut input = self.slots[actor.control_slot.index()]
                    .actor_input(id, actor, activity)
                    .or_else(|| assigned.map(|_| ControlInput::neutral(id)))?;
                input.assigned_arte = assigned;
                // Physical arte assignments replace the old direction-slot command.
                input.technique = Default::default();
                input.assist = Default::default();
                Some(input)
            })
            .collect()
    }
}

/// Gamepads claim slots on controller input, then retain them through query
/// reordering and other pads disconnecting. Consumer HID devices can be exposed
/// as idle gamepads and must not reserve the leader's port.
/// Keyboard always shares the first controller slot.
#[derive(Resource, Default)]
pub(super) struct Controls {
    slots: [LocalControls; 4],
    pads: [Option<Entity>; 4],
    keyboard: LocalControls,
    assigned_artes: Vec<(ActorId, resonance_battle::TechniqueId)>,
    held_bindings: std::collections::BTreeSet<(usize, usize)>,
    consumed_inputs: [std::collections::BTreeSet<resonance_content::input::PhysicalInput>; 4],
}
impl Controls {
    pub(super) fn devices(&self) -> [Option<Entity>; 4] {
        self.pads
    }

    pub fn clear(&mut self) {
        self.keyboard.clear();
        for slot in &mut self.slots {
            slot.clear();
        }
    }

    pub fn reset(&mut self) {
        for slot in self
            .slots
            .iter_mut()
            .chain(std::iter::once(&mut self.keyboard))
        {
            slot.navigation = [DirectionRepeat::default(); 6];
            slot.discard();
        }
    }

    pub fn discard(&mut self) {
        self.assigned_artes.clear();
        self.keyboard.discard();
        for slot in &mut self.slots {
            slot.discard();
        }
    }

    pub fn sample(&mut self, settings: &CustomizeSettings) -> Frame {
        self.keyboard.discard();
        Frame {
            slots: self.slots.each_mut().map(|slot| slot.sample(settings)),
            connected: self.pads.map(|pad| pad.is_some()),
            assigned_artes: std::mem::take(&mut self.assigned_artes),
        }
    }

    /// Bound inputs take priority only in ordinary combat. Keep their held
    /// components consumed until release, so finishing a chord cannot synthesize
    /// an attack, menu press or guard from its remaining keys. Movement modifiers
    /// retain their normal values, and a context switch does not create fresh presses.
    fn route_artes(
        &mut self,
        port: usize,
        physical: crate::input::PhysicalState,
        map: &resonance_content::input::Keymap,
        resolution: crate::input::ArteResolution<'_>,
        accepting: bool,
    ) -> crate::input::PhysicalState {
        if !accepting {
            self.assigned_artes.clear();
        }
        let held = physical.capture_inputs(map);
        let consumed = &mut self.consumed_inputs[port];
        consumed.retain(|input| held.contains(input));
        if accepting {
            consumed.extend(resolution.consumed);
        }
        for selected in resolution.selected.into_iter().filter(|_| accepting) {
            if !self
                .held_bindings
                .contains(&(selected.member, selected.index))
                && !self
                    .assigned_artes
                    .iter()
                    .any(|(target, _)| *target == selected.actor)
            {
                self.assigned_artes.push((
                    selected.actor,
                    resonance_battle::TechniqueId(selected.binding.technique),
                ));
            }
        }
        let mut filtered = physical.without(consumed);
        for action in [
            StandardAction::MoveLeft,
            StandardAction::MoveRight,
            StandardAction::MoveDown,
            StandardAction::MoveUp,
        ] {
            for input in map.get(action) {
                filtered.restore_input(&physical, input);
            }
        }
        // An earlier render can have latched a normal-action edge before the
        // complete chord arrived. Cancel that pending edge too.
        for (button, action) in [
            StandardAction::Confirm,
            StandardAction::Cancel,
            StandardAction::Alternate,
            StandardAction::Menu,
            StandardAction::Previous,
            StandardAction::Next,
            StandardAction::Skit,
        ]
        .into_iter()
        .enumerate()
        {
            if map.get(action).iter().any(|i| consumed.contains(i))
                && !filtered.down(map, action)
                && !filtered.edge(map, action)
            {
                self.slots[port].buttons[button].pressed = false;
            }
        }
        for (button, action) in [
            StandardAction::OrderLeft,
            StandardAction::OrderRight,
            StandardAction::OrderCancel,
            StandardAction::OrderUp,
            StandardAction::Start,
        ]
        .into_iter()
        .enumerate()
        {
            if map.get(action).iter().any(|i| consumed.contains(i))
                && !filtered.down(map, action)
                && !filtered.edge(map, action)
            {
                self.slots[port].extra[button].pressed = false;
            }
        }
        filtered
    }

    fn assign_pads(&mut self, connected: &[Entity]) {
        for (index, pad) in self.pads.iter_mut().enumerate() {
            if pad.is_some_and(|entity| !connected.contains(&entity)) {
                *pad = None;
                self.slots[index].disconnected((index == 0).then_some(&self.keyboard));
            }
        }
        let mut incoming = connected.to_vec();
        incoming.sort_unstable_by_key(|entity| entity.to_bits());
        for entity in incoming {
            if self.pads.contains(&Some(entity)) {
                continue;
            }
            if let Some(slot) = self.pads.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(entity);
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Sample {
    held: [bool; 7],
    pressed: [bool; 7],
    extra_held: [bool; 5],
    extra_pressed: [bool; 5],
    stick: Vec2,
    c_stick: Vec2,
}
impl Sample {
    fn physical(
        physical: &crate::input::PhysicalState,
        map: &resonance_content::input::Keymap,
    ) -> Self {
        use StandardAction as A;
        let actions = [
            A::Confirm,
            A::Cancel,
            A::Alternate,
            A::Menu,
            A::Previous,
            A::Next,
            A::Skit,
        ];
        let extra = [
            A::OrderLeft,
            A::OrderRight,
            A::OrderCancel,
            A::OrderUp,
            A::Start,
        ];
        Sample {
            held: actions.map(|a| physical.down(map, a)),
            pressed: actions.map(|a| physical.edge(map, a)),
            extra_held: extra.map(|a| physical.down(map, a)),
            extra_pressed: extra.map(|a| physical.edge(map, a)),
            stick: physical.movement(map),
            c_stick: physical.camera(map),
        }
    }
}
fn quantize(stick: Vec2) -> [i8; 2] {
    stick
        .clamp(Vec2::NEG_ONE, Vec2::ONE)
        .to_array()
        .map(|value| {
            let value = (value * STICK_RANGE).round() as i8;
            if value.abs() < STICK_CENTER_TOLERANCE {
                0
            } else {
                value
            }
        })
}

pub(super) fn gather(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    mut controls: ResMut<Controls>,
    session: Option<Res<crate::new_game::Session>>,
    preferences: Option<Res<crate::config::Preferences>>,
    mut owner: Option<ResMut<super::Owner>>,
) {
    let settings = crate::input::settings(session.as_deref(), preferences.as_deref());
    let map = &settings.keymap;
    let keyboard_physical = crate::input::PhysicalState::keyboard(&keys);
    controls
        .keyboard
        .gather(Sample::physical(&keyboard_physical, map));
    let connected: Vec<_> = pads
        .iter()
        .filter(|(entity, pad)| {
            controls.pads.contains(&Some(*entity))
                || pad
                    .get_pressed()
                    .any(|button| !matches!(button, GamepadButton::Other(_)))
                || pad.left_stick().abs().max_element() > 0.3
                || pad.right_stick().abs().max_element() > 0.3
        })
        .map(|(entity, _)| entity)
        .collect();
    controls.assign_pads(&connected);
    let capture_port = owner
        .as_ref()
        .and_then(|o| {
            if let super::Phase::Scene(scene) = &o.phase {
                scene.lifecycle.command_controller()
            } else {
                None
            }
        })
        .map(|p| p.index())
        .unwrap_or(0);
    let mut capture_physical = if capture_port == 0 {
        keyboard_physical.clone()
    } else {
        Default::default()
    };
    if let Some((_, pad)) = controls.pads[capture_port].and_then(|entity| pads.get(entity).ok()) {
        capture_physical.merge(&crate::input::PhysicalState::gamepad(pad));
    }
    if let Some(owner) = &mut owner
        && let super::Phase::Scene(scene) = &mut owner.phase
        && let Some(candidate) = &mut scene.candidate
        && scene
            .lifecycle
            .capture_arte_binding(candidate, &capture_physical.capture_inputs(map))
    {
        controls.discard();
        // Discard recorder buttons as well as their pending edges.
        for slot in &mut controls.slots {
            *slot = LocalControls::default();
        }
        scene.published_tick = None;
        return;
    }
    let mut held_bindings = std::collections::BTreeSet::new();
    for index in 0..4 {
        let mut physical = if index == 0 {
            keyboard_physical.clone()
        } else {
            Default::default()
        };
        if let Some((_, pad)) = controls.pads[index].and_then(|entity| pads.get(entity).ok()) {
            physical.merge(&crate::input::PhysicalState::gamepad(pad));
        }

        if let Some(owner) = &owner
            && let super::Phase::Scene(scene) = &owner.phase
            && let Some(candidate) = &scene.candidate
        {
            // Regular arte assignments replace the fixed technique and assist slots.
            // The logical arte button still serves casting EX interactions.
            let party = candidate.persistent_party();
            let held = physical.capture_inputs(map);
            let resolution = crate::input::resolve_arte_bindings(
                index,
                &held,
                map,
                scene
                    .core
                    .actors()
                    .iter()
                    .enumerate()
                    .filter_map(|(actor_index, actor)| {
                        let character = scene.characters.get(actor_index)?;
                        let member = character.index();
                        Some((
                            ActorId::from_index(actor_index).ok()?,
                            actor,
                            member,
                            party.members[member].arte_bindings.as_slice(),
                        ))
                    }),
            );
            held_bindings.extend(resolution.active.iter().copied());
            let accepting = scene.lifecycle.command_frame().is_none()
                && scene.lifecycle.event().is_none()
                && scene.state == super::SceneState::Active
                && scene.core.phase() == resonance_battle::BattlePhase::Combat
                && !scene.core.is_paused()
                && scene
                    .frame
                    .as_ref()
                    .is_none_or(|frame| frame.unison.is_none());
            physical = controls.route_artes(index, physical, map, resolution, accepting);
        }
        controls.slots[index].gather(Sample::physical(&physical, map));
    }
    controls.held_bindings = held_bindings;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ControllerPort;

    #[test]
    fn consumer_hid_does_not_reserve_a_battle_controller_port() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .add_systems(Update, gather);
        let hid = app.world_mut().spawn(Gamepad::default()).id();
        let controller = app.world_mut().spawn(Gamepad::default()).id();
        app.update();
        assert_eq!(app.world().resource::<Controls>().devices(), [None; 4]);
        app.world_mut()
            .get_mut::<Gamepad>(hid)
            .unwrap()
            .digital_mut()
            .press(GamepadButton::Other(123));
        app.update();
        assert_eq!(app.world().resource::<Controls>().devices(), [None; 4]);

        let mut pad = app.world_mut().get_mut::<Gamepad>(controller).unwrap();
        pad.analog_mut().set(GamepadAxis::LeftStickX, 1.);
        pad.digital_mut().press(GamepadButton::South);
        app.update();
        let controls = &mut app.world_mut().resource_mut::<Controls>();
        assert_eq!(controls.devices(), [Some(controller), None, None, None]);
        let frame = controls.sample(&CustomizeSettings::default());
        let leader = actor(ControllerPort::One, Control::Manual);
        let inputs = frame.actor_inputs([(
            ActorId::from_index(0).unwrap(),
            &leader,
            resonance_battle::Activity::Idle,
        )]);
        assert_eq!(inputs[0].stick, [70, 0]);
        assert!(inputs[0].attack.pressed);

        let mut pad = app.world_mut().get_mut::<Gamepad>(controller).unwrap();
        pad.analog_mut().set(GamepadAxis::LeftStickX, 0.);
        pad.digital_mut().reset_all();
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().devices()[0],
            Some(controller)
        );
        let second = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut()
            .get_mut::<Gamepad>(second)
            .unwrap()
            .digital_mut()
            .press(GamepadButton::East);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().devices(),
            [Some(controller), Some(second), None, None]
        );
    }

    fn actor(slot: ControllerPort, control: Control) -> Actor {
        let mut actor = crate::test_support::actor(Side::Party, 100, 20);
        actor.control = control;
        actor.control_slot = slot;
        actor
    }

    fn inputs(frame: &Frame, actors: &[Actor]) -> Vec<ControlInput> {
        frame.actor_inputs(actors.iter().enumerate().map(|(index, actor)| {
            (
                ActorId::from_index(index).unwrap(),
                actor,
                resonance_battle::Activity::Idle,
            )
        }))
    }

    fn routed_frame(
        controls: &mut Controls,
        keys: &mut ButtonInput<KeyCode>,
        bindings: &[resonance_content::input::ArteBinding],
        accepting: bool,
    ) -> Frame {
        let map = resonance_content::input::Keymap::default();
        let physical = crate::input::PhysicalState::keyboard(keys);
        let actor = actor(ControllerPort::One, Control::Manual);
        let resolution = crate::input::resolve_arte_bindings(
            0,
            &physical.capture_inputs(&map),
            &map,
            [(ActorId::from_index(0).unwrap(), &actor, 0, bindings)],
        );
        let active = resolution.active.clone();
        let filtered = controls.route_artes(0, physical, &map, resolution, accepting);
        controls.slots[0].gather(Sample::physical(&filtered, &map));
        controls.held_bindings = active;
        keys.clear();
        controls.sample(&Default::default())
    }
    fn binding(keys: &[&str]) -> resonance_content::input::ArteBinding {
        use resonance_content::input::{ArteBinding, InputChord, PhysicalInput};
        ArteBinding {
            technique: 66,
            chord: InputChord(
                keys.iter()
                    .map(|key| PhysicalInput::Key((*key).into()))
                    .collect(),
            ),
        }
    }
    #[test]
    fn unbound_directions_do_not_fire_neutral_artes_even_when_released() {
        let mut controls = Controls::default();
        let mut keys = ButtonInput::default();
        let bindings = [binding(&["F11"])];
        keys.press(KeyCode::F11);
        keys.press(KeyCode::ArrowRight);
        assert!(
            routed_frame(&mut controls, &mut keys, &bindings, true)
                .assigned_artes
                .is_empty()
        );
        keys.release(KeyCode::ArrowRight);
        assert!(
            routed_frame(&mut controls, &mut keys, &bindings, true)
                .assigned_artes
                .is_empty()
        );
        keys.release(KeyCode::F11);
        routed_frame(&mut controls, &mut keys, &bindings, true);
        keys.press(KeyCode::F11);
        assert_eq!(
            routed_frame(&mut controls, &mut keys, &bindings, true)
                .assigned_artes
                .len(),
            1
        );
    }
    #[test]
    fn cancel_is_an_arte_in_combat_and_cancel_in_menus() {
        let mut controls = Controls::default();
        let mut keys = ButtonInput::default();
        let bindings = [binding(&["Escape"])];
        keys.press(KeyCode::Escape);
        let frame = routed_frame(&mut controls, &mut keys, &bindings, true);
        assert_eq!(frame.assigned_artes.len(), 1);
        assert!(!frame.slots[0].cancel.pressed);
        assert!(
            routed_frame(&mut controls, &mut keys, &bindings, true)
                .assigned_artes
                .is_empty()
        );
        let transition = routed_frame(&mut controls, &mut keys, &bindings, false);
        assert!(
            !transition.slots[0].cancel.pressed && !transition.slots[0].selection[1].pressed,
            "a held arte input must not become a fresh menu or Unison confirmation"
        );
        keys.release(KeyCode::Escape);
        routed_frame(&mut controls, &mut keys, &bindings, true);
        keys.press(KeyCode::Escape);
        let menu = routed_frame(&mut controls, &mut keys, &bindings, false);
        assert!(menu.assigned_artes.is_empty());
        assert_eq!(menu.slots[0].menu, Some(MenuAction::Cancel));
        assert!(
            menu.slots[0].selection[1].pressed,
            "Unison confirmation keeps the physical face buttons"
        );
        assert!(
            routed_frame(&mut controls, &mut keys, &bindings, true)
                .assigned_artes
                .is_empty(),
            "closing a menu while holding Cancel must not cast"
        );
    }
    #[test]
    fn protected_single_inputs_keep_their_normal_combat_actions() {
        for (key, name, action) in [
            (KeyCode::Space, "Space", StandardAction::Confirm),
            (KeyCode::KeyX, "KeyX", StandardAction::Alternate),
            (KeyCode::Tab, "Tab", StandardAction::Menu),
            (KeyCode::KeyQ, "KeyQ", StandardAction::Previous),
            (KeyCode::KeyE, "KeyE", StandardAction::Next),
            (KeyCode::KeyZ, "KeyZ", StandardAction::Skit),
            (KeyCode::Home, "Home", StandardAction::Start),
            (KeyCode::KeyD, "KeyD", StandardAction::MoveRight),
            (KeyCode::KeyW, "KeyW", StandardAction::MoveUp),
            (KeyCode::Digit1, "Digit1", StandardAction::OrderLeft),
            (KeyCode::Digit3, "Digit3", StandardAction::OrderRight),
            (KeyCode::Digit0, "Digit0", StandardAction::OrderCancel),
            (KeyCode::Digit2, "Digit2", StandardAction::OrderUp),
        ] {
            let mut keys = ButtonInput::default();
            let mut controls = Controls::default();
            keys.press(key);
            let frame = routed_frame(&mut controls, &mut keys, &[binding(&[name])], true);
            assert!(
                frame.assigned_artes.is_empty(),
                "{action:?} must not be stolen by an invalid binding"
            );
            let sample = frame.slots[0];
            assert!(
                match action {
                    StandardAction::Confirm => sample.attack.pressed,
                    StandardAction::Alternate => sample.guard.pressed,
                    StandardAction::Menu => sample.open.pressed,
                    StandardAction::Previous => sample.delay_spell.pressed,
                    StandardAction::Next => sample.target.pressed,
                    StandardAction::Skit => sample.unison.pressed,
                    StandardAction::Start => sample.menu == Some(MenuAction::Details),
                    StandardAction::MoveRight => sample.stick[0] > 0,
                    StandardAction::MoveUp => sample.stick[1] > 0,
                    _ => sample.order.is_some(),
                },
                "{action:?} lost its normal behavior"
            );
        }
    }
    #[test]
    fn combos_keep_movement_and_unrelated_actions_without_retriggering_on_release() {
        let mut controls = Controls::default();
        let mut keys = ButtonInput::default();
        let bindings = [binding(&["Space", "KeyD"])];
        keys.press(KeyCode::Space);
        // Simulate multiple renders before a fixed update: Attack is already latched.
        controls.slots[0].gather(Sample::physical(
            &crate::input::PhysicalState::keyboard(&keys),
            &Default::default(),
        ));
        keys.clear();
        keys.press(KeyCode::KeyD);
        keys.press(KeyCode::KeyX);
        let combo = routed_frame(&mut controls, &mut keys, &bindings, true);
        assert_eq!(combo.assigned_artes.len(), 1);
        assert!(!combo.slots[0].attack.pressed && !combo.slots[0].attack.held);
        assert!(
            combo.slots[0].guard.pressed,
            "unrelated Guard remains usable"
        );
        assert!(
            combo.slots[0].stick[0] > 0,
            "direction remains a movement modifier"
        );
        keys.release(KeyCode::KeyD);
        let released_modifier = routed_frame(&mut controls, &mut keys, &bindings, true);
        assert!(
            !released_modifier.slots[0].attack.pressed && !released_modifier.slots[0].attack.held
        );
        keys.release(KeyCode::Space);
        routed_frame(&mut controls, &mut keys, &bindings, true);
        keys.press(KeyCode::Space);
        assert!(
            routed_frame(&mut controls, &mut keys, &bindings, true).slots[0]
                .attack
                .pressed
        );
        keys.press(KeyCode::KeyD);
        keys.press(KeyCode::Enter);
        let alternate = routed_frame(&mut controls, &mut keys, &bindings, true);
        assert!(
            alternate.slots[0].attack.pressed,
            "a different Attack binding still works"
        );
    }
    #[test]
    fn contextual_controls_never_execute_arte_bindings_or_consume_their_buttons() {
        let mut controls = Controls::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Space);
        keys.press(KeyCode::KeyD);
        let frame = routed_frame(
            &mut controls,
            &mut keys,
            &[binding(&["Space", "KeyD"])],
            false,
        );
        assert!(frame.assigned_artes.is_empty());
        assert!(frame.slots[0].confirm.pressed);
        assert!(frame.slots[0].selection[0].pressed);
        assert!(frame.slots[0].stick[0] > 0);
    }

    #[test]
    fn shift_tab_does_not_open_battle_commands() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .add_systems(Update, gather);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::Tab);
        }
        app.update();
        assert!(
            !app.world_mut()
                .resource_mut::<Controls>()
                .sample(&Default::default())
                .slots[0]
                .open
                .pressed
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Tab);
        app.update();
        assert!(
            app.world_mut()
                .resource_mut::<Controls>()
                .sample(&Default::default())
                .slots[0]
                .open
                .pressed
        );
    }

    #[test]
    fn remapped_actions_latch_short_presses_once_and_results_keep_physical_buttons() {
        let settings = CustomizeSettings {
            button_map: [2, 5, 1, 0, 3, 4, 6],
            ..Default::default()
        };
        for physical in 0..settings.button_map.len() {
            let mut controls = Controls::default();
            let mut device = Sample::default();
            device.held[physical] = true;
            controls.slots[0].gather(device);
            controls.slots[0].gather(Sample::default());
            let frame = controls.sample(&settings);
            let sample = frame.slots[0];
            let actions = [
                sample.attack,
                sample.technique,
                sample.guard,
                sample.open,
                sample.delay_spell,
                sample.target,
                sample.unison,
            ];
            for (logical, button) in actions.iter().enumerate() {
                assert_eq!(
                    button.pressed,
                    logical == usize::from(settings.button_map[physical])
                );
                assert_eq!(button.released, button.pressed);
                assert!(!button.held);
            }
            assert_eq!(frame.confirm(), physical == 0);
            assert_eq!(frame.cook(), physical == 2);
            assert_eq!(
                frame.unison_input(&[actor(ControllerPort::One, Control::Manual)], None),
                (settings.button_map[physical] == Action::Unison as u8).then_some(
                    resonance_battle::UnisonInput::Activate(ActorId::from_index(0).unwrap())
                )
            );
            for (slot, button) in sample.selection.iter().enumerate() {
                assert_eq!(button.pressed, slot == physical);
            }
            let next = controls.sample(&settings).slots[0];
            assert!(
                [
                    next.attack,
                    next.technique,
                    next.guard,
                    next.open,
                    next.delay_spell,
                    next.target,
                    next.unison
                ]
                .iter()
                .all(|b| *b == BattleButton::default())
            );
        }
    }

    #[test]
    fn actor_and_command_readers_share_immutable_controller_samples() {
        let mut controls = Controls::default();
        let actors = [
            actor(ControllerPort::Four, Control::Manual),
            actor(ControllerPort::One, Control::SemiAuto),
            actor(ControllerPort::Three, Control::Manual),
            actor(ControllerPort::Two, Control::SemiAuto),
        ];
        for (slot, controls) in controls.slots.iter_mut().enumerate() {
            let mut device = Sample {
                stick: Vec2::new(if slot.is_multiple_of(2) { 1. } else { -1. }, 0.),
                ..Default::default()
            };
            device.held[0] = true;
            device.held[3] = true;
            controls.gather(device);
        }
        let frame = controls.sample(&CustomizeSettings::default());
        assert_eq!(
            frame
                .command_input(None, &actors, InputKind::Command)
                .controller,
            ControllerPort::Four
        );
        assert_eq!(
            frame
                .command_input(Some(ControllerPort::Two), &actors, InputKind::Command)
                .controller,
            ControllerPort::Two
        );
        for _ in 0..2 {
            let inputs = inputs(&frame, &actors);
            assert_eq!(inputs.len(), 4);
            for (input, actor) in inputs.iter().zip(&actors) {
                let sign = if actor.control_slot.index().is_multiple_of(2) {
                    1
                } else {
                    -1
                };
                assert_eq!(input.horizontal_pressed, sign);
                assert_eq!(input.target_step, sign);
                assert!(input.attack.held && input.attack.pressed);
            }
        }
        let next = controls.sample(&CustomizeSettings::default());
        assert!(
            inputs(&next, &actors)
                .iter()
                .all(|input| input.attack.held && !input.attack.pressed)
        );
        let mut changed = actors;
        changed[0].control = Control::Auto;
        changed[2].side = Side::Enemy;
        assert_eq!(inputs(&next, &changed).len(), 2);
    }

    #[test]
    fn direction_repeat_keeps_battle_sticks_separate_from_shared_menu_dpad() {
        let mut analog = LocalControls::default();
        let mut digital = LocalControls::default();
        analog.stick[0] = STICK_RANGE as i8;
        digital.extra[1].held = true;
        let settings = CustomizeSettings::default();
        assert_eq!(analog.sample(&settings).step, 1);
        let first = digital.sample(&settings);
        assert_eq!(first.step, 0);
        assert_eq!(first.menu, Some(MenuAction::Right));
        let events: Vec<_> = (0..60)
            .map(|_| {
                let a = analog.sample(&settings);
                let b = digital.sample(&settings);
                assert_eq!(a.step == 1, b.menu == Some(MenuAction::Right));
                assert_eq!(b.step, 0);
                assert_eq!(a.pressed_direction, [0; 2]);
                a.step
            })
            .collect();
        assert!(events.contains(&0));
        assert!(events.contains(&1));
        analog.extra[0].held = true;
        assert_eq!(analog.sample(&settings).menu, None);
        analog.stick[0] = 0;
        let digital_only = analog.sample(&settings);
        assert_eq!(digital_only.step, 0);
        assert_eq!(digital_only.menu, Some(MenuAction::Left));
    }

    #[test]
    fn quick_orders_latch_once_without_battle_navigation_or_hold_repeat() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .add_systems(Update, gather);
        for (key, order) in [
            (KeyCode::Digit1, QuickOrder::LeftPreset),
            (KeyCode::Digit2, QuickOrder::UpperPreset),
            (KeyCode::Digit3, QuickOrder::RightPreset),
            (KeyCode::Digit0, QuickOrder::Cancel),
        ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.update();
            app.world_mut()
                .resource_mut::<Controls>()
                .sample(&Default::default());
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            let frame = app
                .world_mut()
                .resource_mut::<Controls>()
                .sample(&Default::default());
            assert_eq!(frame.order(), Some(order));
            let actor_input = inputs(&frame, &[actor(ControllerPort::One, Control::Manual)])[0];
            assert_eq!(actor_input.stick, [0; 2]);
            assert_eq!(actor_input.horizontal_pressed, 0);
            assert_eq!(actor_input.vertical_pressed, 0);
            assert_eq!(actor_input.target_step, 0);
            assert_eq!(
                frame
                    .command_input(Some(ControllerPort::One), &[], InputKind::Command)
                    .step,
                0
            );
            for _ in 0..60 {
                assert_eq!(
                    app.world_mut()
                        .resource_mut::<Controls>()
                        .sample(&Default::default())
                        .order(),
                    None
                );
            }
        }
        let mut controls = Controls::default();
        controls.slots[3].gather(Sample {
            extra_held: [false, false, false, true, false],
            ..Default::default()
        });
        controls.slots[3].gather(Sample::default());
        assert_eq!(
            controls.sample(&Default::default()).order(),
            Some(QuickOrder::UpperPreset)
        );
        assert_eq!(controls.sample(&Default::default()).order(), None);
    }

    #[test]
    fn loading_keeps_navigation_edges_quiet_and_never_uses_fixed_assists() {
        let mut controls = Controls::default();
        controls.slots[2].gather(Sample {
            stick: Vec2::ONE,
            c_stick: Vec2::Y,
            ..Default::default()
        });
        controls.discard();
        let frame = controls.sample(&CustomizeSettings::default());
        let mut actors = [actor(ControllerPort::Three, Control::Auto)];
        assert!(inputs(&frame, &actors).is_empty());
        actors[0].control = Control::Manual;
        let manual = inputs(&frame, &actors)[0];
        assert_eq!([manual.horizontal_pressed, manual.vertical_pressed], [0; 2]);
        assert_eq!(manual.assist, [BattleButton::default(); 2]);
        controls.slots[2].gather(Sample::default());
        let released = inputs(&controls.sample(&CustomizeSettings::default()), &actors)[0];
        assert_eq!(released.assist, [BattleButton::default(); 2]);
        controls.slots[2].gather(Sample {
            c_stick: Vec2::NEG_Y,
            ..Default::default()
        });
        let pressed = inputs(&controls.sample(&CustomizeSettings::default()), &actors)[0];
        assert_eq!(pressed.assist, [BattleButton::default(); 2]);
    }

    #[test]
    fn menus_use_the_owner_and_do_not_mix_direction_or_other_pad_buttons() {
        let mut controls = Controls::default();
        controls.slots[2].gather(Sample {
            held: [true, false, false, false, false, false, false],
            stick: Vec2::ONE,
            ..Default::default()
        });
        controls.slots[0].buttons[1].pressed = true;
        let frame = controls.sample(&CustomizeSettings::default());
        let input = frame.command_input(Some(ControllerPort::Three), &[], InputKind::SharedMenu);
        assert_eq!(input.shared_menu, Some(MenuAction::Confirm));
        assert!(input.confirm_a.pressed && !input.cancel_b.pressed);
        // Simultaneous actions have a deterministic priority instead of an invalid combination.
        controls.slots[2].buttons[0].pressed = true;
        controls.slots[2].buttons[1].pressed = true;
        let frame = controls.sample(&CustomizeSettings::default());
        assert_eq!(frame.slots[2].menu, Some(MenuAction::Cancel));
        controls.slots[2].buttons[0].pressed = true;
        controls.slots[2].extra[4].pressed = true;
        assert_eq!(
            controls.sample(&CustomizeSettings::default()).slots[2].menu,
            Some(MenuAction::Details)
        );
    }

    #[test]
    fn disconnect_keeps_other_slots_and_restores_latched_keyboard_input() {
        let mut world = World::new();
        let pads: [_; 5] = std::array::from_fn(|_| world.spawn_empty().id());
        let mut controls = Controls::default();
        controls.assign_pads(&pads[..4]);
        let assigned = controls.devices();
        controls.assign_pads(&[pads[3], pads[1], pads[0], pads[2]]);
        assert_eq!(controls.devices(), assigned);
        let mut key = Sample::default();
        key.held[0] = true;
        controls.keyboard.gather(key);
        controls.keyboard.gather(Sample::default());
        controls.slots[0].buttons[1].held = true;
        controls.slots[1].buttons[2].held = true;
        // Device identity ordering is opaque; disconnect the assigned slots.
        let retained = [assigned[2].unwrap(), assigned[3].unwrap()];
        controls.assign_pads(&retained);
        let frame = controls.sample(&CustomizeSettings::default());
        assert!(frame.confirm());
        assert!(frame.slots[0].confirm.released && frame.slots[0].cancel.released);
        assert!(!frame.slots[0].cancel.held);
        assert!(!frame.connected[0] && !frame.connected[1]);
        assert_eq!(&controls.devices()[2..], &assigned[2..]);
        controls.assign_pads(&[retained[1], retained[0], pads[4]]);
        assert_eq!(controls.devices()[0], Some(pads[4]));
        controls.slots[0].gather(key);
        assert!(controls.sample(&CustomizeSettings::default()).confirm());
    }

    #[test]
    fn automatic_actors_read_only_enabled_interactions_without_consuming_manual_input() {
        use resonance_battle::{Activity, RecoilKind};
        let mut actors: [_; 6] =
            std::array::from_fn(|_| actor(ControllerPort::Three, Control::Auto));
        actors[1].equipment.casting.rhythm = true;
        actors[2].equipment.control_ex.charge = true;
        actors[3].equipment.control_ex.double_jump = true;
        actors[4].equipment.spell_revenge = true;
        actors[4].reaction.recoil.kind = RecoilKind::Down;
        actors[5].control = Control::Manual;
        let mut controls = Controls::default();
        controls.slots[2].gather(Sample {
            held: [true; 7],
            stick: Vec2::ONE,
            ..Default::default()
        });
        let frame = controls.sample(&CustomizeSettings::default());
        let activities = [
            Activity::Idle,
            Activity::Casting { held: false },
            Activity::Idle,
            Activity::Jumping,
            Activity::Hurt,
            Activity::Idle,
        ];
        for reverse in [false, true] {
            let mut readers: Vec<_> = actors
                .iter()
                .enumerate()
                .map(|(i, actor)| (ActorId::from_index(i).unwrap(), actor, activities[i]))
                .collect();
            if reverse {
                readers.reverse();
            }
            let mut routed = frame.actor_inputs(readers);
            routed.sort_by_key(|input| input.actor.index());
            assert_eq!(routed.len(), actors.len());
            assert!(routed.iter().all(|input| input.delay_spell.held));
            assert_eq!(routed[0].attack, BattleButton::default());
            assert!(routed[1].attack.pressed && !routed[1].attack.held);
            assert!(routed[2].attack.held && !routed[2].attack.pressed);
            assert_eq!(routed[3].vertical_pressed, 1);
            assert_eq!(routed[3].stick, [0; 2]);
            assert_eq!(routed[4].technique, BattleButton::default());
            assert_eq!(routed[4].stick, [0; 2]);
            assert!(
                routed[5].attack.pressed && !routed[5].technique.pressed && routed[5].guard.pressed
            );
            assert!(
                routed[..5]
                    .iter()
                    .all(|input| !input.guard.held && !input.taunt.held && !input.target.held)
            );
        }
        controls.slots[2].gather(Sample::default());
        let released = inputs(&controls.sample(&CustomizeSettings::default()), &actors);
        assert!(released.iter().all(|input| input.delay_spell.released));
        let next = inputs(&controls.sample(&CustomizeSettings::default()), &actors);
        assert_eq!(next.len(), 2); // Charge tracks release; the manual actor remains active.
        assert!(!next[0].attack.held);
        for actor in &mut actors[..5] {
            actor.equipment.control_ex.charge = false;
        }
        assert_eq!(inputs(&frame, &actors).len(), 6); // Delay Spell remains independently readable.
        let mut no_delay = frame;
        no_delay.slots[2].delay_spell = BattleButton::default();
        assert_eq!(inputs(&no_delay, &actors).len(), 1);
    }
}

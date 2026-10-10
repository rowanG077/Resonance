//! Device edges are latched between renders and sampled once per fixed update.
use bevy::prelude::*;
use resonance_battle::{Actor, ActorId, ButtonInput as BattleButton, Control, ControlInput, Side};
use resonance_content::menu_data::CustomizeSettings;
use resonance_game::battle::command::{Input as CommandInput, InputKind};
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
    Taunt,
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
    navigation: [DirectionRepeat; 4],
}
impl LocalControls {
    fn clear(&mut self) {
        for button in self.buttons.iter_mut().chain(&mut self.extra) {
            button.pressed = false;
            button.released = false;
        }
    }

    fn directions(&mut self) -> [Direction; 4] {
        let digital = [
            i8::from(self.extra[1].held) - i8::from(self.extra[0].held),
            i8::from(self.extra[3].held) - i8::from(self.extra[2].held),
        ];
        let held = [
            (direction(self.stick[0]) + digital[0]).signum(),
            (direction(self.stick[1]) + digital[1]).signum(),
            direction(self.c_stick[0]),
            direction(self.c_stick[1]),
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
            taunt: action(Action::Taunt),
            confirm,
            cancel,
            cancel_selector,
            cook: cook.pressed,
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
    directions: [Direction; 4],
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
        (directions[1].step > 0, Up),
        (directions[1].step < 0, Down),
        (directions[0].step < 0, Left),
        (directions[0].step > 0, Right),
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
    taunt: BattleButton,
    confirm: BattleButton,
    cancel: BattleButton,
    cancel_selector: BattleButton,
    cook: bool,
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
                delay_spell: self.delay_spell,
                taunt: self.taunt,
                guard: self.guard,
                target: self.target,
                assist: self.assist,
                target_step: self.step,
            });
        }
        let revenge = actor.equipment.spell_revenge
            && activity == resonance_battle::Activity::Hurt
            && actor.reaction.recoil.kind != resonance_battle::RecoilKind::Normal;
        let rhythm = actor.equipment.casting.rhythm
            && matches!(activity, resonance_battle::Activity::Casting { .. });
        let charge = actor.equipment.control_ex.charge;
        let jump = actor.equipment.control_ex.double_jump
            && activity == resonance_battle::Activity::Jumping
            && self.pressed_direction[1] > 0;
        if self.delay_spell == BattleButton::default()
            && !(revenge && self.technique.pressed
                || rhythm && self.attack.pressed
                || charge
                || jump)
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
        if revenge {
            input.technique = self.technique;
            input.stick = self.stick;
        }
        Some(input)
    }
}

/// One immutable sample is shared by command, results and actor readers.
pub(super) struct Frame {
    slots: [ControllerSample; 4],
    connected: [bool; 4],
}
impl Frame {
    pub fn confirm(&self) -> bool {
        self.slots[0].confirm.pressed
    }

    pub fn cook(&self) -> bool {
        self.slots[0].cook
    }

    pub fn command_input(
        &self,
        owner: Option<u8>,
        actors: &[Actor],
        kind: InputKind,
    ) -> CommandInput {
        let controller = owner.filter(|&slot| slot < 4).or_else(|| {
            actors.iter().find_map(|actor| {
                let slot = actor.control_slot;
                (actor.side == Side::Party
                    && slot < 4
                    && (matches!(actor.control, Control::Manual | Control::SemiAuto) || slot == 0)
                    && self.slots[usize::from(slot)].open.pressed)
                    .then_some(slot)
            })
        });
        let Some(controller) = controller else {
            return CommandInput {
                connected: self.connected,
                ..Default::default()
            };
        };
        let sample = self.slots[usize::from(controller)];
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
                self.slots
                    .get(usize::from(actor.control_slot))?
                    .actor_input(id, actor, activity)
            })
            .collect()
    }
}

/// Gamepad slots survive query reordering and other pads disconnecting.
/// Keyboard always shares the first controller slot.
#[derive(Resource, Default)]
pub(super) struct Controls {
    slots: [LocalControls; 4],
    pads: [Option<Entity>; 4],
    keyboard: LocalControls,
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
            slot.navigation = [DirectionRepeat::default(); 4];
            slot.discard();
        }
    }

    pub fn discard(&mut self) {
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
        }
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
) {
    use GamepadButton::*;
    let physical = [South, East, West, North, LeftTrigger, RightTrigger, Z];
    let bindings = [
        &[KeyCode::Enter, KeyCode::Space][..],
        &[KeyCode::Escape][..],
        &[KeyCode::KeyX][..],
        &[KeyCode::Tab][..],
        &[KeyCode::KeyQ][..],
        &[KeyCode::KeyE][..],
        &[KeyCode::KeyZ][..],
    ];
    let axis = |positive: [KeyCode; 2], negative: [KeyCode; 2]| {
        f32::from(positive.into_iter().any(|key| keys.pressed(key)))
            - f32::from(negative.into_iter().any(|key| keys.pressed(key)))
    };
    let keyboard = Sample {
        held: bindings.map(|buttons| buttons.iter().any(|key| keys.pressed(*key))),
        pressed: bindings.map(|buttons| buttons.iter().any(|key| keys.just_pressed(*key))),
        extra_held: [false, false, false, false, keys.pressed(KeyCode::Home)],
        extra_pressed: [false, false, false, false, keys.just_pressed(KeyCode::Home)],
        stick: Vec2::new(
            axis(
                [KeyCode::ArrowRight, KeyCode::KeyD],
                [KeyCode::ArrowLeft, KeyCode::KeyA],
            ),
            axis(
                [KeyCode::ArrowUp, KeyCode::KeyW],
                [KeyCode::ArrowDown, KeyCode::KeyS],
            ),
        ),
        c_stick: Vec2::new(
            axis([KeyCode::BracketRight; 2], [KeyCode::BracketLeft; 2]),
            axis([KeyCode::PageUp; 2], [KeyCode::PageDown; 2]),
        ),
    };
    let connected: Vec<_> = pads.iter().map(|(entity, _)| entity).collect();
    controls.keyboard.gather(keyboard);
    controls.assign_pads(&connected);
    for index in 0..4 {
        let pad = controls.pads[index]
            .and_then(|entity| pads.get(entity).ok())
            .map(|(_, pad)| pad);
        let mut sample = if index == 0 {
            keyboard
        } else {
            Sample::default()
        };
        if let Some(pad) = pad {
            for (index, button) in physical.into_iter().enumerate() {
                sample.held[index] |= pad.pressed(button);
                sample.pressed[index] |= pad.just_pressed(button);
            }
            sample.stick += pad.left_stick();
            sample.c_stick += pad.right_stick();
            for (index, button) in [DPadLeft, DPadRight, DPadDown, DPadUp, Start]
                .into_iter()
                .enumerate()
            {
                sample.extra_held[index] |= pad.pressed(button);
                sample.extra_pressed[index] |= pad.just_pressed(button);
            }
        }
        controls.slots[index].gather(sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn actor(slot: u8, control: Control) -> Actor {
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
                sample.taunt,
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
            let next = controls.sample(&settings).slots[0];
            assert!(
                [
                    next.attack,
                    next.technique,
                    next.guard,
                    next.open,
                    next.delay_spell,
                    next.target,
                    next.taunt
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
            actor(3, Control::Manual),
            actor(0, Control::SemiAuto),
            actor(2, Control::Manual),
            actor(1, Control::SemiAuto),
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
            3
        );
        assert_eq!(
            frame
                .command_input(Some(1), &actors, InputKind::Command)
                .controller,
            1
        );
        for _ in 0..2 {
            let inputs = inputs(&frame, &actors);
            assert_eq!(inputs.len(), 4);
            for (input, actor) in inputs.iter().zip(&actors) {
                let sign = if actor.control_slot.is_multiple_of(2) {
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
    fn direction_repeat_is_shared_by_sticks_and_dpad_without_opposite_events() {
        let mut analog = LocalControls::default();
        let mut digital = LocalControls::default();
        analog.stick[0] = STICK_RANGE as i8;
        digital.extra[1].held = true;
        let settings = CustomizeSettings::default();
        assert_eq!(analog.sample(&settings).step, 1);
        assert_eq!(digital.sample(&settings).step, 1);
        let events: Vec<_> = (0..60)
            .map(|_| {
                let a = analog.sample(&settings);
                let b = digital.sample(&settings);
                assert_eq!(a.step, b.step);
                assert_eq!(a.pressed_direction, [0; 2]);
                a.step
            })
            .collect();
        assert!(events.contains(&0));
        assert!(events.contains(&1));
        analog.extra[0].held = true;
        assert_eq!(analog.sample(&settings).step, 0);
        analog.stick[0] = 0;
        assert_eq!(analog.sample(&settings).step, -1);
    }

    #[test]
    fn loading_and_auto_control_do_not_retrigger_held_directions_or_assists() {
        let mut controls = Controls::default();
        controls.slots[2].gather(Sample {
            stick: Vec2::ONE,
            c_stick: Vec2::Y,
            ..Default::default()
        });
        controls.discard();
        let frame = controls.sample(&CustomizeSettings::default());
        let mut actors = [actor(2, Control::Auto)];
        assert!(inputs(&frame, &actors).is_empty());
        actors[0].control = Control::Manual;
        let manual = inputs(&frame, &actors)[0];
        assert_eq!([manual.horizontal_pressed, manual.vertical_pressed], [0; 2]);
        assert!(manual.assist[0].held && !manual.assist[0].pressed);
        controls.slots[2].gather(Sample::default());
        let released = inputs(&controls.sample(&CustomizeSettings::default()), &actors)[0];
        assert!(released.assist[0].released);
        controls.slots[2].gather(Sample {
            c_stick: Vec2::NEG_Y,
            ..Default::default()
        });
        let pressed = inputs(&controls.sample(&CustomizeSettings::default()), &actors)[0];
        assert!(pressed.assist[1].held && pressed.assist[1].pressed);
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
        let input = frame.command_input(Some(2), &[], InputKind::SharedMenu);
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
        let mut actors: [_; 6] = std::array::from_fn(|_| actor(2, Control::Auto));
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
            assert!(routed[4].technique.pressed);
            assert_eq!(routed[4].stick, [STICK_RANGE as i8; 2]);
            assert!(
                routed[5].attack.pressed && routed[5].technique.pressed && routed[5].guard.pressed
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

//! Translate physical devices once, then apply the user's standard action bindings.
use bevy::prelude::*;
use resonance_content::input::{AxisDirection, Keymap, PhysicalInput, StandardAction};
use std::collections::{BTreeMap, BTreeSet};

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
        let left = pad.left_stick();
        let right = pad.right_stick();
        for (axis, value) in [
            (LeftLeft, -left.x),
            (LeftRight, left.x),
            (LeftDown, -left.y),
            (LeftUp, left.y),
            (RightLeft, -right.x),
            (RightRight, right.x),
            (RightDown, -right.y),
            (RightUp, right.y),
        ] {
            if value > 0.2 {
                state.values.insert(PhysicalInput::Axis(axis), value);
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
    pub fn capture_inputs(&self) -> BTreeSet<PhysicalInput> {
        // A deliberate tilt beyond the movement dead zone is a chord component,
        // including moderate/diagonal tilts that never reach the button threshold.
        self.values
            .iter()
            .filter(|(input, value)| {
                **value
                    > if matches!(input, PhysicalInput::Axis(_)) {
                        0.2
                    } else {
                        0.65
                    }
            })
            .map(|(input, _)| input.clone())
            .chain(self.pressed.iter().cloned())
            .collect()
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
    pub fn running(&self, map: &Keymap, direction: Vec2) -> bool {
        !self.down(map, StandardAction::Walk) && direction.length() > 0.5
    }
    pub fn camera(&self, map: &Keymap) -> Vec2 {
        use StandardAction::*;
        Vec2::new(
            self.value(map, CameraRight) - self.value(map, CameraLeft),
            self.value(map, CameraUp) - self.value(map, CameraDown),
        )
    }
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
            if binding.chord.validate(map).is_ok() && binding.chord.matches(held) {
                active.insert((member, index));
                consumed.extend(binding.chord.0.iter().cloned());
                matches.push(ArteMatch {
                    actor: actor_id,
                    member,
                    index,
                    binding,
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
            if selected_actors.contains(&m.actor.index())
                || m.binding
                    .chord
                    .0
                    .iter()
                    .any(|i| selected_inputs.contains(i))
            {
                return false;
            }
            selected_actors.insert(m.actor.index());
            selected_inputs.extend(m.binding.chord.0.iter().cloned());
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{ActorId, Control, ControllerPort, Side};
    use resonance_content::input::{ArteBinding, InputChord};

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
                let sample = |pad: &Gamepad| PhysicalState::gamepad(pad).capture_inputs();
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

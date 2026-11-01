//! Device-independent physical bindings, shared by preferences, saves and menus.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "device", content = "input", rename_all = "snake_case")]
pub enum PhysicalInput {
    Key(String),
    Button(String),
    Axis(AxisDirection),
    Direction(ArteDirection),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionSource {
    Keyboard,
    DPad,
    LeftStick,
    RightStick,
}
impl DirectionSource {
    pub const ALL: [Self; 4] = [
        Self::Keyboard,
        Self::DPad,
        Self::LeftStick,
        Self::RightStick,
    ];
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionSlot {
    Left,
    Right,
    Down,
    Up,
    Horizontal,
}
impl DirectionSlot {
    fn normalized(self) -> Self {
        match self {
            Self::Left | Self::Right => Self::Horizontal,
            slot => slot,
        }
    }
    pub fn vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArteDirection {
    pub source: DirectionSource,
    pub slot: DirectionSlot,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisDirection {
    LeftLeft,
    LeftRight,
    LeftDown,
    LeftUp,
    RightLeft,
    RightRight,
    RightDown,
    RightUp,
    /// Arte-only directions. Standard movement keeps its signed axes.
    LeftHorizontal,
    RightHorizontal,
}
impl AxisDirection {
    fn arte_slot(self) -> Self {
        match self {
            Self::LeftLeft | Self::LeftRight => Self::LeftHorizontal,
            Self::RightLeft | Self::RightRight => Self::RightHorizontal,
            axis => axis,
        }
    }
}
impl PhysicalInput {
    fn same_arte_input(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Axis(a), Self::Axis(b)) => a.arte_slot() == b.arte_slot(),
            _ => self == other,
        }
    }
    pub fn keyboard(&self) -> bool {
        matches!(
            self,
            Self::Key(_)
                | Self::Direction(ArteDirection {
                    source: DirectionSource::Keyboard,
                    ..
                })
        )
    }
    pub fn label(&self) -> String {
        match self {
            Self::Direction(direction) => {
                let source = match direction.source {
                    DirectionSource::Keyboard => "Keyboard",
                    DirectionSource::DPad => "D-pad",
                    DirectionSource::LeftStick => "Left stick",
                    DirectionSource::RightStick => "Right stick",
                };
                let slot = match direction.slot {
                    DirectionSlot::Horizontal => "Left/Right",
                    DirectionSlot::Left => "Left",
                    DirectionSlot::Right => "Right",
                    DirectionSlot::Up => "Up",
                    DirectionSlot::Down => "Down",
                };
                format!("{source} {slot}")
            }
            Self::Key(key) => key.strip_prefix("Key").unwrap_or(key).to_owned(),
            Self::Button(button) => format!("Pad {button}"),
            Self::Axis(axis) => match axis {
                AxisDirection::LeftLeft => "Left stick Left",
                AxisDirection::LeftRight => "Left stick Right",
                AxisDirection::LeftDown => "Left stick Down",
                AxisDirection::LeftUp => "Left stick Up",
                AxisDirection::RightLeft => "Right stick Left",
                AxisDirection::RightRight => "Right stick Right",
                AxisDirection::RightDown => "Right stick Down",
                AxisDirection::RightUp => "Right stick Up",
                AxisDirection::LeftHorizontal => "Left stick Left/Right",
                AxisDirection::RightHorizontal => "Right stick Left/Right",
            }
            .into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum StandardAction {
    Confirm,
    Cancel,
    Alternate,
    Menu,
    Previous,
    Next,
    Skit,
    Start,
    MoveLeft,
    MoveRight,
    MoveDown,
    MoveUp,
    OrderLeft,
    OrderRight,
    OrderCancel,
    OrderUp,
    CameraLeft,
    CameraRight,
    CameraDown,
    CameraUp,
    #[serde(alias = "run")]
    Walk,
    Perspective,
}
impl StandardAction {
    pub const ALL: [Self; 22] = [
        Self::Confirm,
        Self::Cancel,
        Self::Alternate,
        Self::Menu,
        Self::Previous,
        Self::Next,
        Self::Skit,
        Self::Start,
        Self::MoveLeft,
        Self::MoveRight,
        Self::MoveDown,
        Self::MoveUp,
        Self::OrderLeft,
        Self::OrderRight,
        Self::OrderCancel,
        Self::OrderUp,
        Self::CameraLeft,
        Self::CameraRight,
        Self::CameraDown,
        Self::CameraUp,
        Self::Walk,
        Self::Perspective,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Confirm => "Confirm / Attack",
            Self::Cancel => "Cancel",
            Self::Alternate => "Ring / Guard",
            Self::Menu => "Menu",
            Self::Previous => "Previous / Delay Spell",
            Self::Next => "Next / Target",
            Self::Skit => "Skit / Unison",
            Self::Start => "Pause / Details",
            Self::MoveLeft => "Move Left",
            Self::MoveRight => "Move Right",
            Self::MoveDown => "Move Down",
            Self::MoveUp => "Move Up",
            Self::OrderLeft => "Navigate / Order Left",
            Self::OrderRight => "Navigate / Order Right",
            Self::OrderCancel => "Navigate Down / Cancel Orders",
            Self::OrderUp => "Navigate / Order Up",
            Self::CameraLeft => "Camera / Preview Left",
            Self::CameraRight => "Camera / Preview Right",
            Self::CameraDown => "Camera / Page Down",
            Self::CameraUp => "Camera / Page Up",
            Self::Walk => "Walk",
            Self::Perspective => "Change Perspective",
        }
    }
    /// Actions owned by ordinary combat. Cancel, field/camera actions and
    /// menu navigation retain their meanings only in their respective contexts.
    pub fn combat_action(self) -> Option<&'static str> {
        Some(match self {
            Self::Confirm => "Attack",
            Self::Alternate => "Guard",
            Self::Menu => "Battle Menu",
            Self::Previous => "Delay Spell",
            Self::Next => "Target",
            Self::Skit => "Unison / Taunt",
            Self::Start => "Pause",
            Self::MoveLeft => "Move Left",
            Self::MoveRight => "Move Right",
            Self::MoveDown => "Move Down",
            Self::MoveUp => "Move Up / Jump",
            Self::OrderLeft => "Quick Order Left",
            Self::OrderRight => "Quick Order Right",
            Self::OrderCancel => "Cancel Orders",
            Self::OrderUp => "Quick Order Up",
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    /// Alternatives for each standard action. Keyboard and controller bindings coexist.
    pub bindings: Vec<Vec<PhysicalInput>>,
}
impl Default for Keymap {
    fn default() -> Self {
        use AxisDirection::*;
        use PhysicalInput::*;
        let keys = |names: &[&str]| names.iter().map(|s| Key((*s).into())).collect::<Vec<_>>();
        let mut bindings = vec![
            keys(&["Enter", "Space"]),
            keys(&["Escape"]),
            keys(&["KeyX"]),
            keys(&["Tab"]),
            keys(&["KeyQ"]),
            keys(&["KeyE"]),
            keys(&["KeyZ"]),
            keys(&["Home"]),
            keys(&["ArrowLeft", "KeyA"]),
            keys(&["ArrowRight", "KeyD"]),
            keys(&["ArrowDown", "KeyS"]),
            keys(&["ArrowUp", "KeyW"]),
            keys(&["Digit1"]),
            keys(&["Digit3"]),
            keys(&["Digit0"]),
            keys(&["Digit2"]),
            keys(&["BracketLeft"]),
            keys(&["BracketRight"]),
            keys(&["PageDown"]),
            keys(&["PageUp"]),
            keys(&["ShiftLeft", "ShiftRight"]),
            keys(&["KeyC"]),
        ];
        for (row, button) in [
            "South",
            "East",
            "West",
            "North",
            "LeftTrigger",
            "RightTrigger",
            "Z",
            "Start",
        ]
        .into_iter()
        .enumerate()
        {
            bindings[row].push(Button(button.into()));
        }
        for (row, axis) in [
            (8, LeftLeft),
            (9, LeftRight),
            (10, LeftDown),
            (11, LeftUp),
            (16, RightLeft),
            (17, RightRight),
            (18, RightDown),
            (19, RightUp),
        ] {
            bindings[row].push(Axis(axis));
        }
        for (row, button) in [
            (12, "DPadLeft"),
            (13, "DPadRight"),
            (14, "DPadDown"),
            (15, "DPadUp"),
        ] {
            bindings[row].push(Button(button.into()));
        }
        bindings[20].push(Button("LeftTrigger2".into()));
        bindings[21].push(Button("RightThumb".into()));
        Self { bindings }
    }
}
impl Keymap {
    /// Physical directions and remapped movement keys share one classification.
    pub fn direction(&self, input: &PhysicalInput) -> Option<ArteDirection> {
        use DirectionSlot::*;
        use DirectionSource::*;
        if matches!(input, PhysicalInput::Key(_) | PhysicalInput::Button(_)) {
            if let Some(slot) = [
                (StandardAction::MoveLeft, Left),
                (StandardAction::MoveRight, Right),
                (StandardAction::MoveDown, Down),
                (StandardAction::MoveUp, Up),
            ]
            .into_iter()
            .find_map(|(action, slot)| self.get(action).contains(input).then_some(slot))
            {
                return Some(ArteDirection {
                    source: if input.keyboard() { Keyboard } else { DPad },
                    slot,
                });
            }
        }
        let (source, slot) = match input {
            PhysicalInput::Direction(direction) => return Some(*direction),
            PhysicalInput::Axis(axis) => match axis {
                AxisDirection::LeftLeft => (LeftStick, Left),
                AxisDirection::LeftRight => (LeftStick, Right),
                AxisDirection::LeftDown => (LeftStick, Down),
                AxisDirection::LeftUp => (LeftStick, Up),
                AxisDirection::LeftHorizontal => (LeftStick, Horizontal),
                AxisDirection::RightLeft => (RightStick, Left),
                AxisDirection::RightRight => (RightStick, Right),
                AxisDirection::RightDown => (RightStick, Down),
                AxisDirection::RightUp => (RightStick, Up),
                AxisDirection::RightHorizontal => (RightStick, Horizontal),
            },
            PhysicalInput::Key(key) if key.starts_with("Arrow") => (
                Keyboard,
                match key.as_str() {
                    "ArrowLeft" => Left,
                    "ArrowRight" => Right,
                    "ArrowDown" => Down,
                    "ArrowUp" => Up,
                    _ => return None,
                },
            ),
            PhysicalInput::Button(button) if button.starts_with("DPad") => (
                DPad,
                match button.as_str() {
                    "DPadLeft" => Left,
                    "DPadRight" => Right,
                    "DPadDown" => Down,
                    "DPadUp" => Up,
                    _ => return None,
                },
            ),
            _ => return None,
        };
        Some(ArteDirection { source, slot })
    }
    pub fn same_arte_input(&self, a: &PhysicalInput, b: &PhysicalInput) -> bool {
        match (self.direction(a), self.direction(b)) {
            (Some(a), Some(b)) => {
                a.source == b.source && a.slot.normalized() == b.slot.normalized()
            }
            _ => a == b,
        }
    }
    pub fn get(&self, action: StandardAction) -> &[PhysicalInput] {
        self.bindings
            .get(action as usize)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn assign(&mut self, action: StandardAction, input: PhysicalInput) {
        let previous: Vec<_> = self.bindings[action as usize]
            .iter()
            .filter(|old| old.keyboard() == input.keyboard())
            .cloned()
            .collect();
        // Exchange occupied bindings so assigning an input does not trigger two actions.
        for other in StandardAction::ALL
            .into_iter()
            .filter(|&other| other != action)
        {
            if self.bindings[other as usize].contains(&input) {
                self.bindings[other as usize].retain(|old| old.keyboard() != input.keyboard());
                self.bindings[other as usize].extend(previous.iter().cloned());
            }
        }
        let row = &mut self.bindings[action as usize];
        row.retain(|old| old.keyboard() != input.keyboard());
        row.push(input);
    }
    pub fn clear(&mut self, action: StandardAction) -> Result<(), String> {
        if matches!(
            action,
            StandardAction::Confirm
                | StandardAction::Cancel
                | StandardAction::Menu
                | StandardAction::MoveLeft
                | StandardAction::MoveRight
                | StandardAction::MoveDown
                | StandardAction::MoveUp
        ) {
            return Err(
                "This action is needed for navigation. Assign another input instead.".into(),
            );
        }
        self.bindings[action as usize].clear();
        Ok(())
    }
    pub fn combat_action(&self, input: &PhysicalInput) -> Option<&'static str> {
        StandardAction::ALL.into_iter().find_map(|action| {
            self.get(action)
                .contains(input)
                .then(|| action.combat_action())
                .flatten()
        })
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.bindings.len() == StandardAction::ALL.len(),
            "invalid keymap action count"
        );
        for row in &self.bindings {
            anyhow::ensure!(
                row.iter().collect::<BTreeSet<_>>().len() == row.len(),
                "duplicate keymap input"
            );
            for input in row {
                anyhow::ensure!(
                    !matches!(
                        input,
                        PhysicalInput::Direction(_)
                            | PhysicalInput::Axis(
                                AxisDirection::LeftHorizontal | AxisDirection::RightHorizontal
                            )
                    ),
                    "arte-only direction in standard keymap"
                );
                if let PhysicalInput::Key(name) | PhysicalInput::Button(name) = input {
                    anyhow::ensure!(
                        !name.is_empty() && name.len() <= 64,
                        "invalid physical input name"
                    );
                }
            }
        }
        Ok(())
    }
}

/// An unordered chord held together on the character's controller port.
/// Keyboard and the first controller share port one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputChord(pub Vec<PhysicalInput>);
impl InputChord {
    /// Canonical storage form for captured, assigned and loaded arte bindings.
    /// Retain duplicates so validation can still reject malformed assignments.
    pub fn normalized(mut self, keymap: &Keymap) -> Self {
        for input in &mut self.0 {
            if let Some(mut direction) = keymap.direction(input) {
                direction.slot = direction.slot.normalized();
                *input = PhysicalInput::Direction(direction);
            }
        }
        self.0.sort();
        self
    }
    pub fn same_inputs(&self, other: &Self, keymap: &Keymap) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|input| {
                other
                    .0
                    .iter()
                    .any(|other| keymap.same_arte_input(input, other))
            })
    }
    pub fn label(&self) -> String {
        self.0
            .iter()
            .map(PhysicalInput::label)
            .collect::<Vec<_>>()
            .join(" + ")
    }
    pub fn validate_shape(&self) -> Result<(), String> {
        if self.0.is_empty() {
            return Err("Press a key, button or stick direction.".into());
        }
        if self.0.iter().any(|i| matches!(i, PhysicalInput::Key(n) | PhysicalInput::Button(n) if n.is_empty() || n.len() > 64)) {
            return Err("This input is not supported. Choose another key or controller input.".into());
        }
        let key = |name: &str| self.0.contains(&PhysicalInput::Key(name.into()));
        if key("Tab") && (key("ShiftLeft") || key("ShiftRight")) {
            return Err(
                "Shift + Tab is reserved for the platform overlay. Choose another combo.".into(),
            );
        }
        use AxisDirection::*;
        for (a, b) in [
            (LeftLeft, LeftRight),
            (LeftDown, LeftUp),
            (RightLeft, RightRight),
            (RightDown, RightUp),
        ] {
            if self.0.contains(&PhysicalInput::Axis(a)) && self.0.contains(&PhysicalInput::Axis(b))
            {
                return Err(
                    "A stick cannot point in opposite directions at once. Choose another combo."
                        .into(),
                );
            }
        }
        if self.0.iter().enumerate().any(|(index, input)| {
            self.0[..index]
                .iter()
                .any(|other| input.same_arte_input(other))
        }) {
            return Err("A combo cannot repeat the same input.".into());
        }
        Ok(())
    }
    pub fn validate(&self, keymap: &Keymap) -> Result<(), String> {
        self.validate_shape()?;
        if !self.single_direction_per_source(keymap) {
            return Err("Choose one direction per input source: left/right, up, or down.".into());
        }
        let contains_key = |key: &str| {
            self.0
                .iter()
                .any(|input| keymap.same_arte_input(input, &PhysicalInput::Key(key.into())))
        };
        if contains_key("Tab") && (contains_key("ShiftLeft") || contains_key("ShiftRight")) {
            return Err(
                "Shift + Tab is reserved for the platform overlay. Choose another combo.".into(),
            );
        }
        if self.0.len() == 1
            && let Some(action) = StandardAction::ALL.into_iter().find_map(|action| {
                action.combat_action().filter(|_| {
                    keymap
                        .get(action)
                        .iter()
                        .any(|input| keymap.same_arte_input(&self.0[0], input))
                })
            })
        {
            return Err(format!(
                "This input is used for {action} in combat. Add another input to make a combo, or choose a free input."
            ));
        }
        Ok(())
    }
    pub fn single_direction_per_source(&self, keymap: &Keymap) -> bool {
        let mut seen = [false; 4];
        self.0.iter().all(|input| {
            keymap.direction(input).is_none_or(|direction| {
                !std::mem::replace(&mut seen[direction.source as usize], true)
            })
        })
    }
    pub fn matches(&self, held: &BTreeSet<PhysicalInput>, keymap: &Keymap) -> bool {
        !self.0.is_empty()
            && self
                .0
                .iter()
                .all(|input| held.iter().any(|held| keymap.same_arte_input(input, held)))
    }
    /// Reject unassigned directions without falling back to a neutral arte.
    pub fn matches_directions(&self, held: &BTreeSet<PhysicalInput>, keymap: &Keymap) -> bool {
        held.iter()
            .filter(|input| keymap.direction(input).is_some())
            .all(|input| {
                self.0
                    .iter()
                    .any(|bound| keymap.same_arte_input(bound, input))
            })
    }
    /// Consumption retains the actual physical keys, buttons and signed axes.
    pub fn held_inputs(
        &self,
        held: &BTreeSet<PhysicalInput>,
        keymap: &Keymap,
    ) -> BTreeSet<PhysicalInput> {
        held.iter()
            .filter(|input| {
                self.0
                    .iter()
                    .any(|bound| keymap.same_arte_input(bound, input))
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArteBinding {
    pub technique: u16,
    pub chord: InputChord,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_and_dpad_share_horizontal_slots_and_reject_combined_directions() {
        let mut map = Keymap::default();
        map.assign(StandardAction::MoveLeft, PhysicalInput::Key("KeyJ".into()));
        map.assign(StandardAction::MoveRight, PhysicalInput::Key("KeyL".into()));
        let button = PhysicalInput::Button("East".into());
        for (source, left, right, up) in [
            (
                DirectionSource::Keyboard,
                PhysicalInput::Key("ArrowLeft".into()),
                PhysicalInput::Key("ArrowRight".into()),
                PhysicalInput::Key("ArrowUp".into()),
            ),
            (
                DirectionSource::Keyboard,
                PhysicalInput::Key("KeyJ".into()),
                PhysicalInput::Key("KeyL".into()),
                PhysicalInput::Key("KeyW".into()),
            ),
            (
                DirectionSource::DPad,
                PhysicalInput::Button("DPadLeft".into()),
                PhysicalInput::Button("DPadRight".into()),
                PhysicalInput::Button("DPadUp".into()),
            ),
        ] {
            let left_chord = InputChord(vec![button.clone(), left.clone()]);
            let binding = left_chord.clone().normalized(&map);
            assert_eq!(
                binding.0,
                [
                    button.clone(),
                    PhysicalInput::Direction(ArteDirection {
                        source,
                        slot: DirectionSlot::Horizontal
                    })
                ]
            );
            assert!(binding.same_inputs(&InputChord(vec![button.clone(), right.clone()]), &map));
            for direction in [left, right] {
                let held = [button.clone(), direction].into();
                assert!(binding.matches(&held, &map));
                assert!(binding.matches_directions(&held, &map));
                assert_eq!(binding.held_inputs(&held, &map), held);
            }
            binding.validate(&map).unwrap();
            assert!(
                InputChord(vec![button.clone(), up.clone(), left_chord.0[1].clone()])
                    .validate(&map)
                    .is_err()
            );
            assert!(!binding.matches(&[button.clone(), up].into(), &map));
        }
    }
    #[test]
    fn arte_bindings_allow_only_one_direction_from_each_stick() {
        use AxisDirection::*;
        let chord = |a, b| {
            InputChord(vec![
                PhysicalInput::Button("East".into()),
                PhysicalInput::Axis(a),
                PhysicalInput::Axis(b),
            ])
        };
        for (horizontal, vertical) in [
            (LeftLeft, LeftUp),
            (LeftHorizontal, LeftDown),
            (RightRight, RightDown),
            (RightHorizontal, RightUp),
        ] {
            let binding = chord(horizontal, vertical);
            // The old serialized shape remains readable for save migration.
            binding.validate_shape().unwrap();
            assert!(
                binding
                    .validate(&Keymap::default())
                    .unwrap_err()
                    .contains("one direction")
            );
        }
        chord(LeftHorizontal, RightUp)
            .validate(&Keymap::default())
            .unwrap();
    }
    #[test]
    fn horizontal_artes_share_both_signs_but_keep_sticks_and_vertical_slots_separate() {
        use AxisDirection::*;
        let button = PhysicalInput::Button("East".into());
        let chord = |axis| InputChord(vec![button.clone(), PhysicalInput::Axis(axis)]);
        for (left, right, horizontal, other, up, down) in [
            (
                LeftLeft,
                LeftRight,
                LeftHorizontal,
                RightLeft,
                LeftUp,
                LeftDown,
            ),
            (
                RightLeft,
                RightRight,
                RightHorizontal,
                LeftLeft,
                RightUp,
                RightDown,
            ),
        ] {
            let binding = chord(left).normalized(&Keymap::default());
            assert_eq!(binding, chord(horizontal).normalized(&Keymap::default()));
            assert!(binding.same_inputs(&chord(right), &Keymap::default()));
            for axis in [left, right] {
                let held = [button.clone(), PhysicalInput::Axis(axis)].into();
                assert!(binding.matches(&held, &Keymap::default()));
                assert!(binding.matches_directions(&held, &Keymap::default()));
                assert_eq!(binding.held_inputs(&held, &Keymap::default()), held);
            }
            for axis in [other, up, down] {
                assert!(!binding.matches(
                    &[button.clone(), PhysicalInput::Axis(axis)].into(),
                    &Keymap::default()
                ));
            }
            // Existing signed saves stay readable and acquire shared matching.
            let old: InputChord =
                serde_json::from_str(&serde_json::to_string(&chord(left)).unwrap()).unwrap();
            assert!(old.matches(
                &[button.clone(), PhysicalInput::Axis(right)].into(),
                &Keymap::default()
            ));
            assert_eq!(old.normalized(&Keymap::default()).label(), binding.label());
            assert_eq!(
                serde_json::from_str::<InputChord>(&serde_json::to_string(&binding).unwrap())
                    .unwrap(),
                binding
            );
        }
    }
    #[test]
    fn neutral_artes_reject_unassigned_directions_including_remapped_movement() {
        let button = PhysicalInput::Button("East".into());
        let chord = InputChord(vec![button.clone()]);
        let mut map = Keymap::default();
        map.assign(StandardAction::MoveRight, PhysicalInput::Key("KeyJ".into()));
        assert!(chord.matches_directions(&[button.clone()].into(), &map));
        for direction in [
            PhysicalInput::Axis(AxisDirection::LeftRight),
            PhysicalInput::Axis(AxisDirection::RightUp),
            PhysicalInput::Key("ArrowRight".into()),
            PhysicalInput::Key("KeyJ".into()),
            PhysicalInput::Button("DPadLeft".into()),
        ] {
            let held = [button.clone(), direction].into();
            assert!(chord.matches(&held, &map));
            assert!(!chord.matches_directions(&held, &map));
        }
        assert!(chord.matches_directions(&[button, PhysicalInput::Key("F11".into())].into(), &map));
    }
    #[test]
    fn combat_controls_are_protected_but_contextual_inputs_are_available() {
        let mut map = Keymap::default();
        for action in StandardAction::ALL {
            for input in map.get(action) {
                let chord = InputChord(vec![input.clone()]);
                assert_eq!(
                    chord.validate(&map).is_err(),
                    action.combat_action().is_some(),
                    "{action:?} {input:?}"
                );
            }
        }
        let cancel = PhysicalInput::Button("East".into());
        InputChord(vec![cancel.clone()]).validate(&map).unwrap();
        map.assign(StandardAction::Confirm, cancel.clone());
        assert!(
            InputChord(vec![cancel.clone()])
                .validate(&map)
                .unwrap_err()
                .contains("Attack")
        );
        InputChord(vec![cancel, PhysicalInput::Axis(AxisDirection::LeftRight)])
            .validate(&map)
            .unwrap();
    }
    #[test]
    fn invalid_combos_explain_why_they_cannot_work() {
        use PhysicalInput::*;
        for (inputs, reason) in [
            (vec![], "Press a key"),
            (vec![Key("KeyA".into()), Key("KeyA".into())], "repeat"),
            (vec![Key("ShiftLeft".into()), Key("Tab".into())], "overlay"),
            (
                vec![
                    Axis(AxisDirection::LeftLeft),
                    Axis(AxisDirection::LeftRight),
                ],
                "opposite",
            ),
        ] {
            assert!(
                InputChord(inputs)
                    .validate(&Keymap::default())
                    .unwrap_err()
                    .contains(reason)
            );
        }
    }
    #[test]
    fn keymap_keeps_keyboard_and_controller_independent_and_roundtrips() {
        let mut map = Keymap::default();
        map.assign(StandardAction::Confirm, PhysicalInput::Key("KeyJ".into()));
        assert!(
            map.get(StandardAction::Confirm)
                .contains(&PhysicalInput::Button("South".into()))
        );
        map.validate().unwrap();
        assert_eq!(
            serde_json::from_str::<Keymap>(&serde_json::to_string(&map).unwrap()).unwrap(),
            map
        );
    }
}

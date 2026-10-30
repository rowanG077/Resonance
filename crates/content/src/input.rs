//! Device-independent physical bindings, shared by preferences, saves and menus.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "device", content = "input", rename_all = "snake_case")]
pub enum PhysicalInput {
    Key(String),
    Button(String),
    Axis(AxisDirection),
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
}
impl PhysicalInput {
    pub fn keyboard(&self) -> bool {
        matches!(self, Self::Key(_))
    }
    pub fn label(&self) -> String {
        match self {
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
    pub fn same_inputs(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().all(|input| other.0.contains(input))
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
        if self.0.iter().collect::<BTreeSet<_>>().len() != self.0.len() {
            return Err("A combo cannot repeat the same input.".into());
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
        Ok(())
    }
    pub fn validate(&self, keymap: &Keymap) -> Result<(), String> {
        self.validate_shape()?;
        if self.0.len() == 1
            && let Some(action) = keymap.combat_action(&self.0[0])
        {
            return Err(format!(
                "This input is used for {action} in combat. Add another input to make a combo, or choose a free input."
            ));
        }
        Ok(())
    }
    pub fn matches(&self, held: &BTreeSet<PhysicalInput>) -> bool {
        !self.0.is_empty() && self.0.iter().all(|input| held.contains(input))
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

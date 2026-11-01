//! Release the opener, hold a chord, then release it to finish recording.
use resonance_content::input::{InputChord, PhysicalInput};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Capture {
    armed: bool,
    inputs: BTreeSet<PhysicalInput>,
    opener: Option<BTreeSet<PhysicalInput>>,
    opener_combo: bool,
}
impl Capture {
    /// Wait only for the action that opened the recorder. Held modifiers and
    /// stick directions may already be part of the chord the player wants.
    pub fn after_opening(inputs: impl IntoIterator<Item = PhysicalInput>) -> Self {
        Self {
            opener: Some(inputs.into_iter().collect()),
            ..Default::default()
        }
    }
    /// A multi-input chord is deliberate even when it contains the button that
    /// opened the recorder. Ignore only the opener on its own.
    pub fn chord(inputs: impl IntoIterator<Item = PhysicalInput>) -> Self {
        Self {
            opener_combo: true,
            ..Self::after_opening(inputs)
        }
    }
    pub fn sample(&mut self, held: &BTreeSet<PhysicalInput>) -> Option<InputChord> {
        if !self.armed {
            self.armed = self
                .opener
                .as_ref()
                .map_or_else(|| held.is_empty(), |opener| opener.is_disjoint(held))
                || self.opener_combo && held.len() > 1;
            if !self.armed {
                return None;
            }
        }
        if held.is_empty() {
            if self.inputs.is_empty() {
                return None;
            }
            return Some(InputChord(self.inputs.iter().cloned().collect()));
        }
        // Record a chord the player actually held together. Releasing one
        // component before adding another must not create an impossible union.
        if held.len() >= self.inputs.len() {
            self.inputs.clone_from(held);
        }
        None
    }
    pub fn label(&self) -> String {
        InputChord(self.inputs.iter().cloned().collect()).label()
    }
    pub fn inputs(&self) -> impl Iterator<Item = &PhysicalInput> {
        self.inputs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn releasing_a_button_does_not_replace_a_larger_raw_chord_with_aliased_directions() {
        use resonance_content::input::AxisDirection;
        let button = PhysicalInput::Button("East".into());
        let left = PhysicalInput::Axis(AxisDirection::LeftLeft);
        let right = PhysicalInput::Axis(AxisDirection::LeftRight);
        let mut capture = Capture::chord([]);
        capture.sample(&BTreeSet::new());
        capture.sample(&[button.clone(), left.clone(), right.clone()].into());
        capture.sample(&[left.clone(), right.clone()].into());
        let chord = capture.sample(&BTreeSet::new()).unwrap();
        assert_eq!(chord.0, [button, left, right]);
    }
    #[test]
    fn standard_keymap_capture_retains_signed_stick_directions() {
        use resonance_content::input::AxisDirection;
        let left = PhysicalInput::Axis(AxisDirection::LeftLeft);
        let mut capture = Capture::after_opening([]);
        capture.sample(&BTreeSet::new());
        capture.sample(&[left.clone()].into());
        assert_eq!(capture.sample(&BTreeSet::new()).unwrap().0, [left]);
    }
    #[test]
    fn opener_is_ignored_and_chords_finish_only_after_all_components_release() {
        let opener = PhysicalInput::Key("Enter".into());
        let a = PhysicalInput::Button("Other(9)".into());
        let b = PhysicalInput::Axis(resonance_content::input::AxisDirection::RightUp);
        let mut capture = Capture::default();
        assert!(capture.sample(&[opener].into()).is_none());
        assert!(capture.sample(&BTreeSet::new()).is_none());
        assert!(capture.sample(&[a.clone()].into()).is_none());
        assert!(capture.sample(&[a.clone(), b.clone()].into()).is_none());
        assert!(capture.sample(&[b.clone()].into()).is_none());
        let chord = capture.sample(&BTreeSet::new()).unwrap();
        assert_eq!(chord.0.into_iter().collect::<BTreeSet<_>>(), [a, b].into());
    }
    #[test]
    fn rolling_inputs_record_the_last_largest_chord_that_was_actually_held() {
        let mut capture = Capture::default();
        let key = |name: &str| PhysicalInput::Key(name.into());
        capture.sample(&BTreeSet::new());
        capture.sample(&[key("KeyA"), key("KeyB")].into());
        capture.sample(&[key("KeyB")].into());
        capture.sample(&[key("KeyB"), key("KeyC")].into());
        let chord = capture.sample(&BTreeSet::new()).unwrap();
        assert_eq!(chord.0, [key("KeyB"), key("KeyC")]);
    }

    #[test]
    fn held_modifiers_are_kept_after_releasing_the_opener() {
        let enter = PhysicalInput::Key("Enter".into());
        let shift = PhysicalInput::Key("ShiftLeft".into());
        let key = PhysicalInput::Key("KeyA".into());
        let mut capture = Capture::after_opening([enter.clone()]);
        capture.sample(&[enter, shift.clone()].into());
        capture.sample(&[shift.clone()].into());
        capture.sample(&[shift.clone(), key.clone()].into());
        capture.sample(&[shift.clone()].into());
        assert_eq!(
            capture
                .sample(&BTreeSet::new())
                .unwrap()
                .0
                .into_iter()
                .collect::<BTreeSet<_>>(),
            [shift, key].into()
        );
    }

    #[test]
    fn the_opener_can_be_part_of_a_combo_without_releasing_it_first() {
        use resonance_content::input::{AxisDirection, Keymap};
        let confirm = PhysicalInput::Button("South".into());
        let direction = PhysicalInput::Axis(AxisDirection::LeftRight);
        let mut capture = Capture::chord([confirm.clone()]);
        assert!(capture.sample(&[confirm.clone()].into()).is_none());
        assert!(
            capture
                .sample(&[confirm.clone(), direction.clone()].into())
                .is_none()
        );
        assert!(capture.sample(&[confirm.clone()].into()).is_none());
        let chord = capture.sample(&BTreeSet::new()).unwrap();
        assert_eq!(
            chord.0.iter().cloned().collect::<BTreeSet<_>>(),
            [confirm, direction].into()
        );
        chord.validate(&Keymap::default()).unwrap();
    }
}

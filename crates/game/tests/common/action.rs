use resonance_events::input::Button;
use resonance_game::field::FieldInput;

#[derive(Clone, Copy)]
#[allow(dead_code)] // Each integration test uses a different subset of the controls.
pub enum Action {
    Idle,
    Accept,
    Cancel,
    Alternate,
    OpenMenu,
    Start,
    Skit,
    Up,
    Down,
    Left,
    Right,
    Next,
    Previous,
    PageUp,
    PageDown,
}

impl Action {
    pub fn input(self) -> FieldInput {
        let mut input = FieldInput::default();
        match self {
            Self::Idle => {}
            Self::Accept => input.pressed_buttons = [Button::Accept].into(),
            Self::Cancel => input.pressed_buttons = [Button::Cancel].into(),
            Self::Alternate => input.pressed_buttons = [Button::Ring].into(),
            Self::OpenMenu => input.pressed_buttons = [Button::Menu].into(),
            Self::Start => input.pressed_buttons = [Button::Start].into(),
            Self::Skit => input.pressed_buttons = [Button::Skit].into(),
            Self::Up => input.direction = [0., 1.],
            Self::Down => input.direction = [0., -1.],
            Self::Left => input.direction = [-1., 0.],
            Self::Right => input.direction = [1., 0.],
            Self::Next => input.pressed_buttons = [Button::NextPage].into(),
            Self::Previous => input.pressed_buttons = [Button::PreviousPage].into(),
            Self::PageUp => input.scroll_direction = 1,
            Self::PageDown => input.scroll_direction = -1,
        }
        input
    }
}

impl From<Action> for FieldInput {
    fn from(action: Action) -> Self {
        action.input()
    }
}

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
            Self::Accept => input.interact = true,
            Self::Cancel => input.cancel = true,
            Self::Alternate => input.alternate = true,
            Self::OpenMenu => input.menu = true,
            Self::Start => input.start = true,
            Self::Skit => input.skit = true,
            Self::Up => input.direction = [0., 1.],
            Self::Down => input.direction = [0., -1.],
            Self::Left => input.direction = [-1., 0.],
            Self::Right => input.direction = [1., 0.],
            Self::Next => input.next_page = true,
            Self::Previous => input.previous_page = true,
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

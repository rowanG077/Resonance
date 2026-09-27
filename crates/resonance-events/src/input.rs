//! Mapped field buttons, sampled once per gameplay update.
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum Button {
    Left = 0x0001,
    Right = 0x0002,
    Down = 0x0004,
    Up = 0x0008,
    Skit = 0x0010,
    NextPage = 0x0020,
    PreviousPage = 0x0040,
    Accept = 0x0100,
    Cancel = 0x0200,
    Ring = 0x0400,
    Menu = 0x0800,
    Start = 0x1000,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Buttons(u16);

impl Buttons {
    pub fn contains(self, button: Button) -> bool {
        self.0 & button as u16 != 0
    }
}

impl FromIterator<Button> for Buttons {
    fn from_iter<T: IntoIterator<Item = Button>>(buttons: T) -> Self {
        Self(
            buttons
                .into_iter()
                .fold(0, |bits, button| bits | button as u16),
        )
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    pub held: Buttons,
    pressed: Buttons,
    released: Buttons,
}

impl Input {
    /// Explicit edges preserve a tap between fixed updates and replayed actions.
    pub fn sample(&mut self, held: Buttons, pressed: Buttons) {
        let held = held.0 | pressed.0;
        self.pressed = Buttons(pressed.0 | held & !self.held.0);
        self.released = Buttons(self.held.0 & !held);
        self.held = Buttons(held);
    }

    pub(crate) fn read(&self, player: i32, mode: i32, paused: bool) -> i32 {
        const IGNORE_PAUSE: i32 = 0x8000;
        // Field control currently merges devices into player one.
        if paused && mode & IGNORE_PAUSE == 0 || (2..=4).contains(&player) {
            return 0;
        }
        i32::from(match mode & 0xf {
            0 => self.held.0,
            1 => self.pressed.0,
            2 => self.released.0,
            _ => 0,
        })
    }
}

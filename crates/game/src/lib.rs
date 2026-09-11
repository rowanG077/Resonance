//! High-level title behavior. No rendering, original RAM, or disc dependencies.
pub mod boot;
pub mod choice;
pub mod clock;
pub mod dialogue;
pub mod field;
pub mod menu;
pub mod replay;
pub mod title_events;
pub const TITLE_REVEAL_TICKS: u32 = 843;

#[derive(Default, Debug, Clone, Copy)]
pub struct MenuInput {
    pub reveal: bool,
    pub up: bool,
    pub down: bool,
    pub accept: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    NewGame,
    Load,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TitleState {
    pub tick: u32,
    pub revealed: bool,
    pub opacity: u8,
    pub selected: usize,
    pub sound_test: bool,
    pub pulse_tick: u32,
    pub expansion: [f32; 4],
}

impl Default for TitleState {
    fn default() -> Self {
        Self {
            tick: 0,
            revealed: false,
            opacity: 0,
            selected: 0,
            sound_test: false,
            pulse_tick: 90,
            expansion: [0.; 4],
        }
    }
}

impl TitleState {
    /// The original DISC1 indicator follows bit 6 of the presentation clock.
    /// It must not restart when entering the title after a movie or another scene.
    pub fn disc_label_visible(&self, clock: clock::PresentationClock) -> bool {
        clock.tick() & 64 != 0
    }
    /// Selected rows receive a second additive draw.
    pub fn pulse_alpha(&self) -> u8 {
        let intensity =
            (128. * (self.pulse_tick as f32 * std::f32::consts::PI / 180.).sin()).abs() as u8;
        (u16::from(intensity) * u16::from(self.opacity) / 255) as u8
    }

    pub fn step(&mut self, input: MenuInput) -> Option<TitleAction> {
        // Revealing the menu must not also confirm its first entry.
        let can_confirm = self.revealed && self.opacity >= 128;
        self.tick += 1;
        self.pulse_tick += 1;
        for value in &mut self.expansion {
            *value = (*value - 0.6).max(0.);
        }
        if self.revealed {
            self.opacity = self.opacity.saturating_add(8);
        }
        if self.tick >= TITLE_REVEAL_TICKS || input.reveal {
            self.revealed = true;
        }
        if self.opacity < 128 {
            return None;
        }
        let count = if self.sound_test { 4 } else { 3 };
        if input.up || input.down {
            self.selected = if input.up {
                (self.selected + count - 1) % count
            } else {
                (self.selected + 1) % count
            };
            self.pulse_tick = 90;
            self.expansion[self.selected] = 6.;
        }
        if can_confirm && input.accept {
            match self.selected {
                0 => Some(TitleAction::NewGame),
                1 => Some(TitleAction::Load),
                _ => None,
            }
        } else {
            None
        }
    }
}

/// Repeat immediately on press, then every fourth update after a 30-update hold.
#[derive(Default)]
pub struct DirectionRepeat {
    held_ticks: u8,
}

impl DirectionRepeat {
    pub fn step(&mut self, held: bool, pressed: bool, phase: u32) -> bool {
        if !held && !pressed {
            self.held_ticks = 0;
            return false;
        }
        if pressed {
            self.held_ticks = 0;
        }
        let fire = self.held_ticks == 0 || (self.held_ticks == 30 && phase.is_multiple_of(4));
        self.held_ticks = (self.held_ticks + 1).min(30);
        fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirm_requires_a_visible_menu_and_dispatches_supported_entries() {
        let accept = MenuInput {
            reveal: true,
            accept: true,
            ..Default::default()
        };
        let mut title = TitleState::default();
        assert_eq!(title.step(accept), None);
        for _ in 0..32 {
            title.step(MenuInput::default());
        }
        assert_eq!(title.step(accept), Some(TitleAction::NewGame));
        title.selected = 1;
        assert_eq!(title.step(accept), Some(TitleAction::Load));
        title.selected = 2;
        assert_eq!(title.step(accept), None);
    }
    #[test]
    fn reveal_and_wrap_visible_entries() {
        let mut title = TitleState::default();
        title.step(MenuInput {
            reveal: true,
            up: true,
            ..Default::default()
        });
        assert_eq!(title.selected, 0);
        for _ in 0..32 {
            title.step(MenuInput::default());
        }
        assert_eq!(title.opacity, 255);
        title.step(MenuInput {
            up: true,
            ..Default::default()
        });
        assert_eq!(title.selected, 2);
        title.step(MenuInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(title.selected, 0);
    }

    #[test]
    fn held_direction_waits_before_repeating_and_rearms_after_release() {
        let mut repeat = DirectionRepeat::default();
        let events: Vec<_> = (0..42)
            .filter(|tick| repeat.step(true, *tick == 0, *tick))
            .collect();
        assert_eq!(events, [0, 32, 36, 40]);
        assert!(!repeat.step(false, false, 42));
        assert!(repeat.step(true, true, 43));
    }
}

//! Presentation time continues across scene changes, independently of scene age.
//! Pure asset preparation does not present frames or advance this counter.

/// Simulation runs at the NTSC cadence of 60000/1001 updates per second.
/// Animation timestamps use a separate nominal 60 ticks per second.
pub const UPDATE_RATE_NUMERATOR: u64 = 60_000;
pub const UPDATE_RATE_DENOMINATOR: u64 = 1001;
pub const UPDATE_HZ: f64 = UPDATE_RATE_NUMERATOR as f64 / UPDATE_RATE_DENOMINATOR as f64;
/// Bevy's nanosecond clock rounds the rational field duration to the nearest ns.
pub const UPDATE_STEP: std::time::Duration = std::time::Duration::from_nanos(
    (1_000_000_000 * UPDATE_RATE_DENOMINATOR + UPDATE_RATE_NUMERATOR / 2) / UPDATE_RATE_NUMERATOR,
);

/// Play time counts presented game updates, including menus and story movies.
/// Only the total is saved; the current session starts over when loading.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlayTime {
    saved: u64,
    session: u64,
}
impl PlayTime {
    pub const fn resume(total: u64) -> Self {
        Self {
            saved: total,
            session: 0,
        }
    }
    /// Restore an observed running session without changing its total play time.
    pub const fn with_session(total: u64, session: u64) -> Option<Self> {
        if session > total {
            return None;
        }
        Some(Self {
            saved: total - session,
            session,
        })
    }
    pub fn advance(&mut self) {
        self.session = self.session.saturating_add(1);
    }
    pub fn total(self) -> u64 {
        self.saved.saturating_add(self.session)
    }
    pub fn session(self) -> u64 {
        self.session
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PresentationClock(u32);

impl PresentationClock {
    /// Restore an observed presentation counter for a development checkpoint.
    pub const fn new(tick: u32) -> Self {
        Self(tick)
    }

    pub const fn tick(self) -> u32 {
        self.0
    }

    pub fn advance(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MenuInput, TitleState};

    #[test]
    fn observed_session_preserves_total_and_resets_on_load() {
        let mut time = PlayTime::with_session(100, 40).unwrap();
        time.advance();
        assert_eq!((time.total(), time.session()), (101, 41));
        let loaded = PlayTime::resume(time.total());
        assert_eq!((loaded.total(), loaded.session()), (101, 0));
        assert!(PlayTime::with_session(100, 101).is_none());
    }

    #[test]
    fn title_entry_preserves_the_clock_including_counter_wrap() {
        let mut clock = PresentationClock::new(u32::MAX);
        clock.advance();
        assert_eq!(clock.tick(), 0);
        for _ in 0..75 {
            clock.advance();
        }
        let mut title = TitleState::default();
        assert!(title.disc_label_visible(clock));
        for _ in 0..53 {
            clock.advance();
            title.step(MenuInput::default());
        }
        assert_eq!((clock.tick(), title.tick), (128, 53));
        assert!(!title.disc_label_visible(clock));
        let next_title = TitleState::default();
        assert!(!next_title.disc_label_visible(clock));
    }
}

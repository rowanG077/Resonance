//! Temporary wind on an actor's hair and clothing.
use crate::Random;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SecondaryWind {
    pub force: [f32; 3],
    remaining: Option<u16>,
}

impl SecondaryWind {
    pub(crate) fn set(&mut self, enabled: bool, duration: u16) {
        self.remaining = enabled.then_some(duration);
    }

    pub(crate) fn advance(&mut self, random: &mut Random) {
        self.force = if let Some(remaining) = self.remaining {
            self.remaining = remaining.checked_sub(1);
            let gust = |random: &mut Random| (random.next_u32() as i32 % 20) as f32 * 0.1;
            [gust(random), 0.2, gust(random)]
        } else {
            [0.; 3]
        };
    }
}

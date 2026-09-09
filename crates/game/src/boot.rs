//! Authored startup logo timing and fades.
use serde::Serialize;

const TEXTURES: [usize; 4] = [3, 0, 1, 2];
const HOLD_UNTIL: [u32; 4] = [180, 240, 180, 120];
pub const LOGO_TICKS: u32 = 976;

#[derive(Debug, Clone, Serialize)]
pub struct Logos {
    pub tick: u32,
    pub texture: usize,
    pub alpha: u8,
    phase: usize,
    phase_tick: u32,
    alpha_step: i16,
    leaving: bool,
}

impl Default for Logos {
    fn default() -> Self {
        Self {
            tick: 0,
            texture: TEXTURES[0],
            alpha: 0,
            phase: 0,
            phase_tick: 0,
            alpha_step: 4,
            leaving: false,
        }
    }
}

impl Logos {
    pub fn active(&self) -> bool {
        self.phase < TEXTURES.len()
    }

    pub fn step(&mut self, accept: bool) {
        if !self.active() {
            return;
        }
        self.tick += 1;
        self.phase_tick += 1;
        // The original selects the old phase's texture before its transition.
        self.texture = TEXTURES[self.phase];
        if self.alpha_step == 0 {
            if self.leaving {
                self.phase += 1;
                self.phase_tick = 0;
                self.leaving = false;
                self.alpha_step = if self.active() { 4 } else { 0 };
            } else if self.phase_tick >= HOLD_UNTIL[self.phase]
                || (self.phase == 1 && self.phase_tick >= 120 && accept)
            {
                self.leaving = true;
                self.alpha_step = -4;
            }
        }
        let alpha = i16::from(self.alpha) + self.alpha_step;
        self.alpha = alpha.clamp(0, 255) as u8;
        if !(0..=255).contains(&alpha) {
            self.alpha_step = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_logos_match_the_independent_adx_checkpoint() {
        let mut logos = Logos::default();
        for _ in 0..972 {
            logos.step(false);
        }
        // Dolphin startup-1020-silent state: phase 3, local tick 180, alpha 11.
        assert_eq!((logos.texture, logos.phase_tick, logos.alpha), (2, 180, 11));
        // This savestate is inside presentation, before the counter increments.
        // Its earlier memory-card checks are intentionally outside this sequence.
        for _ in 0..4 {
            logos.step(false);
        }
        assert!(!logos.active());
        assert_eq!((logos.tick, logos.alpha), (LOGO_TICKS, 0));
        logos.step(true);
        assert_eq!(logos.tick, LOGO_TICKS);
    }

    #[test]
    fn accept_only_shortens_the_namco_hold_after_its_minimum() {
        let mut logos = Logos::default();
        while logos.active() {
            logos.step(true);
        }
        assert_eq!(logos.tick, LOGO_TICKS - 120);
    }

    #[test]
    fn phase_transition_keeps_the_previous_texture_for_its_last_draw() {
        let mut logos = Logos::default();
        for _ in 0..244 {
            logos.step(false);
        }
        assert_eq!((logos.texture, logos.alpha), (3, 4));
        logos.step(false);
        assert_eq!((logos.texture, logos.alpha), (0, 8));
    }
}

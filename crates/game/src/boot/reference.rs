// Independent pre-migration timing reference, confined to tests.

const TEXTURES: [usize; 4] = [3, 0, 1, 2];
const HOLD_UNTIL: [u32; 4] = [180, 240, 180, 120];
pub const LOGO_TICKS: u32 = 976;

#[derive(Debug, Clone)]
pub struct Reference {
    pub tick: u32,
    pub texture: usize,
    pub alpha: u8,
    phase: usize,
    phase_tick: u32,
    alpha_step: i16,
    leaving: bool,
}

impl Default for Reference {
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

impl Reference {
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

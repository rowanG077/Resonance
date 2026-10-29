//! Playback of the authored startup logo sequence.
use resonance_content::BootAssets;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Logos {
    pub tick: u32,
    pub texture: usize,
    pub alpha: u8,
    phase: usize,
    phase_tick: u32,
    fade_tick: u32,
    fade: Fade,
    #[serde(skip)]
    sequence: BootAssets,
    #[serde(skip)]
    duration: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
enum Fade {
    In,
    Hold,
    Out,
}

impl Logos {
    pub fn new(sequence: BootAssets) -> anyhow::Result<Self> {
        let duration = sequence.duration()?;
        Ok(Self {
            tick: 0,
            texture: 0,
            alpha: 0,
            phase: 0,
            phase_tick: 0,
            fade_tick: 0,
            fade: Fade::In,
            sequence,
            duration,
        })
    }

    pub fn sequence(&self) -> &BootAssets {
        &self.sequence
    }
    pub fn duration(&self) -> u32 {
        self.duration
    }
    pub fn active(&self) -> bool {
        self.phase < self.sequence.0.len()
    }

    pub fn step(&mut self, accept: bool) {
        if !self.active() {
            return;
        }
        self.tick += 1;
        self.phase_tick += 1;
        // The transition update still draws the previous logo while beginning
        // the next fade. This preserves the original presented frame order.
        self.texture = self.phase;
        let logo = &self.sequence.0[self.phase];
        match self.fade {
            Fade::Hold
                if self.phase_tick >= logo.fade_out_at
                    || (accept
                        && logo
                            .skip_after
                            .is_some_and(|after| self.phase_tick >= after)) =>
            {
                self.fade = Fade::Out;
                self.fade_tick = 0;
            }
            Fade::Out if self.fade_tick == logo.fade_ticks => {
                self.phase += 1;
                self.phase_tick = 0;
                self.fade_tick = 0;
                self.fade = Fade::In;
                if !self.active() {
                    return;
                }
            }
            _ => {}
        }
        let fade_ticks = self.sequence.0[self.phase].fade_ticks;
        match self.fade {
            Fade::In | Fade::Out => {
                self.fade_tick += 1;
                let amount =
                    (u64::from(self.fade_tick) * 256 / u64::from(fade_ticks)).min(255) as u8;
                self.alpha = match self.fade {
                    Fade::In => amount,
                    _ => 255 - amount,
                };
                if matches!(self.fade, Fade::In) && self.fade_tick == fade_ticks {
                    self.fade = Fade::Hold;
                }
            }
            Fade::Hold => self.alpha = 255,
        }
    }
}

#[cfg(test)]
mod tests;

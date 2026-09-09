//! Group volume fades, advanced after the first control update in each block.
use anyhow::{Result, ensure};

#[derive(Clone, Copy)]
pub struct Fade {
    value: f32,
    previous: f32,
    target: f32,
    progress: f32,
    step: f32,
}

impl Fade {
    /// Convert seven-bit volume using a single-precision reciprocal.
    pub fn to_control(initial: f32, amount: u8, duration_ms: u16) -> Result<Self> {
        ensure!(amount <= 127, "invalid group volume control");
        Self::new(initial, f32::from(amount) * (1.0 / 127.0), duration_ms)
    }

    pub fn new(initial: f32, target: f32, duration_ms: u16) -> Result<Self> {
        ensure!(
            [initial, target]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
            "invalid group volume"
        );
        Ok(Self {
            value: if duration_ms == 0 { target } else { initial },
            previous: initial,
            target,
            progress: if duration_ms == 0 { 0.0 } else { 1.0 },
            step: if duration_ms == 0 {
                0.0
            } else {
                1280.0 / (f32::from(duration_ms) * 256.0)
            },
        })
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn advance_block(&mut self) {
        if self.progress > 0.0 {
            // Preserve single-precision rounding and the original update order.
            // The original function explicitly disables multiply-add contraction.
            let delta = self.target - self.previous;
            let weighted = self.progress * delta;
            self.value = self.target - weighted;
            self.progress -= self.step;
            if self.progress <= 0.0 {
                self.value = self.target;
            }
        }
    }
}

/// Two group fades sampled before each millisecond's voice controls.
pub struct Startup {
    master: Fade,
    sequence: Fade,
}

impl Startup {
    pub fn new(master_ms: u16, sequence_ms: u16, lead_ms: u16) -> anyhow::Result<Self> {
        anyhow::ensure!(
            lead_ms.is_multiple_of(5),
            "master fade lead must align to a five-ms block"
        );
        let mut master = Fade::new(0.0, 1.0, master_ms)?;
        let sequence = Fade::new(0.0, 1.0, sequence_ms)?;
        for _ in 0..lead_ms / 5 {
            master.advance_block();
        }
        Ok(Self { master, sequence })
    }

    pub fn value_at(&mut self, frame: u64) -> f32 {
        let value = self.sequence.value() * self.master.value();
        if frame.is_multiple_of(160) {
            self.master.advance_block();
            self.sequence.advance_block();
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_millisecond_fade_keeps_first_step_and_exact_endpoint() {
        let mut fade = Fade::new(0.0, 1.0, 100).unwrap();
        assert_eq!(fade.value(), 0.0);
        fade.advance_block();
        assert_eq!(fade.value(), 0.0);
        fade.advance_block();
        assert_eq!(fade.value(), 1.0 - 0.95_f32);
        for _ in 2..20 {
            fade.advance_block();
        }
        assert_eq!(fade.value(), 1.0);
        fade.advance_block();
        assert_eq!(fade.value(), 1.0);
        assert_eq!(Fade::new(1.0, 0.25, 0).unwrap().value(), 0.25);
        assert!(Fade::new(f32::NAN, 1.0, 20).is_err());
    }

    #[test]
    fn classroom_music_fade_matches_the_independent_original_group_state() {
        // GQSEAF lesson-three checkpoint, VI 17986, volume groups 23..30.
        // The target amount was 72, then the script requested zero over 2 s.
        let initial = Fade::to_control(1., 72, 0).unwrap().value();
        assert_eq!(initial, 0.566_929_1);
        let mut fade = Fade::to_control(initial, 0, 2000).unwrap();
        for _ in 0..241 {
            fade.advance_block();
        }
        assert_eq!(fade.value(), 0.226_771_97);
        assert_eq!(fade.progress, 0.397_500_57);
        // Single-precision accumulation needs the 401st update to cross
        // zero for this duration, as in the original envelope controller.
        for _ in 241..401 {
            fade.advance_block();
        }
        assert_eq!(fade.value(), 0.);
        assert!(Fade::to_control(1., 128, 10).is_err());
    }
}

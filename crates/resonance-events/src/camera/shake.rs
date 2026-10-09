//! Native screen-space shake: a hold followed by optional linear decay.
#[derive(Debug, Clone, Copy, Default)]
enum Hold {
    #[default]
    Indefinite,
    Ticks(u32),
}

#[derive(Debug, Clone, Default)]
pub struct Shake {
    amount: f32,
    decay: f32,
    hold: Hold,
    /// Translation of the view matrix along screen right/up, in scene units.
    pub offset: [f32; 2],
}
impl Shake {
    pub fn new(amount: f32, decay_ticks: u16, hold_ticks: i32) -> Self {
        Self {
            amount,
            decay: if decay_ticks == 0 {
                0.
            } else {
                amount / f32::from(decay_ticks)
            },
            hold: if hold_ticks <= 0 {
                Hold::Indefinite
            } else {
                Hold::Ticks(hold_ticks as u32)
            },
            offset: [0.; 2],
        }
    }
    /// Commands change the envelope without erasing this update's sampled view.
    pub fn configure(&mut self, amount: f32, decay_ticks: u16, hold_ticks: i32) {
        *self = Self {
            offset: self.offset,
            ..Self::new(amount, decay_ticks, hold_ticks)
        };
    }
    pub(crate) fn step(&mut self, random: &mut u32) {
        self.offset = [0.; 2];
        if self.amount.trunc() == 0. {
            return;
        }
        if matches!(self.hold, Hold::Ticks(0)) && self.decay == 0. {
            self.amount = 0.;
            return;
        }
        let vertical = crate::world::random(random) as f32 % self.amount - self.amount * 0.5;
        let horizontal = crate::world::random(random) as f32 % self.amount - self.amount * 0.5;
        self.offset = [horizontal, vertical];
        if let Hold::Ticks(remaining) = &mut self.hold {
            if *remaining > 0 {
                *remaining -= 1;
            } else {
                self.amount = (self.amount - self.decay).max(0.);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_holds_then_decays_and_stops_consuming_randomness() {
        let mut shake = Shake::new(12., 3, 2);
        let mut random = 1;
        for envelope in [12., 12., 12., 8., 4.] {
            let before = random;
            shake.step(&mut random);
            assert_ne!(random, before);
            assert!(shake.offset.into_iter().all(|v| v.abs() <= envelope * 0.5));
        }
        let before = random;
        shake.step(&mut random);
        assert_eq!(shake.offset, [0.; 2]);
        assert_eq!(random, before);

        let mut shake = Shake::new(10., 0, 1);
        shake.step(&mut random);
        let before = random;
        shake.step(&mut random);
        assert_eq!(shake.offset, [0.; 2]);
        assert_eq!(random, before);
        let mut shake = Shake::new(10., 0, 0);
        for _ in 0..20 {
            let before = random;
            shake.step(&mut random);
            assert_ne!(random, before);
        }
        assert_eq!(shake.amount, 10.);
        let offset = shake.offset;
        shake.configure(4., 8, 8);
        assert_eq!(shake.offset, offset);
    }
}

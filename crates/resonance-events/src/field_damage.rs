//! Transient field-hazard numbers anchored to the controlled actor.
const CAPACITY: usize = 6;
const FADE_IN_STEP: u32 = 16;
const FADE_OUT_STEP: u32 = 4;
const FADE_IN_UPDATES: u32 = 16;
const HOLD_UPDATES: u32 = 38;
const FADE_OUT_UPDATES: u32 = 63;
const LIFETIME: u32 = FADE_IN_UPDATES + HOLD_UPDATES + FADE_OUT_UPDATES;
const FULL_DRIFT: f32 = 64.;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Number {
    amount: u16,
    born: u32,
}

#[derive(Default, Clone)]
pub struct DamageNumbers([Option<Number>; CAPACITY]);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub amount: u16,
    pub alpha: u8,
    pub drift: f32,
}

impl DamageNumbers {
    pub(crate) fn push(&mut self, amount: u16, tick: u32) {
        let slot = self
            .0
            .iter()
            .position(|n| n.is_none_or(|n| tick.saturating_sub(n.born) >= LIFETIME))
            .unwrap_or(0);
        self.0[slot] = Some(Number { amount, born: tick });
    }

    pub fn samples(&self, tick: u32) -> impl Iterator<Item = Sample> + '_ {
        self.0.iter().flatten().filter_map(move |n| n.sample(tick))
    }

    /// Keep only committed digits. A replacement in the same slot is a new number.
    pub fn sample_pairs<'a>(
        &'a self,
        tick: u32,
        next: Option<(&'a Self, u32)>,
    ) -> impl Iterator<Item = (Sample, Sample)> + 'a {
        self.0.iter().enumerate().filter_map(move |(slot, number)| {
            let number = number.as_ref()?;
            let from = number.sample(tick)?;
            let to =
                next.and_then(|(next, tick)| next.0[slot].filter(|n| n == number)?.sample(tick));
            Some((from, to.unwrap_or(from)))
        })
    }
}

impl Number {
    fn sample(&self, tick: u32) -> Option<Sample> {
        let age = tick.saturating_sub(self.born);
        if age >= LIFETIME {
            return None;
        }
        let (alpha, drift) = if age < FADE_IN_UPDATES {
            (((age + 1) * FADE_IN_STEP).min(u32::from(u8::MAX)), 0.)
        } else if age < FADE_IN_UPDATES + HOLD_UPDATES {
            (u32::from(u8::MAX), 0.)
        } else {
            let alpha =
                u32::from(u8::MAX) - (age - FADE_IN_UPDATES - HOLD_UPDATES + 1) * FADE_OUT_STEP;
            (alpha, FULL_DRIFT - alpha as f32 / FADE_OUT_STEP as f32)
        };
        Some(Sample {
            amount: self.amount,
            alpha: alpha as u8,
            drift,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_an_expired_number_keeps_surviving_digits_moving() {
        let mut numbers = DamageNumbers::default();
        numbers.push(7, 0);
        numbers.push(7, LIFETIME - 1);
        let mut next = numbers.clone();
        next.push(7, LIFETIME);
        let alphas: Vec<_> = numbers
            .sample_pairs(LIFETIME - 1, Some((&next, LIFETIME)))
            .map(|(a, b)| (a.alpha, b.alpha))
            .collect();
        assert_eq!(alphas, [(3, 3), (16, 32)]);
    }
}

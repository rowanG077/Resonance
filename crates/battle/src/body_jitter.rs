//! Actor-common jitter (2503C), consumed by the model's secondary-chain solver.
use crate::Random;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BodyJitter {
    pub(crate) active: bool,
    pub(crate) remaining: i16,
    acceleration: [f32; 3],
}

impl BodyJitter {
    pub(crate) fn request(&mut self, duration: i16) {
        self.active = true;
        self.remaining = duration;
    }

    /// Hurt entry and ordinary reset clear the flag, leaving the last model
    /// sample intact until composition or the next inactive common callback.
    pub(crate) fn stop(&mut self) {
        self.active = false;
    }

    pub(crate) fn advance(&mut self, random: &mut Random) {
        if !self.active {
            self.acceleration = [0.; 3];
            return;
        }
        // Original instructions25C08..25D10: 4DE7C returns signed i16; both
        // remainders truncate toward zero. Rodata15C0=0.1,1668=0.2.
        let x = f32::from((random.next() as i16) % 20) * 0.1;
        let z = f32::from((random.next() as i16) % 20) * 0.1;
        self.acceleration = [x, 0.2, z];
        // Entry zero still draws and publishes once; the following visit clears.
        if self.remaining <= 0 {
            self.active = false;
        }
        self.remaining = self.remaining.wrapping_sub(1);
    }

    /// 8006CEB0 clears model+600/604/608 after the secondary-chain visit,
    /// including held compositions. These values never displace the root.
    pub(crate) fn take_acceleration(&mut self) -> [f32; 3] {
        std::mem::take(&mut self.acceleration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_opening_draw_pairs_produce_signed_secondary_acceleration() {
        // Source watch04 C25/27/29/30: observed RNG boundaries, actor2
        // acceleration bits, active flag=1 and timer=7. Original REL25C08..25D10
        // plus rodata15C0/1668 independently establish signed remainders/scales.
        // memory.jsonl SHA256:3c931310eeb1ad556654d23dfe0be11255a267ac33f01a7304aeb4ad21623954.
        for (before, after, bits) in [
            (2809806486, 2056599432, [1056964608, 1045220557, 1036831949]),
            (3267667593, 1550321715, [1056964608, 1045220557, 1070386381]),
            (2710487920, 2866731250, [3214514586, 1045220557, 3216192307]),
            (2866731250, 3387678532, [3212836864, 1045220557, 3204448256]),
        ] {
            let mut jitter = BodyJitter::default();
            let mut random = Random::from_state(before);
            jitter.request(8);
            jitter.advance(&mut random);
            assert_eq!(random.state(), after);
            assert_eq!(jitter.remaining, 7);
            assert!(jitter.active);
            assert_eq!(jitter.take_acceleration().map(f32::to_bits), bits);
            assert_eq!(jitter.take_acceleration(), [0.; 3]);
        }
    }

    #[test]
    fn zero_entry_draws_before_expiry_and_stopping_preserves_the_pending_sample() {
        let mut jitter = BodyJitter::default();
        let mut random = Random::from_state(1);
        jitter.advance(&mut random);
        assert_eq!(random.state(), 1);
        jitter.request(8);
        for remaining in (0..=7).rev() {
            jitter.advance(&mut random);
            assert_eq!(jitter.remaining, remaining);
            assert!(jitter.active);
        }
        let before = random.state();
        jitter.advance(&mut random);
        assert_ne!(random.state(), before);
        assert!(!jitter.active);
        assert_eq!(jitter.remaining, -1);
        assert_eq!(jitter.acceleration[1], 0.2);
        let expired = random.state();
        jitter.advance(&mut random);
        assert_eq!(random.state(), expired);
        assert_eq!(jitter.acceleration, [0.; 3]);

        jitter.request(8);
        jitter.advance(&mut random);
        let pending = jitter.acceleration;
        jitter.stop();
        assert_eq!(jitter.take_acceleration(), pending);
        let stopped = random.state();
        jitter.advance(&mut random);
        assert_eq!(random.state(), stopped);
        assert_eq!(jitter.remaining, 7);
    }
}

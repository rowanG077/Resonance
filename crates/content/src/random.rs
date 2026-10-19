//! Small deterministic streams for gameplay and cosmetic decisions.
use serde::{Deserialize, Serialize};

/// SplitMix64. Every bit pattern is a valid seed and a complete saved state.
/// Algorithm: <https://prng.di.unimi.it/splitmix64.c> (public domain).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Random(u64);

impl Random {
    pub const fn new(state: u64) -> Self {
        Self(state)
    }

    pub const fn state(self) -> u64 {
        self.0
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn next_u16(&mut self) -> u16 {
        (self.next_u32() >> 16) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_stream_resumes_independently_of_other_streams() {
        let mut gameplay = Random::default();
        let mut cosmetic = Random::new(u64::MAX);
        gameplay.next_u32();
        let saved = serde_json::to_vec(&gameplay).unwrap();
        let mut restored: Random = serde_json::from_slice(&saved).unwrap();
        let mut draws = Vec::new();
        for _ in 0..16 {
            cosmetic.next_u16();
            let draw = gameplay.next_u32();
            assert_eq!(restored.next_u32(), draw);
            draws.push(draw);
        }
        assert!(draws.windows(2).any(|pair| pair[0] != pair[1]));
        assert_eq!(gameplay, restored);
    }
}

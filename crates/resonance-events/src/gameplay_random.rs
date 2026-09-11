//! Gameplay randomness is independent of field animation and particle effects.
use serde::{Deserialize, Serialize};

const WORDS: usize = 624;
const OFFSET: usize = 397;

/// MT19937 with the game's odd-seed, multiplicative initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "State", into = "State")]
pub struct GameplayRandom {
    words: Box<[u32; WORDS]>,
    index: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    words: Vec<u32>,
    index: usize,
}
impl TryFrom<State> for GameplayRandom {
    type Error = &'static str;
    fn try_from(state: State) -> Result<Self, Self::Error> {
        if state.index > WORDS || state.words.iter().all(|&word| word == 0) {
            return Err("invalid gameplay random state");
        }
        Ok(Self {
            words: state
                .words
                .into_boxed_slice()
                .try_into()
                .map_err(|_| "invalid gameplay random state length")?,
            index: state.index,
        })
    }
}
impl From<GameplayRandom> for State {
    fn from(random: GameplayRandom) -> Self {
        let words: Box<[u32]> = random.words;
        Self {
            words: words.into_vec(),
            index: random.index,
        }
    }
}
impl Default for GameplayRandom {
    fn default() -> Self {
        Self::new(4357)
    }
}
impl GameplayRandom {
    pub fn new(seed: u32) -> Self {
        let mut word = seed | 1;
        let words = std::array::from_fn(|index| {
            if index != 0 {
                word = word.wrapping_mul(69069);
            }
            word
        });
        Self {
            words: Box::new(words),
            index: WORDS,
        }
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn next_u32(&mut self) -> u32 {
        if self.index == WORDS {
            for i in 0..WORDS {
                let joined =
                    (self.words[i] & 0x8000_0000) | (self.words[(i + 1) % WORDS] & 0x7fff_ffff);
                self.words[i] = self.words[(i + OFFSET) % WORDS]
                    ^ (joined >> 1)
                    ^ if joined & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut value = self.words[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^ (value >> 18)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_and_restore_cross_twist_boundaries() {
        let mut random = GameplayRandom::default();
        let expected = [
            (0, 3510405877),
            (1, 4290933890),
            (2, 2191955339),
            (3, 564929546),
            (623, 730882493),
            (624, 2222118351),
            (1247, 3705639611),
            (1248, 1244107692),
            (1872, 2654627331),
        ];
        let mut restored = None;
        for draw in 0..=1872 {
            let value = random.next_u32();
            if let Some((_, expected)) = expected.iter().find(|(index, _)| *index == draw) {
                assert_eq!(value, *expected, "draw {draw}");
            }
            if let Some(saved) = &mut restored {
                assert_eq!(GameplayRandom::next_u32(saved), value);
            }
            if draw == 610 {
                restored =
                    Some(serde_json::from_slice(&serde_json::to_vec(&random).unwrap()).unwrap());
            }
        }
        assert_eq!(restored.unwrap(), random);
    }

    #[test]
    fn invalid_saved_generators_are_rejected() {
        for state in [
            State {
                words: vec![1; WORDS - 1],
                index: 0,
            },
            State {
                words: vec![1; WORDS],
                index: WORDS + 1,
            },
            State {
                words: vec![0; WORDS],
                index: 0,
            },
        ] {
            assert!(
                serde_json::from_slice::<GameplayRandom>(&serde_json::to_vec(&state).unwrap())
                    .is_err()
            );
        }
    }
}

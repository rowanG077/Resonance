//! Editable Fatal Strike balance, independent of animation and ordinary HP damage.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Gauge and contribution arrays always use blue, red, green order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Color {
    Blue,
    Red,
    Green,
}

impl Color {
    pub const ALL: [Self; 3] = [Self::Blue, Self::Red, Self::Green];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Blue => [74, 166, 255],
            Self::Red => [255, 80, 100],
            Self::Green => [77, 240, 163],
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    #[default]
    Defeat,
    Damage,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnemyRules {
    pub capacity: [u16; 3],
    /// Percentage of the hit's FS contribution accepted by each gauge.
    pub vulnerability: [u16; 3],
    pub outcome: Outcome,
}

impl Default for EnemyRules {
    fn default() -> Self {
        Self {
            capacity: [90; 3],
            vulnerability: [100; 3],
            outcome: Outcome::Defeat,
        }
    }
}

impl EnemyRules {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.capacity.iter().all(|&value| value != 0),
            "empty Fatal Strike gauge"
        );
        ensure!(
            self.vulnerability.iter().all(|&value| value <= 1000),
            "invalid Fatal Strike vulnerability"
        );
        Ok(())
    }
}

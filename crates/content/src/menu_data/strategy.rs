use super::*;

pub const STRATEGY_COUNTS: [usize; 3] = [9, 9, 7];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyOption {
    pub characters: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyPreset {
    pub name: String,
    /// Action, skill/magic, and position for each character.
    pub members: [[u8; 3]; 9],
}

impl StrategyPreset {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.name.len() <= 7
                && self.name.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                && self
                    .members
                    .iter()
                    .flatten()
                    .enumerate()
                    .all(|(i, v)| usize::from(*v) < STRATEGY_COUNTS[i % 3]),
            "invalid strategy preset"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyData {
    pub groups: [Vec<StrategyOption>; 3],
    pub presets: [[[u8; 3]; 9]; 3],
    pub default_positions: [u8; 9],
    pub positions: [u8; 6],
}

impl StrategyData {
    pub fn validate_rules(&self) -> Result<()> {
        ensure!(
            self.groups
                .iter()
                .zip(STRATEGY_COUNTS)
                .all(|(group, count)| group.len() == count
                    && group
                        .iter()
                        .all(|item| item.characters > 0 && item.characters < 512))
                && self
                    .default_positions
                    .iter()
                    .chain(&self.positions)
                    .all(|v| *v < 3),
            "invalid strategy definitions"
        );
        ensure!(
            self.presets.iter().all(|preset| preset
                .iter()
                .flatten()
                .enumerate()
                .all(|(i, choice)| usize::from(*choice) < STRATEGY_COUNTS[i % 3])),
            "invalid strategy preset choices"
        );
        Ok(())
    }
    pub fn lane(&self, member: usize, choice: u8) -> usize {
        usize::from(if choice == 0 {
            self.default_positions[member]
        } else {
            self.positions[usize::from(choice - 1)]
        })
    }
}

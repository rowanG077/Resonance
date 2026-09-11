use super::*;

pub const STRATEGY_COUNTS: [usize; 3] = [9, 9, 7];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyOption {
    pub name: String,
    pub description: String,
    pub details: String,
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
    pub presets: [StrategyPreset; 3],
    pub default_positions: [u8; 9],
    pub positions: [u8; 6],
    pub keyboard: String,
    pub keys: [String; 9],
    pub labels: [String; 3],
}

impl StrategyData {
    pub fn validate(&self) -> Result<()> {
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
                    .all(|v| *v < 3)
                && self.keyboard.len() == 90
                && self.keyboard.is_ascii(),
            "invalid strategy definitions"
        );
        for preset in &self.presets {
            preset.validate()?;
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.groups
            .iter()
            .flatten()
            .flat_map(|item| [&item.name, &item.description, &item.details])
            .chain(self.presets.iter().map(|p| &p.name))
            .chain(&self.labels)
            .chain(&self.keys)
            .chain([&self.keyboard])
            .map(String::as_str)
    }
    pub fn lane(&self, member: usize, choice: u8) -> usize {
        usize::from(if choice == 0 {
            self.default_positions[member]
        } else {
            self.positions[usize::from(choice - 1)]
        })
    }
}

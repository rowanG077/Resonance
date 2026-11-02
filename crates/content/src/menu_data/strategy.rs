use super::*;

/// Maximum editable preset length in character cells, independent of UTF-8 bytes.
pub const STRATEGY_NAME_CHARACTERS: usize = 31;

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
                && self.name.chars().count() <= STRATEGY_NAME_CHARACTERS
                && self.name.chars().all(|c| !c.is_control())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_names_preserve_localized_native_captions() {
        for name in ["Défense", "Economie", "Reserve", "防御"] {
            assert!(
                StrategyPreset {
                    name: name.into(),
                    members: [[0; 3]; 9]
                }
                .validate()
                .is_ok()
            );
        }
        for name in ["", "Bad\n", &"é".repeat(STRATEGY_NAME_CHARACTERS + 1)] {
            assert!(
                StrategyPreset {
                    name: name.into(),
                    members: [[0; 3]; 9]
                }
                .validate()
                .is_err()
            );
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

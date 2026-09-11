use super::*;
use resonance_content::menu_data::StrategyData;

impl Party {
    /// Preset edits do not alter the character's current battle instructions.
    pub fn set_strategy(
        &mut self,
        data: &StrategyData,
        member: usize,
        group: usize,
        option: u8,
        preset: Option<usize>,
    ) -> Result<bool, String> {
        let definition = data
            .groups
            .get(group)
            .and_then(|g| g.get(usize::from(option)))
            .ok_or("unknown strategy option")?;
        if member >= self.members.len() || definition.characters & (1 << member) == 0 {
            return Err("strategy is unavailable for this character".into());
        }
        let target = if let Some(index) = preset {
            if index >= data.presets.len() {
                return Err("unknown strategy preset".into());
            }
            &mut self
                .strategy_presets
                .get_or_insert_with(|| data.presets.clone())[index]
                .members[member][group]
        } else {
            &mut self.members[member].strategy[group]
        };
        let changed = *target != option;
        *target = option;
        Ok(changed)
    }
}

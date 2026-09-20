use super::*;
use crate::all_assets::strategy_ui::Catalogue;
use resonance_content::menu_data::{StrategyData, StrategyOption, StrategyPreset};

pub(super) fn cook(source: &Catalogue) -> Result<StrategyData> {
    let text = |reference| -> Result<String> { Ok(source.required_text(reference)?.to_owned()) };
    Ok(StrategyData {
        groups: source
            .groups
            .iter()
            .map(|group| {
                group
                    .choices
                    .iter()
                    .map(|row| {
                        let mask = u16::from(row.characters);
                        Ok(StrategyOption {
                            name: text(row.name)?,
                            description: text(row.description)?,
                            details: text(row.details)?,
                            characters: mask | ((mask & 0x20) << 3),
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        presets: source
            .presets
            .iter()
            .map(|row| StrategyPreset {
                name: source.text(row.name).into(),
                members: std::array::from_fn(|member| {
                    let row = &row.members[member];
                    [row.action, row.skill_magic, row.position]
                }),
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        default_positions: source
            .default_positions
            .map(|id| source.lanes[id as usize - 1] as u8),
        positions: source.lanes.map(|lane| lane as u8),
        keyboard: source.keyboard.cells.clone(),
        keys: source
            .keyboard
            .keys
            .map(text)
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        labels: source
            .groups
            .iter()
            .map(|group| text(group.label))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
    })
}

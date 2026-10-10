use super::*;
use crate::all_assets::strategy_ui::Catalogue;
use resonance_content::menu_data::{ItemText, StrategyData, StrategyOption, StrategyText};

pub(super) fn cook(source: &Catalogue) -> Result<(StrategyData, StrategyText)> {
    let text = |reference| -> Result<String> { Ok(source.required_text(reference)?.to_owned()) };
    Ok((
        StrategyData {
            groups: source.groups.each_ref().map(|group| {
                group
                    .choices
                    .iter()
                    .map(|row| {
                        let mask = u16::from(row.characters);
                        StrategyOption {
                            characters: mask | ((mask & 0x20) << 3),
                        }
                    })
                    .collect()
            }),
            presets: source.presets.each_ref().map(|preset| {
                preset
                    .members
                    .each_ref()
                    .map(|row| [row.action, row.skill_magic, row.position])
            }),
            default_positions: source
                .default_positions
                .map(|id| source.lanes[id as usize - 1] as u8),
            positions: source.lanes.map(|lane| lane as u8),
        },
        StrategyText {
            groups: source
                .groups
                .iter()
                .map(|group| {
                    group
                        .choices
                        .iter()
                        .map(|row| {
                            Ok(Some(ItemText {
                                name: text(row.name)?,
                                description: text(row.description)?,
                                details: text(row.details)?,
                            }))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            presets: Some(
                source
                    .presets
                    .each_ref()
                    .map(|row| source.text(row.name).into()),
            ),
            keyboard: Some(source.keyboard.cells.clone()),
            keys: Some(
                source
                    .keyboard
                    .keys
                    .map(text)
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            ),
            labels: Some(
                source
                    .groups
                    .iter()
                    .map(|group| text(group.label))
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            ),
        },
    ))
}

use super::*;
use crate::all_assets::options_ui::Catalogue;
use resonance_content::menu_data::{CustomizeData, CustomizeOption};

pub(super) fn cook(ui: &Catalogue) -> Result<CustomizeData> {
    let labels = [
        ("cancel_help", ui.help.cancel_description),
        ("default_help", ui.help.default_description),
        ("cancel", ui.help.cancel),
        ("default", ui.help.default),
        ("on", ui.choices.toggle[0]),
        ("off", ui.choices.toggle[1]),
        ("stereo", ui.choices.audio_output[0]),
        ("mono", ui.choices.audio_output[1]),
        ("color", Some(ui.symbols.color)),
        ("volume", Some(ui.symbols.volume)),
        ("position", Some(ui.symbols.position)),
        ("position_help", Some(ui.symbols.position_help)),
    ]
    .into_iter()
    .map(|(key, reference)| Ok((key.into(), ui.required(reference)?.to_owned())))
    .collect::<Result<_>>()?;
    Ok(CustomizeData {
        options: ui
            .options
            .iter()
            .map(|option| {
                Ok(CustomizeOption {
                    name: ui.required(option.name)?.to_owned(),
                    description: ui.required(option.description)?.to_owned(),
                })
            })
            .collect::<Result<_>>()?,
        difficulties: ui.strings(&ui.difficulties)?,
        actions: ui.strings(&ui.actions)?,
        control_buttons: ui.control_buttons,
        color_groups: ui.strings(&ui.color_groups)?,
        volume_channels: ui.strings(&ui.volume_channels)?,
        themes: ui.themes.clone(),
        defaults: ui.defaults.clone(),
        labels,
    })
}

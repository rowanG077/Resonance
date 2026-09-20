use super::*;
use crate::all_assets::options_ui::{Catalogue, CopyOnlyDefaults};
use resonance_content::menu_data::{CustomizeData, CustomizeOption};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(super) struct Source {
    #[serde(flatten)]
    data: CustomizeData,
    default_font_slot: u8,
    copy_only_defaults: CopyOnlyDefaults,
}

pub(super) fn cook(ui: &Catalogue) -> Result<Source> {
    let mut labels: BTreeMap<_, _> = [
        ("cancel_help", ui.help.cancel_description),
        ("default_help", ui.help.default_description),
        ("cancel", ui.help.cancel),
        ("default", ui.help.default),
        ("on", ui.choices.toggle[0]),
        ("off", ui.choices.toggle[1]),
        ("stereo", ui.choices.audio_output[0]),
        ("mono", ui.choices.audio_output[1]),
    ]
    .into_iter()
    .map(|(key, reference)| Ok((key.into(), ui.required(reference)?.to_owned())))
    .collect::<Result<_>>()?;
    for (key, reference) in [
        ("color", ui.symbols.color),
        ("volume", ui.symbols.volume),
        ("position", ui.symbols.position),
        ("position_help", ui.symbols.position_help),
    ] {
        labels.insert(key.into(), ui.text(reference).to_owned());
    }
    Ok(Source {
        data: CustomizeData {
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
        },
        default_font_slot: ui.default_font_slot,
        copy_only_defaults: ui.copy_only_defaults.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::all_assets::options_ui::{
        self, DEFAULTS_ADDRESS, DEFAULTS_SIZE, defaults, reconstruct_defaults,
    };
    use resonance_content::menu_data::CustomizeSettings;

    #[test]
    fn defaults_preserve_font_slot_copy_only_storage_and_signed_positions() -> Result<()> {
        for (flags, positions) in [
            (0, [i16::MIN, i16::MAX]),
            (255, [-12, -32]),
            (85, [32, 32]),
            (170, [0, -1]),
        ] {
            let mut bytes = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(19));
            bytes[1] = flags;
            bytes[7] = flags;
            bytes[44..46].copy_from_slice(&positions[0].to_be_bytes());
            bytes[46..48].copy_from_slice(&positions[1].to_be_bytes());
            let parsed = defaults(&bytes);
            let (settings, font_slot, copy_only): (CustomizeSettings, u8, CopyOnlyDefaults) =
                serde_json::from_slice(&serde_json::to_vec(&parsed)?)?;
            assert_eq!(settings.screen_position, positions);
            assert_eq!(font_slot, flags >> 6);
            assert_eq!(
                reconstruct_defaults(&settings, font_slot, &copy_only),
                bytes
            );
            bytes[1] ^= 1;
            bytes[48..].fill(0xAD);
            assert_eq!(defaults(&bytes).0, settings);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted executables; no cooking or devices"]
    fn original_customization_defaults_preserve_complete_records() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = options_ui::read(&executable)?;
            let source = cook(&catalogue)?;
            let json = serde_json::to_vec(&source)?;
            let restored: Source = serde_json::from_slice(&json)?;
            assert_eq!(
                reconstruct_defaults(
                    &restored.data.defaults,
                    restored.default_font_slot,
                    &restored.copy_only_defaults,
                )
                .as_slice(),
                dol::slice(&executable, DEFAULTS_ADDRESS, DEFAULTS_SIZE)?
            );
            assert_eq!(restored.default_font_slot, 0);
            let runtime: CustomizeData = serde_json::from_slice(&json)?;
            runtime.validate()?;
            assert_eq!(runtime.defaults, CustomizeSettings::default());
            assert_eq!(
                serde_json::to_value(runtime)?,
                serde_json::to_value(source.data)?
            );
        }
        Ok(())
    }
}

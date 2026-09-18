//! Customization choices, display resources, defaults and preserved table storage.
use super::text::{TextPool, TextRef, TextSource};
use crate::dol;
use anyhow::{Context, Result};
use resonance_content::menu_data::{CUSTOMIZE_OPTIONS, CustomizeSettings, Volumes, WindowColors};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "options-ui";
const NAMES: u32 = 0x80219cc0;
const DESCRIPTIONS: u32 = 0x8019ac94;
pub(crate) const DEFAULTS_ADDRESS: u32 = 0x80219cf8;
pub(crate) const DEFAULTS_SIZE: usize = 112;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) headings: [Option<TextRef>; 2],
    /// Native option selector order; each name binds to the corresponding help row.
    pub(crate) options: [OptionEntry; CUSTOMIZE_OPTIONS],
    pub(crate) description_storage: Option<TextRef>,
    pub(crate) difficulties: [Option<TextRef>; 3],
    pub(crate) actions: [Option<TextRef>; 7],
    pub(crate) help: Help,
    pub(crate) choices: Choices,
    pub(crate) color_groups: [Option<TextRef>; 7],
    /// Red, green, blue, alpha, in the native editor order.
    pub(crate) channels: [ColorChannel; 4],
    pub(crate) volume_channels: [Option<TextRef>; 6],
    pub(crate) control_buttons: [u8; 7],
    pub(crate) control_button_storage: u8,
    pub(crate) symbols: Symbols,
    pub(crate) themes: [WindowColors; 3],
    pub(crate) defaults: CustomizeSettings,
    // The English font loader uses its first font regardless of this stored slot.
    pub(crate) default_font_slot: u8,
    pub(crate) copy_only_defaults: CopyOnlyDefaults,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required customization label")?))
    }

    pub(crate) fn strings<const N: usize>(
        &self,
        references: &[Option<TextRef>; N],
    ) -> Result<[String; N]> {
        Ok(references
            .iter()
            .map(|&r| Ok(self.required(r)?.to_owned()))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap())
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct OptionEntry {
    pub(crate) name: Option<TextRef>,
    pub(crate) description: Option<TextRef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Help {
    pub(crate) cancel_description: Option<TextRef>,
    pub(crate) default_description: Option<TextRef>,
    pub(crate) cancel: Option<TextRef>,
    pub(crate) default: Option<TextRef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Choices {
    /// Window styles use the first three; background styles use all six.
    pub(crate) appearance: [Option<TextRef>; 6],
    pub(crate) message_speed: [Option<TextRef>; 10],
    /// Enabled, disabled.
    pub(crate) toggle: [Option<TextRef>; 2],
    /// Stereo, mono.
    pub(crate) audio_output: [Option<TextRef>; 2],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ColorChannel {
    pub(crate) label: Option<TextRef>,
    pub(crate) slider_color: [u8; 4],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Symbols {
    pub(crate) value_format: TextRef,
    pub(crate) previous: TextRef,
    pub(crate) next: TextRef,
    pub(crate) up: TextRef,
    pub(crate) down: TextRef,
    pub(crate) left: TextRef,
    pub(crate) right: TextRef,
    pub(crate) color: TextRef,
    pub(crate) volume: TextRef,
    pub(crate) position: TextRef,
    pub(crate) position_help: TextRef,
    pub(crate) position_format: TextRef,
}

// The native customization UI copies all 112 bytes on open/apply/reset, but only
// interprets flags 0xFE and fields through byte 47 (800AAC48/800AC32C). These fields
// are copy-only within that subsystem; their authoring semantics remain unknown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CopyOnlyDefaults {
    /// Uninterpreted bit 0 of the flags byte at offset 1.
    flags: u8,
    /// Bytes 48..112, copied together with the editable settings.
    storage: Vec<u8>,
}

fn colors(row: &[u8]) -> WindowColors {
    let rgba = |i| row[i..i + 4].try_into().unwrap();
    WindowColors {
        menu: rgba(0),
        dialogue: rgba(4),
        choice: rgba(8),
        popup: rgba(12),
        shade_top: rgba(16),
        shade_bottom: rgba(20),
        selection: rgba(24),
    }
}

pub(crate) fn defaults(bytes: &[u8; DEFAULTS_SIZE]) -> (CustomizeSettings, u8, CopyOnlyDefaults) {
    let enabled = |mask| bytes[1] & mask != 0;
    (
        CustomizeSettings {
            message_speed: bytes[0],
            battle_rank: bytes[15],
            window: (bytes[7] >> 4) & 3,
            background: bytes[7] & 15,
            colors: colors(&bytes[16..44]),
            volumes: Volumes {
                music: bytes[2],
                effects: bytes[3],
                voice: bytes[4],
                battle_effects: bytes[5],
                battle_voice: bytes[6],
            },
            stereo: enabled(2),
            button_map: bytes[8..15].try_into().unwrap(),
            battle_voiceover: enabled(128),
            event_voiceover: enabled(64),
            skit_notifications: enabled(32),
            movie_subtitles: enabled(16),
            battle_auto_zoom: enabled(8),
            rumble: enabled(4),
            screen_position: [
                i16::from_be_bytes(bytes[44..46].try_into().unwrap()),
                i16::from_be_bytes(bytes[46..48].try_into().unwrap()),
            ],
        },
        bytes[7] >> 6,
        CopyOnlyDefaults {
            flags: bytes[1] & 1,
            storage: bytes[48..].to_vec(),
        },
    )
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    let names = texts.array::<CUSTOMIZE_OPTIONS>(executable, NAMES)?;
    let descriptions = texts.array::<15>(executable, DESCRIPTIONS)?;
    let options = std::array::from_fn(|i| OptionEntry {
        name: names[i],
        description: descriptions[i],
    });
    let [cancel_description, default_description, cancel, default] =
        texts.array(executable, 0x8019aa40)?;
    let channel_labels = texts.array::<4>(executable, 0x8019ade8)?;
    let channel_colors = dol::slice(executable, 0x8021cd7c, 16)?;
    let channels = std::array::from_fn(|i| ColorChannel {
        label: channel_labels[i],
        slider_color: channel_colors[i * 4..i * 4 + 4].try_into().unwrap(),
    });
    let buttons = dol::slice(executable, 0x8035caec, 8)?;
    let (defaults, default_font_slot, copy_only_defaults) =
        defaults(dol::slice(executable, DEFAULTS_ADDRESS, DEFAULTS_SIZE)?.try_into()?);
    Ok((
        Catalogue {
            headings: texts.array(executable, 0x8019aa10)?,
            options,
            description_storage: descriptions[14],
            difficulties: texts.array(executable, 0x8019aa18)?,
            actions: texts.array(executable, 0x8019aa24)?,
            help: Help {
                cancel_description,
                default_description,
                cancel,
                default,
            },
            choices: Choices {
                appearance: texts.array(executable, 0x8019acd0)?,
                message_speed: texts.array(executable, 0x8019ace8)?,
                toggle: texts.array(executable, 0x8035ca7c)?,
                audio_output: texts.array(executable, 0x8035cae4)?,
            },
            color_groups: texts.array(executable, 0x8019adcc)?,
            channels,
            volume_channels: texts.array(executable, 0x8019ae48)?,
            control_buttons: buttons[..7].try_into()?,
            control_button_storage: buttons[7],
            symbols: Symbols {
                value_format: texts.required(executable, 0x8035cbb8)?,
                previous: texts.required(executable, 0x8035cbbc)?,
                next: texts.required(executable, 0x8035cbc0)?,
                up: texts.required(executable, 0x8035cbc4)?,
                down: texts.required(executable, 0x8035cbc8)?,
                left: texts.required(executable, 0x8035cbcc)?,
                right: texts.required(executable, 0x8035cbd0)?,
                color: texts.required(executable, 0x8035cbd4)?,
                volume: texts.required(executable, 0x8035cbd8)?,
                position: texts.required(executable, 0x8035cbdc)?,
                position_help: texts.required(executable, 0x8019b910)?,
                position_format: texts.required(executable, 0x8019b930)?,
            },
            themes: dol::slice(executable, 0x8019ad10, 84)?
                .chunks_exact(28)
                .map(colors)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            defaults,
            default_font_slot,
            copy_only_defaults,
            texts: texts.values,
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, _) = parse(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
pub(crate) fn reconstruct_defaults(
    settings: &CustomizeSettings,
    font_slot: u8,
    copy_only: &CopyOnlyDefaults,
) -> [u8; DEFAULTS_SIZE] {
    let mut bytes = [0; DEFAULTS_SIZE];
    bytes[0] = settings.message_speed;
    for (enabled, mask) in [
        (settings.stereo, 2),
        (settings.rumble, 4),
        (settings.battle_auto_zoom, 8),
        (settings.movie_subtitles, 16),
        (settings.skit_notifications, 32),
        (settings.event_voiceover, 64),
        (settings.battle_voiceover, 128),
    ] {
        bytes[1] |= if enabled { mask } else { 0 };
    }
    bytes[2..7].copy_from_slice(&settings.volumes.channels());
    bytes[7] = font_slot << 6 | settings.window << 4 | settings.background;
    bytes[8..15].copy_from_slice(&settings.button_map);
    bytes[15] = settings.battle_rank;
    for (target, color) in bytes[16..44]
        .chunks_exact_mut(4)
        .zip(settings.colors.groups())
    {
        target.copy_from_slice(&color);
    }
    for (target, value) in bytes[44..48]
        .chunks_exact_mut(2)
        .zip(settings.screen_position)
    {
        target.copy_from_slice(&value.to_be_bytes());
    }
    assert_eq!(copy_only.flags & !1, 0);
    bytes[1] |= copy_only.flags;
    bytes[48..].copy_from_slice(&copy_only.storage);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both extracted discs; only publishes JSON tables"]
    fn original_options_ui_preserves_all_tables_storage_and_publication() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let (catalogue, sources) = parse(&executable)?;
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue =
                    serde_json::from_slice(&fs::read(destination.join(&paths[0]))?)?;
                assert_eq!(restored, catalogue);
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["data"], paths[0]);
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                payloads.insert(paths[0].clone());

                let mut headings = restored.headings.to_vec();
                headings.extend(restored.difficulties);
                headings.extend(restored.actions);
                headings.extend([
                    restored.help.cancel_description,
                    restored.help.default_description,
                    restored.help.cancel,
                    restored.help.default,
                ]);
                let mut descriptions: Vec<_> =
                    restored.options.iter().map(|o| o.description).collect();
                descriptions.push(restored.description_storage);
                let mut color_labels = restored.color_groups.to_vec();
                color_labels.extend(restored.channels.iter().map(|c| c.label));
                for (address, references) in [
                    (NAMES, restored.options.iter().map(|o| o.name).collect()),
                    (DESCRIPTIONS, descriptions),
                    (0x8019aa10, headings),
                    (0x8019acd0, restored.choices.appearance.to_vec()),
                    (0x8019ace8, restored.choices.message_speed.to_vec()),
                    (0x8019adcc, color_labels),
                    (0x8019ae48, restored.volume_channels.to_vec()),
                    (0x8035ca7c, restored.choices.toggle.to_vec()),
                    (0x8035cae4, restored.choices.audio_output.to_vec()),
                ] {
                    let bytes: Vec<_> = references
                        .iter()
                        .flat_map(|reference| {
                            reference
                                .map_or(0, |id| sources[id.0].address)
                                .to_be_bytes()
                        })
                        .collect();
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, references.len() * 4)?
                    );
                }
                for (source, text) in sources.iter().zip(&restored.texts) {
                    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
                    assert!(!invalid);
                    let terminated = [encoded.as_ref(), &[0]].concat();
                    assert_eq!(terminated.len() as u32, source.source_size);
                    assert_eq!(
                        terminated,
                        dol::slice(&executable, source.address, source.source_size as usize)?
                    );
                }
                let themes: Vec<_> = restored
                    .themes
                    .iter()
                    .flat_map(|theme| theme.groups().into_iter().flatten())
                    .collect();
                assert_eq!(themes, dol::slice(&executable, 0x8019ad10, 84)?);
                let slider_colors: Vec<_> = restored
                    .channels
                    .iter()
                    .flat_map(|channel| channel.slider_color)
                    .collect();
                assert_eq!(slider_colors, dol::slice(&executable, 0x8021cd7c, 16)?);
                let buttons: Vec<_> = restored
                    .control_buttons
                    .into_iter()
                    .chain([restored.control_button_storage])
                    .collect();
                assert_eq!(buttons, dol::slice(&executable, 0x8035caec, 8)?);
                assert_eq!(
                    reconstruct_defaults(
                        &restored.defaults,
                        restored.default_font_slot,
                        &restored.copy_only_defaults
                    )
                    .as_slice(),
                    dol::slice(&executable, DEFAULTS_ADDRESS, DEFAULTS_SIZE)?
                );
                assert_eq!(
                    restored.strings(&restored.choices.appearance)?,
                    ["A", "B", "C", "D", "E", "F"]
                );
                assert_eq!(
                    restored.strings(&restored.choices.message_speed)?,
                    ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]
                );
                assert_eq!(
                    restored.description_storage,
                    restored.options[2].description
                );
                assert_eq!(restored.channels[2].label, restored.choices.appearance[1]);
                assert_eq!(restored.channels[3].label, restored.choices.appearance[0]);
                assert_eq!(
                    restored.text(restored.symbols.position_format),
                    "  \x0c\x08X:\x0c\x09%3d \x0c\x08Y:\x0c\x09%3d"
                );

                // Retain nullable unused help and control-table storage without
                // confusing them with a shared empty string or a displayed button.
                for (address, replacement) in [
                    (DESCRIPTIONS + 14 * 4, 0u32.to_be_bytes().to_vec()),
                    (0x8035caf3, vec![173]),
                    (0x8021cd7c, vec![1, 2, 3, 4]),
                    (DEFAULTS_ADDRESS + 1, vec![255]),
                    (DEFAULTS_ADDRESS + 48, (0..64).collect()),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
                }
                let changed = read(&executable)?;
                assert!(changed.description_storage.is_none());
                assert_eq!(changed.control_button_storage, 173);
                assert_eq!(changed.control_buttons, restored.control_buttons);
                assert_eq!(changed.channels[0].slider_color, [1, 2, 3, 4]);
                assert_eq!(
                    reconstruct_defaults(
                        &changed.defaults,
                        changed.default_font_slot,
                        &changed.copy_only_defaults
                    )
                    .as_slice(),
                    dol::slice(&executable, DEFAULTS_ADDRESS, DEFAULTS_SIZE)?
                );
            }
            assert_eq!(payloads.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}

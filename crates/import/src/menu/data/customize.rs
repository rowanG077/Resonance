use super::*;
use resonance_content::menu_data::{
    CUSTOMIZE_OPTIONS, CustomizeData, CustomizeOption, CustomizeSettings, Volumes, WindowColors,
};
use std::collections::BTreeMap;

pub(super) fn cook(
    executable: &[u8],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<CustomizeData> {
    const LABELS: u32 = 0x8019a9a0;
    let strings = |address, count| -> Result<Vec<String>> {
        dol::slice(executable, address, count * 4)?
            .chunks_exact(4)
            .map(|p| text(p, 0))
            .collect()
    };
    let colors = |row: &[u8]| -> WindowColors {
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
    };
    let names = strings(0x80219cc0, CUSTOMIZE_OPTIONS)?;
    let descriptions = strings(LABELS + 0x2f4, CUSTOMIZE_OPTIONS)?;
    let defaults = dol::slice(executable, 0x80219cf8, 44)?;
    let enabled = |mask| defaults[1] & mask != 0;
    let mut labels: BTreeMap<_, _> = [
        ("cancel_help", 0xa0),
        ("default_help", 0xa4),
        ("cancel", 0xa8),
        ("default", 0xac),
    ]
    .into_iter()
    .map(|(key, offset)| {
        Ok((
            key.into(),
            text(dol::slice(executable, LABELS + offset, 4)?, 0)?,
        ))
    })
    .collect::<Result<_>>()?;
    for (key, address) in [
        ("on", 0x8035ca7c),
        ("off", 0x8035ca80),
        ("stereo", 0x8035cad4),
        ("mono", 0x8035cadc),
        ("color", 0x8035cbd4),
        ("volume", 0x8035cbd8),
        ("position", 0x8035cbdc),
        ("position_help", LABELS + 0xf70),
    ] {
        // ON/OFF are a pointer pair; the remaining symbols contain the strings.
        let pointer = if matches!(key, "on" | "off") {
            dol::slice(executable, address, 4)?.try_into()?
        } else {
            address.to_be_bytes()
        };
        labels.insert(key.into(), text(&pointer, 0)?);
    }
    Ok(CustomizeData {
        options: names
            .into_iter()
            .zip(descriptions)
            .map(|(name, description)| CustomizeOption { name, description })
            .collect(),
        difficulties: strings(LABELS + 0x78, 3)?.try_into().unwrap(),
        actions: strings(LABELS + 0x84, 7)?.try_into().unwrap(),
        control_buttons: dol::slice(executable, 0x8035caec, 7)?.try_into()?,
        color_groups: strings(LABELS + 0x42c, 7)?.try_into().unwrap(),
        volume_channels: strings(LABELS + 0x4a8, 6)?.try_into().unwrap(),
        themes: dol::slice(executable, 0x8019ad10, 3 * 28)?
            .chunks_exact(28)
            .map(colors)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        defaults: CustomizeSettings {
            message_speed: defaults[0],
            battle_rank: defaults[15],
            window: (defaults[7] >> 4) & 3,
            background: defaults[7] & 15,
            colors: colors(&defaults[16..]),
            volumes: Volumes {
                music: defaults[2],
                effects: defaults[3],
                voice: defaults[4],
                battle_effects: defaults[5],
                battle_voice: defaults[6],
            },
            stereo: enabled(2),
            button_map: defaults[8..15].try_into()?,
            battle_voiceover: enabled(128),
            event_voiceover: enabled(64),
            skit_notifications: enabled(32),
            movie_subtitles: enabled(16),
            battle_auto_zoom: enabled(8),
            rumble: enabled(4),
            screen_position: [0; 2],
        },
        labels,
    })
}

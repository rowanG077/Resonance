//! Native customization choices and defaults to `game/options.json`.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    menu_data::{CustomizeOption, Volumes, WindowColors},
    options::{self, Defaults, Document},
};
use std::{collections::BTreeMap, fs, path::Path};

pub fn cook_options(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&exe)?;
    let path = library.join(options::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn named<T>(names: &[&str], values: impl IntoIterator<Item = T>) -> BTreeMap<String, T> {
    names
        .iter()
        .map(|name| (*name).into())
        .zip(values)
        .collect()
}

fn colors(row: &[u8]) -> WindowColors {
    let rgba = |start| row[start..start + 4].try_into().unwrap();
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

fn convert(exe: &[u8]) -> Result<Document> {
    let engine: serde_json::Value = serde_json::from_str(include_str!("options-engine.json"))?;
    let names = text::table::<14>(exe, 0x80219cc0)?;
    let help = text::table::<14>(exe, 0x8019ac94)?;
    let mut choices = named(
        &options::OPTIONS[..13],
        names
            .into_iter()
            .zip(help)
            .map(|(name, description)| CustomizeOption { name, description }),
    );
    for name in &options::OPTIONS[13..] {
        choices.insert(
            (*name).into(),
            serde_json::from_value(engine["options"][name].clone())
                .with_context(|| format!("missing engine option {name}"))?,
        );
    }
    let mut labels = BTreeMap::new();
    for (name, address) in [
        ("cancel_help", 0x8019aa40),
        ("default_help", 0x8019aa44),
        ("cancel", 0x8019aa48),
        ("default", 0x8019aa4c),
        ("on", 0x8035ca7c),
        ("off", 0x8035ca80),
        ("stereo", 0x8035cae4),
        ("mono", 0x8035cae8),
    ] {
        labels.insert(name.into(), text::pointer(exe, address)?);
    }
    for (name, address) in [("color", 0x8035cbd4), ("volume", 0x8035cbd8)] {
        labels.insert(name.into(), text::string(exe, address)?);
    }
    ensure!(
        dol::slice(exe, 0x8035caec, 7)? == options::control_buttons(),
        "unsupported native control button glyphs"
    );
    let themes: BTreeMap<_, _> = named(
        &options::WINDOWS,
        dol::slice(exe, 0x8019ad10, 84)?
            .chunks_exact(28)
            .map(colors),
    );
    let original = dol::slice(exe, 0x80219cf8, 48)?;
    let selected_window = usize::from((original[7] >> 4) & 3);
    let selected_theme = options::WINDOWS
        .get(selected_window)
        .context("invalid default window")?;
    let current_colors = colors(&original[16..44]);
    let action_names = original[8..15]
        .iter()
        .map(|&id| {
            Ok(options::ACTIONS
                .get(usize::from(id))
                .context("invalid default button action")?
                .to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        action_names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == 7,
        "duplicate default button action"
    );
    let enabled = |mask| original[1] & mask != 0;
    let defaults = Defaults {
        message_speed: original[0],
        difficulty: options::DIFFICULTIES
            .get(usize::from(original[15]))
            .context("invalid default difficulty")?
            .to_string(),
        window: (*selected_theme).into(),
        background: options::BACKGROUNDS
            .get(usize::from(original[7] & 15))
            .context("invalid default background")?
            .to_string(),
        colors: (current_colors != themes[*selected_theme]).then_some(current_colors),
        volumes: Volumes {
            music: original[2],
            effects: original[3],
            voice: original[4],
            battle_effects: original[5],
            battle_voice: original[6],
        },
        stereo: enabled(2),
        controls: named(&options::BUTTONS, action_names),
        battle_voiceover: enabled(128),
        event_voiceover: enabled(64),
        skit_notifications: enabled(32),
        movie_subtitles: enabled(16),
        battle_auto_zoom: enabled(8),
        rumble: enabled(4),
        flexible_battle_party: false,
        combined_battles: false,
        learn_elemental_artes: false,
        learn_both_arte_paths: false,
        graphics_backend: Default::default(),
        anti_aliasing: Default::default(),
        display_mode: Default::default(),
        resolution: resonance_content::menu_data::default_display_resolution(),
    };
    let document = Document {
        options: choices,
        difficulties: named(&options::DIFFICULTIES, text::table::<3>(exe, 0x8019aa18)?),
        actions: named(&options::ACTIONS, text::table::<7>(exe, 0x8019aa24)?),
        color_groups: named(&options::COLOR_GROUPS, text::table::<7>(exe, 0x8019adcc)?),
        volume_channels: named(
            &options::VOLUME_CHANNELS,
            text::table::<6>(exe, 0x8019ae48)?,
        ),
        themes,
        defaults,
        labels,
        confirmations: serde_json::from_value(engine["confirmations"].clone())?,
    };
    Ok(document)
}

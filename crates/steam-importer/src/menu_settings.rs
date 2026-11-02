//! Localized menu tables written directly to the current GC documents.
use crate::{output, pe, source::Source, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{menu_data::*, options};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let menu = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: text::Bank::parse(&bytes)?,
        slot: source.language.slot(),
    };
    let manual = manual(&menu)?;
    manual.validate()?;
    output::json(&library.join(MANUAL_PATH), &manual)?;
    let rename = rename(&menu)?;
    rename.validate()?;
    output::json(&library.join(RENAME_PATH), &rename)?;
    let options = customize(&menu)?;
    options.prepare()?;
    output::json(&library.join(options::PATH), &options)
}
struct Menu<'a> {
    image: pe::Image<'a>,
    bank: text::Bank<'a>,
    slot: u32,
}
impl Menu<'_> {
    fn word(&self, at: u32) -> Result<u32> {
        pe::word(self.image.slice(at, 4)?, 0)
    }
    fn directory(&self, at: u32) -> Result<u32> {
        self.word(at + self.slot * 4)
    }
    fn caption(&self, at: u32, index: u32) -> Result<String> {
        self.bank
            .menu(self.word(at + index * 4)?, encoding_rs::WINDOWS_1252)
    }
    fn strings(&self, at: u32, count: u32) -> Result<Vec<String>> {
        let directory = self.directory(at)?;
        (0..count).map(|i| self.caption(directory, i)).collect()
    }
}
fn manual(source: &Menu<'_>) -> Result<TrainingManual> {
    // 0x510b50 filters this nine-by-five grid using these counts and flags.
    let names = source.directory(0x8b95f8)?;
    let topics = source.directory(0x8b9610)?;
    let counts = source.image.slice(0x768e9c, MANUAL_CHAPTERS)?;
    let flags = source.image.slice(0x770a68, MANUAL_CHAPTERS * 5)?;
    let mut chapters = Vec::new();
    for (chapter, &count) in counts.iter().enumerate() {
        ensure!(
            (1..=5).contains(&count),
            "invalid Steam manual chapter length"
        );
        let mut entries = Vec::new();
        for topic in 0..usize::from(count) {
            let index = chapter * 5 + topic;
            let name = source.caption(topics, index as u32 * 2)?;
            let body = source.word(topics + index as u32 * 8 + 4)?;
            let paragraphs = source
                .bank
                .menu_paragraphs(body)?
                .iter()
                .map(|text| resonance_importer::decode_menu_text(text, 9))
                .collect::<Result<_>>()?;
            entries.push(ManualTopic {
                name,
                learned_flag: u16::from(flags[index]),
                paragraphs,
            });
        }
        chapters.push(ManualChapter {
            name: source.caption(names, chapter as u32)?,
            topics: entries,
        });
    }
    let data = TrainingManual {
        title: source.caption(source.directory(0x8b943c)?, 3)?,
        chapters,
    };
    data.validate()?;
    Ok(data)
}

fn rename(source: &Menu<'_>) -> Result<RenameData> {
    // Labels: 0x50c538; default names: 0x50d4f6. The keyboard has its own
    // language directory, independent of the initial party-name directory.
    let labels = source.directory(0x8bfcf0)?;
    let names = source.directory(0x8bfcd8)?;
    let keyboard = source.directory(0x8bfcc0)?;
    let data = RenameData {
        defaults: (0..9)
            .map(|i| source.caption(names, i))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        keyboard: source.caption(keyboard, 0)?,
        heading: source.caption(labels, 0)?,
        delete: source.caption(labels, 1)?,
        default: source.caption(labels, 2)?,
        commands: (3..6)
            .map(|i| source.caption(labels, i))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
    };
    data.validate()?;
    Ok(data)
}

fn named<T>(names: &[&str], values: impl IntoIterator<Item = T>) -> BTreeMap<String, T> {
    names.iter().map(|s| (*s).to_owned()).zip(values).collect()
}
fn colors(row: &[u8]) -> WindowColors {
    let rgba = |at| row[at..at + 4].try_into().unwrap();
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
fn customize(source: &Menu<'_>) -> Result<options::Document> {
    let engine: serde_json::Value =
        serde_json::from_str(include_str!("../../importer/src/options-engine.json"))?;
    let names = source.directory(0x8c6550)?;
    let help = source.directory(0x8c6568)?;
    let labels = source.directory(0x8c6688)?;
    let mut rows = (0..10)
        .map(|i| {
            Ok(CustomizeOption {
                name: source.caption(names, i)?,
                description: source.caption(help, i)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let movie_text = |table| {
        source
            .bank
            .menu(source.word(table)? + 11, encoding_rs::WINDOWS_1252)
    };
    rows.push(CustomizeOption {
        name: movie_text(names)?,
        description: movie_text(help)?,
    });
    for i in [10, 11] {
        rows.push(CustomizeOption {
            name: source.caption(names, i)?,
            description: source.caption(help, i)?,
        });
    }
    let mut choices = named(&options::OPTIONS[..13], rows);
    for name in &options::OPTIONS[13..] {
        choices.insert(
            (*name).into(),
            serde_json::from_value(engine["options"][name].clone())
                .with_context(|| format!("missing shared option {name}"))?,
        );
    }
    let themes = named(
        &options::WINDOWS,
        source
            .image
            .slice(0x7838d4, 84)?
            .chunks_exact(28)
            .map(colors),
    );
    let native = source.image.slice(0x8c5b0c, 0x11c)?;
    let window = *options::WINDOWS
        .get(usize::from((native[0xaa] >> 2) & 3))
        .context("invalid default window")?;
    let shared = CustomizeSettings::default();
    let defaults = options::Defaults {
        message_speed: native[0],
        difficulty: options::DIFFICULTIES
            .get(usize::from(native[0xb9]))
            .context("invalid default difficulty")?
            .to_string(),
        window: window.into(),
        background: options::BACKGROUNDS
            .get(usize::from(native[0xaa] >> 4))
            .context("invalid default background")?
            .to_string(),
        colors: None,
        volumes: Volumes {
            music: native[0xa5],
            effects: native[0xa6],
            voice: native[0xa7],
            battle_effects: native[0xa8],
            battle_voice: native[0xa9],
        },
        stereo: native[0xa4] & 1 != 0,
        controls: named(
            &options::BUTTONS,
            shared
                .button_map
                .map(|action| options::ACTIONS[usize::from(action)].to_owned()),
        ),
        battle_voiceover: native[1] & 1 != 0,
        event_voiceover: native[1] & 2 != 0,
        skit_notifications: native[1] & 4 != 0,
        movie_subtitles: native[1] & 8 != 0,
        battle_auto_zoom: native[1] & 16 != 0,
        rumble: native[1] & 32 != 0,
        flexible_battle_party: false,
        combined_battles: false,
        learn_elemental_artes: false,
        learn_both_arte_paths: false,
        graphics_backend: Default::default(),
        anti_aliasing: Default::default(),
        display_mode: Default::default(),
        resolution: default_display_resolution(),
    };
    let mut captions = BTreeMap::from([
        ("stereo".into(), "STEREO".into()),
        ("mono".into(), "MONO".into()),
        ("color".into(), "□".into()),
        ("volume".into(), "♪".into()),
    ]);
    for (key, table, row) in [
        ("cancel", labels, 13),
        ("default", labels, 14),
        ("cancel_help", source.directory(0x8c6640)?, 0),
        ("default_help", source.directory(0x8c6640)?, 1),
        ("on", source.directory(0x8c6580)?, 0),
        ("off", source.directory(0x8c6580)?, 1),
    ] {
        captions.insert(key.into(), source.caption(table, row)?);
    }
    let mut actions = source.strings(0x8c6628, 6)?;
    actions.insert(4, "Delay Spell".into());
    let mut volumes = source.strings(0x8c6610, 5)?;
    volumes.push("Audio Output".into());
    Ok(options::Document {
        options: choices,
        difficulties: named(&options::DIFFICULTIES, source.strings(0x8c65b0, 3)?),
        actions: named(&options::ACTIONS, actions),
        color_groups: named(
            &options::COLOR_GROUPS,
            (1..=7)
                .map(|i| source.caption(labels, i))
                .collect::<Result<Vec<_>>>()?,
        ),
        volume_channels: named(&options::VOLUME_CHANNELS, volumes),
        themes,
        defaults,
        labels: captions,
        confirmations: serde_json::from_value(engine["confirmations"].clone())?,
    })
}

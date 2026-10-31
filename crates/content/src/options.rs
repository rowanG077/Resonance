//! Named menu options and reset defaults; saved preferences keep their own values.
use crate::menu_data::{
    AntiAliasing, CustomizeData, CustomizeOption, CustomizeSettings, DisplayMode, GraphicsBackend,
    Volumes, WindowColors,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "game/options.json";
/// Fixed engine meanings and menu order.
pub const OPTIONS: [&str; 21] = [
    "message_speed",
    "difficulty",
    "window",
    "background",
    "colors",
    "volume",
    "controls",
    "battle_voiceover",
    "event_voiceover",
    "skit_notifications",
    "movie_subtitles",
    "battle_auto_zoom",
    "rumble",
    "display_mode",
    "resolution",
    "anti_aliasing",
    "graphics_backend",
    "flexible_battle_party",
    "combined_battles",
    "learn_elemental_artes",
    "learn_both_arte_paths",
];
pub const DIFFICULTIES: [&str; 3] = ["normal", "hard", "mania"];
pub const WINDOWS: [&str; 3] = ["plain", "ornate", "gold"];
pub const BACKGROUNDS: [&str; 6] = [
    "cracked-stone",
    "rough-stone",
    "fine-grain",
    "speckled",
    "small-stones",
    "window-fill",
];
pub const BUTTONS: [&str; 7] = ["a", "b", "x", "y", "l", "r", "z"];
pub const ACTIONS: [&str; 7] = [
    "attack",
    "techniques",
    "defend",
    "menu",
    "delay_spell",
    "target_change",
    "unison_attack",
];
pub const COLOR_GROUPS: [&str; 7] = [
    "menu",
    "dialogue",
    "choice",
    "popup",
    "shade_top",
    "shade_bottom",
    "selection",
];
pub const VOLUME_CHANNELS: [&str; 6] = [
    "music",
    "effects",
    "voice",
    "battle_effects",
    "battle_voice",
    "output",
];
pub const CONFIRMATIONS: [&str; 3] = [
    "forget_elemental_artes",
    "forget_arte_paths",
    "forget_artes",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub options: BTreeMap<String, CustomizeOption>,
    pub difficulties: BTreeMap<String, String>,
    pub actions: BTreeMap<String, String>,
    pub color_groups: BTreeMap<String, String>,
    pub volume_channels: BTreeMap<String, String>,
    pub themes: BTreeMap<String, WindowColors>,
    pub defaults: Defaults,
    pub labels: BTreeMap<String, String>,
    pub confirmations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Updates per character, zero means immediate.
    pub message_speed: u8,
    pub difficulty: String,
    pub window: String,
    pub background: String,
    /// Omitted colors come from the selected window theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<WindowColors>,
    pub volumes: Volumes,
    pub stereo: bool,
    /// Button name to action name; each action must occur exactly once.
    pub controls: BTreeMap<String, String>,
    pub battle_voiceover: bool,
    pub event_voiceover: bool,
    pub skit_notifications: bool,
    pub movie_subtitles: bool,
    pub battle_auto_zoom: bool,
    pub rumble: bool,
    pub flexible_battle_party: bool,
    pub combined_battles: bool,
    pub learn_elemental_artes: bool,
    pub learn_both_arte_paths: bool,
    #[serde(default)]
    pub graphics_backend: GraphicsBackend,
    pub anti_aliasing: AntiAliasing,
    pub display_mode: DisplayMode,
    pub resolution: [u32; 2],
}

fn ordered<T: Clone, const N: usize>(
    rows: &BTreeMap<String, T>,
    names: [&str; N],
) -> Result<[T; N]> {
    ensure!(
        rows.len() == N,
        "expected {N} named option entries, got {}",
        rows.len()
    );
    names
        .into_iter()
        .map(|name| {
            rows.get(name)
                .cloned()
                .with_context(|| format!("missing option entry {name}"))
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid option entries"))
}
fn index(name: &str, names: &[&str]) -> Result<u8> {
    names
        .iter()
        .position(|&value| value == name)
        .map(|index| index as u8)
        .with_context(|| format!("unknown option choice {name}"))
}
fn named<T>(names: &[&str], rows: impl IntoIterator<Item = T>) -> BTreeMap<String, T> {
    names.iter().map(|s| (*s).to_owned()).zip(rows).collect()
}
/// Button glyphs belong to UI artwork. Options only assign actions to those buttons.
pub fn control_buttons() -> [u8; 7] {
    BUTTONS.map(|name| {
        crate::menu_data::button_sprite(&format!("{name}_pressed")).expect("known controller glyph")
    })
}
impl Defaults {
    pub fn prepare(&self, themes: &BTreeMap<String, WindowColors>) -> Result<CustomizeSettings> {
        let controls = ordered(&self.controls, BUTTONS)?;
        let buttons: Vec<_> = controls
            .iter()
            .map(|action| index(action, &ACTIONS))
            .collect::<Result<_>>()?;
        let settings = CustomizeSettings {
            message_speed: self.message_speed,
            battle_rank: index(&self.difficulty, &DIFFICULTIES)?,
            window: index(&self.window, &WINDOWS)?,
            background: index(&self.background, &BACKGROUNDS)?,
            colors: self.colors.clone().unwrap_or(
                themes
                    .get(&self.window)
                    .context("missing default window theme")?
                    .clone(),
            ),
            volumes: self.volumes,
            stereo: self.stereo,
            button_map: buttons.try_into().unwrap(),
            keymap: Default::default(),
            battle_voiceover: self.battle_voiceover,
            event_voiceover: self.event_voiceover,
            skit_notifications: self.skit_notifications,
            movie_subtitles: self.movie_subtitles,
            battle_auto_zoom: self.battle_auto_zoom,
            rumble: self.rumble,
            flexible_battle_party: self.flexible_battle_party,
            combined_battles: self.combined_battles,
            learn_elemental_artes: self.learn_elemental_artes,
            learn_both_arte_paths: self.learn_both_arte_paths,
            graphics_backend: self.graphics_backend,
            anti_aliasing: self.anti_aliasing,
            display_mode: self.display_mode,
            resolution: self.resolution,
        };
        settings.validate()?;
        Ok(settings)
    }
    fn from_settings(settings: &CustomizeSettings, themes: &[WindowColors; 3]) -> Result<Self> {
        settings.validate()?;
        Ok(Self {
            message_speed: settings.message_speed,
            difficulty: DIFFICULTIES[usize::from(settings.battle_rank)].into(),
            window: WINDOWS[usize::from(settings.window)].into(),
            background: BACKGROUNDS[usize::from(settings.background)].into(),
            colors: (settings.colors != themes[usize::from(settings.window)])
                .then(|| settings.colors.clone()),
            volumes: settings.volumes,
            stereo: settings.stereo,
            controls: named(
                &BUTTONS,
                settings
                    .button_map
                    .map(|id| ACTIONS[usize::from(id)].to_owned()),
            ),
            battle_voiceover: settings.battle_voiceover,
            event_voiceover: settings.event_voiceover,
            skit_notifications: settings.skit_notifications,
            movie_subtitles: settings.movie_subtitles,
            battle_auto_zoom: settings.battle_auto_zoom,
            rumble: settings.rumble,
            flexible_battle_party: settings.flexible_battle_party,
            combined_battles: settings.combined_battles,
            learn_elemental_artes: settings.learn_elemental_artes,
            learn_both_arte_paths: settings.learn_both_arte_paths,
            graphics_backend: settings.graphics_backend,
            anti_aliasing: settings.anti_aliasing,
            display_mode: settings.display_mode,
            resolution: settings.resolution,
        })
    }
}
impl Document {
    pub fn prepare(&self) -> Result<CustomizeData> {
        ordered(&self.confirmations, CONFIRMATIONS)?;
        // Existing cooked libraries predate the host graphics selector.
        let mut options = self.options.clone();
        options
            .entry("graphics_backend".into())
            .or_insert_with(|| CustomizeOption {
                name: "Graphics Backend".into(),
                description:
                    "Choose a supported graphics backend.\nRestart the game to apply changes."
                        .into(),
            });
        let data = CustomizeData {
            options: ordered(&options, OPTIONS)?.into(),
            difficulties: ordered(&self.difficulties, DIFFICULTIES)?,
            actions: ordered(&self.actions, ACTIONS)?,
            color_groups: ordered(&self.color_groups, COLOR_GROUPS)?,
            volume_channels: ordered(&self.volume_channels, VOLUME_CHANNELS)?,
            themes: ordered(&self.themes, WINDOWS)?,
            defaults: self.defaults.prepare(&self.themes)?,
            control_buttons: control_buttons(),
            labels: self.labels.clone(),
            confirmations: self.confirmations.clone(),
        };
        data.validate()?;
        Ok(data)
    }
    pub fn from_data(data: &CustomizeData) -> Result<Self> {
        data.validate()?;
        ensure!(
            data.control_buttons == control_buttons(),
            "unsupported control button glyph roles"
        );
        Ok(Self {
            options: named(&OPTIONS, data.options.clone()),
            difficulties: named(&DIFFICULTIES, data.difficulties.clone()),
            actions: named(&ACTIONS, data.actions.clone()),
            color_groups: named(&COLOR_GROUPS, data.color_groups.clone()),
            volume_channels: named(&VOLUME_CHANNELS, data.volume_channels.clone()),
            themes: named(&WINDOWS, data.themes.clone()),
            defaults: Defaults::from_settings(&data.defaults, &data.themes)?,
            labels: data.labels.clone(),
            confirmations: data.confirmations.clone(),
        })
    }
}

pub fn load(files: &crate::prepared::Files) -> Result<CustomizeData> {
    files.json::<Document>(PATH)?.prepare()
}

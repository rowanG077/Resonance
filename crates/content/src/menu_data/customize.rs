use super::*;

pub const FLEXIBLE_BATTLE_PARTY_OPTION: usize = 17;
pub const COMBINED_BATTLES_OPTION: usize = 18;
pub const LEARN_ELEMENTAL_ARTES_OPTION: usize = 19;
pub const LEARN_BOTH_ARTE_PATHS_OPTION: usize = 20;
pub const DISPLAY_MODE_OPTION: usize = 13;
pub const RESOLUTION_OPTION: usize = 14;
pub const ANTI_ALIASING_OPTION: usize = 15;
pub const GRAPHICS_BACKEND_OPTION: usize = 16;
pub const CUSTOMIZE_OPTIONS: usize = 21;

/// Portable saved preference; Automatic follows the host platform's default.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsBackend {
    #[default]
    Automatic,
    Dx12,
    Vulkan,
    Metal,
}
impl GraphicsBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Dx12 => "DirectX 12",
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
        }
    }
}

/// Spatial techniques only: no history, reprojection or projection jitter.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiAliasing {
    Off,
    SmaaLow,
    SmaaMedium,
    SmaaHigh,
    SmaaUltra,
    Msaa2,
    #[default]
    Msaa4,
    Msaa8,
    SmaaS2x,
}
impl AntiAliasing {
    pub const ALL: [Self; 9] = [
        Self::Off,
        Self::SmaaLow,
        Self::SmaaMedium,
        Self::SmaaHigh,
        Self::SmaaUltra,
        Self::Msaa2,
        Self::Msaa4,
        Self::Msaa8,
        Self::SmaaS2x,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::SmaaLow => "SMAA Low",
            Self::SmaaMedium => "SMAA Medium",
            Self::SmaaHigh => "SMAA High",
            Self::SmaaUltra => "SMAA Ultra",
            Self::Msaa2 => "2x MSAA",
            Self::Msaa4 => "4x MSAA",
            Self::Msaa8 => "8x MSAA",
            Self::SmaaS2x => "SMAA S2x",
        }
    }
    /// Preserve the selected technique when available, otherwise use a
    /// supported spatial fallback. The renderer always offers Off and SMAA.
    pub fn supported_or_fallback(self, supported: &[Self]) -> Self {
        if supported.contains(&self) {
            self
        } else if matches!(self, Self::Msaa2 | Self::Msaa4 | Self::Msaa8)
            && supported.contains(&Self::Msaa4)
        {
            Self::Msaa4
        } else if supported.contains(&Self::SmaaUltra) {
            Self::SmaaUltra
        } else {
            Self::Off
        }
    }
    pub fn samples(self) -> u32 {
        match self {
            Self::Msaa2 | Self::SmaaS2x => 2,
            Self::Msaa4 => 4,
            Self::Msaa8 => 8,
            _ => 1,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    #[default]
    Windowed,
    Borderless,
    Fullscreen,
}
impl DisplayMode {
    pub const ALL: [Self; 3] = [Self::Windowed, Self::Borderless, Self::Fullscreen];
    pub fn label(self) -> &'static str {
        match self {
            Self::Windowed => "Windowed",
            Self::Borderless => "Borderless",
            Self::Fullscreen => "Fullscreen",
        }
    }
}

/// Physical render sizes, also used for the window size in windowed mode.
pub const DISPLAY_RESOLUTIONS: [[u32; 2]; 18] = [
    [640, 480],
    [800, 600],
    [960, 720],
    [1024, 768],
    [1280, 720],
    [1280, 960],
    [1366, 768],
    [1600, 900],
    [1920, 1080],
    [1920, 1200],
    [1920, 1440],
    [2560, 1080],
    [2560, 1440],
    [2560, 1600],
    [3440, 1440],
    [3840, 1600],
    [3840, 2160],
    [5120, 1440],
];
pub fn default_display_resolution() -> [u32; 2] {
    [640, 480]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizeOption {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowColors {
    pub menu: [u8; 4],
    pub dialogue: [u8; 4],
    pub choice: [u8; 4],
    pub popup: [u8; 4],
    pub shade_top: [u8; 4],
    pub shade_bottom: [u8; 4],
    pub selection: [u8; 4],
}
impl WindowColors {
    pub fn groups(&self) -> [[u8; 4]; 7] {
        [
            self.menu,
            self.dialogue,
            self.choice,
            self.popup,
            self.shade_top,
            self.shade_bottom,
            self.selection,
        ]
    }
    pub fn group_mut(&mut self, index: usize) -> &mut [u8; 4] {
        [
            &mut self.menu,
            &mut self.dialogue,
            &mut self.choice,
            &mut self.popup,
            &mut self.shade_top,
            &mut self.shade_bottom,
            &mut self.selection,
        ][index]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volumes {
    pub music: u8,
    pub effects: u8,
    pub voice: u8,
    pub battle_effects: u8,
    pub battle_voice: u8,
}
impl Volumes {
    pub fn channels(&self) -> [u8; 5] {
        [
            self.music,
            self.effects,
            self.voice,
            self.battle_effects,
            self.battle_voice,
        ]
    }
    pub fn channel_mut(&mut self, index: usize) -> &mut u8 {
        [
            &mut self.music,
            &mut self.effects,
            &mut self.voice,
            &mut self.battle_effects,
            &mut self.battle_voice,
        ][index]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomizeSettings {
    /// Updates between characters; zero reveals the text immediately.
    pub message_speed: u8,
    pub battle_rank: u8,
    pub window: u8,
    pub background: u8,
    pub colors: WindowColors,
    pub volumes: Volumes,
    pub stereo: bool,
    pub button_map: [u8; 7],
    pub keymap: crate::input::Keymap,
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
    pub graphics_backend: GraphicsBackend,
    pub anti_aliasing: AntiAliasing,
    pub display_mode: DisplayMode,
    pub resolution: [u32; 2],
}
impl Default for CustomizeSettings {
    fn default() -> Self {
        Self {
            message_speed: 3,
            battle_rank: 0,
            window: 1,
            background: 5,
            colors: WindowColors {
                menu: [48, 104, 120, 216],
                dialogue: [0, 72, 144, 232],
                choice: [136, 40, 40, 232],
                popup: [24, 88, 80, 232],
                shade_top: [24, 48, 48, 216],
                shade_bottom: [32, 80, 88, 216],
                selection: [112, 144, 24, 128],
            },
            volumes: Volumes {
                music: 127,
                effects: 127,
                voice: 127,
                battle_effects: 127,
                battle_voice: 127,
            },
            stereo: true,
            button_map: [0, 1, 2, 3, 4, 5, 6],
            keymap: Default::default(),
            battle_voiceover: true,
            event_voiceover: true,
            skit_notifications: true,
            movie_subtitles: true,
            battle_auto_zoom: true,
            rumble: true,
            flexible_battle_party: false,
            combined_battles: false,
            learn_elemental_artes: false,
            learn_both_arte_paths: false,
            graphics_backend: Default::default(),
            anti_aliasing: Default::default(),
            display_mode: Default::default(),
            resolution: default_display_resolution(),
        }
    }
}
impl CustomizeSettings {
    /// Import the former seven-action permutation into the physical keymap.
    pub fn migrate_button_map(&mut self) {
        if self.button_map == [0, 1, 2, 3, 4, 5, 6] {
            return;
        }
        let old = self.keymap.bindings.clone();
        for (physical, &logical) in self.button_map.iter().enumerate() {
            self.keymap.bindings[usize::from(logical)] = old[physical].clone();
        }
        self.button_map = [0, 1, 2, 3, 4, 5, 6];
    }

    pub fn validate(&self) -> Result<()> {
        self.keymap.validate()?;
        ensure!(
            self.message_speed <= 9
                && self.battle_rank <= 2
                && self.window < 3
                && self.background < 6
                && self.volumes.channels().iter().all(|&v| v <= 127)
                && self.resolution.iter().all(|v| (1..=8192).contains(v))
                && self
                    .button_map
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    == (0..7).collect(),
            "invalid customization settings"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizeData {
    pub options: Vec<CustomizeOption>,
    pub difficulties: [String; 3],
    pub actions: [String; 7],
    pub color_groups: [String; 7],
    pub volume_channels: [String; 6],
    pub themes: [WindowColors; 3],
    pub defaults: CustomizeSettings,
    pub control_buttons: [u8; 7],
    pub labels: BTreeMap<String, String>,
    pub confirmations: BTreeMap<String, String>,
}
impl CustomizeData {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.options.len() == CUSTOMIZE_OPTIONS,
            "invalid customization option count"
        );
        self.defaults.validate()?;
        for key in crate::options::CONFIRMATIONS {
            ensure!(
                self.confirmations.get(key).is_some_and(|s| !s.is_empty()),
                "missing option confirmation {key}"
            );
        }
        for key in [
            "cancel",
            "default",
            "cancel_help",
            "default_help",
            "on",
            "off",
            "stereo",
            "mono",
            "color",
            "volume",
        ] {
            ensure!(
                self.labels.get(key).is_some_and(|s| !s.is_empty()),
                "missing customization label {key}"
            );
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.options
            .iter()
            .flat_map(|o| [&o.name, &o.description])
            .chain(&self.difficulties)
            .chain(&self.actions)
            .chain(&self.color_groups)
            .chain(&self.volume_channels)
            .chain(self.labels.values())
            .chain(self.confirmations.values())
            .map(String::as_str)
    }

    pub fn option_name(&self, row: usize) -> &str {
        if row == 6 {
            "Keymap"
        } else {
            &self.options[row].name
        }
    }
    pub fn option_description(&self, row: usize) -> &str {
        if row == 6 {
            "Map keyboard keys, controller buttons, D-pad and sticks to standard actions."
        } else {
            &self.options[row].description
        }
    }
    pub fn confirmation(&self, key: &str) -> Result<&str> {
        self.confirmations
            .get(key)
            .map(String::as_str)
            .with_context(|| format!("missing option confirmation {key}"))
    }
}

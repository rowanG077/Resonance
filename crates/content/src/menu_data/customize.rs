use super::*;

pub const CUSTOMIZE_OPTIONS: usize = 14;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizeOption {
    pub name: String,
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
    pub battle_voiceover: bool,
    pub event_voiceover: bool,
    pub skit_notifications: bool,
    pub movie_subtitles: bool,
    pub battle_auto_zoom: bool,
    pub rumble: bool,
    pub screen_position: [i16; 2],
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
            battle_voiceover: true,
            event_voiceover: true,
            skit_notifications: true,
            movie_subtitles: true,
            battle_auto_zoom: true,
            rumble: true,
            screen_position: [0; 2],
        }
    }
}
impl CustomizeSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.message_speed <= 9
                && self.battle_rank <= 2
                && self.window < 3
                && self.background < 6
                && self.volumes.channels().iter().all(|&v| v <= 127)
                && (-12..=32).contains(&self.screen_position[0])
                && (-32..=32).contains(&self.screen_position[1])
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
}
impl CustomizeData {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.options.len() == CUSTOMIZE_OPTIONS,
            "invalid customization option count"
        );
        self.defaults.validate()?;
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
            "position",
            "position_help",
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
            .map(String::as_str)
    }
}

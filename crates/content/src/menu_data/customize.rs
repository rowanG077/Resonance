use super::*;

pub const IMPORTED_CUSTOMIZE_OPTIONS: usize = 14;
pub const FLEXIBLE_BATTLE_PARTY_OPTION: usize = IMPORTED_CUSTOMIZE_OPTIONS;
pub const COMBINED_BATTLES_OPTION: usize = IMPORTED_CUSTOMIZE_OPTIONS + 1;
pub const LEARN_ELEMENTAL_ARTES_OPTION: usize = IMPORTED_CUSTOMIZE_OPTIONS + 2;
pub const LEARN_BOTH_ARTE_PATHS_OPTION: usize = IMPORTED_CUSTOMIZE_OPTIONS + 3;
pub const CUSTOMIZE_OPTIONS: usize = IMPORTED_CUSTOMIZE_OPTIONS + 4;
const COMBINED_BATTLES_NAME: &str = "Combined Battles";
const COMBINED_BATTLES_HELP: &str = "Enemies within 500 units join.\nEach extra group adds 50% to EXP,\nGald, Grade and item drop chances.";
const FLEXIBLE_BATTLE_PARTY_NAME: &str = "Flexible Battle Party";
const FLEXIBLE_BATTLE_PARTY_HELP: &str =
    "Allow required characters to stay in reserve\nfor boss battles. Your active lineup is kept.";
const LEARN_ELEMENTAL_ARTES_NAME: &str = "Learn Elemental Artes";
const LEARN_ELEMENTAL_ARTES_HELP: &str = "Learn elemental variants after\n50 uses outside Unison.\nTurning this off makes everyone\nforget their elemental artes.";
pub const FORGET_ELEMENTAL_ARTES_CONFIRMATION: &str = "Turn off elemental arte learning?\nAll party members will forget\ntheir learned elemental variants.\nAre you sure?";
const LEARN_BOTH_ARTE_PATHS_NAME: &str = "Both T/S Arte Paths";
const LEARN_BOTH_ARTE_PATHS_HELP: &str = "Learn both T and S arte paths.\nLevel and use requirements remain.\nTurning this off forgets artes\nopposite each character's T/S type.";
pub const FORGET_ARTE_PATHS_CONFIRMATION: &str = "Turn off learning both T/S paths?\nAll party members will forget\nartes opposite their T/S type.\nAre you sure?";
pub const FORGET_ARTES_CONFIRMATION: &str = "Turn off both arte learning options?\nAll party members will forget\nelemental variants and artes\nopposite their current T/S type.\nAre you sure?";

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
    pub flexible_battle_party: bool,
    pub combined_battles: bool,
    pub learn_elemental_artes: bool,
    pub learn_both_arte_paths: bool,
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
            flexible_battle_party: false,
            combined_battles: false,
            learn_elemental_artes: false,
            learn_both_arte_paths: false,
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
            self.options.len() == IMPORTED_CUSTOMIZE_OPTIONS,
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
            .chain([
                FLEXIBLE_BATTLE_PARTY_NAME,
                FLEXIBLE_BATTLE_PARTY_HELP,
                COMBINED_BATTLES_NAME,
                COMBINED_BATTLES_HELP,
                LEARN_ELEMENTAL_ARTES_NAME,
                LEARN_ELEMENTAL_ARTES_HELP,
                FORGET_ELEMENTAL_ARTES_CONFIRMATION,
                LEARN_BOTH_ARTE_PATHS_NAME,
                LEARN_BOTH_ARTE_PATHS_HELP,
                FORGET_ARTE_PATHS_CONFIRMATION,
                FORGET_ARTES_CONFIRMATION,
            ])
    }

    pub fn option_name(&self, row: usize) -> &str {
        if row == FLEXIBLE_BATTLE_PARTY_OPTION {
            FLEXIBLE_BATTLE_PARTY_NAME
        } else if row == COMBINED_BATTLES_OPTION {
            COMBINED_BATTLES_NAME
        } else if row == LEARN_ELEMENTAL_ARTES_OPTION {
            LEARN_ELEMENTAL_ARTES_NAME
        } else if row == LEARN_BOTH_ARTE_PATHS_OPTION {
            LEARN_BOTH_ARTE_PATHS_NAME
        } else {
            &self.options[row].name
        }
    }

    pub fn option_description(&self, row: usize) -> &str {
        if row == FLEXIBLE_BATTLE_PARTY_OPTION {
            FLEXIBLE_BATTLE_PARTY_HELP
        } else if row == COMBINED_BATTLES_OPTION {
            COMBINED_BATTLES_HELP
        } else if row == LEARN_ELEMENTAL_ARTES_OPTION {
            LEARN_ELEMENTAL_ARTES_HELP
        } else if row == LEARN_BOTH_ARTE_PATHS_OPTION {
            LEARN_BOTH_ARTE_PATHS_HELP
        } else {
            &self.options[row].description
        }
    }
}

use super::*;
use resonance_content::menu_data::StrategyPreset;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Character,
    Setting,
    Options,
    Presets,
    PresetCharacter,
    PresetSetting,
    PresetOptions,
    Rename,
}
impl Focus {
    pub fn preset(self) -> bool {
        matches!(
            self,
            Self::Presets
                | Self::PresetCharacter
                | Self::PresetSetting
                | Self::PresetOptions
                | Self::Rename
        )
    }
    pub fn setting(self) -> bool {
        matches!(self, Self::Setting | Self::PresetSetting)
    }
    pub fn options(self) -> bool {
        matches!(self, Self::Options | Self::PresetOptions)
    }
    pub fn character(self) -> bool {
        matches!(self, Self::Character | Self::PresetCharacter)
    }
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Strategy {
    #[serde(flatten)]
    pub transition: Transition,
    pub focus: Focus,
    pub character: usize,
    pub group: usize,
    pub option: usize,
    pub preset: usize,
    pub first: usize,
    /// Signed row-scroll pose; zero is settled and +/-1 starts a nine-pose move.
    pub scroll: i8,
    pub preset_opacity: u8,
    pub preset_closing: bool,
    pub rename_opacity: u8,
    pub description_previous: Option<[usize; 2]>,
    pub description_fade: u8,
    pub description_opacity: u8,
    pub rename: NameEditor,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct NameEditor {
    pub value: String,
    pub position: usize,
    pub column: usize,
    pub row: usize,
}

impl Menu {
    fn strategy_member_index(&self) -> usize {
        usize::from(self.party().formation[self.strategy.character] - 1)
    }
    pub fn strategy_description(&self) -> Option<[usize; 2]> {
        let state = &self.strategy;
        let option = if state.focus.options() {
            self.strategy_options()[state.option]
        } else if state.focus.setting() {
            usize::from(self.strategy_choices(self.strategy_member_index())[state.group])
        } else {
            return None;
        };
        Some([state.group, option])
    }
    pub(super) fn remember_strategy_description(&mut self) {
        if self.strategy.description_fade == 0 {
            self.strategy.description_previous = self.strategy_description();
        }
    }
    pub(super) fn fade_strategy_description(&mut self) {
        if let Some(description) = self.strategy_description() {
            let state = &mut self.strategy;
            // Keep the outgoing text until its fade ends, even during rapid navigation.
            state.description_opacity = fade_description(
                &mut state.description_fade,
                state.description_previous != Some(description),
            );
        }
    }
    pub fn strategy_presets(&self) -> &[StrategyPreset; 3] {
        self.party()
            .strategy_presets
            .as_ref()
            .unwrap_or(&self.resources.as_ref().unwrap().data.strategy.presets)
    }
    pub fn strategy_choices(&self, member: usize) -> [u8; 3] {
        if self.strategy.focus.preset() {
            self.strategy_presets()[self.strategy.preset].members[member]
        } else {
            self.party().members[member].strategy
        }
    }
    pub fn strategy_options(&self) -> Vec<usize> {
        let member = self.strategy_member_index();
        self.resources.as_ref().unwrap().data.strategy.groups[self.strategy.group]
            .iter()
            .enumerate()
            .filter_map(|(index, row)| (row.characters & (1 << member) != 0).then_some(index))
            .collect()
    }
    pub(super) fn step_strategy(
        &mut self,
        input: crate::field::FieldInput,
        directions: [bool; 4],
    ) -> Option<i16> {
        let state = &mut self.strategy;
        if state.preset_closing {
            state.preset_opacity = state.preset_opacity.saturating_sub(32);
            if state.preset_opacity != 0 {
                return None;
            }
            state.preset_closing = false;
            state.focus = Focus::Character;
        } else if state.focus.preset() && state.preset_opacity < 255 {
            state.preset_opacity = state.preset_opacity.saturating_add(32);
            if state.preset_opacity != 255 {
                return None;
            }
        }
        if self.strategy.scroll != 0 {
            self.strategy.scroll = (self.strategy.scroll + self.strategy.scroll.signum()) % 10;
            if self.strategy.scroll != 0 {
                return None;
            }
        }
        let cue = self.strategy_input(input, directions);
        let state = &mut self.strategy;
        state.rename_opacity = if state.focus == Focus::Rename {
            state.rename_opacity.saturating_add(32)
        } else {
            state.rename_opacity.saturating_sub(32)
        };
        cue
    }

    fn strategy_input(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down]: [bool; 4],
    ) -> Option<i16> {
        use Focus::*;
        let focus = self.strategy.focus;
        if focus == Rename {
            return self.rename_strategy(input, [left, right, up, down]);
        }
        if input.cancel || focus.setting() && left {
            self.strategy.focus = match focus {
                Character => {
                    self.strategy.transition.page_closing = true;
                    self.select_main(Page::Strategy);
                    Character
                }
                Setting => Character,
                Presets => {
                    self.strategy.preset_closing = true;
                    Presets
                }
                Options => Setting,
                PresetCharacter => Presets,
                PresetSetting => PresetCharacter,
                PresetOptions => PresetSetting,
                Rename => unreachable!(),
            };
            return Some(3);
        }
        if focus == Presets {
            if left || right {
                let old = self.strategy.preset;
                self.strategy.preset = if left {
                    old.saturating_sub(1)
                } else {
                    (old + 1).min(2)
                };
                return (old != self.strategy.preset).then_some(1);
            }
            if input.interact {
                self.strategy.focus = PresetCharacter;
                return Some(2);
            }
            if input.alternate {
                self.strategy.rename = NameEditor {
                    value: self.strategy_presets()[self.strategy.preset].name.clone(),
                    ..Default::default()
                };
                self.strategy.focus = Rename;
                return Some(1);
            }
            if input.menu {
                let defaults = &self.resources.as_ref().unwrap().data.strategy.presets;
                let target = &mut self
                    .checkpoint
                    .as_mut()
                    .unwrap()
                    .progress
                    .party
                    .strategy_presets
                    .get_or_insert_with(|| defaults.clone())[self.strategy.preset];
                self.party_changed |= *target != defaults[self.strategy.preset];
                *target = defaults[self.strategy.preset].clone();
                return Some(2);
            }
        } else if focus.character() {
            if input.alternate && focus == Character {
                self.strategy.focus = Presets;
                self.strategy.preset = 0;
                return Some(2);
            }
            if input.interact || right {
                self.strategy.focus = if focus.preset() {
                    PresetSetting
                } else {
                    Setting
                };
                self.strategy.group = 0;
                return Some(2);
            }
            if input.previous_page || input.next_page {
                let count = self.party().formation.len();
                let shift = super::party::page_shift(input, count, self.strategy.first);
                self.strategy.first = self.strategy.first.saturating_add_signed(shift);
                self.strategy.character = self.strategy.character.saturating_add_signed(shift);
                return (shift != 0).then_some(0x26);
            }
        } else if focus.setting() && input.interact {
            let current = usize::from(
                self.strategy_choices(self.strategy_member_index())[self.strategy.group],
            );
            self.strategy.option = self
                .strategy_options()
                .iter()
                .position(|id| *id == current)
                .unwrap_or(0);
            self.strategy.focus = if focus.preset() {
                PresetOptions
            } else {
                Options
            };
            return Some(2);
        } else if focus.options() {
            let options = self.strategy_options();
            if input.interact {
                let option = *options.get(self.strategy.option)? as u8;
                let member = self.strategy_member_index();
                let data = &self.resources.as_ref().unwrap().data.strategy;
                let result = self
                    .checkpoint
                    .as_mut()
                    .unwrap()
                    .progress
                    .party
                    .set_strategy(
                        data,
                        member,
                        self.strategy.group,
                        option,
                        focus.preset().then_some(self.strategy.preset),
                    );
                return match result {
                    Ok(changed) => {
                        self.party_changed |= changed;
                        self.strategy.focus = if focus.preset() {
                            PresetSetting
                        } else {
                            Setting
                        };
                        Some(2)
                    }
                    Err(error) => {
                        self.notice = Some(error);
                        Some(4)
                    }
                };
            }
            let old = self.strategy.option;
            if up {
                self.strategy.option = old.saturating_sub(1);
            }
            if down {
                self.strategy.option = (old + 1).min(options.len().saturating_sub(1));
            }
            return (old != self.strategy.option).then_some(1);
        }
        if (up || down) && (focus.character() || focus.setting()) {
            let count = self.party().formation.len();
            let groups = if focus.setting() { 3 } else { 1 };
            let old = self.strategy.character * groups
                + if focus.setting() {
                    self.strategy.group
                } else {
                    0
                };
            let next = if up {
                old.saturating_sub(1)
            } else {
                (old + 1).min(count * groups - 1)
            };
            self.strategy.character = next / groups;
            if focus.setting() {
                self.strategy.group = next % groups;
            }
            let first = self.strategy.first;
            self.strategy.first = self
                .strategy
                .first
                .min(self.strategy.character)
                .max(self.strategy.character.saturating_sub(3));
            self.strategy.scroll = (self.strategy.first as i8 - first as i8).signum();
            return (old != next).then_some(1);
        }
        None
    }

    fn rename_strategy(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down]: [bool; 4],
    ) -> Option<i16> {
        let original = self.strategy_presets()[self.strategy.preset].name.clone();
        let edit = &mut self.strategy.rename;
        if input.cancel {
            edit.column = 10;
            edit.row = 7;
            return Some(3);
        }
        if input.interact {
            if edit.column < 10 {
                let c = self
                    .resources
                    .as_ref()
                    .unwrap()
                    .data
                    .strategy
                    .keyboard
                    .as_bytes()[edit.row * 10 + edit.column] as char;
                if edit.position < edit.value.len() {
                    edit.value.remove(edit.position);
                }
                edit.value.insert(edit.position, c);
                edit.position = (edit.position + 1).min(6);
                return Some(2);
            }
            match edit.row {
                3 => {
                    if edit.position < edit.value.len() && edit.position < 6 {
                        edit.position += 1;
                        return Some(1);
                    }
                }
                4 => {
                    if edit.position > 0 {
                        edit.position -= 1;
                        return Some(1);
                    }
                }
                5 => {
                    if edit.position < edit.value.len() {
                        edit.value.remove(edit.position);
                        return Some(1);
                    }
                }
                6 => {
                    if edit.value.is_empty() {
                        return Some(4);
                    }
                    let defaults = &self.resources.as_ref().unwrap().data.strategy.presets;
                    let target = &mut self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .strategy_presets
                        .get_or_insert_with(|| defaults.clone())[self.strategy.preset];
                    self.party_changed |= target.name != edit.value;
                    target.name = edit.value.clone();
                    self.strategy.focus = Focus::Presets;
                    return Some(2);
                }
                7 => {
                    self.strategy.focus = Focus::Presets;
                    return Some(3);
                }
                8 => {
                    edit.value = original;
                    edit.position = edit.position.min(edit.value.len().saturating_sub(1));
                    return Some(2);
                }
                _ => unreachable!(),
            }
        }
        if input.previous_page && edit.position > 0 {
            edit.position -= 1;
            return Some(38);
        }
        if input.next_page && edit.position < edit.value.len() && edit.position < 6 {
            edit.position += 1;
            return Some(38);
        }
        if left {
            edit.column = (edit.column + 10) % 11;
        }
        if right {
            edit.column = (edit.column + 1) % 11;
        }
        if up {
            edit.row = if edit.column == 10 && edit.row <= 3 {
                8
            } else {
                (edit.row + 8) % 9
            };
        }
        if down {
            edit.row = (edit.row + 1) % 9;
        }
        if edit.column == 10 {
            edit.row = edit.row.max(3);
        }
        (left || right || up || down).then_some(1)
    }
}

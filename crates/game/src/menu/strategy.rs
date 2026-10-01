pub use super::Input;
use super::{MenuAction, Transition, TransitionStatus, fade_description};
use anyhow::{Result, ensure};
use resonance_content::menu_data::{MenuData, StrategyPreset};
use resonance_events::party::Party;

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

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct NameEditor {
    /// Start cycles the keyboard labels.
    pub mode: u8,
    pub value: String,
    pub position: usize,
    pub column: usize,
    pub row: usize,
}

/// Outcome of a Strategy page update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Visit {
    pub cue: Option<u16>,
    pub changed: bool,
    pub closed: bool,
}

/// The full page borrows its authoritative party and already prepared artwork
/// data. Neither the page state nor an immutable drawing sample owns a Party.
#[derive(Clone, Copy)]
pub struct Page<'a> {
    pub state: &'a Strategy,
    pub party: &'a Party,
    pub data: &'a MenuData,
}
impl<'a> Page<'a> {
    fn member_index(self) -> usize {
        usize::from(self.party.formation[self.state.character] - 1)
    }
    pub fn description(self) -> Option<[usize; 2]> {
        let option = if self.state.focus.options() {
            *self.options().get(self.state.option)?
        } else if self.state.focus.setting() {
            usize::from(self.choices(self.member_index())[self.state.group])
        } else {
            return None;
        };
        Some([self.state.group, option])
    }
    pub fn presets(self) -> Result<[StrategyPreset; 3]> {
        match &self.party.strategy_presets {
            Some(presets) => Ok(presets.clone()),
            None => self.data.strategy_presets(),
        }
    }
    pub fn choices(self, member: usize) -> [u8; 3] {
        if self.state.focus.preset() {
            self.party.strategy_presets.as_ref().map_or(
                self.data.strategy.presets[self.state.preset][member],
                |presets| presets[self.state.preset].members[member],
            )
        } else {
            self.party.members[member].strategy
        }
    }
    pub fn options(self) -> Vec<usize> {
        let member = self.member_index();
        self.data.strategy.groups[self.state.group]
            .iter()
            .enumerate()
            .filter_map(|(index, row)| (row.characters & (1 << member) != 0).then_some(index))
            .collect()
    }
    pub fn character_name(self, member: usize) -> &'a str {
        self.party.members[member]
            .name
            .as_deref()
            .unwrap_or(&self.data.initial_names[member])
    }
}

impl Strategy {
    /// Begin the page fade before its first drawing update.
    pub fn opening() -> Self {
        Self {
            transition: Transition::opening(),
            ..Self::default()
        }
    }
    pub fn page<'a>(&'a self, party: &'a Party, data: &'a MenuData) -> Page<'a> {
        Page {
            state: self,
            party,
            data,
        }
    }
    /// Input is already sampled by the caller's repeat owner.
    pub fn step(&mut self, input: Input, party: &mut Party, data: &MenuData) -> Result<Visit> {
        ensure!(
            (1..=8).contains(&party.formation.len())
                && party.members.len() == 9
                && party.formation.iter().all(|&id| (1..=9).contains(&id))
                && self.character < party.formation.len()
                && self.first <= self.character
                && self.group < 3
                && self.preset < 3
                && (-9..=9).contains(&self.scroll),
            "invalid Strategy page state"
        );
        ensure!(
            self.rename.mode < 3
                && self.rename.column <= 10
                && self.rename.row < 9
                && self.rename.value.is_ascii()
                && self.rename.value.len() <= 7
                && self.rename.position <= self.rename.value.len()
                && self.rename.position <= 6,
            "invalid Strategy name editor"
        );
        if self.description_fade == 0 {
            self.description_previous = self.page(party, data).description();
        }
        let visit = self.advance(input, party, data)?;
        if let Some(description) = self.page(party, data).description() {
            self.description_opacity = fade_description(
                &mut self.description_fade,
                self.description_previous != Some(description),
            );
        }
        Ok(visit)
    }

    fn advance(&mut self, input: Input, party: &mut Party, data: &MenuData) -> Result<Visit> {
        if self.preset_closing {
            self.preset_opacity = self.preset_opacity.saturating_sub(32);
            self.preset_closing = self.preset_opacity != 0;
        } else if self.focus.preset() && self.preset_opacity < 255 {
            self.preset_opacity = self.preset_opacity.saturating_add(32);
        }
        let transition = self.transition.advance();
        if self.transition.page_closing {
            return Ok(Visit {
                closed: transition == TransitionStatus::Closed,
                ..Default::default()
            });
        }
        if self.scroll != 0 {
            self.scroll = (self.scroll + self.scroll.signum()) % 10;
        }
        let mut changed = false;
        let cue = self.input(input, party, data, &mut changed)?;
        self.rename_opacity = if self.focus == Focus::Rename {
            self.rename_opacity.saturating_add(32)
        } else {
            self.rename_opacity.saturating_sub(32)
        };
        Ok(Visit {
            cue,
            changed,
            closed: false,
        })
    }

    fn input(
        &mut self,
        input: Input,
        party: &mut Party,
        data: &MenuData,
        changed: &mut bool,
    ) -> Result<Option<u16>> {
        use Focus::*;
        let focus = self.focus;
        let action = input;
        if focus == Rename {
            return self.rename(input, party, data, changed);
        }
        if action == Some(MenuAction::Cancel) || focus.setting() && action == Some(MenuAction::Left)
        {
            self.focus = match focus {
                Character => {
                    self.transition.close();
                    Character
                }
                Setting => Character,
                Presets => {
                    self.preset_closing = true;
                    Character
                }
                Options => Setting,
                PresetCharacter => Presets,
                PresetSetting => PresetCharacter,
                PresetOptions => PresetSetting,
                Rename => unreachable!(),
            };
            return Ok(Some(3));
        }
        if focus == Presets {
            if action == Some(MenuAction::Confirm) {
                self.focus = PresetCharacter;
                return Ok(Some(2));
            }
            if action == Some(MenuAction::Alternate) {
                self.rename = NameEditor {
                    value: self.page(party, data).presets()?[self.preset].name.clone(),
                    ..Default::default()
                };
                self.focus = Rename;
                return Ok(Some(1));
            }
            if action == Some(MenuAction::Menu) {
                let defaults = data.strategy_presets()?;
                let target = &mut party
                    .strategy_presets
                    .get_or_insert_with(|| defaults.clone())[self.preset];
                *changed = *target != defaults[self.preset];
                *target = defaults[self.preset].clone();
                return Ok(Some(2));
            }
            if action == Some(MenuAction::Left) || action == Some(MenuAction::Right) {
                let old = self.preset;
                self.preset = if action == Some(MenuAction::Left) {
                    old.saturating_sub(1)
                } else {
                    (old + 1).min(2)
                };
                return Ok((old != self.preset).then_some(1));
            }
        } else if focus.character() {
            if action == Some(MenuAction::Confirm) || action == Some(MenuAction::Right) {
                self.focus = if focus.preset() {
                    PresetSetting
                } else {
                    Setting
                };
                self.group = 0;
                return Ok(Some(2));
            }
            if action == Some(MenuAction::Alternate) && focus == Character {
                self.focus = Presets;
                self.preset_closing = false;
                self.preset = 0;
                return Ok(Some(2));
            }
        } else if focus.setting() && action == Some(MenuAction::Confirm) {
            let page = self.page(party, data);
            let current = usize::from(page.choices(page.member_index())[self.group]);
            self.option = page
                .options()
                .iter()
                .position(|id| *id == current)
                .unwrap_or(0);
            self.focus = if focus.preset() {
                PresetOptions
            } else {
                Options
            };
            return Ok(Some(2));
        } else if focus.options() {
            let options = self.page(party, data).options();
            if action == Some(MenuAction::Confirm) {
                let option = *options
                    .get(self.option)
                    .ok_or_else(|| anyhow::anyhow!("invalid Strategy option"))?
                    as u8;
                let member = self.page(party, data).member_index();
                if focus.preset() && party.strategy_presets.is_none() {
                    party.strategy_presets = Some(data.strategy_presets()?);
                }
                *changed = party
                    .set_strategy(
                        &data.strategy,
                        member,
                        self.group,
                        option,
                        focus.preset().then_some(self.preset),
                    )
                    .map_err(anyhow::Error::msg)?;
                self.focus = if focus.preset() {
                    PresetSetting
                } else {
                    Setting
                };
                return Ok(Some(2));
            }
            let old = self.option;
            if action == Some(MenuAction::Up) {
                self.option = old.saturating_sub(1);
            } else if action == Some(MenuAction::Down) {
                self.option = (old + 1).min(options.len().saturating_sub(1));
            }
            return Ok((old != self.option).then_some(1));
        }
        if (action == Some(MenuAction::Up) || action == Some(MenuAction::Down))
            && (focus.character() || focus.setting())
        {
            let groups = if focus.setting() { 3 } else { 1 };
            let old = self.character * groups + if focus.setting() { self.group } else { 0 };
            let next = if action == Some(MenuAction::Up) {
                old.saturating_sub(1)
            } else {
                (old + 1).min(party.formation.len() * groups - 1)
            };
            self.character = next / groups;
            if focus.setting() {
                self.group = next % groups;
            }
            let first = self.first;
            self.first = self
                .first
                .min(self.character)
                .max(self.character.saturating_sub(3));
            self.scroll = (self.first as i8 - first as i8).signum();
            return Ok((old != next).then_some(1));
        }
        if focus.character()
            && matches!(
                action,
                Some(
                    MenuAction::PageDown
                        | MenuAction::PageUp
                        | MenuAction::PreviousTab
                        | MenuAction::NextTab
                )
            )
        {
            let shift = if matches!(action, Some(MenuAction::PageDown | MenuAction::NextTab)) {
                4.min(party.formation.len().saturating_sub(self.first + 4)) as isize
            } else {
                -(4.min(self.first) as isize)
            };
            self.first = self.first.saturating_add_signed(shift);
            self.character = self.character.saturating_add_signed(shift);
            return Ok((shift != 0).then_some(38));
        }
        Ok(None)
    }

    fn rename(
        &mut self,
        input: Input,
        party: &mut Party,
        data: &MenuData,
        changed: &mut bool,
    ) -> Result<Option<u16>> {
        let original = self.page(party, data).presets()?[self.preset].name.clone();
        let edit = &mut self.rename;
        let action = input;
        if action == Some(MenuAction::Cancel) {
            edit.column = 10;
            edit.row = 7;
            return Ok(Some(3));
        }
        if action == Some(MenuAction::Confirm) {
            if edit.column < 10 {
                let c = *data
                    .strategy_text()?
                    .keyboard()?
                    .as_bytes()
                    .get(edit.row * 10 + edit.column)
                    .filter(|c| c.is_ascii_graphic() || **c == b' ')
                    .ok_or_else(|| anyhow::anyhow!("invalid Strategy keyboard character"))?
                    as char;
                if edit.position < edit.value.len() {
                    edit.value.remove(edit.position);
                }
                edit.value.insert(edit.position, c);
                edit.position = (edit.position + 1).min(6);
                return Ok(Some(2));
            }
            return Ok(match edit.row {
                3 if edit.position < edit.value.len() && edit.position < 6 => {
                    edit.position += 1;
                    Some(1)
                }
                4 if edit.position > 0 => {
                    edit.position -= 1;
                    Some(1)
                }
                5 if edit.position < edit.value.len() => {
                    edit.value.remove(edit.position);
                    Some(1)
                }
                6 if edit.value.is_empty() => Some(4),
                6 => {
                    if party.strategy_presets.is_none() {
                        party.strategy_presets = Some(data.strategy_presets()?);
                    }
                    let target = &mut party.strategy_presets.as_mut().unwrap()[self.preset];
                    *changed = target.name != edit.value;
                    target.name = edit.value.clone();
                    self.focus = Focus::Presets;
                    Some(2)
                }
                7 => {
                    self.focus = Focus::Presets;
                    Some(3)
                }
                8 => {
                    edit.value = original;
                    edit.position = edit.position.min(edit.value.len().saturating_sub(1));
                    Some(2)
                }
                _ => None,
            });
        }
        match action {
            Some(MenuAction::Left) => edit.column = (edit.column + 10) % 11,
            Some(MenuAction::Right) => edit.column = (edit.column + 1) % 11,
            Some(MenuAction::Up) => {
                edit.row = if edit.column == 10 && edit.row <= 3 {
                    8
                } else {
                    (edit.row + 8) % 9
                }
            }
            Some(MenuAction::Down) => edit.row = (edit.row + 1) % 9,
            Some(MenuAction::NextPosition | MenuAction::NextTab) => {
                return Ok(if edit.position < edit.value.len() && edit.position < 6 {
                    edit.position += 1;
                    Some(38)
                } else {
                    None
                });
            }
            Some(MenuAction::PreviousPosition | MenuAction::PreviousTab) => {
                return Ok(if edit.position > 0 {
                    edit.position -= 1;
                    Some(38)
                } else {
                    None
                });
            }
            _ if action == Some(MenuAction::Details) => {
                edit.mode = (edit.mode + 1) % 3;
                return Ok((edit.mode == 0).then_some(1));
            }
            _ => return Ok(None),
        }
        if edit.column == 10 {
            edit.row = edit.row.max(3);
        }
        Ok(Some(1))
    }
}

impl super::Menu {
    pub fn strategy_page(&self) -> Page<'_> {
        self.strategy
            .page(self.party(), &self.resources.as_ref().unwrap().data)
    }
    pub fn strategy_description(&self) -> Option<[usize; 2]> {
        self.strategy_page().description()
    }
    pub fn strategy_presets(&self) -> Result<[StrategyPreset; 3]> {
        self.strategy_page().presets()
    }
    pub fn strategy_choices(&self, member: usize) -> [u8; 3] {
        self.strategy_page().choices(member)
    }
    pub fn strategy_options(&self) -> Vec<usize> {
        self.strategy_page().options()
    }

    pub(super) fn step_strategy(&mut self, input: Input) -> Option<i16> {
        let result = self.strategy.step(
            input,
            &mut self.checkpoint.as_mut()?.progress_mut().party,
            &self.resources.as_ref()?.data,
        );
        match result {
            Ok(visit) => {
                self.party_changed |= visit.changed;
                if self.strategy.transition.page_closing {
                    self.select_main(super::Page::Strategy);
                }
                if visit.closed {
                    self.return_to_main();
                }
                visit.cue.map(|cue| cue as i16)
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                Some(4)
            }
        }
    }
}

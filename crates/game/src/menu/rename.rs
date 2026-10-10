use super::*;
use resonance_content::menu_data::RENAME_GEM;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Name,
    Keyboard,
    Commands,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Origin {
    #[default]
    Status,
    Items,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Rename {
    pub focus: Focus,
    pub origin: Origin,
    pub character: usize,
    pub value: String,
    pub position: usize,
    pub column: usize,
    pub row: usize,
    pub command: usize,
    pub fade: u8,
    pub closing: bool,
    pub pending: bool,
}

impl Menu {
    pub fn rename_data(&self) -> anyhow::Result<&menu_data::RenameData> {
        self.rename_data
            .as_ref()
            .context("name editor page has not been prepared")
    }

    pub fn character_name(&self, member: usize) -> &str {
        self.party().members[member]
            .name
            .as_deref()
            .unwrap_or(&self.resources.as_ref().unwrap().data.initial_names[member])
    }
    pub fn full_name(&self, member: usize) -> anyhow::Result<String> {
        Ok(self
            .resources
            .as_ref()
            .context("menu resources are unavailable")?
            .data
            .full_names()?
            .get(member)
            .context("unknown character full name")?
            .replace("{name}", self.character_name(member)))
    }
    pub fn can_rename(&self) -> bool {
        self.checkpoint
            .as_ref()
            .is_some_and(|c| c.progress.party.items.contains_key(&RENAME_GEM))
    }
    pub(super) fn open_rename(&mut self, origin: Origin, character: usize) -> bool {
        if !self.admit_page(Page::Rename) {
            return false;
        }
        self.rename = Rename {
            origin,
            character,
            value: self.character_name(character).into(),
            fade: 231,
            pending: true,
            ..Default::default()
        };
        if origin == Origin::Status {
            self.status.closing = true;
        } else {
            self.close_items(Page::Rename);
        }
        true
    }
    pub(super) fn advance_rename(&mut self) {
        let state = &mut self.rename;
        let finished = state.closing && state.fade == 255;
        state.pending = false;
        state.fade = if state.closing {
            state.fade.saturating_add(25)
        } else {
            state.fade.saturating_sub(25)
        };
        if finished {
            self.page = match state.origin {
                Origin::Status => {
                    self.status.page_fade = 206;
                    Page::Status
                }
                Origin::Items => {
                    self.open_items();
                    Page::Items
                }
            };
        }
    }
    pub(super) fn step_rename(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let [left, right, up, down] = [Left, Right, Up, Down].map(|action| input == Some(action));
        let original = self.character_name(self.rename.character).to_owned();
        let data = match self
            .rename_data
            .as_ref()
            .context("name editor page has not been prepared")
        {
            Ok(data) => data,
            Err(error) => {
                self.report_failure("Name editor is unavailable", error);
                return Some(4);
            }
        };
        let names = &self.resources.as_ref()?.data.initial_names;
        let state = &mut self.rename;
        if state.pending || state.fade != 0 || state.closing {
            return None;
        }
        match state.focus {
            Focus::Name => {
                if input == Some(Cancel) || up {
                    state.focus = Focus::Commands;
                    state.command = 0;
                    return Some(1);
                }
                if input == Some(Confirm) || down {
                    state.focus = Focus::Keyboard;
                    let key = state
                        .value
                        .as_bytes()
                        .get(state.position)
                        .and_then(|c| data.keyboard.as_bytes().iter().position(|b| b == c))
                        .unwrap_or(0);
                    state.column = key % 13;
                    state.row = key / 13;
                    return Some(if input == Some(Confirm) { 2 } else { 1 });
                }
                if input == Some(Menu) {
                    state.value.clone_from(&data.defaults[state.character]);
                    state.position = 0;
                    return Some(2);
                }
                if input == Some(Alternate) {
                    if state.position < state.value.len() {
                        state.value.remove(state.position);
                    } else if state.position > 0 {
                        state.position -= 1;
                        state.value.truncate(state.position);
                    } else {
                        return None;
                    }
                    return Some(1);
                }
                if left && state.position > 0 {
                    state.position -= 1;
                    return Some(1);
                }
                if right && state.position < 5 && state.position < state.value.len() {
                    state.position += 1;
                    return Some(1);
                }
            }
            Focus::Keyboard => {
                if input == Some(Cancel) {
                    state.focus = Focus::Name;
                    return Some(3);
                }
                if input == Some(Confirm) {
                    if state.position < state.value.len() {
                        state.value.remove(state.position);
                    }
                    state.value.insert(
                        state.position,
                        data.keyboard.as_bytes()[state.row * 13 + state.column] as char,
                    );
                    state.position = (state.position + 1).min(5);
                    return Some(2);
                }
                if left {
                    state.column = (state.column + 12) % 13;
                }
                if right {
                    state.column = (state.column + 1) % 13;
                }
                if up {
                    state.row = (state.row + 7) % 8;
                }
                if down {
                    state.row = (state.row + 1) % 8;
                }
                return (left || right || up || down).then_some(1);
            }
            Focus::Commands => {
                if input == Some(Cancel) || down {
                    state.focus = Focus::Name;
                    return Some(1);
                }
                if input == Some(Confirm) {
                    match state.command {
                        0 => {
                            if state.value.is_empty() {
                                return Some(4);
                            }
                            let member = &mut self.checkpoint.as_mut()?.progress.party.members
                                [state.character];
                            self.party_changed |= state.value != original;
                            member.name = (state.value != names[state.character])
                                .then(|| state.value.clone());
                            state.closing = true;
                            return Some(2);
                        }
                        1 => {
                            state.value = original;
                            state.position = state.position.min(state.value.len());
                            return Some(2);
                        }
                        2 => {
                            state.closing = true;
                            return Some(3);
                        }
                        _ => unreachable!(),
                    }
                }
                if left {
                    state.command = (state.command + 2) % 3;
                }
                if right {
                    state.command = (state.command + 1) % 3;
                }
                return (left || right).then_some(1);
            }
        }
        None
    }
}

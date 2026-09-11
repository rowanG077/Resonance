use super::*;
use resonance_events::party::TechniqueShortcut;

const UNLOCK_STORY: i32 = 1_403_000;
pub const VISIBLE_TECHNIQUES: usize = 8;

#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Slots,
    List,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Unison {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub focus: Focus,
    pub character: usize,
    pub slot: usize,
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    pub list_opacity: u8,
    pub list_closing: bool,
    pub description_previous: Option<TechniqueShortcut>,
    pub description_fade: u8,
    pub description_opacity: u8,
}

impl Unison {
    pub fn animating(&self) -> bool {
        self.transition.animating()
            || self.scroll != 0
            || self.list_closing
            || self.focus == Focus::List && self.list_opacity != 255
    }
}

impl Menu {
    pub(super) fn remember_unison_description(&mut self) {
        if self.unison.description_fade == 0 {
            self.unison.description_previous = self.unison_selection();
        }
    }
    pub(super) fn fade_unison_description(&mut self) {
        let changed = self.unison.description_fade == 0
            && self.unison.description_previous.map(|s| s.technique)
                != self.unison_selection().map(|s| s.technique);
        self.unison.description_opacity =
            fade_description(&mut self.unison.description_fade, changed);
    }
    pub fn has_unison(&self) -> bool {
        self.resources.is_some()
            && self
                .checkpoint
                .as_ref()
                .is_some_and(|c| c.progress.script_globals[16] >= UNLOCK_STORY)
    }

    pub fn unison_party_count(&self) -> usize {
        self.party().formation.len().min(VISIBLE_PARTY)
    }

    pub fn unison_member_index(&self) -> usize {
        usize::from(self.party().formation[self.unison.character] - 1)
    }

    pub fn unison_techniques(&self) -> Vec<u16> {
        let member = self.unison_member_index();
        let party = self.party();
        self.resources.as_ref().unwrap().session.characters[member]
            .allowed_techniques
            .iter()
            .copied()
            .filter(|id| party.members[member].techniques.contains(id))
            .collect()
    }

    pub fn unison_selection(&self) -> Option<TechniqueShortcut> {
        let character = self.unison_member_index();
        let technique = if self.unison.focus == Focus::List {
            *self.unison_techniques().get(self.unison.row)?
        } else {
            self.party().members[character].shortcuts[self.unison.slot]
        };
        (technique != 0).then_some(TechniqueShortcut {
            character,
            technique,
        })
    }

    pub(super) fn open_unison(&mut self) {
        self.unison = Unison {
            transition: super::Transition::opening(),
            character: if self.unison.character < self.unison_party_count() {
                self.unison.character
            } else {
                0
            },
            description_fade: DESCRIPTION_FADE_START,
            description_opacity: 15,
            ..Default::default()
        };
        self.entering = Some(Page::Unison);
    }

    pub(super) fn step_unison(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let state = &mut self.unison;
        if state.focus == Focus::List {
            state.scroll = (state.scroll + state.scroll.signum()) % 5;
            if state.list_closing {
                state.list_opacity = state.list_opacity.saturating_sub(32);
                if state.list_opacity == 0 {
                    state.list_closing = false;
                    state.focus = Focus::Slots;
                }
            } else {
                state.list_opacity = state.list_opacity.saturating_add(32);
            }
        }
        if state.animating() {
            return None;
        }
        if input.cancel {
            if self.unison.focus == Focus::List {
                self.unison.list_closing = true;
            } else {
                self.character = self.unison.character;
                self.unison.transition.page_closing = true;
                self.select_main(Page::Unison);
            }
            return Some(3);
        }
        if self.unison.focus == Focus::Slots {
            let count = self.unison_party_count();
            let before = (self.unison.character, self.unison.slot);
            let state = &mut self.unison;
            if up {
                if state.slot > 0 {
                    state.slot -= 1;
                } else if state.character > 0 {
                    state.character -= 1;
                    state.slot = 3;
                }
            } else if down {
                if state.slot < 3 {
                    state.slot += 1;
                } else if state.character + 1 < count {
                    state.character += 1;
                    state.slot = 0;
                }
            } else if left && state.character >= 2 {
                state.character -= 2;
            } else if right && state.character + 2 < count {
                state.character += 2;
            } else if input.interact {
                let choices = self.unison_techniques();
                if choices.is_empty() {
                    return Some(4);
                }
                let equipped = self.unison_selection().map(|s| s.technique);
                self.unison.row = choices
                    .iter()
                    .position(|&id| Some(id) == equipped)
                    .unwrap_or(0);
                self.unison.first = self.unison.row.saturating_sub(VISIBLE_TECHNIQUES - 1);
                self.unison.focus = Focus::List;
                self.unison.list_opacity = 0;
                return Some(1);
            } else if input.alternate {
                let member = self.unison_member_index();
                let changed = self
                    .checkpoint
                    .as_mut()
                    .unwrap()
                    .progress
                    .party
                    .assign_technique(member, self.unison.slot, None)
                    .expect("validated unison shortcut");
                self.party_changed |= changed;
                return changed.then_some(1);
            }
            return (before != (self.unison.character, self.unison.slot)).then_some(1);
        }
        if input.interact {
            let member = self.unison_member_index();
            let selected = self.unison_selection();
            let changed = self
                .checkpoint
                .as_mut()
                .unwrap()
                .progress
                .party
                .assign_technique(member, self.unison.slot, selected)
                .expect("validated unison technique selection");
            self.party_changed |= changed;
            self.unison.list_closing = true;
            return Some(2);
        }
        let count = self.unison_techniques().len();
        let state = &mut self.unison;
        let before = (state.row, state.first);
        if page_up {
            let first = state.first.saturating_sub(VISIBLE_TECHNIQUES);
            state.row = if state.first == 0 {
                0
            } else {
                state.row - (state.first - first)
            };
            state.first = first;
        } else if page_down {
            if state.first + VISIBLE_TECHNIQUES < count {
                state.first += VISIBLE_TECHNIQUES;
                state.row = (state.row + VISIBLE_TECHNIQUES).min(count - 1);
            } else {
                state.row = count - 1;
            }
        } else if up || down {
            state.row = if up {
                state.row.saturating_sub(1)
            } else {
                (state.row + 1).min(count - 1)
            };
            state.first = state
                .first
                .min(state.row)
                .max(state.row.saturating_sub(VISIBLE_TECHNIQUES - 1));
            state.scroll = match state.first.cmp(&before.1) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
        }
        (before != (state.row, state.first)).then_some(if page_up || page_down { 38 } else { 1 })
    }
}

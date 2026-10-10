pub use super::Input;
use super::techniques::{Edit, EditResult, shortcut_choice, shortcut_selection};
use super::{MenuAction, VISIBLE_PARTY};
use anyhow::{Result, ensure};
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::party::{Party, TechniqueShortcut};

pub const UNLOCK_STORY: i32 = 1_403_000;
pub use super::techniques::VISIBLE_SHORTCUT_CHOICES as VISIBLE_TECHNIQUES;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Slots,
    List,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Unison {
    pub focus: Focus,
    pub character: usize,
    pub slot: usize,
    pub row: usize,
    pub first: usize,
}

/// Outcome of a Unison page update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub cue: Option<u16>,
    pub changed: bool,
    pub closed: bool,
}

#[derive(Clone, Copy)]
pub struct Page<'a> {
    pub state: &'a Unison,
    pub party: &'a Party,
    pub session: &'a SessionData,
    pub data: &'a MenuData,
}
impl<'a> Page<'a> {
    pub fn party_count(self) -> usize {
        self.party.formation.len().min(VISIBLE_PARTY)
    }
    pub fn member_index(self) -> Option<usize> {
        if self.state.character >= self.party_count() {
            return None;
        }
        let id = *self.party.formation.get(self.state.character)?;
        let member = usize::from(id.checked_sub(1)?);
        self.party.members.get(member)?;
        Some(member)
    }
    /// Learned techniques in catalogue order; availability controls display colour.
    pub fn techniques(self) -> Vec<u16> {
        let Some(member) = self.member_index() else {
            return Vec::new();
        };
        let Some(definition) = self.session.characters.get(member) else {
            return Vec::new();
        };
        definition
            .allowed_techniques
            .iter()
            .copied()
            .filter(|id| self.party.members[member].techniques.contains(id))
            .collect()
    }
    pub fn selection(self) -> Option<TechniqueShortcut> {
        let character = self.member_index()?;
        if self.state.focus == Focus::List {
            self.techniques()
                .get(self.state.row)
                .map(|&technique| TechniqueShortcut {
                    character,
                    technique,
                })
        } else {
            shortcut_selection(self.party, character, self.state.slot)
        }
    }
    pub fn character_name(self, member: usize) -> Option<&'a str> {
        self.party
            .members
            .get(member)?
            .name
            .as_deref()
            .or_else(|| self.data.initial_names.get(member).map(String::as_str))
    }
}

impl Unison {
    /// Open the page and retain a valid active-party selection.
    pub fn opening(remembered_character: usize, party: &Party) -> Result<Self> {
        let count = party.formation.len().min(VISIBLE_PARTY);
        ensure!(
            count != 0
                && party.members.len() == 9
                && party.formation[..count]
                    .iter()
                    .all(|id| (1..=9).contains(id)),
            "invalid U. Attack formation"
        );
        Ok(Self {
            character: if remembered_character < count {
                remembered_character
            } else {
                0
            },
            ..Self::default()
        })
    }
    pub fn page<'a>(
        &'a self,
        party: &'a Party,
        session: &'a SessionData,
        data: &'a MenuData,
    ) -> Page<'a> {
        Page {
            state: self,
            party,
            session,
            data,
        }
    }
    /// Field convenience path; battle supplies its own prevalidated binding commit.
    pub fn step(
        &mut self,
        input: Input,
        party: &mut Party,
        session: &SessionData,
        data: &MenuData,
    ) -> Result<Visit> {
        self.step_with_edit(input, party, session, data, |party, edit| {
            edit.apply_field(party, data)
        })
    }
    /// The caller retains its own repeat owner.
    /// The callback is called at most once and must validate before mutation.
    /// It owns the only Party/live-binding commit. This page never reapplies it.
    pub fn step_with_edit(
        &mut self,
        input: Input,
        party: &mut Party,
        session: &SessionData,
        data: &MenuData,
        mut edit: impl FnMut(&mut Party, Edit) -> Result<EditResult>,
    ) -> Result<Visit> {
        let (cue, changed, closed) = self.advance(input, party, session, data, &mut edit)?;
        Ok(Visit {
            cue,
            changed,
            closed,
        })
    }
    fn advance(
        &mut self,
        input: Input,
        party: &mut Party,
        session: &SessionData,
        data: &MenuData,
        edit: &mut impl FnMut(&mut Party, Edit) -> Result<EditResult>,
    ) -> Result<(Option<u16>, bool, bool)> {
        let action = input;
        if action.is_none() {
            return Ok((None, false, false));
        }
        if action == Some(MenuAction::Cancel) {
            if self.focus == Focus::List {
                self.focus = Focus::Slots;
                return Ok((Some(3), false, false));
            }
            return Ok((Some(3), false, true));
        }
        if self.focus == Focus::Slots {
            let count = party.formation.len().min(VISIBLE_PARTY);
            let before = (self.character, self.slot);
            match action {
                Some(MenuAction::Up) if self.slot != 0 => self.slot -= 1,
                Some(MenuAction::Up) if self.character != 0 => {
                    self.character -= 1;
                    self.slot = 3;
                }
                Some(MenuAction::Down) if self.slot < 3 => self.slot += 1,
                Some(MenuAction::Down) if self.character + 1 < count => {
                    self.character += 1;
                    self.slot = 0;
                }
                Some(MenuAction::Left) if self.character >= 2 => self.character -= 2,
                Some(MenuAction::Right) if self.character + 2 < count => self.character += 2,
                Some(MenuAction::Left | MenuAction::Right | MenuAction::Up | MenuAction::Down) => {}
                _ if action == Some(MenuAction::Alternate) => {
                    let page = self.page(party, session, data);
                    if page.selection().is_some() {
                        let member = page.member_index().unwrap();
                        let changed = edit(
                            party,
                            Edit::Shortcut {
                                member,
                                slot: self.slot,
                                selected: None,
                            },
                        )?
                        .changed;
                        return Ok((Some(1), changed, false));
                    }
                }
                _ if action == Some(MenuAction::Confirm) => {
                    let page = self.page(party, session, data);
                    let choices = page.techniques();
                    let Some((row, first)) = shortcut_choice(&choices, page.selection()) else {
                        return Ok((Some(4), false, false));
                    };
                    self.row = row;
                    self.first = first;
                    self.focus = Focus::List;
                    return Ok((Some(1), false, false));
                }
                _ => {}
            }
            return Ok((
                (before != (self.character, self.slot)).then_some(1),
                false,
                false,
            ));
        }
        if action == Some(MenuAction::Confirm) {
            let page = self.page(party, session, data);
            let Some(selected) = page.selection() else {
                return Ok((Some(4), false, false));
            };
            ensure!(
                selected.technique != 0
                    && data
                        .techniques
                        .get(usize::from(selected.technique))
                        .is_some(),
                "selected U. Attack technique description is missing"
            );
            let changed = edit(
                party,
                Edit::Shortcut {
                    member: selected.character,
                    slot: self.slot,
                    selected: Some(selected),
                },
            )?
            .changed;
            self.focus = Focus::Slots;
            return Ok((Some(2), changed, false));
        }
        let count = self.page(party, session, data).techniques().len();
        if count == 0 {
            self.row = 0;
            self.first = 0;
            return Ok((None, false, false));
        }
        self.row = self.row.min(count - 1);
        self.first = self.first.min(self.row);
        let before = (self.row, self.first);
        match action {
            Some(MenuAction::Up) if self.row != 0 => {
                self.row -= 1;
                if self.first > self.row {
                    self.first -= 1;
                }
            }
            Some(MenuAction::Down) if self.row + 1 < count => {
                self.row += 1;
                if self.first + VISIBLE_TECHNIQUES <= self.row {
                    self.first += 1;
                }
            }
            Some(MenuAction::PageUp | MenuAction::PreviousTab) => {
                let first = self.first.saturating_sub(VISIBLE_TECHNIQUES);
                self.row = if self.first == 0 {
                    0
                } else {
                    self.row - (self.first - first)
                };
                self.first = first;
            }
            Some(MenuAction::PageDown | MenuAction::NextTab) => {
                if self.first + VISIBLE_TECHNIQUES < count {
                    self.first += VISIBLE_TECHNIQUES;
                    self.row = (self.row + VISIBLE_TECHNIQUES).min(count - 1);
                } else {
                    self.row = count - 1;
                }
            }
            _ => {}
        }
        Ok((
            (before != (self.row, self.first)).then_some(
                if matches!(
                    action,
                    Some(
                        MenuAction::PageDown
                            | MenuAction::PageUp
                            | MenuAction::PreviousTab
                            | MenuAction::NextTab
                    )
                ) {
                    38
                } else {
                    1
                },
            ),
            false,
            false,
        ))
    }
}

impl super::Menu {
    pub fn unison_page(&self) -> Page<'_> {
        let resources = self.resources.as_ref().unwrap();
        self.unison
            .page(self.party(), &resources.session, &resources.data)
    }
    pub fn has_unison(&self) -> bool {
        self.resources.is_some()
            && self.checkpoint.as_ref().is_some_and(|c| {
                c.progress
                    .script_globals
                    .get(16)
                    .is_some_and(|&story| story >= UNLOCK_STORY)
            })
    }
    pub fn unison_party_count(&self) -> usize {
        self.unison_page().party_count()
    }
    pub fn unison_member_index(&self) -> usize {
        self.unison_page()
            .member_index()
            .expect("validated U. Attack formation")
    }
    pub fn unison_techniques(&self) -> Vec<u16> {
        self.unison_page().techniques()
    }
    pub fn unison_selection(&self) -> Option<TechniqueShortcut> {
        self.unison_page().selection()
    }

    pub(super) fn open_unison(&mut self) -> Option<i16> {
        self.resources.as_ref()?;
        match Unison::opening(self.unison.character, self.party()) {
            Ok(state) => {
                self.unison = state;
                self.entering = Some(super::Page::Unison);
                Some(2)
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                Some(4)
            }
        }
    }
    pub(super) fn step_unison(&mut self, input: Input) -> Option<i16> {
        let resources = self.resources.as_ref()?;
        let visit = self.unison.step(
            input,
            &mut self.checkpoint.as_mut()?.progress.party,
            &resources.session,
            &resources.data,
        );
        match visit {
            Ok(visit) => {
                self.party_changed |= visit.changed;
                if visit.closed {
                    self.character = self.unison.character;
                    self.select_main(super::Page::Unison);
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

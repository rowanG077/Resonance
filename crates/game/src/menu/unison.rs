//! One independent Unison arte per party member; no physical shortcut slots.
pub use super::Input;
use super::techniques::{Edit, EditResult};
use super::{MenuAction, VISIBLE_PARTY};
use anyhow::{Result, ensure};
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::party::{Party, TechniqueShortcut};

pub const UNLOCK_STORY: i32 = 1_403_000;
pub const VISIBLE_TECHNIQUES: usize = 8;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Loadout,
    List,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Unison {
    pub focus: Focus,
    pub character: usize,
    pub row: usize,
    pub first: usize,
}
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
        let member = usize::from(
            self.party
                .formation
                .get(self.state.character)?
                .checked_sub(1)?,
        );
        self.party.members.get(member)?;
        Some(member)
    }
    pub fn techniques(self) -> Vec<u16> {
        let Some(member) = self.member_index() else {
            return Vec::new();
        };
        self.session
            .characters
            .get(member)
            .into_iter()
            .flat_map(|c| c.allowed_techniques.iter().copied())
            .filter(|id| {
                self.party.members[member].techniques.contains(id)
                    && self
                        .data
                        .techniques
                        .get(usize::from(*id))
                        .is_some_and(|t| t.unison_usable)
            })
            .collect()
    }
    pub fn selection(self) -> Option<TechniqueShortcut> {
        let character = self.member_index()?;
        let technique = if self.state.focus == Focus::List {
            *self.techniques().get(self.state.row)?
        } else {
            self.party.members[character].unison_arte?
        };
        Some(TechniqueShortcut {
            character,
            technique,
        })
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
    pub fn opening(remembered: usize, party: &Party) -> Result<Self> {
        let count = party.formation.len().min(VISIBLE_PARTY);
        ensure!(
            count != 0
                && party.members.len() == 9
                && party.formation[..count]
                    .iter()
                    .all(|id| (1..=9).contains(id)),
            "invalid Unison formation"
        );
        Ok(Self {
            character: if remembered < count { remembered } else { 0 },
            ..Default::default()
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
    pub fn step_with_edit(
        &mut self,
        input: Input,
        party: &mut Party,
        session: &SessionData,
        data: &MenuData,
        mut edit: impl FnMut(&mut Party, Edit) -> Result<EditResult>,
    ) -> Result<Visit> {
        let mut visit = Visit {
            cue: None,
            changed: false,
            closed: false,
        };
        let Some(action) = input else {
            return Ok(visit);
        };
        if action == MenuAction::Cancel {
            if self.focus == Focus::List {
                self.focus = Focus::Loadout;
            } else {
                visit.closed = true;
            }
            visit.cue = Some(3);
            return Ok(visit);
        }
        if self.focus == Focus::Loadout {
            let count = self.page(party, session, data).party_count();
            match action {
                MenuAction::Up | MenuAction::Left | MenuAction::PreviousTab => {
                    self.character = (self.character + count - 1) % count;
                    visit.cue = Some(1);
                }
                MenuAction::Down | MenuAction::Right | MenuAction::NextTab => {
                    self.character = (self.character + 1) % count;
                    visit.cue = Some(1);
                }
                MenuAction::Alternate => {
                    let member = self.page(party, session, data).member_index().unwrap();
                    visit.changed = edit(
                        party,
                        Edit::Unison {
                            member,
                            technique: None,
                        },
                    )?
                    .changed;
                    visit.cue = Some(1);
                }
                MenuAction::Confirm => {
                    let page = self.page(party, session, data);
                    let choices = page.techniques();
                    if choices.is_empty() {
                        visit.cue = Some(4);
                        return Ok(visit);
                    }
                    self.row = page
                        .selection()
                        .and_then(|s| choices.iter().position(|&id| id == s.technique))
                        .unwrap_or(0);
                    self.first = self.row.saturating_sub(VISIBLE_TECHNIQUES - 1);
                    self.focus = Focus::List;
                    visit.cue = Some(1);
                }
                _ => {}
            }
            return Ok(visit);
        }
        if action == MenuAction::Confirm {
            let Some(selected) = self.page(party, session, data).selection() else {
                visit.cue = Some(4);
                return Ok(visit);
            };
            visit.changed = edit(
                party,
                Edit::Unison {
                    member: selected.character,
                    technique: Some(selected.technique),
                },
            )?
            .changed;
            self.focus = Focus::Loadout;
            visit.cue = Some(2);
            return Ok(visit);
        }
        let count = self.page(party, session, data).techniques().len();
        if count == 0 {
            self.row = 0;
            self.first = 0;
            return Ok(visit);
        }
        let delta = match action {
            MenuAction::Up => -1,
            MenuAction::Down => 1,
            MenuAction::PageUp | MenuAction::PreviousTab => -(VISIBLE_TECHNIQUES as isize),
            MenuAction::PageDown | MenuAction::NextTab => VISIBLE_TECHNIQUES as isize,
            _ => return Ok(visit),
        };
        let before = (self.row, self.first);
        super::move_list(
            &mut self.row,
            &mut self.first,
            count,
            VISIBLE_TECHNIQUES,
            delta,
        );
        visit.cue = (before != (self.row, self.first)).then_some(if delta.unsigned_abs() > 1 {
            38
        } else {
            1
        });
        Ok(visit)
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
                c.progress()
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
            &mut self.checkpoint.as_mut()?.progress_mut().party,
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

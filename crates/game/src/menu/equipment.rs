use super::*;
use anyhow::{Context, ensure};

pub const VISIBLE_EQUIPMENT: usize = 9;
/// Display order; saved parties store the arm slot after both accessories.
pub const SLOTS: [usize; 6] = [0, 1, 2, 5, 3, 4];
pub const LABELS: [&str; 6] = [
    "weapon",
    "body",
    "head",
    "arm",
    "accessory_1",
    "accessory_2",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    Character,
    #[default]
    Slots,
    List,
    Optimal {
        thrust: bool,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Equipment {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub focus: Focus,
    pub slot: usize,
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    pub by_parameter: bool,
    pub description_previous: Option<u16>,
    pub description_fade: u8,
    pub description_opacity: u8,
}
impl Default for Equipment {
    fn default() -> Self {
        Self {
            transition: Default::default(),
            focus: Default::default(),
            slot: 0,
            row: 0,
            first: 0,
            scroll: 0,
            by_parameter: false,
            description_previous: None,
            description_fade: 0,
            description_opacity: 255,
        }
    }
}
impl Equipment {
    pub fn opening() -> Self {
        Self {
            transition: super::Transition::opening(),
            description_fade: DESCRIPTION_FADE_START,
            ..Default::default()
        }
    }

    fn reset_list(&mut self) {
        self.row = 0;
        self.first = 0;
        self.scroll = 0;
    }

    /// Update the shared page and commit accepted edits to its party.
    pub fn step_shared(
        &mut self,
        input: Input,
        party: &mut resonance_events::party::Party,
        session: &resonance_content::session::SessionData,
        data: &resonance_content::menu_data::MenuData,
        member: &mut usize,
    ) -> anyhow::Result<Visit> {
        if self.description_fade == 0 {
            self.description_previous = self.selected_item(party, session, data, *member);
        }
        let visit = self.advance(input, party, session, data, member)?;
        let selected = self.selected_item(party, session, data, *member);
        self.description_opacity = fade_description(
            &mut self.description_fade,
            self.description_previous != selected,
        );
        Ok(visit)
    }

    fn selected_item(
        &self,
        party: &resonance_events::party::Party,
        session: &resonance_content::session::SessionData,
        data: &resonance_content::menu_data::MenuData,
        character: usize,
    ) -> Option<u16> {
        let member = selected_member(party, character).ok()?;
        match self.focus {
            Focus::Slots => {
                Some(party.members[member].equipment[SLOTS[self.slot]]).filter(|id| *id != 0)
            }
            Focus::List => equipment_items_for(party, session, data, member, self)
                .get(self.row)
                .copied(),
            _ => None,
        }
    }

    fn advance(
        &mut self,
        input: Input,
        party: &mut resonance_events::party::Party,
        session: &resonance_content::session::SessionData,
        data: &resonance_content::menu_data::MenuData,
        member: &mut usize,
    ) -> anyhow::Result<Visit> {
        let action = input;
        let left = action == Some(MenuAction::Left);
        let right = action == Some(MenuAction::Right);
        let up = action == Some(MenuAction::Up);
        let down = action == Some(MenuAction::Down);
        let page_up = matches!(action, Some(MenuAction::PageUp | MenuAction::PreviousTab));
        let page_down = matches!(action, Some(MenuAction::PageDown | MenuAction::NextTab));
        let old_character = *member;
        let old_member = selected_member(party, *member)?;
        let old_equipment = party.members[old_member].equipment;
        let old_item = party
            .members
            .get(old_member)
            .and_then(|row| row.equipment.get(SLOTS[self.slot]).copied())
            .context("equipment page member is absent")?;

        let transition = self.transition.advance();
        if self.transition.page_closing {
            return Ok(Visit {
                closed: transition == TransitionStatus::Closed,
                ..Default::default()
            });
        }
        self.scroll = (self.scroll + self.scroll.signum()) % 5;

        if action == Some(MenuAction::Cancel) {
            match self.focus {
                Focus::Character => self.transition.close(),
                Focus::Slots | Focus::Optimal { .. } => self.focus = Focus::Character,
                Focus::List => {
                    self.focus = Focus::Slots;
                    self.reset_list();
                }
            }
            return Ok(Visit {
                cue: Some(3),
                ..Default::default()
            });
        }

        if matches!(self.focus, Focus::Character | Focus::Slots)
            && (page_up || page_down || self.focus == Focus::Character && (left || right))
        {
            let count = party.formation.len().min(VISIBLE_PARTY);
            ensure!(count != 0, "equipment page has no active party");
            let back = page_up || self.focus == Focus::Character && left;
            for _ in 0..count {
                *member = (*member + if back { count.saturating_sub(1) } else { 1 }) % count;
                let selected = selected_member(party, *member)?;
                if !party.members[selected].knocked_out() {
                    break;
                }
            }
            self.reset_list();
            let changed = old_character != *member;
            return Ok(Visit {
                cue: changed.then_some(1),
                ..Default::default()
            });
        }

        let mut cue = None;
        match self.focus {
            Focus::Character => {
                if action == Some(MenuAction::Menu) {
                    if old_member == 0 {
                        self.focus = Focus::Optimal { thrust: false };
                        cue = Some(1);
                    } else {
                        let before = party.members[old_member].equipment;
                        party
                            .optimize_equipment(session, data, old_member, false)
                            .map_err(anyhow::Error::msg)?;
                        cue = (party.members[old_member].equipment != before).then_some(1);
                    }
                } else if action == Some(MenuAction::Confirm) || up || down {
                    self.focus = Focus::Slots;
                    self.slot = if up { SLOTS.len() - 1 } else { 0 };
                    self.reset_list();
                    cue = Some(if action == Some(MenuAction::Confirm) {
                        2
                    } else {
                        1
                    });
                }
            }
            Focus::Optimal { thrust } => {
                if up || down {
                    self.focus = Focus::Optimal { thrust: !thrust };
                    cue = Some(1);
                } else if action == Some(MenuAction::Confirm) {
                    let before = party.members[old_member].equipment;
                    party
                        .optimize_equipment(session, data, old_member, thrust)
                        .map_err(anyhow::Error::msg)?;
                    cue = (party.members[old_member].equipment != before).then_some(1);
                }
            }
            Focus::Slots | Focus::List => {
                if action == Some(MenuAction::Menu) {
                    self.by_parameter = !self.by_parameter;
                    cue = Some(1);
                } else if self.focus == Focus::Slots {
                    if action == Some(MenuAction::Alternate) {
                        if self.slot == 0
                            || old_item == 0
                            || party.members[old_member].knocked_out()
                        {
                            cue = Some(4);
                        } else {
                            party
                                .equip_slot(session, old_member, SLOTS[self.slot], 0)
                                .map_err(anyhow::Error::msg)?;
                            self.reset_list();
                            cue = Some(1);
                        }
                    } else if action == Some(MenuAction::Confirm) {
                        let items = equipment_items_for(party, session, data, old_member, self);
                        if items.is_empty() || party.members[old_member].knocked_out() {
                            cue = Some(4);
                        } else {
                            self.focus = Focus::List;
                            self.reset_list();
                            cue = Some(2);
                        }
                    } else if up || down {
                        if up && self.slot == 0 || down && self.slot == SLOTS.len() - 1 {
                            self.focus = Focus::Character;
                        } else if up {
                            self.slot -= 1;
                        } else {
                            self.slot += 1;
                        }
                        self.reset_list();
                        cue = Some(1);
                    }
                } else {
                    let items = equipment_items_for(party, session, data, old_member, self);
                    if action == Some(MenuAction::Confirm) {
                        if let Some(&id) = items.get(self.row) {
                            party
                                .equip_slot(session, old_member, SLOTS[self.slot], id)
                                .map_err(anyhow::Error::msg)?;
                            self.focus = Focus::Slots;
                            self.reset_list();
                            cue = Some(2);
                        } else {
                            cue = Some(4);
                        }
                    } else {
                        let old = self.row;
                        let first = self.first;
                        if up {
                            self.row = self.row.saturating_sub(1);
                        } else if down {
                            self.row = (self.row + 1).min(items.len().saturating_sub(1));
                        } else if page_up && first > 0 {
                            self.first = first.saturating_sub(VISIBLE_EQUIPMENT);
                            self.row -= first - self.first;
                            cue = Some(38);
                        } else if page_down && first + VISIBLE_EQUIPMENT < items.len() {
                            self.first += VISIBLE_EQUIPMENT;
                            self.row = (self.row + VISIBLE_EQUIPMENT).min(items.len() - 1);
                            cue = Some(38);
                        }
                        self.first = self
                            .first
                            .min(self.row)
                            .max(self.row.saturating_sub(VISIBLE_EQUIPMENT - 1));
                        if cue.is_none() && self.first != first {
                            self.scroll = (self.first as isize - first as isize).signum() as i8;
                        }
                        if cue.is_none() && old != self.row {
                            cue = Some(1);
                        }
                    }
                }
            }
        }
        let changed = old_equipment != party.members[old_member].equipment;
        Ok(Visit {
            cue,
            changed,
            closed: false,
        })
    }
}

fn selected_member(
    party: &resonance_events::party::Party,
    character: usize,
) -> anyhow::Result<usize> {
    let id = *party
        .formation
        .get(character)
        .context("equipment page character is absent")?;
    let index = usize::from(id.checked_sub(1).context("invalid equipment character")?);
    ensure!(
        index < party.members.len(),
        "equipment page member is absent"
    );
    Ok(index)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visit {
    pub cue: Option<u16>,
    pub changed: bool,
    pub closed: bool,
}

/// Borrowed data used by the field and battle renderers.  Keeping the page
/// view here makes both owners consume the same cursor/equipment projection.
#[derive(Clone, Copy)]
pub struct Page<'a> {
    pub state: &'a Equipment,
    pub party: &'a resonance_events::party::Party,
    pub session: &'a resonance_content::session::SessionData,
    pub data: &'a resonance_content::menu_data::MenuData,
    /// Initial names are owned by MenuData's rename resource rather than the
    /// persistent party.  Keep that fallback in the borrowed page so field
    /// and battle artwork display the same name for an untouched member.
    pub names: &'a [String; 9],
    pub character: usize,
}
impl<'a> Page<'a> {
    pub fn member_index(self) -> anyhow::Result<usize> {
        selected_member(self.party, self.character)
    }
    pub fn items(self) -> Vec<u16> {
        self.member_index().map_or_else(
            |_| Vec::new(),
            |member| equipment_items_for(self.party, self.session, self.data, member, self.state),
        )
    }

    pub fn character_name(self) -> &'a str {
        let Ok(index) = self.member_index() else {
            return "";
        };
        let Some(member) = self.party.members.get(index) else {
            return "";
        };
        member
            .name
            .as_deref()
            .or_else(|| self.names.get(index).map(String::as_str))
            .unwrap_or("")
    }
}

pub fn equipment_items_for(
    party: &resonance_events::party::Party,
    session: &resonance_content::session::SessionData,
    data: &resonance_content::menu_data::MenuData,
    member: usize,
    state: &Equipment,
) -> Vec<u16> {
    let mut items: Vec<_> = party
        .items
        .keys()
        .copied()
        .filter(|id| {
            session.items.get(usize::from(*id)).is_some_and(|item| {
                SLOTS
                    .get(state.slot)
                    .is_some_and(|&slot| item.fits_slot(member, slot))
            })
        })
        .filter(|id| data.items.get(usize::from(*id)).is_some())
        .collect();
    let name = |id: u16| data.item_text(id).ok().map(|text| text.name.as_str());
    items.sort_by(|a_id, b_id| {
        let a = &data.items[usize::from(*a_id)];
        let b = &data.items[usize::from(*b_id)];
        if state.by_parameter {
            let stat = if state.slot == 0 { 0 } else { 2 };
            b.equipment_stats[stat].cmp(&a.equipment_stats[stat])
        } else {
            a.category.cmp(&b.category)
        }
        .then_with(|| name(*a_id).cmp(&name(*b_id)))
    });
    items
}

impl Menu {
    pub fn equipment_items(&self) -> Vec<u16> {
        let resources = self.resources.as_ref().unwrap();
        let member = self.member_index();
        equipment_items_for(
            self.party(),
            &resources.session,
            &resources.data,
            member,
            &self.equipment,
        )
    }

    pub fn equipment_item(&self) -> Option<u16> {
        match self.equipment.focus {
            Focus::Slots => {
                Some(self.member().equipment[SLOTS[self.equipment.slot]]).filter(|id| *id != 0)
            }
            Focus::List => self.equipment_items().get(self.equipment.row).copied(),
            _ => None,
        }
    }

    pub(super) fn step_equipment(&mut self, input: Input) -> Option<i16> {
        let resources = self.resources.as_ref()?;
        let result = self.equipment.step_shared(
            input,
            &mut self.checkpoint.as_mut()?.progress_mut().party,
            &resources.session,
            &resources.data,
            &mut self.character,
        );
        match result {
            Ok(visit) => {
                self.party_changed |= visit.changed;
                if self.equipment.transition.page_closing {
                    self.select_main(super::Page::Equip);
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

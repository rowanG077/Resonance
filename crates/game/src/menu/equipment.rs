use super::*;

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
#[derive(Debug, serde::Serialize)]
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
    pub(super) fn opening() -> Self {
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
}

impl Menu {
    pub fn equipment_items(&self) -> Vec<u16> {
        let resources = self.resources.as_ref().unwrap();
        let member = self.member_index();
        let kind = self.equipment.slot.min(4) as u8;
        let mut items: Vec<_> = self
            .party()
            .items
            .keys()
            .copied()
            .filter(|id| {
                let item = &resources.session.items[usize::from(*id)];
                item.equipment_kind == Some(kind) && item.allowed_characters & (1 << member) != 0
            })
            .collect();
        items.sort_by(|a, b| {
            let a = &resources.data.items[usize::from(*a)];
            let b = &resources.data.items[usize::from(*b)];
            if self.equipment.by_parameter {
                let stat = if kind == 0 { 0 } else { 2 };
                b.equipment_stats[stat].cmp(&a.equipment_stats[stat])
            } else {
                a.category.cmp(&b.category)
            }
            .then_with(|| a.name.cmp(&b.name))
        });
        items
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

    pub(super) fn remember_equipment_description(&mut self) {
        if self.equipment.description_fade == 0 {
            self.equipment.description_previous = self.equipment_item();
        }
    }

    pub(super) fn fade_equipment_description(&mut self) {
        let selected = self.equipment_item();
        let state = &mut self.equipment;
        state.description_opacity = fade_description(
            &mut state.description_fade,
            state.description_previous != selected,
        );
    }

    pub(super) fn step_equipment(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let state = &mut self.equipment;
        state.scroll = (state.scroll + state.scroll.signum()) % 5;
        if state.scroll != 0 {
            return None;
        }
        let focus = self.equipment.focus;
        if input.cancel {
            self.equipment.focus = match focus {
                Focus::Character => {
                    self.equipment.transition.page_closing = true;
                    self.select_main(Page::Equip);
                    Focus::Character
                }
                Focus::Slots | Focus::Optimal { .. } => Focus::Character,
                Focus::List => Focus::Slots,
            };
            return Some(3);
        }
        if matches!(focus, Focus::Character | Focus::Slots)
            && (input.previous_page
                || input.next_page
                || focus == Focus::Character && (left || right))
        {
            let party = &self.checkpoint.as_ref().unwrap().progress.party;
            let old = self.character;
            let back = input.previous_page || left;
            for _ in 0..party.formation.len() {
                self.character = (self.character
                    + if back { party.formation.len() - 1 } else { 1 })
                    % party.formation.len();
                if !self.member().knocked_out() {
                    break;
                }
            }
            self.equipment.reset_list();
            return (old != self.character).then_some(1);
        }
        let member = self.member_index();
        let resources = self.resources.as_ref().unwrap().clone();
        match focus {
            Focus::Character => {
                if input.menu {
                    if member == 0 {
                        self.equipment.focus = Focus::Optimal { thrust: false };
                        return Some(1);
                    }
                    let result = self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .optimize_equipment(&resources.session, &resources.data, member, false);
                    return self.party_result(result);
                }
                if up || down || input.interact {
                    self.equipment.focus = Focus::Slots;
                    self.equipment.slot = if up { SLOTS.len() - 1 } else { 0 };
                    self.equipment.reset_list();
                    return Some(if input.interact { 2 } else { 1 });
                }
            }
            Focus::Optimal { thrust } => {
                if up || down {
                    self.equipment.focus = Focus::Optimal { thrust: !thrust };
                    return Some(1);
                }
                if input.interact {
                    let result = self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .optimize_equipment(&resources.session, &resources.data, member, thrust);
                    return self.party_result(result);
                }
            }
            Focus::Slots | Focus::List => {
                if input.menu {
                    self.equipment.by_parameter = !self.equipment.by_parameter;
                    return Some(1);
                }
                let items = self.equipment_items();
                if focus == Focus::Slots {
                    if input.alternate {
                        if self.equipment.slot == 0
                            || self.equipment_item().is_none()
                            || self.member().knocked_out()
                        {
                            return Some(4);
                        }
                        let result = self.checkpoint.as_mut().unwrap().progress.party.equip_slot(
                            &resources.session,
                            member,
                            SLOTS[self.equipment.slot],
                            0,
                        );
                        self.equipment.reset_list();
                        return self.party_result(result);
                    }
                    if input.interact {
                        if items.is_empty() || self.member().knocked_out() {
                            return Some(4);
                        }
                        self.equipment.focus = Focus::List;
                        self.equipment.reset_list();
                        return Some(2);
                    }
                    if up || down {
                        if up && self.equipment.slot == 0
                            || down && self.equipment.slot == SLOTS.len() - 1
                        {
                            self.equipment.focus = Focus::Character;
                        } else if up {
                            self.equipment.slot -= 1;
                        } else {
                            self.equipment.slot += 1;
                        }
                        self.equipment.reset_list();
                        return Some(1);
                    }
                } else {
                    if input.interact {
                        let id = *items.get(self.equipment.row)?;
                        let result = self.checkpoint.as_mut().unwrap().progress.party.equip_slot(
                            &resources.session,
                            member,
                            SLOTS[self.equipment.slot],
                            id,
                        );
                        if result.is_ok() {
                            self.equipment.focus = Focus::Slots;
                            self.equipment.reset_list();
                        }
                        return self.party_result(result);
                    }
                    let state = &mut self.equipment;
                    let old = state.row;
                    let first = state.first;
                    let mut cue = 1;
                    if up {
                        state.row = state.row.saturating_sub(1);
                    } else if down {
                        state.row = (state.row + 1).min(items.len().saturating_sub(1));
                    } else if page_up && first > 0 {
                        state.first = first.saturating_sub(VISIBLE_EQUIPMENT);
                        state.row -= first - state.first;
                        cue = 38;
                    } else if page_down && first + VISIBLE_EQUIPMENT < items.len() {
                        state.first += VISIBLE_EQUIPMENT;
                        state.row = (state.row + VISIBLE_EQUIPMENT).min(items.len() - 1);
                        cue = 38;
                    }
                    state.first = state
                        .first
                        .min(state.row)
                        .max(state.row.saturating_sub(VISIBLE_EQUIPMENT - 1));
                    if cue == 1 && state.first != first {
                        state.scroll = (state.first as isize - first as isize).signum() as i8;
                    }
                    return (old != state.row).then_some(cue);
                }
            }
        }
        None
    }
}

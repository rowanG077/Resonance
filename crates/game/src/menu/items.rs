use super::*;
use resonance_content::menu_data::{ItemUse, ItemView, RENAME_GEM};
mod transform;

pub const VISIBLE_ITEMS: usize = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Categories,
    List,
    Target,
    Transform(u16),
    Discard(bool),
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Description {
    #[default]
    None,
    Category(usize),
    Item(u16),
}
pub struct Inventory {
    pub page_fade: u8,
    pub page_closing: bool,
    pub(super) destination: Option<Page>,
    pub category: usize,
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    pub focus: Focus,
    pub notice: Option<String>,
    pending_discard: Option<u16>,
    pub transform: transform::Transform,
    pub target: usize,
    pub target_all: bool,
    pub target_equipment: bool,
    pub target_preview: usize,
    pub target_ticks: u8,
    pub target_opacity: u8,
    pub target_closing: bool,
    pub description_previous: Description,
    pub description_fade: u8,
    pub description_opacity: u8,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            page_fade: 0,
            page_closing: false,
            destination: None,
            category: 1,
            row: 0,
            first: 0,
            scroll: 0,
            focus: Focus::List,
            notice: None,
            pending_discard: None,
            transform: Default::default(),
            target: 0,
            target_all: false,
            target_equipment: false,
            target_preview: 0,
            target_ticks: 0,
            target_opacity: 0,
            target_closing: false,
            description_previous: Description::None,
            description_fade: 0,
            description_opacity: 255,
        }
    }
}
impl Inventory {
    pub(super) fn clamp(&mut self, length: usize) {
        self.row = self.row.min(length.saturating_sub(1));
        self.first = self
            .first
            .min(self.row / 2 * 2)
            .max((self.row / 2 * 2).saturating_sub(VISIBLE_ITEMS - 2));
    }
}

impl Menu {
    pub(super) fn open_items(&mut self) {
        let state = &mut self.inventory;
        state.page_fade = 231 - MAIN_SLIDE_STEP;
        state.page_closing = false;
        state.scroll = 0;
        state.destination = None;
        state.target_opacity = 0;
        state.target_closing = false;
        state.description_previous = Description::None;
        state.description_fade = DESCRIPTION_FADE_START;
    }

    pub(super) fn close_items(&mut self, destination: Page) {
        self.inventory.destination = Some(destination);
        self.inventory.page_closing = true;
    }

    pub(super) fn advance_item_page(&mut self) -> bool {
        if self.inventory.page_fade == 255 {
            self.page = self
                .inventory
                .destination
                .take()
                .expect("Items destination");
            match self.page {
                Page::Main => self.return_to_main(),
                Page::Collection => self.open_collection(),
                Page::WorldMap => self.open_world_map(),
                Page::Monsters => {
                    self.monsters = monsters::MonsterList {
                        view: preview::View::open(),
                        ..Default::default()
                    };
                }
                Page::Figurines => {
                    self.figurines = figurines::Figurines {
                        view: preview::View::open(),
                        ..Default::default()
                    };
                }
                Page::Manual => {
                    self.manual = manual::Manual {
                        page_fade: 231 - MAIN_SLIDE_STEP,
                        ..Default::default()
                    };
                }
                Page::Rename => self.advance_rename(),
                _ => unreachable!("unsupported Items transition"),
            }
            return true;
        }
        let state = &mut self.inventory;
        if state.target_closing {
            return false;
        }
        state.page_fade = if state.page_closing {
            state.page_fade.saturating_add(MAIN_SLIDE_STEP)
        } else {
            state.page_fade.saturating_sub(MAIN_SLIDE_STEP)
        };
        if state.page_closing && state.page_fade == 255 {
            state.page_closing = false;
        }
        false
    }

    pub fn item_description(&self) -> Description {
        if self.inventory.notice.is_some() {
            return Description::None;
        }
        match self.inventory.focus {
            Focus::Categories => Description::Category(self.inventory.category),
            Focus::List | Focus::Transform(_) => self
                .inventory_items()
                .get(self.inventory.row)
                .copied()
                .map_or(Description::None, Description::Item),
            _ => Description::None,
        }
    }
    pub(super) fn remember_item_description(&mut self) {
        if self.inventory.description_fade == 0 {
            self.inventory.description_previous = self.item_description();
        }
    }
    pub(super) fn fade_item_description(&mut self) {
        let description = self.item_description();
        if description != Description::None {
            let state = &mut self.inventory;
            state.description_opacity = fade_description(
                &mut state.description_fade,
                state.description_previous != description,
            );
        }
    }
    pub(super) fn fade_item_target(&mut self) {
        let target = &mut self.inventory;
        if !matches!(target.focus, Focus::Target | Focus::Transform(_)) {
            return;
        }
        target.target_opacity = if target.target_closing {
            target.target_opacity.saturating_sub(32)
        } else {
            target.target_opacity.saturating_add(32)
        };
        if target.target_closing && target.target_opacity == 0 {
            if matches!(target.focus, Focus::Transform(_)) {
                target.notice = None;
                target.transform.result = None;
            }
            target.target_closing = false;
            target.focus = Focus::List;
        }
    }
    pub(super) fn step_item_preview(&mut self) {
        let target = &mut self.inventory;
        target.target_preview = target.target;
        if target.focus == Focus::Target && target.target_all {
            const PREVIEW_TICKS: u8 = 121;
            target.target_ticks += 1;
            if target.target_ticks == PREVIEW_TICKS {
                target.target_ticks = 0;
                target.target = (target.target + 1)
                    % self
                        .checkpoint
                        .as_ref()
                        .unwrap()
                        .progress
                        .party
                        .formation
                        .len();
            }
        }
    }

    pub fn inventory_items(&self) -> Vec<u16> {
        let party = self.party();
        let data = &self.resources.as_ref().unwrap().data;
        let transforming =
            matches!(self.inventory.focus, Focus::Transform(_)) && !self.inventory.target_closing;
        let source: Vec<_> = if self.inventory.category == 0 && !transforming {
            party.recent_items.clone()
        } else {
            party.items.keys().copied().collect()
        };
        let mut items = source
            .into_iter()
            .filter(|id| {
                if !party.items.contains_key(id) {
                    return false;
                }
                let item = &data.items[usize::from(*id)];
                if transforming {
                    return item.transforms_to != 0;
                }
                self.inventory.category == 0
                    || item.inventory_category() == Some(self.inventory.category)
            })
            .collect::<Vec<_>>();
        if transforming {
            items.sort_by_key(|&id| {
                (
                    data.items[usize::from(id)].category,
                    &data.items[usize::from(id)].name,
                )
            });
        } else {
            self.sort_items(&mut items, self.inventory.category);
        }
        items
    }

    pub(super) fn sort_items(&self, items: &mut [u16], category: usize) {
        if category == 0 {
            return;
        }
        let data = &self.resources.as_ref().unwrap().data;
        let has_figurines = !self.party().figurines.is_empty();
        items.sort_by_key(|&id| {
            let item = &data.items[usize::from(id)];
            let priority = match category {
                1 => !item.field_usable,
                8 => {
                    !(id == RENAME_GEM
                        || match item.view {
                            Some(ItemView::FigurineBook) => has_figurines,
                            Some(_) => true,
                            None => false,
                        })
                }
                _ => false,
            };
            (
                priority,
                if category == 8 { 0 } else { item.category },
                &item.name,
            )
        });
    }

    pub(super) fn step_items(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        self.inventory.scroll = (self.inventory.scroll + self.inventory.scroll.signum()) % 5;
        if self.inventory.scroll != 0 {
            return None;
        }
        if matches!(self.inventory.focus, Focus::Target | Focus::Transform(_))
            && (self.inventory.target_closing || self.inventory.target_opacity < 255)
        {
            return None;
        }
        let resources = self.resources.as_ref().unwrap().clone();
        let items = self.inventory_items();
        let selected = items.get(self.inventory.row).copied();
        if self.inventory.notice.is_some() {
            if input.interact || input.cancel {
                if let Some(id) = self.inventory.transform.result {
                    return self.finish_transformation(id);
                }
                self.inventory.notice = None;
                if let Some(id) = self.inventory.pending_discard.take() {
                    let result = self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .change_item(&resources.session, id, -1)
                        .map(|changed| changed.then_some(2));
                    return self.item_result(result);
                }
                return Some(2);
            }
            return None;
        }
        if input.cancel || input.menu {
            match self.inventory.focus {
                Focus::Categories => {
                    self.close_items(Page::Main);
                    self.select_main(Page::Items);
                }
                Focus::List => self.inventory.focus = Focus::Categories,
                Focus::Target => self.inventory.target_closing = true,
                Focus::Transform(_) => self.close_transformation(),
                _ => {
                    self.inventory.focus = Focus::List;
                    self.inventory.clamp(self.inventory_items().len());
                }
            }
            return Some(3);
        }
        if self.inventory.focus == Focus::List
            && !(left || right || up || down)
            && (input.previous_page || input.next_page)
            || self.inventory.focus == Focus::Categories && (left || right)
        {
            let previous = if self.inventory.focus == Focus::Categories {
                left
            } else {
                input.previous_page
            };
            self.inventory.category = (self.inventory.category + if previous { 8 } else { 1 }) % 9;
            self.inventory.row = 0;
            self.inventory.first = 0;
            return Some(1);
        }
        match self.inventory.focus {
            Focus::Categories => {
                if down || input.interact {
                    self.inventory.focus = Focus::List;
                    self.inventory.row = 0;
                    self.inventory.first = 0;
                    return Some(2);
                }
            }
            Focus::Target => {
                let old = self.inventory.target;
                let party = &mut self.checkpoint.as_mut().unwrap().progress.party;
                if up && !self.inventory.target_all {
                    self.inventory.target = old.saturating_sub(1);
                }
                if down && !self.inventory.target_all {
                    self.inventory.target = (old + 1).min(party.formation.len() - 1);
                }
                if left && old >= VISIBLE_PARTY && !self.inventory.target_all {
                    self.inventory.target = old - VISIBLE_PARTY;
                }
                if right
                    && old + VISIBLE_PARTY < party.formation.len()
                    && !self.inventory.target_all
                {
                    self.inventory.target = old + VISIBLE_PARTY;
                }
                if input.interact {
                    let id = selected?;
                    let target = usize::from(party.formation[self.inventory.target] - 1);
                    if id == RENAME_GEM {
                        self.open_rename(rename::Origin::Items, target);
                        return Some(2);
                    }
                    let result = if let Some(kind) =
                        resources.session.items[usize::from(id)].equipment_kind
                    {
                        let slot = party.members[target].preferred_equipment_slot(kind)?;
                        if party.members[target].knocked_out() {
                            return Some(4);
                        }
                        let equipped = party.members[target].equipment[slot] == id;
                        party
                            .equip_slot(&resources.session, target, slot, id)
                            .map(|changed| (changed || equipped).then_some(2))
                    } else {
                        party.use_item(&resources.session, &resources.data, id, target)
                    };
                    if !party.items.contains_key(&id) {
                        self.inventory.target_closing = true;
                    }
                    return self.item_result(if self.inventory.target_all {
                        result.map(|cue| cue.map(|_| 2))
                    } else {
                        result
                    });
                }
                return (old != self.inventory.target).then_some(1);
            }
            Focus::Discard(yes) => {
                if up || down {
                    self.inventory.focus = Focus::Discard(!yes);
                    return Some(1);
                }
                if input.interact {
                    self.inventory.focus = Focus::List;
                    if !yes {
                        return Some(2);
                    }
                    let id = selected?;
                    // Keep the item row visible until the player acknowledges it.
                    self.inventory.pending_discard = Some(id);
                    self.inventory.notice = Some(
                        resources.data.labels["discarded"]
                            .replace("%s", &resources.data.items[usize::from(id)].name),
                    );
                    return Some(2);
                }
            }
            Focus::List | Focus::Transform(_) => {
                let old = self.inventory.row;
                let first = self.inventory.first;
                if page_up && first != 0 {
                    self.inventory.first = first.saturating_sub(VISIBLE_ITEMS);
                    self.inventory.row -= first - self.inventory.first;
                    return Some(38);
                }
                if page_down && first + VISIBLE_ITEMS < items.len() {
                    self.inventory.first += VISIBLE_ITEMS;
                    self.inventory.row = (old + VISIBLE_ITEMS).min(items.len() - 1);
                    return Some(38);
                }
                if up && old < 2 && self.inventory.focus == Focus::List {
                    self.inventory.focus = Focus::Categories;
                    return Some(1);
                }
                if left {
                    self.inventory.row = old.saturating_sub(1);
                }
                if right {
                    self.inventory.row = old + 1;
                }
                if up {
                    self.inventory.row = old.saturating_sub(2);
                }
                if down && old + 2 < items.len() {
                    self.inventory.row = old + 2;
                }
                self.inventory.clamp(items.len());
                self.inventory.scroll =
                    (self.inventory.first as isize - first as isize).signum() as i8;
                let selected = items.get(self.inventory.row).copied();
                if input.alternate && self.inventory.focus == Focus::List {
                    if selected.is_some_and(|id| resources.data.items[usize::from(id)].price != 0) {
                        self.inventory.focus = Focus::Discard(false);
                        return Some(2);
                    }
                    return Some(4);
                }
                if input.interact
                    && let Some(id) = selected
                {
                    if matches!(self.inventory.focus, Focus::Transform(_)) {
                        return Some(self.preview_transformation(id));
                    }
                    let definition = &resources.data.items[usize::from(id)];
                    if definition.view == Some(ItemView::MonsterList) {
                        self.close_items(Page::Monsters);
                        return Some(2);
                    }
                    if definition.view == Some(ItemView::FigurineBook) {
                        if self.party().figurines.is_empty() {
                            return Some(4);
                        }
                        self.close_items(Page::Figurines);
                        return Some(2);
                    }
                    if definition.view == Some(ItemView::CollectorsBook) {
                        self.close_items(Page::Collection);
                        return Some(2);
                    }
                    if definition.view == Some(ItemView::TrainingManual) {
                        self.close_items(Page::Manual);
                        return Some(2);
                    }
                    if matches!(
                        definition.view,
                        Some(ItemView::SylvarantMap | ItemView::TetheallaMap)
                    ) {
                        self.world_map.world =
                            u8::from(definition.view == Some(ItemView::TetheallaMap));
                        self.close_items(Page::WorldMap);
                        return Some(2);
                    }
                    match definition.field_use {
                        Some(ItemUse::Recover { party: true, .. })
                            if !self.party().can_use_group_item(&resources.data, id) =>
                        {
                            return Some(4);
                        }
                        Some(ItemUse::Transform) => {
                            return Some(self.open_transformation(id));
                        }
                        Some(ItemUse::EncounterRate { rate }) => {
                            let target = self.member_index();
                            let result = self.checkpoint.as_mut().unwrap().progress.party.use_item(
                                &resources.session,
                                &resources.data,
                                id,
                                target,
                            );
                            if matches!(result, Ok(Some(_))) {
                                self.inventory.notice = Some(
                                    resources.data.labels
                                        [if rate == 1 { "holy_aura" } else { "dark_aura" }]
                                    .clone(),
                                );
                            }
                            return self.item_result(result);
                        }
                        Some(_) => self.inventory.focus = Focus::Target,
                        None if resources.session.items[usize::from(id)]
                            .equipment_kind
                            .is_some() =>
                        {
                            self.inventory.focus = Focus::Target
                        }
                        None if id == RENAME_GEM => self.inventory.focus = Focus::Target,
                        None => return Some(4),
                    }
                    if self.inventory.focus == Focus::Target {
                        self.inventory.target_equipment = resources.session.items[usize::from(id)]
                            .equipment_kind
                            .is_some();
                        self.inventory.target_all = matches!(
                            definition.field_use,
                            Some(ItemUse::Recover { party: true, .. })
                        );
                        self.inventory.target_closing = false;
                        if self.inventory.target_all {
                            self.inventory.target = 0;
                            self.inventory.target_ticks = 0;
                        }
                        self.inventory.target_preview = self.inventory.target;
                    }
                    return Some(2);
                }
                return (old != self.inventory.row).then_some(1);
            }
        }
        None
    }

    fn item_result(&mut self, result: Result<Option<i16>, String>) -> Option<i16> {
        match result {
            Ok(Some(cue)) => {
                self.party_changed = true;
                let length = self.inventory_items().len();
                self.inventory.clamp(length);
                if length == 0
                    && !matches!(self.inventory.focus, Focus::Target | Focus::Transform(_))
                {
                    self.inventory.focus = Focus::List;
                }
                Some(cue)
            }
            Ok(None) => Some(4),
            Err(error) => {
                self.notice = Some(error);
                Some(4)
            }
        }
    }
}

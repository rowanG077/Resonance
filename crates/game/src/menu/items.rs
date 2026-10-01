use super::*;
use resonance_content::menu_data::{ItemAttention, ItemUse, ItemView, RENAME_GEM};
mod transform;

pub const VISIBLE_ITEMS: usize = 18;

/// Inventory presentation data shared by field and battle menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRow {
    pub id: u16,
    pub count: u8,
    pub recent: bool,
    pub urgent: bool,
}

/// The same item policy applies to saved field members and live battle actors.
pub(crate) struct ItemNeeds {
    pub hp: [i64; 2],
    pub tp: [i64; 2],
    pub knocked_out: bool,
    pub physical_ailment: bool,
    pub magical_ailment: bool,
}
impl ItemNeeds {
    pub fn urgent(&self, attention: Option<ItemAttention>) -> bool {
        let low = |[current, maximum]: [i64; 2]| maximum > 0 && current <= (maximum + 2) / 4;
        match attention {
            Some(ItemAttention::LowHp) => low(self.hp),
            Some(ItemAttention::LowTp) => low(self.tp),
            Some(ItemAttention::LowVitals) => low(self.hp) || low(self.tp),
            Some(ItemAttention::Knockout) => self.knocked_out,
            Some(ItemAttention::Ailment) => self.physical_ailment,
            Some(ItemAttention::AllAilments) => self.physical_ailment || self.magical_ailment,
            Some(ItemAttention::MagicalAilment) => self.magical_ailment,
            None => false,
        }
    }
}

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
    pub category: usize,
    pub row: usize,
    pub first: usize,
    pub focus: Focus,
    pub notice: Option<String>,
    pending_discard: Option<u16>,
    pub transform: transform::Transform,
    pub target: usize,
    pub target_all: bool,
    pub target_equipment: bool,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            category: 1,
            row: 0,
            first: 0,
            focus: Focus::List,
            notice: None,
            pending_discard: None,
            transform: Default::default(),
            target: 0,
            target_all: false,
            target_equipment: false,
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
        self.page = Page::Items;
        self.inventory.clamp(self.inventory_items().len());
    }

    pub(super) fn close_items(&mut self, destination: Page) -> bool {
        if !self.admit_page(destination) {
            return false;
        }
        self.page = destination;
        match destination {
            Page::Main => self.return_to_main(),
            Page::Collection => self.open_collection(),
            Page::WorldMap => self.open_world_map(),
            Page::Monsters => {
                self.monsters = monsters::MonsterList {
                    view: preview::View::open(),
                    ..Default::default()
                }
            }
            Page::Figurines => {
                self.figurines = figurines::Figurines {
                    view: preview::View::open(),
                    ..Default::default()
                }
            }
            Page::Manual => {
                self.manual = manual::Manual {
                    page_fade: 231 - MAIN_SLIDE_STEP,
                    ..Default::default()
                }
            }
            Page::Rename => self.advance_rename(),
            _ => unreachable!("unsupported Items transition"),
        }
        true
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
    pub fn inventory_rows(&self) -> Vec<ItemRow> {
        let party = self.party();
        let data = &self.resources.as_ref().unwrap().data;
        let needs: Vec<_> = party
            .formation
            .iter()
            .map(|&id| {
                let member = &party.members[usize::from(id - 1)];
                let stats = member.stats(data);
                ItemNeeds {
                    hp: [i64::from(member.hp), i64::from(stats.hp)],
                    tp: [i64::from(member.tp), i64::from(stats.tp)],
                    knocked_out: member.knocked_out(),
                    physical_ailment: member.has_curable_ailment(),
                    magical_ailment: false,
                }
            })
            .collect();
        self.inventory_items()
            .into_iter()
            .map(|id| ItemRow {
                id,
                count: party.items[&id],
                recent: party.recent_items.contains(&id),
                urgent: data.items[usize::from(id)].field_usable
                    && needs
                        .iter()
                        .any(|need| need.urgent(data.items[usize::from(id)].attention)),
            })
            .collect()
    }

    pub fn inventory_items(&self) -> Vec<u16> {
        let party = self.party();
        let data = &self.resources.as_ref().unwrap().data;
        let transforming = matches!(self.inventory.focus, Focus::Transform(_));
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
                    data.item_text(id).ok().map(|text| text.name.as_str()),
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
                data.item_text(id).ok().map(|text| text.name.as_str()),
            )
        });
    }

    pub(super) fn step_items(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let [left, right, up, down, page_up, page_down] =
            [Left, Right, Up, Down, PageUp, PageDown].map(|action| input == Some(action));
        let resources = self.resources.as_ref().unwrap().clone();
        let items = self.inventory_items();
        let selected = items.get(self.inventory.row).copied();
        if self.inventory.notice.is_some() {
            if matches!(input, Some(Confirm | Cancel)) {
                if let Some(id) = self.inventory.transform.result {
                    return self.finish_transformation(id);
                }
                self.inventory.notice = None;
                if let Some(id) = self.inventory.pending_discard.take() {
                    let result = self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress_mut()
                        .party
                        .change_item(&resources.session, id, -1)
                        .map(|changed| changed.then_some(2));
                    return self.item_result(result);
                }
                return Some(2);
            }
            return None;
        }
        if matches!(input, Some(Cancel | Menu)) {
            match self.inventory.focus {
                Focus::Categories => {
                    self.close_items(Page::Main);
                    self.select_main(Page::Items);
                }
                Focus::List => self.inventory.focus = Focus::Categories,
                Focus::Target => self.inventory.focus = Focus::List,
                Focus::Transform(_) => self.close_transformation(),
                _ => {
                    self.inventory.focus = Focus::List;
                    self.inventory.clamp(self.inventory_items().len());
                }
            }
            return Some(3);
        }
        if self.inventory.focus == Focus::List && (matches!(input, Some(PreviousTab | NextTab)))
            || self.inventory.focus == Focus::Categories && (left || right)
        {
            let previous = if self.inventory.focus == Focus::Categories {
                left
            } else {
                input == Some(PreviousTab)
            };
            self.inventory.category = (self.inventory.category + if previous { 8 } else { 1 }) % 9;
            self.inventory.row = 0;
            self.inventory.first = 0;
            return Some(1);
        }
        match self.inventory.focus {
            Focus::Categories => {
                if down || input == Some(Confirm) {
                    self.inventory.focus = Focus::List;
                    self.inventory.row = 0;
                    self.inventory.first = 0;
                    return Some(2);
                }
            }
            Focus::Target => {
                let old = self.inventory.target;
                let party = &mut self.checkpoint.as_mut().unwrap().progress_mut().party;
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
                if input == Some(Confirm) {
                    let id = selected?;
                    let target = usize::from(party.formation[self.inventory.target] - 1);
                    if id == RENAME_GEM {
                        return Some(if self.open_rename(rename::Origin::Items, target) {
                            2
                        } else {
                            4
                        });
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
                        self.inventory.focus = Focus::List;
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
                if input == Some(Confirm) {
                    self.inventory.focus = Focus::List;
                    if !yes {
                        return Some(2);
                    }
                    let id = selected?;
                    let message = match (|| -> anyhow::Result<String> {
                        Ok(resources
                            .data
                            .label("discarded")?
                            .replace("%s", &resources.data.item_text(id)?.name))
                    })() {
                        Ok(text) => text,
                        Err(error) => {
                            self.report_failure("Item description unavailable", error);
                            return Some(4);
                        }
                    };
                    // Keep the item row visible until the player acknowledges it.
                    self.inventory.pending_discard = Some(id);
                    self.inventory.notice = Some(message);
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
                let selected = items.get(self.inventory.row).copied();
                if input == Some(Alternate) && self.inventory.focus == Focus::List {
                    if selected.is_some_and(|id| resources.data.items[usize::from(id)].price != 0) {
                        self.inventory.focus = Focus::Discard(false);
                        return Some(2);
                    }
                    return Some(4);
                }
                if input == Some(Confirm)
                    && let Some(id) = selected
                {
                    if matches!(self.inventory.focus, Focus::Transform(_)) {
                        return Some(self.preview_transformation(id));
                    }
                    let definition = &resources.data.items[usize::from(id)];
                    if let Some(view) = definition.view {
                        if view == ItemView::FigurineBook && self.party().figurines.is_empty() {
                            return Some(4);
                        }
                        let destination = match view {
                            ItemView::CollectorsBook => Page::Collection,
                            ItemView::MonsterList => Page::Monsters,
                            ItemView::FigurineBook => Page::Figurines,
                            ItemView::TrainingManual => Page::Manual,
                            ItemView::SylvarantMap | ItemView::TetheallaMap => {
                                self.world_map.world = u8::from(view == ItemView::TetheallaMap);
                                Page::WorldMap
                            }
                        };
                        return Some(if self.close_items(destination) { 2 } else { 4 });
                    }
                    match definition.field_use {
                        Some(ItemUse::Recover { party: true, .. }) => {
                            if !self.party().can_use_group_item(&resources.data, id) {
                                return Some(4);
                            }
                            if let Err(error) = resources.data.item_group_prompt() {
                                self.report_failure("Item prompt unavailable", error);
                                return Some(4);
                            }
                            self.inventory.focus = Focus::Target;
                        }
                        Some(ItemUse::Transform) => {
                            if let Err(error) = resources.data.item_bottle_count() {
                                self.report_failure("Item prompt unavailable", error);
                                return Some(4);
                            }
                            return Some(self.open_transformation(id));
                        }
                        Some(ItemUse::EncounterRate { rate }) => {
                            let notice = match resources.data.label(if rate == 1 {
                                "holy_aura"
                            } else {
                                "dark_aura"
                            }) {
                                Ok(text) => text.to_owned(),
                                Err(error) => {
                                    self.report_failure("Item notice unavailable", error);
                                    return Some(4);
                                }
                            };
                            let target = self.member_index();
                            let result = self
                                .checkpoint
                                .as_mut()
                                .unwrap()
                                .progress_mut()
                                .party
                                .use_item(&resources.session, &resources.data, id, target);
                            if matches!(result, Ok(Some(_))) {
                                self.inventory.notice = Some(notice);
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
                        if self.inventory.target_all {
                            self.inventory.target = 0;
                        }
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

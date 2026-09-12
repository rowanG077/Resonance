use super::*;
use resonance_content::menu_data::{MapLocation, Shop};

pub const LOCATION_ROWS: usize = 9;
pub const ITEM_ROWS: usize = 8;

#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    #[default]
    Locations,
    Shops,
    Items,
}

#[derive(Default, serde::Serialize)]
pub struct WorldMap {
    pub page_fade: u8,
    pub page_closing: bool,
    pub shops_opacity: u8,
    pub items_opacity: u8,
    pub location_scroll: i8,
    pub item_scroll: i8,
    pub description_previous: u16,
    pub description_fade: u8,
    pub description_opacity: u8,
    pub world: u8,
    pub focus: Focus,
    pub location: usize,
    pub first_location: usize,
    pub shop: usize,
    pub item: usize,
    pub first_item: usize,
}

impl Menu {
    pub(super) fn open_world_map(&mut self) {
        self.world_map = WorldMap {
            world: self.world_map.world,
            page_fade: 231 - MAIN_SLIDE_STEP,
            description_fade: DESCRIPTION_FADE_START,
            ..Default::default()
        };
        self.fade_world_map_description();
    }

    pub(super) fn advance_world_map(&mut self) -> bool {
        if self.world_map.description_fade == 0 {
            self.world_map.description_previous = self.map_description();
        }
        let map = &mut self.world_map;
        if map.page_fade == 255 {
            self.page = Page::Items;
            self.open_items();
            self.fade_item_description();
            return true;
        }
        map.page_fade = if map.page_closing {
            map.page_fade.saturating_add(MAIN_SLIDE_STEP)
        } else {
            map.page_fade.saturating_sub(MAIN_SLIDE_STEP)
        };
        if map.page_fade == 255 {
            map.page_closing = false;
        }
        false
    }

    pub fn map_description(&self) -> u16 {
        if self.world_map.focus == Focus::Items {
            self.map_shop()
                .and_then(|(_, shop)| shop.items.get(self.world_map.item).copied())
                .unwrap_or(0)
        } else {
            0
        }
    }

    pub(super) fn fade_world_map_description(&mut self) {
        let item = self.map_description();
        let map = &mut self.world_map;
        map.description_opacity =
            fade_description(&mut map.description_fade, map.description_previous != item);
    }

    pub fn map_locations(&self) -> Vec<(u16, &MapLocation)> {
        let travel = &self.party().travel;
        self.resources
            .as_ref()
            .unwrap()
            .data
            .world_map
            .locations
            .iter()
            .filter(|&(id, location)| {
                id / 256 == u16::from(self.world_map.world)
                    && location.listed
                    && travel.visited_locations.contains(id)
            })
            .map(|(&id, location)| (id, location))
            .collect()
    }

    pub fn map_shops(&self) -> &[u8] {
        self.map_locations()
            .get(self.world_map.location)
            .map_or(&[], |(_, location)| {
                location.shops(&self.checkpoint.as_ref().unwrap().progress.script_globals)
            })
    }

    pub fn map_shop(&self) -> Option<(u8, &Shop)> {
        let &id = self.map_shops().get(self.world_map.shop)?;
        Some((
            id,
            &self.resources.as_ref().unwrap().data.world_map.shops[usize::from(id)],
        ))
    }

    pub(super) fn step_world_map(
        &mut self,
        input: crate::field::FieldInput,
        [up, down, page_up, page_down]: [bool; 4],
    ) -> Option<i16> {
        let map = &mut self.world_map;
        if map.page_closing || map.page_fade != 0 {
            return None;
        }
        for scroll in [&mut map.location_scroll, &mut map.item_scroll] {
            *scroll = (*scroll + scroll.signum()) % 5;
        }
        if map.location_scroll != 0 || map.item_scroll != 0 {
            return None;
        }
        let fade = |opacity: &mut u8, visible| {
            *opacity = if visible {
                opacity.saturating_add(32)
            } else {
                opacity.saturating_sub(32)
            };
            *opacity == if visible { 255 } else { 0 }
        };
        if !fade(&mut map.shops_opacity, map.focus != Focus::Locations)
            || !fade(&mut map.items_opacity, map.focus == Focus::Items)
        {
            return None;
        }
        if input.cancel {
            match self.world_map.focus {
                Focus::Locations => self.world_map.page_closing = true,
                Focus::Shops => self.world_map.focus = Focus::Locations,
                Focus::Items => self.world_map.focus = Focus::Shops,
            }
            return Some(3);
        }
        if input.interact {
            match self.world_map.focus {
                Focus::Locations => {
                    if self.map_shops().is_empty() {
                        return Some(4);
                    }
                    self.world_map.focus = Focus::Shops;
                    self.world_map.shop = 0;
                }
                Focus::Shops => {
                    let (id, _) = self.map_shop()?;
                    if !self.party().travel.visited_shops.contains(&id) {
                        return Some(4);
                    }
                    self.world_map.focus = Focus::Items;
                    self.world_map.item = 0;
                    self.world_map.first_item = 0;
                }
                Focus::Items => return None,
            }
            return Some(2);
        }
        let count = match self.world_map.focus {
            Focus::Locations => self.map_locations().len(),
            Focus::Shops => self.map_shops().len(),
            Focus::Items => self.map_shop()?.1.items.len(),
        };
        let map = &mut self.world_map;
        let mut unused = 0;
        let (row, first, visible) = match map.focus {
            Focus::Locations => (&mut map.location, &mut map.first_location, LOCATION_ROWS),
            Focus::Shops => (&mut map.shop, &mut unused, usize::MAX),
            Focus::Items => (&mut map.item, &mut map.first_item, ITEM_ROWS),
        };
        let old = *row;
        let old_first = *first;
        if page_up && *first != 0 {
            let delta = visible.min(*first);
            *row -= delta;
            *first -= delta;
            return Some(38);
        }
        if page_down && first.saturating_add(visible) < count {
            *first += visible;
            *row = (*row + visible).min(count - 1);
            return Some(38);
        }
        if up {
            *row = row.saturating_sub(1);
        }
        if down {
            *row = (*row + 1).min(count.saturating_sub(1));
        }
        *first = (*first).min(*row).max(row.saturating_sub(visible - 1));
        let changed = old != *row;
        let scroll = (*first as isize - old_first as isize).signum() as i8;
        match map.focus {
            Focus::Locations => map.location_scroll = scroll,
            Focus::Items => map.item_scroll = scroll,
            Focus::Shops => {}
        }
        changed.then_some(1)
    }
}

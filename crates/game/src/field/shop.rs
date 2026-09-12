//! A shop owns a basket until confirmation; only checkout changes saved state.
use super::FieldInput;
use crate::{
    DirectionRepeat,
    menu::{DESCRIPTION_FADE_START, Resources, fade_description, items::Description},
};
use anyhow::{Result, ensure};
use resonance_content::{field_audio::ServiceCue, menu_data::ShopTrade};
use resonance_events::{Operation, party::Party};
use std::sync::Arc;

pub const VISIBLE_ITEMS: usize = 7;
const SLIDE_STEP: u8 = 25;
const OPENING_FADE: u8 = 231;
const REGAL: u8 = 8;
const PERSONAL: u8 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Buy,
    Sell,
    Equip,
    Leave,
}
impl Choice {
    pub const ALL: [Self; 4] = [Self::Buy, Self::Sell, Self::Equip, Self::Leave];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|v| *v == self).unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    Root,
    Categories,
    Items,
    Characters,
    Equipment,
    Confirm { yes: bool },
    Empty,
}

#[derive(Debug, serde::Serialize)]
pub struct Row {
    pub id: u16,
    pub quantity: u8,
}

#[derive(Clone, Copy)]
enum Exit {
    Field,
    Equip,
}

pub struct Shop {
    pub resources: Arc<Resources>,
    pub id: u8,
    pub choice: Choice,
    pub focus: Focus,
    pub rows: Vec<Row>,
    pub row: usize,
    pub first: usize,
    pub category: usize,
    pub character: usize,
    pub fade: u8,
    pub scroll: i8,
    pub statistics: bool,
    pub description_previous: Description,
    pub description_opacity: u8,
    pub closed: bool,
    operation: Operation,
    closing: Option<Exit>,
    equipment_requested: bool,
    description_fade: u8,
    tick: u32,
    held: [bool; 6],
    repeat: [DirectionRepeat; 6],
}

impl Shop {
    pub fn open(
        id: u8,
        resources: Arc<Resources>,
        party: &mut Party,
        operation: Operation,
    ) -> Result<Self> {
        ensure!(
            usize::from(id) < resources.data.world_map.shops.len(),
            "shop {id} is not cooked"
        );
        ensure!(!party.formation.is_empty(), "shop requires a party");
        party.travel.visited_shops.insert(id);
        if let Some(alias) = match id {
            17 => Some(50),
            50 => Some(17),
            18 => Some(40),
            40 => Some(18),
            29 => Some(45),
            45 => Some(29),
            _ => None,
        } {
            party.travel.visited_shops.insert(alias);
        }
        Ok(Self {
            resources,
            id,
            choice: Choice::Buy,
            focus: Focus::Root,
            rows: Vec::new(),
            row: 0,
            first: 0,
            category: 0,
            character: 0,
            fade: OPENING_FADE,
            scroll: 0,
            statistics: false,
            description_previous: Description::None,
            description_opacity: 255 - DESCRIPTION_FADE_START,
            closed: false,
            operation,
            closing: None,
            equipment_requested: false,
            description_fade: DESCRIPTION_FADE_START,
            tick: 0,
            held: [false; 6],
            repeat: Default::default(),
        })
    }

    pub fn selected_item(&self) -> Option<u16> {
        self.rows.get(self.row).map(|r| r.id)
    }
    pub fn description(&self) -> Description {
        match self.focus {
            Focus::Categories => Description::Category(self.category + 1),
            Focus::Items | Focus::Characters | Focus::Equipment => self
                .selected_item()
                .map_or(Description::None, Description::Item),
            _ => Description::None,
        }
    }
    pub fn trade(&self) -> ShopTrade {
        if self.choice == Choice::Buy {
            ShopTrade::Buy
        } else {
            ShopTrade::Sell
        }
    }
    pub fn unit_price(&self, id: u16, party: &Party) -> u32 {
        let personal = party.formation.contains(&REGAL)
            && party.members[usize::from(REGAL - 1)]
                .ex_skills
                .contains(&PERSONAL);
        self.resources.data.items[usize::from(id)].shop_price(self.trade(), personal)
    }
    pub fn total(&self, party: &Party) -> u32 {
        self.rows
            .iter()
            .map(|r| self.unit_price(r.id, party) * u32::from(r.quantity))
            .sum()
    }
    pub fn take_equipment_request(&mut self) -> bool {
        std::mem::take(&mut self.equipment_requested)
    }
    pub fn return_from_equipment(&mut self) {
        self.fade = OPENING_FADE;
        self.held = [false; 6];
    }

    fn rebuild(&mut self, party: &Party) {
        let data = &self.resources.data;
        let mut ids = if self.choice == Choice::Buy {
            data.world_map.shops[usize::from(self.id)].items.clone()
        } else {
            party
                .items
                .keys()
                .copied()
                .filter(|id| {
                    let item = &data.items[usize::from(*id)];
                    item.price != 0 && item.inventory_category() == Some(self.category + 1)
                })
                .collect()
        };
        if self.choice == Choice::Sell {
            ids.sort_by_key(|&id| {
                let item = &data.items[usize::from(id)];
                (
                    self.category == 0 && !item.field_usable,
                    item.category,
                    &item.name,
                )
            });
        }
        self.rows = ids.into_iter().map(|id| Row { id, quantity: 0 }).collect();
        self.row = 0;
        self.first = 0;
        self.scroll = 0;
    }
    fn change_category(&mut self, previous: bool, party: &Party) {
        self.category = (self.category + if previous { 6 } else { 1 }) % 7;
        self.rebuild(party);
    }
    fn adjust_quantity(&mut self, party: &Party, delta: i8) -> bool {
        let Some(row) = self.rows.get(self.row) else {
            return false;
        };
        let quantity = row.quantity;
        let owned = party.items.get(&row.id).copied().unwrap_or(0);
        let maximum = if self.choice == Choice::Buy {
            let capacity = self.resources.session.items[usize::from(row.id)]
                .stack_limit
                .saturating_sub(owned);
            let price = self.unit_price(row.id, party);
            let affordable = party
                .gald
                .saturating_sub(self.total(party))
                .checked_div(price)
                .map_or(u32::from(capacity), |n| n + u32::from(quantity));
            capacity.min(affordable.min(u32::from(u8::MAX)) as u8)
        } else {
            owned
        };
        let next = (i16::from(quantity) + i16::from(delta)).clamp(0, i16::from(maximum)) as u8;
        self.rows[self.row].quantity = next;
        next != quantity
    }

    fn checkout(&mut self, party: &mut Party) -> Result<()> {
        let total = self.total(party);
        ensure!(total != 0, "shop checkout has an empty basket");
        ensure!(
            self.choice != Choice::Buy || total <= party.gald,
            "shop basket exceeds available gald"
        );
        // Validate every line first so a rejected basket cannot partly modify a save.
        for row in &self.rows {
            let owned = party.items.get(&row.id).copied().unwrap_or(0);
            ensure!(
                if self.choice == Choice::Buy {
                    u16::from(owned) + u16::from(row.quantity)
                        <= u16::from(self.resources.session.items[usize::from(row.id)].stack_limit)
                } else {
                    row.quantity <= owned
                },
                "shop basket exceeds item capacity or ownership"
            );
        }
        for row in &mut self.rows {
            if row.quantity != 0 {
                let delta = row.quantity as i8 * if self.choice == Choice::Buy { 1 } else { -1 };
                ensure!(
                    party
                        .change_item(&self.resources.session, row.id, delta)
                        .map_err(anyhow::Error::msg)?,
                    "shop item transfer failed"
                );
                row.quantity = 0;
            }
        }
        party.add_gald(if self.choice == Choice::Buy {
            -(total as i32)
        } else {
            total as i32
        });
        party.spent_gald = party.spent_gald.min(999_999_999);
        if self.choice == Choice::Buy {
            self.focus = Focus::Root;
        } else {
            self.rebuild(party);
            self.focus = if self.rows.is_empty() {
                Focus::Categories
            } else {
                Focus::Items
            };
        }
        Ok(())
    }

    pub fn step(&mut self, input: FieldInput, party: &mut Party) -> Result<Option<ServiceCue>> {
        self.tick = self.tick.wrapping_add(1);
        if self.closed {
            return Ok(None);
        }
        if let Some(exit) = self.closing {
            self.fade = self.fade.saturating_add(SLIDE_STEP);
            if self.fade == 255 {
                self.closing = None;
                match exit {
                    Exit::Field => {
                        self.operation
                            .complete(Some(0))
                            .map_err(anyhow::Error::msg)?;
                        self.closed = true;
                    }
                    Exit::Equip => self.equipment_requested = true,
                }
            }
            return Ok(None);
        }
        self.fade = self.fade.saturating_sub(SLIDE_STEP);
        if self.fade != 0 {
            return Ok(None);
        }
        if self.description_fade == 0 {
            self.description_previous = self.description();
        }
        let held = [
            input.direction[0] < -0.5,
            input.direction[0] > 0.5,
            input.direction[1] > 0.5,
            input.direction[1] < -0.5,
            input.scroll_direction > 0,
            input.scroll_direction < 0,
        ];
        let directions = std::array::from_fn(|i| {
            self.repeat[i].step(held[i], held[i] && !self.held[i], self.tick)
        });
        self.held = held;
        self.scroll = (self.scroll + self.scroll.signum()) % 5;
        let cue = if self.scroll == 0 {
            self.input(input, directions, party)?
        } else {
            None
        };
        if self.description() != Description::None {
            let changed = self.description_previous != self.description();
            self.description_opacity = fade_description(&mut self.description_fade, changed);
        }
        Ok(cue)
    }

    fn input(
        &mut self,
        input: FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
        party: &mut Party,
    ) -> Result<Option<ServiceCue>> {
        use ServiceCue::{Cancel, Confirm, Error, Navigate, Page};
        if input.start {
            self.statistics = !self.statistics;
            return Ok(Some(Navigate));
        }
        if input.cancel {
            let cue = if self.focus == Focus::Empty {
                Confirm
            } else {
                Cancel
            };
            self.focus = match self.focus {
                Focus::Root => {
                    self.closing = Some(Exit::Field);
                    Focus::Root
                }
                Focus::Categories => Focus::Root,
                Focus::Items if self.choice == Choice::Buy => Focus::Root,
                Focus::Items | Focus::Empty => Focus::Categories,
                Focus::Characters | Focus::Confirm { .. } => Focus::Items,
                Focus::Equipment => Focus::Characters,
            };
            return Ok(Some(cue));
        }
        match self.focus {
            Focus::Root => {
                if input.interact {
                    match self.choice {
                        Choice::Buy | Choice::Sell => {
                            self.rebuild(party);
                            self.focus = if self.choice == Choice::Buy {
                                Focus::Items
                            } else {
                                Focus::Categories
                            };
                        }
                        Choice::Equip => self.closing = Some(Exit::Equip),
                        Choice::Leave => self.closing = Some(Exit::Field),
                    }
                    return Ok(Some(if self.choice == Choice::Leave {
                        Cancel
                    } else {
                        Confirm
                    }));
                }
                let old = self.choice.index();
                self.choice = Choice::ALL
                    [(old as i32 + i32::from(right) - i32::from(left)).clamp(0, 3) as usize];
                return Ok((old != self.choice.index()).then_some(Navigate));
            }
            Focus::Categories => {
                if input.interact || down {
                    self.focus = if self.rows.is_empty() {
                        Focus::Empty
                    } else {
                        Focus::Items
                    };
                    return Ok(Some(Navigate));
                }
                if left || right {
                    self.change_category(left, party);
                    return Ok(Some(Navigate));
                }
            }
            Focus::Empty => {
                if input.interact {
                    self.focus = Focus::Categories;
                    return Ok(Some(Confirm));
                }
            }
            Focus::Confirm { yes } => {
                if input.interact {
                    if yes {
                        self.checkout(party)?;
                    } else {
                        self.focus = Focus::Items;
                    }
                    return Ok(Some(Confirm));
                }
                if up || down {
                    self.focus = Focus::Confirm { yes: !yes };
                    return Ok(Some(Navigate));
                }
            }
            Focus::Characters => {
                if input.interact {
                    self.focus = Focus::Equipment;
                    return Ok(Some(Confirm));
                }
                let old = self.character;
                let next = old as i32 + i32::from(down) - i32::from(up)
                    + 4 * (i32::from(right) - i32::from(left));
                if (0..party.formation.len() as i32).contains(&next) {
                    self.character = next as usize;
                }
                return Ok((old != self.character).then_some(Navigate));
            }
            Focus::Items | Focus::Equipment => {
                if input.interact {
                    if self.focus == Focus::Equipment {
                        return Ok(self.adjust_quantity(party, 1).then_some(Navigate));
                    }
                    if self.total(party) == 0 {
                        self.adjust_quantity(party, 1);
                    }
                    if self.total(party) == 0 {
                        return Ok(Some(Error));
                    }
                    self.focus = Focus::Confirm { yes: true };
                    return Ok(Some(Confirm));
                }
                if input.menu && self.focus == Focus::Items {
                    if self.rows.is_empty() {
                        return Ok(Some(Error));
                    }
                    self.focus = Focus::Characters;
                    return Ok(Some(Confirm));
                }
                if self.focus == Focus::Items {
                    if left || right {
                        return Ok(self
                            .adjust_quantity(party, if left { -1 } else { 1 })
                            .then_some(Navigate));
                    }
                    if input.alternate {
                        let changed = self.adjust_quantity(party, i8::MAX);
                        return Ok((changed || self.choice == Choice::Buy).then_some(Navigate));
                    }
                }
                if input.previous_page || input.next_page {
                    if self.focus == Focus::Equipment {
                        self.character = (self.character
                            + if input.previous_page {
                                party.formation.len() - 1
                            } else {
                                1
                            })
                            % party.formation.len();
                    } else if self.choice == Choice::Sell {
                        self.change_category(input.previous_page, party);
                    } else {
                        return Ok(None);
                    }
                    return Ok(Some(Navigate));
                }
                if self.rows.is_empty() {
                    return Ok(None);
                }
                let old = self.row;
                if self.focus == Focus::Items && (page_up || page_down) {
                    if page_down {
                        if self.first + VISIBLE_ITEMS < self.rows.len() {
                            self.first += VISIBLE_ITEMS;
                            self.row = (self.row + VISIBLE_ITEMS).min(self.rows.len() - 1);
                        } else {
                            self.row = self.rows.len() - 1;
                        }
                    } else {
                        let first = self.first.saturating_sub(VISIBLE_ITEMS);
                        self.row = if self.first == 0 {
                            0
                        } else {
                            self.row - (self.first - first)
                        };
                        self.first = first;
                    }
                    return Ok((old != self.row).then_some(Page));
                }
                if up && old == 0 && self.choice == Choice::Sell && self.focus == Focus::Items {
                    self.focus = Focus::Categories;
                    return Ok(Some(Navigate));
                }
                let shift = i32::from(down) - i32::from(up);
                self.row = (old as i32 + shift).clamp(0, self.rows.len() as i32 - 1) as usize;
                if self.row != old {
                    let first = self.first;
                    self.first = self
                        .first
                        .min(self.row)
                        .max(self.row.saturating_sub(VISIBLE_ITEMS - 1));
                    if first != self.first {
                        self.scroll = if self.first > first { 1 } else { -1 };
                    }
                    return Ok(Some(Navigate));
                }
            }
        }
        Ok(None)
    }
}

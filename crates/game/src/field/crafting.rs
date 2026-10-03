//! Native equipment customization: one confirmed exchange at a time.
use super::FieldInput;
use crate::{
    DirectionRepeat,
    menu::{DESCRIPTION_FADE_START, Resources, fade_description},
};
use anyhow::{Result, ensure};
use resonance_content::{field_audio::ServiceCue, menu_data::crafting::Recipe};
use resonance_events::{
    Operation,
    party::{CraftError, Party},
};
use std::sync::Arc;

pub const VISIBLE_ROWS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Recipes,
    Confirm { yes: bool },
    MissingMaterials,
    InventoryFull,
}

pub struct Crafting {
    pub resources: Arc<Resources>,
    pub vendor: u8,
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    pub focus: Focus,
    pub statistics: bool,
    pub fade: u8,
    pub description_previous: Option<u16>,
    pub description_opacity: u8,
    pub closed: bool,
    operation: Operation,
    closing: bool,
    description_fade: u8,
    tick: u32,
    held: [bool; 4],
    repeat: [DirectionRepeat; 4],
}

impl Crafting {
    pub fn open(vendor: u8, resources: Arc<Resources>, operation: Operation) -> Result<Self> {
        ensure!(
            resources
                .data
                .crafting
                .vendors
                .get(usize::from(vendor))
                .is_some(),
            "crafting vendor {vendor} is not cooked"
        );
        Ok(Self {
            resources,
            vendor,
            operation,
            row: 0,
            first: 0,
            scroll: 0,
            focus: Focus::Recipes,
            statistics: false,
            fade: super::shop::OPENING_FADE,
            description_previous: None,
            description_opacity: 15,
            closed: false,
            closing: false,
            description_fade: DESCRIPTION_FADE_START,
            tick: 0,
            held: [false; 4],
            repeat: Default::default(),
        })
    }
    pub fn recipes(&self) -> &[u16] {
        &self.resources.data.crafting.vendors[usize::from(self.vendor)].recipes
    }
    pub fn selected(&self) -> Option<&Recipe> {
        self.recipes()
            .get(self.row)
            .map(|&id| &self.resources.data.crafting.recipes[usize::from(id)])
    }
    pub fn selected_item(&self) -> Option<u16> {
        self.selected().map(|r| r.result)
    }
    fn availability(&self, party: &Party) -> Focus {
        let result = self
            .selected()
            .ok_or(CraftError::MissingMaterials)
            .and_then(|recipe| party.check_recipe(&self.resources.session, recipe));
        Self::focus(result, Focus::Confirm { yes: true })
    }
    fn focus(result: std::result::Result<(), CraftError>, success: Focus) -> Focus {
        match result {
            Ok(()) => success,
            Err(CraftError::MissingMaterials) => Focus::MissingMaterials,
            Err(CraftError::InventoryFull) => Focus::InventoryFull,
        }
    }
    pub fn step(&mut self, input: FieldInput, party: &mut Party) -> Result<Option<ServiceCue>> {
        use ServiceCue::{Cancel, Confirm, Error, Navigate, Page};
        self.tick = self.tick.wrapping_add(1);
        if self.closed {
            return Ok(None);
        }
        if self.closing {
            self.fade = self.fade.saturating_add(super::shop::SLIDE_STEP);
            if self.fade == 255 {
                self.operation
                    .complete(Some(0))
                    .map_err(anyhow::Error::msg)?;
                self.closed = true;
            }
            return Ok(None);
        }
        self.fade = self.fade.saturating_sub(super::shop::SLIDE_STEP);
        if self.fade != 0 {
            return Ok(None);
        }
        self.scroll = (self.scroll + self.scroll.signum()) % 5;
        let held = [
            input.direction[1] > 0.5,
            input.direction[1] < -0.5,
            input.scroll_direction > 0,
            input.scroll_direction < 0,
        ];
        let [up, down, page_up, page_down] = std::array::from_fn(|i| {
            self.repeat[i].step(held[i], held[i] && !self.held[i], self.tick)
        });
        self.held = held;
        if self.description_fade == 0 {
            self.description_previous = self.selected_item();
        }
        let mut cue = if input.start {
            self.statistics = !self.statistics;
            Some(Navigate)
        } else {
            None
        };
        if self.scroll == 0 {
            if input.cancel {
                if self.focus == Focus::Recipes {
                    self.closing = true;
                } else {
                    self.focus = Focus::Recipes;
                }
                cue = Some(Cancel);
            } else {
                match self.focus {
                    Focus::Recipes if input.interact => {
                        if !self.recipes().is_empty() {
                            self.focus = self.availability(party);
                            cue = Some(if matches!(self.focus, Focus::Confirm { .. }) {
                                Confirm
                            } else {
                                Error
                            });
                        } else {
                            cue = Some(Error);
                        }
                    }
                    Focus::Recipes if !self.recipes().is_empty() => {
                        let old = self.row;
                        let delta = if up {
                            -1
                        } else if down {
                            1
                        } else if page_up {
                            -(VISIBLE_ROWS as isize)
                        } else if page_down {
                            VISIBLE_ROWS as isize
                        } else {
                            0
                        };
                        let len = self.recipes().len();
                        self.scroll = crate::menu::move_list(
                            &mut self.row,
                            &mut self.first,
                            len,
                            VISIBLE_ROWS,
                            delta,
                        );
                        if page_up || page_down {
                            self.scroll = 0;
                        }
                        if old != self.row {
                            cue = Some(if page_up || page_down { Page } else { Navigate });
                        }
                    }
                    Focus::Confirm { yes } if input.interact => {
                        if yes {
                            self.focus = Self::focus(
                                party.craft(&self.resources.session, self.selected().unwrap()),
                                Focus::Recipes,
                            );
                            cue = Some(if self.focus == Focus::Recipes {
                                Confirm
                            } else {
                                Error
                            });
                        } else {
                            self.focus = Focus::Recipes;
                            cue = Some(Cancel);
                        }
                    }
                    Focus::Confirm { yes } if up || down => {
                        self.focus = Focus::Confirm { yes: !yes };
                        cue = Some(Navigate);
                    }
                    Focus::MissingMaterials | Focus::InventoryFull if input.interact => {
                        self.focus = Focus::Recipes;
                        cue = Some(Cancel);
                    }
                    _ => {}
                }
            }
        }
        if self.focus == Focus::Recipes {
            let changed = self.description_previous != self.selected_item();
            self.description_opacity = fade_description(&mut self.description_fade, changed);
        }
        Ok(cue)
    }
}

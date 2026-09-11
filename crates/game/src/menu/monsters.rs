use super::*;
use resonance_content::monster::Monster;
use resonance_events::party::MonsterKnowledge;

pub const VISIBLE: usize = 12;
pub const DEFAULT_YAW: f32 = 330.;
pub const DEFAULT_DISTANCE: f32 = 960.;

// The preview's C-stick response removes a 15-unit dead zone from each
// signed 8-bit axis, then moves one degree/distance unit per 20 remaining units.
fn preview_step(axis: f32) -> f32 {
    let raw = (axis * 128.).clamp(-128., 127.);
    raw.signum() * (raw.abs() - 15.).max(0.) / 20.
}

#[derive(serde::Serialize)]
pub struct MonsterList {
    pub row: usize,
    pub variant: usize,
    pub listing: bool,
    pub list_row: usize,
    pub first: usize,
    pub yaw: f32,
    pub distance: f32,
    pub scroll: i8,
    #[serde(flatten)]
    pub view: preview::View,
}
impl Default for MonsterList {
    fn default() -> Self {
        Self {
            row: 0,
            variant: 0,
            listing: false,
            list_row: 0,
            first: 0,
            yaw: DEFAULT_YAW,
            distance: DEFAULT_DISTANCE,
            scroll: 0,
            view: Default::default(),
        }
    }
}
impl MonsterList {
    pub fn sample(&self, duration: u32) -> u32 {
        super::preview::animation_sample(self.view.animation_tick, duration)
    }
}
impl Menu {
    pub fn monster_records(&self) -> Vec<(&Monster, &MonsterKnowledge)> {
        let book = &self.resources.as_ref().unwrap().data.monsters;
        self.party()
            .monsters
            .iter()
            .map(|(&id, knowledge)| {
                let record = &book.records[usize::from(id)];
                assert!(
                    usize::from(knowledge.variant) < record.statistics.len(),
                    "monster knowledge references an uncooked variant"
                );
                (record, knowledge)
            })
            .collect()
    }
    pub fn monster(&self) -> Option<(&Monster, &MonsterKnowledge)> {
        self.monster_records().get(self.monsters.row).copied()
    }
    pub fn displayed_monster(&self) -> Option<(&Monster, &MonsterKnowledge)> {
        self.monster_records()
            .get(self.monsters.view.model_row)
            .copied()
    }
    pub(super) fn step_monsters(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let book = &mut self.monsters;
        if book.listing {
            book.scroll = (book.scroll + book.scroll.signum()) % 5;
        }
        if book.view.page_fade != 0 || book.view.page_closing {
            return None;
        }
        if !self.monster_records().is_empty() && self.monsters.view.advance_model(self.monsters.row)
        {
            return None;
        }
        if input.start {
            self.monsters.yaw = DEFAULT_YAW;
            self.monsters.distance = DEFAULT_DISTANCE;
        }
        let count = self.monster_records().len();
        let maximum = self
            .monster()
            .filter(|(_, k)| k.scanned)
            .map_or(0, |(_, k)| usize::from(k.variant));
        let book = &mut self.monsters;
        if input.cancel {
            if book.listing {
                book.listing = false;
            } else {
                book.view.page_closing = true;
            }
            return Some(3);
        }
        if count == 0 {
            return None;
        }
        if book.listing {
            if input.interact {
                if book.row != book.list_row {
                    book.row = book.list_row;
                    book.variant = 0;
                }
                book.listing = false;
                return Some(2);
            }
            let old = book.list_row;
            let first = book.first;
            if page_up && book.first != 0 {
                let first = book.first.saturating_sub(VISIBLE);
                book.list_row -= book.first - first;
                book.first = first;
                return Some(38);
            }
            if page_down && book.first + VISIBLE < count {
                book.first += VISIBLE;
                book.list_row = (book.list_row + VISIBLE).min(count - 1);
                return Some(38);
            }
            if up {
                book.list_row = book.list_row.saturating_sub(1);
            }
            if down {
                book.list_row = (book.list_row + 1).min(count - 1);
            }
            book.first = book
                .first
                .min(book.list_row)
                .max((book.list_row + 1).saturating_sub(VISIBLE));
            if book.first != first {
                book.scroll = (book.first as isize - first as isize).signum() as i8;
            }
            return (old != book.list_row).then_some(1);
        }
        if input.alternate {
            book.listing = true;
            book.list_row = book.row;
            book.first = book
                .first
                .min(book.row)
                .max((book.row + 1).saturating_sub(VISIBLE));
            return Some(1);
        }
        if !input.start {
            book.yaw = (book.yaw + preview_step(input.preview_direction[0])).rem_euclid(360.);
            book.distance =
                (book.distance + preview_step(input.preview_direction[1])).clamp(600., 1400.);
        }
        let old = (book.row, book.variant);
        let jump = if count > 10 { 10 } else { 0 };
        let shift = if left {
            -1
        } else if right {
            1
        } else if input.previous_page {
            -jump
        } else if input.next_page {
            jump
        } else {
            0
        };
        if shift != 0 && count > 1 {
            book.row = (book.row as i32 + shift).rem_euclid(count as i32) as usize;
            book.variant = 0;
        } else if up {
            book.variant = book.variant.saturating_sub(1);
        } else if down {
            book.variant = (book.variant + 1).min(maximum);
        }
        (old != (book.row, book.variant)).then_some(1)
    }
}

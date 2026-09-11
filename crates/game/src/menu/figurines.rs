use super::*;
use resonance_content::figurine::Figurine;

pub const VISIBLE: usize = 12;

#[derive(Default, serde::Serialize)]
pub struct Figurines {
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    #[serde(flatten)]
    pub view: preview::View,
}
impl Menu {
    pub fn figurine_records(&self) -> Vec<&Figurine> {
        let records = &self.resources.as_ref().unwrap().data.figurines.records;
        self.party()
            .figurines
            .iter()
            .map(|&id| &records[usize::from(id)])
            .collect()
    }
    pub fn figurine(&self) -> Option<&Figurine> {
        self.figurine_records().get(self.figurines.row).copied()
    }
    pub(super) fn step_figurines(
        &mut self,
        input: crate::field::FieldInput,
        [up, down, page_up, page_down]: [bool; 4],
    ) -> Option<i16> {
        let state = &mut self.figurines;
        state.scroll = (state.scroll + state.scroll.signum()) % 5;
        if state.view.page_fade != 0 || state.view.page_closing || state.scroll != 0 {
            return None;
        }
        if state.view.advance_model(state.row) {
            return None;
        }
        if input.cancel {
            state.view.page_closing = true;
            return Some(3);
        }
        let count = self.figurine_records().len();
        let state = &mut self.figurines;
        if count == 0 {
            return None;
        }
        let old = state.row;
        let first = state.first;
        let mut cue = 1;
        if up {
            state.row = state.row.saturating_sub(1);
        } else if down {
            state.row = (state.row + 1).min(count - 1);
        } else if page_up && state.first > 0 {
            let first = state.first.saturating_sub(VISIBLE);
            state.row -= state.first - first;
            state.first = first;
            cue = 38;
        } else if page_down && state.first + VISIBLE < count {
            state.first += VISIBLE;
            state.row = (state.row + VISIBLE).min(count - 1);
            cue = 38;
        }
        state.first = state
            .first
            .min(state.row)
            .max((state.row + 1).saturating_sub(VISIBLE));
        if old == state.row {
            return None;
        }
        if cue == 1 && state.first != first {
            state.scroll = (state.first as isize - first as isize).signum() as i8;
        }
        Some(cue)
    }
}

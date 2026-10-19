use super::*;
use resonance_content::menu_data::{
    SYNOPSIS_COUNT, SYNOPSIS_LIST_ROWS, SYNOPSIS_TEXT_ROWS, SynopsisEntry,
};

#[derive(Debug, Default, serde::Serialize)]
pub struct Synopsis {
    #[serde(flatten)]
    pub transition: Transition,
    pub row: usize,
    pub first: usize,
    pub reading: bool,
    pub line: usize,
    pub list_scroll: i8,
    pub text_scroll: i8,
    pub text_opacity: u8,
    pub text_closing: bool,
}
impl Menu {
    pub fn synopsis_data(&self) -> anyhow::Result<&menu_data::SynopsisData> {
        self.synopsis_data
            .as_ref()
            .context("synopsis page has not been prepared")
    }

    pub fn synopsis_records(&self) -> Vec<u8> {
        self.checkpoint
            .as_ref()
            .map_or_else(Vec::new, |checkpoint| {
                checkpoint
                    .progress()
                    .event_records
                    .iter()
                    .filter_map(|(&id, record)| {
                        (usize::from(id) < SYNOPSIS_COUNT && (1..=3).contains(&record.value))
                            .then_some(id)
                    })
                    .collect()
            })
    }
    pub fn has_synopsis(&self) -> bool {
        self.resources.is_some() && !self.synopsis_records().is_empty()
    }
    pub fn synopsis_entry(
        &self,
    ) -> anyhow::Result<(&SynopsisEntry, &resonance_events::EventRecord)> {
        let id = *self
            .synopsis_records()
            .get(self.synopsis.row)
            .context("missing synopsis selection")?;
        let entry = self
            .synopsis_data()?
            .entries
            .get(usize::from(id))
            .context("missing synopsis entry")?;
        let record = self
            .checkpoint
            .as_ref()
            .and_then(|checkpoint| checkpoint.progress().event_records.get(&id))
            .context("missing synopsis event record")?;
        Ok((entry, record))
    }
    pub(super) fn step_synopsis(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        for scroll in [
            &mut self.synopsis.list_scroll,
            &mut self.synopsis.text_scroll,
        ] {
            *scroll = (*scroll + scroll.signum()) % 5;
        }
        if self.synopsis.list_scroll != 0 || self.synopsis.text_scroll != 0 {
            return None;
        }
        let state = &mut self.synopsis;
        if state.text_closing {
            state.text_opacity = state.text_opacity.saturating_sub(21);
            if state.text_opacity != 0 {
                return None;
            }
            state.reading = false;
            state.text_closing = false;
        } else if state.reading && state.text_opacity < 255 {
            state.text_opacity = state.text_opacity.saturating_add(21);
            if state.text_opacity != 255 {
                return None;
            }
        }
        if input == Some(Cancel) {
            if self.synopsis.reading {
                self.synopsis.text_closing = true;
            } else {
                self.synopsis.transition.page_closing = true;
                self.select_main(Page::Synopsis);
            }
            return Some(3);
        }
        if input == Some(Confirm) && !self.synopsis.reading {
            let (entry, record) = match self.synopsis_entry() {
                Ok(entry) => entry,
                Err(error) => {
                    self.report_failure("Synopsis is unavailable", error);
                    return Some(4);
                }
            };
            if entry.lines(record.value).is_empty() {
                return Some(4);
            }
            self.synopsis.reading = true;
            self.synopsis.text_opacity = 0;
            self.synopsis.line = 0;
            return Some(2);
        }
        let page = matches!(input, Some(PreviousTab | NextTab));
        let up = matches!(input, Some(Up | PreviousTab));
        let down = matches!(input, Some(Down | NextTab));
        if !up && !down {
            return None;
        }
        if self.synopsis.reading {
            let (entry, record) = match self.synopsis_entry() {
                Ok(entry) => entry,
                Err(error) => {
                    self.report_failure("Synopsis is unavailable", error);
                    return Some(4);
                }
            };
            let count = entry.lines(record.value).len();
            let old = self.synopsis.line;
            let step = if page { SYNOPSIS_TEXT_ROWS } else { 1 };
            self.synopsis.line = if up {
                old.saturating_sub(step)
            } else if old + SYNOPSIS_TEXT_ROWS < count {
                (old + step).min(count - 1)
            } else {
                old
            };
            if !page {
                self.synopsis.text_scroll =
                    (self.synopsis.line as isize - old as isize).signum() as i8;
            }
            return (old != self.synopsis.line).then_some(if page { 38 } else { 1 });
        }
        let count = self.synopsis_records().len();
        let old = self.synopsis.row;
        let state = &mut self.synopsis;
        let first = state.first;
        if page {
            if up {
                let step = state.first.min(SYNOPSIS_LIST_ROWS);
                state.first -= step;
                state.row -= step;
            } else if state.first + SYNOPSIS_LIST_ROWS < count {
                state.first += SYNOPSIS_LIST_ROWS;
                state.row = (state.row + SYNOPSIS_LIST_ROWS).min(count - 1);
            }
        } else {
            state.row = if up {
                old.saturating_sub(1)
            } else {
                (old + 1).min(count - 1)
            };
            state.first = state
                .first
                .min(state.row)
                .max(state.row.saturating_sub(SYNOPSIS_LIST_ROWS - 1));
            state.list_scroll = (state.first as isize - first as isize).signum() as i8;
        }
        (old != state.row).then_some(if page { 38 } else { 1 })
    }
}

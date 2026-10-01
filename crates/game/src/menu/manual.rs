use super::*;
use resonance_content::menu_data::{ManualChapter, ManualTopic};

#[derive(Default, serde::Serialize)]
pub struct Manual {
    pub page_fade: u8,
    pub page_closing: bool,
    pub chapter: usize,
    pub topic: usize,
    pub paragraph: usize,
    pub reading: bool,
}
impl Menu {
    pub fn manual_data(&self) -> anyhow::Result<&menu_data::TrainingManual> {
        self.manual_data
            .as_ref()
            .context("manual page has not been prepared")
    }

    pub(super) fn advance_manual(&mut self) -> bool {
        let state = &mut self.manual;
        if state.page_fade == 255 {
            self.open_items();
            return true;
        }
        state.page_fade = if state.page_closing {
            state.page_fade.saturating_add(MAIN_SLIDE_STEP)
        } else {
            state.page_fade.saturating_sub(MAIN_SLIDE_STEP)
        };
        if state.page_fade == 255 {
            state.page_closing = false;
        }
        false
    }

    pub fn manual_chapters(&self) -> Vec<(&ManualChapter, Vec<&ManualTopic>)> {
        let Some(data) = &self.manual_data else {
            return Vec::new();
        };
        let Some(checkpoint) = &self.checkpoint else {
            return Vec::new();
        };
        let flags = &checkpoint.progress().event_flags;
        data.chapters
            .iter()
            .filter_map(|chapter| {
                let topics: Vec<_> = chapter
                    .topics
                    .iter()
                    .filter(|t| flags.contains(&t.learned_flag))
                    .collect();
                (!topics.is_empty()).then_some((chapter, topics))
            })
            .collect()
    }
    pub(super) fn step_manual(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let [up, down, page_up, page_down] =
            [Up, Down, PageUp, PageDown].map(|action| input == Some(action));
        if input == Some(Cancel) {
            if self.manual.reading {
                self.manual.reading = false;
            } else {
                self.manual.page_closing = true;
            }
            return Some(3);
        }
        let chapters = self.manual_chapters();
        let (_, topics) = chapters.get(self.manual.chapter)?;
        let count = if self.manual.reading {
            topics.len()
        } else {
            chapters.len()
        };
        let paragraphs = topics
            .get(self.manual.topic)
            .map_or(0, |t| t.paragraphs.len());
        let state = &mut self.manual;
        if input == Some(Confirm) {
            if state.reading {
                return None;
            }
            state.reading = true;
            state.topic = 0;
            state.paragraph = 0;
            return Some(2);
        }
        if (up || down) && count > 1 {
            let row = if state.reading {
                &mut state.topic
            } else {
                &mut state.chapter
            };
            *row = if up {
                (*row + count - 1) % count
            } else {
                (*row + 1) % count
            };
            if state.reading {
                state.paragraph = 0;
            }
            return Some(1);
        }
        if state.reading {
            if page_up && state.paragraph > 0 {
                state.paragraph -= 1;
                return Some(38);
            }
            if page_down && state.paragraph + 1 < paragraphs {
                state.paragraph += 1;
                return Some(38);
            }
        }
        None
    }
}

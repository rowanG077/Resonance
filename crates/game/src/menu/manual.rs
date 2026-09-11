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
    pub(super) fn advance_manual(&mut self) -> bool {
        let state = &mut self.manual;
        if state.page_fade == 255 {
            self.page = Page::Items;
            self.open_items();
            self.fade_item_description();
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
        let flags = &self.checkpoint.as_ref().unwrap().progress.event_flags;
        self.resources
            .as_ref()
            .unwrap()
            .data
            .manual
            .chapters
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
    pub(super) fn step_manual(
        &mut self,
        input: crate::field::FieldInput,
        [up, down, page_up, page_down]: [bool; 4],
    ) -> Option<i16> {
        if input.cancel {
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
        if input.interact {
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

use super::*;
use resonance_events::party::Member;

#[derive(Debug, Default, serde::Serialize)]
pub struct Status {
    pub details: bool,
    pub title_focus: bool,
    pub row: usize,
    pub page_fade: u8,
    pub closing: bool,
    pub previous: Option<usize>,
    pub portrait_fade: u8,
    pub title_opacity: u8,
    pub title_closing: bool,
}

const PORTRAIT_FADE: u8 = 240;

impl Status {
    pub(super) fn opening() -> Self {
        Self {
            page_fade: 231 - MAIN_SLIDE_STEP,
            portrait_fade: PORTRAIT_FADE,
            ..Default::default()
        }
    }

    pub fn animating(&self) -> bool {
        self.page_fade != 0
            || self.closing
            || self.title_closing
            || self.title_opacity != 0 && self.title_opacity != 255
    }
}

impl Menu {
    pub(super) fn advance_status(&mut self) -> bool {
        let member = self.member_index();
        let state = &mut self.status;
        if state.portrait_fade == 0 {
            state.previous = Some(member);
        }
        if state.page_fade == 255 {
            if self.rename.pending {
                self.page = Page::Rename;
                self.advance_rename();
            } else {
                self.select_main(Page::Status);
                self.return_to_main();
            }
            return true;
        }
        state.page_fade = if state.closing {
            state.page_fade.saturating_add(MAIN_SLIDE_STEP)
        } else {
            state.page_fade.saturating_sub(MAIN_SLIDE_STEP)
        };
        if state.closing && state.page_fade == 255 {
            state.closing = false;
        }
        if self.page == Page::Titles {
            state.title_opacity = if state.title_closing {
                state.title_opacity.saturating_sub(32)
            } else {
                state.title_opacity.saturating_add(32)
            };
            if state.title_closing && state.title_opacity == 0 {
                self.page = Page::Status;
                state.title_closing = false;
            }
        }
        false
    }

    pub(super) fn animate_status_portrait(&mut self) {
        let member = self.member_index();
        let state = &mut self.status;
        if state.portrait_fade != 0 {
            state.portrait_fade -= 16;
        } else if state.previous != Some(member) {
            state.portrait_fade = PORTRAIT_FADE;
        }
    }

    pub fn member_index(&self) -> usize {
        usize::from(self.party().formation[self.character] - 1)
    }
    pub fn member(&self) -> &Member {
        &self.party().members[self.member_index()]
    }
    pub fn titles(&self) -> Vec<u8> {
        self.member().titles.iter().copied().collect()
    }
    pub(super) fn step_status(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let [left, right, up, down] = [Left, Right, Up, Down].map(|action| input == Some(action));
        let length = self.checkpoint.as_ref()?.progress.party.formation.len();
        match self.page {
            Page::Status => {
                if matches!(input, Some(NextTab | PageDown)) && !self.status.details
                    || matches!(input, Some(PreviousTab | PageUp)) && self.status.details
                {
                    self.status.details = matches!(input, Some(NextTab | PageDown));
                    return Some(0x26);
                }
                if left || right {
                    self.character = (self.character + if left { length - 1 } else { 1 }) % length;
                    return Some(1);
                }
                if up && self.status.title_focus || down && !self.status.title_focus {
                    self.status.title_focus = down;
                    return Some(1);
                }
                if input == Some(Confirm) && self.status.title_focus {
                    if !self.admit_page(Page::Titles) {
                        return Some(4);
                    }
                    self.status.row = self
                        .titles()
                        .iter()
                        .position(|id| *id == self.member().title)
                        .unwrap();
                    self.page = Page::Titles;
                    self.status.title_closing = false;
                    return Some(2);
                }
                if input == Some(Confirm) && self.can_rename() {
                    return Some(
                        if self.open_rename(rename::Origin::Status, self.member_index()) {
                            2
                        } else {
                            4
                        },
                    );
                }
                None
            }
            Page::Titles => {
                let titles = self.titles();
                let previous = self.status.row;
                if up {
                    self.status.row = self.status.row.saturating_sub(1);
                }
                if down {
                    self.status.row = (self.status.row + 1).min(titles.len() - 1);
                }
                if input == Some(Confirm) {
                    let member = self.member_index();
                    self.checkpoint.as_mut().unwrap().progress.party.members[member].title =
                        titles[self.status.row];
                    self.party_changed = true;
                    self.status.title_closing = true;
                    return Some(2);
                }
                (previous != self.status.row).then_some(1)
            }
            _ => unreachable!(),
        }
    }
}

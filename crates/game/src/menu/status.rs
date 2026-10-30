use super::*;
use resonance_content::appearance;
use resonance_events::party::Member;
pub const VISIBLE_TITLES: usize = 9;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Name,
    Title,
    Costume,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Status {
    pub details: bool,
    pub focus: Focus,
    pub row: usize,
    pub title_first: usize,
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
    /// Default appearance followed by unique costumes unlocked by owned titles.
    /// Title IDs supply captions; zero denotes the story's default outfit.
    pub fn costumes(&self) -> Vec<u8> {
        let mut costumes = vec![0];
        let Some(resources) = &self.resources else {
            return costumes;
        };
        let titles = &resources.data.titles[self.member_index()];
        let mut seen = std::collections::BTreeSet::new();
        for &id in &self.member().titles {
            if let Some(costume) = titles[usize::from(id - 1)].costume
                && seen.insert(costume)
            {
                costumes.push(id);
            }
        }
        costumes
    }

    fn costume_title(&self) -> Option<u8> {
        let rules = &self.resources.as_ref()?.data.titles[self.member_index()];
        self.member().costume_title(rules)
    }

    fn default_costume(&self) -> u8 {
        let progress = self.checkpoint.as_ref().unwrap().progress();
        appearance::story_costume(
            self.member_index() as u32 + 1,
            progress.script_globals[usize::from(appearance::ANGEL_PROGRESS / 4)],
            &progress.event_flags,
        )
    }

    /// Entries use title IDs; zero is Default. None means the equipped outfit
    /// has no selectable entry (for example, an outfit selected by an event).
    pub fn status_choices(&self) -> (Vec<u8>, Option<u8>) {
        if self.status.focus == Focus::Costume {
            (
                self.costumes(),
                self.costume_title()
                    .or_else(|| (self.member().costume == self.default_costume()).then_some(0)),
            )
        } else {
            (self.titles(), Some(self.member().title))
        }
    }

    pub(super) fn step_status(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let length = self.checkpoint.as_ref()?.progress().party.formation.len();
        match self.page {
            Page::Status => {
                if matches!(input, Some(NextTab | PageDown)) && !self.status.details
                    || matches!(input, Some(PreviousTab | PageUp)) && self.status.details
                {
                    self.status.details = matches!(input, Some(NextTab | PageDown));
                    return Some(0x26);
                }
                if matches!(input, Some(Left | Right)) {
                    self.character = (self.character
                        + if input == Some(Left) { length - 1 } else { 1 })
                        % length;
                    return Some(1);
                }
                let focus = match (self.status.focus, input) {
                    (Focus::Name, Some(Down)) | (Focus::Costume, Some(Up)) => Focus::Title,
                    (Focus::Title, Some(Up)) => Focus::Name,
                    (Focus::Title, Some(Down)) => Focus::Costume,
                    (focus, _) => focus,
                };
                if focus != self.status.focus {
                    self.status.focus = focus;
                    return Some(1);
                }
                if input == Some(Confirm) && self.status.focus != Focus::Name {
                    if !self.admit_page(Page::Titles) {
                        return Some(4);
                    }
                    let (choices, selected) = self.status_choices();
                    self.status.row = choices
                        .iter()
                        .position(|id| Some(*id) == selected)
                        .unwrap_or(0);
                    self.status.title_first = self.status.row.saturating_sub(VISIBLE_TITLES - 1);
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
                let costumes = self.status.focus == Focus::Costume;
                let (choices, _) = self.status_choices();
                let previous = self.status.row;
                let delta = match input {
                    Some(Up) => -1,
                    Some(Down) => 1,
                    Some(PageUp | PreviousTab) => -(VISIBLE_TITLES as isize),
                    Some(PageDown | NextTab) => VISIBLE_TITLES as isize,
                    _ => 0,
                };
                move_list(
                    &mut self.status.row,
                    &mut self.status.title_first,
                    choices.len(),
                    VISIBLE_TITLES,
                    delta,
                );
                if input == Some(Confirm) {
                    let index = self.member_index();
                    let title = choices[self.status.row];
                    if costumes {
                        let costume = title
                            .checked_sub(1)
                            .and_then(|title| {
                                self.resources.as_ref()?.data.titles[index][usize::from(title)]
                                    .costume
                            })
                            .map_or_else(|| self.default_costume(), |costume| costume as u8);
                        self.checkpoint.as_mut()?.progress_mut().party.members[index].costume =
                            costume;
                    } else {
                        self.checkpoint.as_mut()?.progress_mut().party.members[index].title = title;
                    }
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

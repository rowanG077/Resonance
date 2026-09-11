use super::*;

pub const VISIBLE_CHOICES: usize = 4;
const PREVIEW_STEP: u8 = 32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    Character,
    #[default]
    Gems,
    Skills,
    GemList,
    SkillList,
    Confirm {
        yes: bool,
        replacing: bool,
    },
    Compounds,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Description {
    #[default]
    None,
    Gem(u8),
    Skill(u8),
}

#[derive(Debug, serde::Serialize)]
pub struct ExSkills {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub focus: Focus,
    pub slot: usize,
    pub skill: usize,
    pub gem: usize,
    pub compound: usize,
    pub first: usize,
    pub scroll: i8,
    pub preview_opacity: u8,
    pub preview_closing: Option<Focus>,
    pub preview_previous_opacity: u8,
    pub popup_opacity: u8,
    pub description_previous: Description,
    pub description_fade: u8,
    pub description_opacity: u8,
}

impl Default for ExSkills {
    fn default() -> Self {
        Self {
            transition: Default::default(),
            focus: Default::default(),
            slot: 0,
            skill: 0,
            gem: 0,
            compound: 0,
            first: 0,
            scroll: 0,
            preview_opacity: 0,
            preview_closing: None,
            preview_previous_opacity: 0,
            popup_opacity: 0,
            description_previous: Description::None,
            description_fade: 0,
            description_opacity: 255,
        }
    }
}

impl ExSkills {
    pub fn animating(&self) -> bool {
        self.transition.animating()
            || self.scroll != 0
            || self.preview_closing.is_some()
            || self.has_preview() && self.preview_opacity != 255
            || self.preview_previous_opacity != 0
    }

    pub(super) fn opening() -> Self {
        Self {
            transition: super::Transition::opening(),
            ..Default::default()
        }
    }

    fn has_preview(&self) -> bool {
        matches!(
            self.focus,
            Focus::GemList | Focus::SkillList | Focus::Confirm { .. }
        )
    }
}

impl Menu {
    pub fn has_ex_skills(&self) -> bool {
        let Some(resources) = &self.resources else {
            return false;
        };
        self.checkpoint.as_ref().is_some_and(|c| {
            let party = &c.progress.party;
            resources.data.ex_skills.gem_items[..4]
                .iter()
                .any(|id| party.items.contains_key(id))
                || party
                    .formation
                    .iter()
                    .any(|&id| party.members[usize::from(id - 1)].ex_gems != [0; 4])
        })
    }

    pub fn ex_gems(&self) -> Vec<u8> {
        let party = self.party();
        self.resources
            .as_ref()
            .unwrap()
            .data
            .ex_skills
            .gem_items
            .iter()
            .enumerate()
            .filter_map(|(i, id)| party.items.contains_key(id).then_some(i as u8 + 1))
            .collect()
    }

    pub fn ex_choices(&self) -> Vec<u8> {
        let level = if matches!(self.ex_skills.focus, Focus::GemList | Focus::Confirm { .. }) {
            self.ex_gems().get(self.ex_skills.gem).copied().unwrap_or(0)
        } else {
            self.member().ex_gems[self.ex_skills.slot]
        };
        let choices =
            &self.resources.as_ref().unwrap().data.ex_skills.characters[self.member_index()].levels;
        match level {
            1..=4 => choices[usize::from(level - 1)].to_vec(),
            5 => choices.iter().flatten().copied().collect(),
            _ => Vec::new(),
        }
    }

    /// Candidate loadout for display; never changes the saved party.
    pub fn ex_preview(&self) -> resonance_events::party::Member {
        let mut member = self.member().clone();
        if self.ex_skills.focus == Focus::SkillList {
            member.ex_skills[self.ex_skills.slot] = self.ex_choices()[self.ex_skills.skill];
        }
        member
    }

    /// Equipped, learned compounds available for navigation.
    pub fn ex_compounds(&self) -> Vec<u8> {
        self.member().active_compound_ex(
            &self.resources.as_ref().unwrap().data.ex_skills,
            self.member_index(),
        )
    }

    pub fn ex_description(&self) -> Description {
        let state = &self.ex_skills;
        let skill = match state.focus {
            Focus::Skills => self.member().ex_skills[state.slot],
            Focus::SkillList => self.ex_choices()[state.skill],
            Focus::Compounds => {
                self.resources.as_ref().unwrap().data.ex_skills.characters[self.member_index()]
                    .compounds[usize::from(self.ex_compounds()[state.compound])]
                .skill
            }
            _ => 0,
        };
        let gem = match state.focus {
            Focus::GemList | Focus::Confirm { .. } => self.ex_gems()[state.gem],
            Focus::Gems => self.member().ex_gems[state.slot],
            _ => 0,
        };
        match (skill, gem) {
            (0, 0) => Description::None,
            (0, gem) => Description::Gem(gem),
            (skill, _) => Description::Skill(skill),
        }
    }

    pub(super) fn remember_ex_description(&mut self) {
        if self.ex_skills.description_fade == 0 {
            self.ex_skills.description_previous = self.ex_description();
        }
    }

    pub(super) fn fade_ex_description(&mut self) {
        let selected = self.ex_description();
        let state = &mut self.ex_skills;
        state.description_opacity = fade_description(
            &mut state.description_fade,
            state.description_previous != selected,
        );
    }

    pub(super) fn step_ex_skills(
        &mut self,
        input: crate::field::FieldInput,
        directions: [bool; 6],
    ) -> Option<i16> {
        let state = &mut self.ex_skills;
        if let Some(focus) = state.preview_closing {
            state.preview_opacity = state.preview_opacity.saturating_sub(PREVIEW_STEP);
            if state.preview_opacity != 0 {
                return None;
            }
            state.preview_closing = None;
            state.focus = focus;
        } else if state.has_preview() && state.preview_opacity != 255 {
            state.preview_opacity = state.preview_opacity.saturating_add(PREVIEW_STEP);
            state.preview_previous_opacity =
                state.preview_previous_opacity.saturating_sub(PREVIEW_STEP);
            if state.preview_opacity != 255 {
                return None;
            }
        }
        state.scroll = (state.scroll + state.scroll.signum()) % 5;
        if state.scroll != 0 {
            return None;
        }
        let cue = self.ex_input(input, directions);
        let state = &mut self.ex_skills;
        state.popup_opacity = if matches!(state.focus, Focus::Confirm { .. }) {
            state.popup_opacity.saturating_add(PREVIEW_STEP)
        } else {
            state.popup_opacity.saturating_sub(PREVIEW_STEP)
        };
        cue
    }

    fn ex_input(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let focus = self.ex_skills.focus;
        if input.cancel {
            self.ex_skills.focus = match focus {
                Focus::Character | Focus::Gems | Focus::Skills => {
                    self.ex_skills.transition.page_closing = true;
                    self.select_main(Page::ExSkills);
                    focus
                }
                Focus::GemList | Focus::SkillList => {
                    self.ex_skills.preview_closing = Some(if focus == Focus::GemList {
                        Focus::Gems
                    } else {
                        Focus::Skills
                    });
                    focus
                }
                Focus::Compounds => Focus::Skills,
                Focus::Confirm { .. } => Focus::GemList,
            };
            return Some(if focus == Focus::Compounds { 1 } else { 3 });
        }
        if matches!(focus, Focus::Character | Focus::Gems | Focus::Skills)
            && (input.previous_page
                || input.next_page
                || focus == Focus::Character && (left || right))
        {
            let count = self.party().formation.len();
            self.character = (self.character
                + if input.previous_page || left {
                    count - 1
                } else {
                    1
                })
                % count;
            if focus == Focus::Skills && self.member().ex_gems[self.ex_skills.slot] == 0 {
                self.ex_skills.focus = Focus::Gems;
            }
            self.ex_skills.first = 0;
            return Some(1);
        }
        match focus {
            Focus::Character => {
                if input.interact || down {
                    self.ex_skills.focus = Focus::Gems;
                    self.ex_skills.slot = 0;
                    return Some(2);
                }
            }
            Focus::Gems | Focus::Skills => {
                if up || down {
                    let next = if focus == Focus::Gems {
                        if up {
                            self.ex_skills.slot.checked_sub(1)
                        } else {
                            Some((self.ex_skills.slot + 1).min(3))
                        }
                    } else if up {
                        (0..self.ex_skills.slot)
                            .rev()
                            .find(|&i| self.member().ex_gems[i] != 0)
                    } else {
                        (self.ex_skills.slot + 1..4)
                            .find(|&i| self.member().ex_gems[i] != 0)
                            .or(Some(self.ex_skills.slot))
                    };
                    if let Some(slot) = next {
                        if slot == self.ex_skills.slot {
                            return None;
                        }
                        self.ex_skills.slot = slot;
                    } else {
                        self.ex_skills.focus = Focus::Character;
                    }
                    self.ex_skills.first = 0;
                    return Some(1);
                }
                if focus == Focus::Gems && right {
                    if self.member().ex_gems[self.ex_skills.slot] == 0 {
                        return Some(4);
                    }
                    self.ex_skills.focus = Focus::Skills;
                    self.ex_skills.first = 0;
                    return Some(1);
                }
                if focus == Focus::Skills && left {
                    self.ex_skills.focus = Focus::Gems;
                    self.ex_skills.first = 0;
                    return Some(1);
                }
                if focus == Focus::Skills && right && !self.ex_compounds().is_empty() {
                    self.ex_skills.focus = Focus::Compounds;
                    self.ex_skills.compound = 0;
                    self.ex_skills.first = 0;
                    return Some(1);
                }
                if input.interact {
                    if focus == Focus::Gems {
                        if self.ex_gems().is_empty() {
                            return Some(4);
                        }
                        self.ex_skills.gem = 0;
                        self.ex_skills.focus = Focus::GemList;
                    } else {
                        let equipped = self.member().ex_skills[self.ex_skills.slot];
                        self.ex_skills.skill = self
                            .ex_choices()
                            .iter()
                            .position(|&s| s == equipped)
                            .unwrap_or(0);
                        self.ex_skills.focus = Focus::SkillList;
                    }
                    self.ex_skills.first = if focus == Focus::Skills {
                        self.ex_skills.skill.saturating_sub(VISIBLE_CHOICES - 1)
                    } else {
                        0
                    };
                    self.ex_skills.scroll = 0;
                    self.ex_skills.preview_opacity = 0;
                    return Some(2);
                }
            }
            Focus::GemList | Focus::SkillList => {
                let count = if focus == Focus::GemList {
                    self.ex_gems().len()
                } else {
                    self.ex_choices().len()
                };
                if input.interact {
                    if focus == Focus::GemList {
                        let current = self.member().ex_gems[self.ex_skills.slot];
                        if current == self.ex_gems()[self.ex_skills.gem] {
                            return Some(4);
                        }
                        self.ex_skills.focus = Focus::Confirm {
                            yes: true,
                            replacing: current != 0,
                        };
                    } else {
                        let skill = self.ex_choices()[self.ex_skills.skill];
                        let member = self.member_index();
                        let session = &self.resources.as_ref().unwrap().session;
                        let result = self
                            .checkpoint
                            .as_mut()
                            .unwrap()
                            .progress
                            .party
                            .set_ex_skill(session, member, self.ex_skills.slot, skill)
                            .expect("validated EX skill selection");
                        if !result {
                            return Some(4);
                        }
                        self.party_changed = true;
                        self.ex_skills.preview_closing = Some(Focus::Skills);
                    }
                    return Some(2);
                }
                let row = if focus == Focus::GemList {
                    &mut self.ex_skills.gem
                } else {
                    &mut self.ex_skills.skill
                };
                let before = (*row, self.ex_skills.first);
                if page_up || page_down {
                    self.ex_skills.first = if page_up {
                        self.ex_skills.first.saturating_sub(VISIBLE_CHOICES)
                    } else {
                        (self.ex_skills.first + VISIBLE_CHOICES)
                            .min(count.saturating_sub(VISIBLE_CHOICES))
                    };
                    *row = (*row).clamp(
                        self.ex_skills.first,
                        (self.ex_skills.first + VISIBLE_CHOICES - 1).min(count - 1),
                    );
                } else {
                    if up {
                        *row = row.saturating_sub(1);
                    }
                    if down {
                        *row = (*row + 1).min(count - 1);
                    }
                    self.ex_skills.first = self
                        .ex_skills
                        .first
                        .min(*row)
                        .max(row.saturating_sub(VISIBLE_CHOICES - 1));
                    self.ex_skills.scroll =
                        (self.ex_skills.first as isize - before.1 as isize).signum() as i8;
                }
                if before != (*row, self.ex_skills.first) {
                    return Some(if page_up || page_down { 38 } else { 1 });
                }
            }
            Focus::Confirm { yes, replacing } => {
                if up || down {
                    self.ex_skills.focus = Focus::Confirm {
                        yes: !yes,
                        replacing,
                    };
                    return Some(1);
                }
                if input.interact {
                    if !yes {
                        self.ex_skills.focus = Focus::GemList;
                        return Some(3);
                    }
                    let member = self.member_index();
                    let level = self.ex_gems()[self.ex_skills.gem];
                    let session = &self.resources.as_ref().unwrap().session;
                    let changed = self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .set_ex_gem(session, member, self.ex_skills.slot, level)
                        .expect("validated EX gem selection");
                    assert!(changed, "confirmed EX gem became unavailable");
                    self.party_changed = true;
                    self.ex_skills.focus = Focus::SkillList;
                    self.ex_skills.skill = 0;
                    self.ex_skills.first = 0;
                    self.ex_skills.scroll = 0;
                    self.ex_skills.preview_opacity = 0;
                    self.ex_skills.preview_previous_opacity = 255;
                    return Some(2);
                }
            }
            Focus::Compounds => {
                if left {
                    self.ex_skills.focus = Focus::Skills;
                    return Some(1);
                }
                let previous = self.ex_skills.compound;
                if up {
                    self.ex_skills.compound = previous.saturating_sub(1);
                }
                if down {
                    self.ex_skills.compound = (previous + 1).min(self.ex_compounds().len() - 1);
                }
                if previous != self.ex_skills.compound {
                    return Some(1);
                }
            }
        }
        None
    }
}

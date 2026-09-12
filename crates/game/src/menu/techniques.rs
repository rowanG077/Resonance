use super::*;
use resonance_content::menu_data::TechniqueUse;
use resonance_events::party::TechniqueShortcut;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    Character,
    Control,
    #[default]
    Shortcuts,
    List,
    AssistCharacter,
    AssistList,
    Target,
    CannotForget,
    Forget {
        yes: bool,
    },
}
#[derive(Debug, Default, serde::Serialize)]
pub struct Tech {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub focus: Focus,
    pub unison: bool,
    pub slot: usize,
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    pub description_previous: Option<TechniqueShortcut>,
    pub description_fade: u8,
    pub description_opacity: u8,
    pub banner_opacity: u8,
    pub banner_closing: Option<Focus>,
    pub cannot_forget_opacity: u8,
    pub forget_opacity: u8,
    pub forget_yes: bool,
    pub assist: usize,
    pub target: usize,
    pub target_preview: usize,
    pub target_ticks: u8,
    pub target_opacity: u8,
    pub return_to: Focus,
}

impl Tech {
    pub(super) fn opening() -> Self {
        Self {
            transition: super::Transition::opening(),
            description_fade: DESCRIPTION_FADE_START,
            description_opacity: 15,
            ..Default::default()
        }
    }
    fn banner_visible(&self) -> bool {
        self.unison
            || matches!(self.focus, Focus::AssistCharacter | Focus::AssistList)
            || self.focus == Focus::Target && self.return_to == Focus::AssistList
    }
    pub fn animating(&self) -> bool {
        self.transition.animating()
            || self.scroll != 0
            || self.banner_closing.is_some()
            || self.banner_visible() && self.banner_opacity != 255
            || self.focus != Focus::Target && self.target_opacity != 0
    }
    fn fade_popups(&mut self) {
        if self.focus == Focus::CannotForget {
            if self.forget_opacity != 0 {
                self.cannot_forget_opacity = std::mem::take(&mut self.forget_opacity);
            }
            self.cannot_forget_opacity = self.cannot_forget_opacity.saturating_add(32);
        } else {
            self.cannot_forget_opacity = self.cannot_forget_opacity.saturating_sub(32);
        }
        if let Focus::Forget { yes } = self.focus {
            self.forget_yes = yes;
            if self.cannot_forget_opacity != 0 {
                self.forget_opacity = std::mem::take(&mut self.cannot_forget_opacity);
            }
            self.forget_opacity = self.forget_opacity.saturating_add(32);
        } else {
            self.forget_opacity = self.forget_opacity.saturating_sub(32);
        }
    }
}

impl Menu {
    pub fn tech_description(&self) -> Option<TechniqueShortcut> {
        self.selected_technique()
            .filter(|_| !self.tech_target_visible())
    }
    pub(super) fn remember_tech_description(&mut self) {
        if self.tech.description_fade == 0 {
            self.tech.description_previous = self.tech_description();
        }
    }
    pub(super) fn fade_tech_description(&mut self) {
        let changed = self.tech.description_fade == 0
            && self.tech.description_previous != self.tech_description();
        self.tech.description_opacity = fade_description(&mut self.tech.description_fade, changed);
    }
    fn close_tech_banner(&mut self, focus: Focus) {
        self.tech.banner_closing = Some(focus);
    }
    pub fn tech_target_visible(&self) -> bool {
        self.tech.focus == Focus::Target || self.tech.target_opacity > 0
    }
    pub(super) fn fade_tech_target(&mut self) {
        const FADE_STEP: u8 = 32;
        self.tech.target_opacity = if self.tech.focus == Focus::Target {
            self.tech.target_opacity.saturating_add(FADE_STEP)
        } else {
            self.tech.target_opacity.saturating_sub(FADE_STEP)
        };
    }
    pub(super) fn step_tech_preview(&mut self) {
        self.tech.target_preview = self.tech.target;
        if self.tech_target_visible() && self.tech_targets_all() {
            const PREVIEW_TICKS: u8 = 121;
            self.tech.target_ticks += 1;
            if self.tech.target_ticks == PREVIEW_TICKS {
                self.tech.target_ticks = 0;
                let count = self.party().formation.len();
                self.tech.target = (self.tech.target + 1) % count;
            }
        }
    }
    pub fn tech_unison_available(&self) -> bool {
        // Input currently belongs to player one; other party slots have no controller.
        (1..VISIBLE_PARTY).contains(&self.character) && self.tech_auto()
    }
    pub fn tech_targets_all(&self) -> bool {
        self.selected_technique().is_some_and(|selected| {
            matches!(
                self.resources.as_ref().unwrap().data.techniques[usize::from(selected.technique)]
                    .field_use,
                Some(
                    TechniqueUse::Recover { party: true, .. } | TechniqueUse::Cure { party: true }
                )
            )
        })
    }
    pub fn tech_auto(&self) -> bool {
        self.party()
            .settings
            .battle_controls
            .get(self.character)
            .is_none_or(|&control| control == 2)
    }
    pub fn tech_member_index(&self) -> usize {
        let assisted = matches!(self.tech.focus, Focus::AssistCharacter | Focus::AssistList)
            || self.tech.focus == Focus::Target && self.tech.return_to == Focus::AssistList;
        let index = if assisted {
            self.tech.assist
        } else {
            self.character
        };
        usize::from(self.party().formation[index] - 1)
    }
    pub fn tech_columns(&self) -> usize {
        if self.tech.unison {
            return 1;
        }
        let party = self.party();
        let slot = party
            .formation
            .iter()
            .position(|&id| usize::from(id - 1) == self.tech_member_index())
            .unwrap();
        if party
            .settings
            .battle_controls
            .get(slot)
            .is_none_or(|&control| control == 2)
        {
            2
        } else {
            1
        }
    }
    pub fn technique_list(&self) -> Vec<u16> {
        let index = self.tech_member_index();
        let resources = self.resources.as_ref().unwrap();
        let member = &self.party().members[index];
        resources.session.characters[index]
            .allowed_techniques
            .iter()
            .copied()
            .filter(|id| {
                let tech = &resources.data.techniques[usize::from(*id)];
                member.techniques.contains(id)
                    || tech.level <= u16::from(member.level)
                        && match tech.route {
                            0 => true,
                            1 => member.technique_balance <= 0,
                            2 => member.technique_balance > 0,
                            _ => false,
                        }
            })
            .collect()
    }
    pub fn selected_technique(&self) -> Option<TechniqueShortcut> {
        if self.tech.focus == Focus::Shortcuts {
            if self.tech.slot >= 4 {
                return self.member().assist_shortcuts[self.tech.slot - 4];
            }
            let id = self.member().shortcuts[self.tech.slot];
            (id != 0).then_some(TechniqueShortcut {
                character: self.member_index(),
                technique: id,
            })
        } else if matches!(
            self.tech.focus,
            Focus::Character | Focus::Control | Focus::AssistCharacter
        ) {
            None
        } else {
            self.technique_list()
                .get(self.tech.row)
                .map(|id| TechniqueShortcut {
                    character: self.tech_member_index(),
                    technique: *id,
                })
        }
    }
    pub(super) fn reset_tech_focus(&mut self) {
        self.tech.unison = false;
        self.tech.slot = 0;
        self.tech.focus = if self.tech_auto() {
            Focus::List
        } else {
            Focus::Shortcuts
        };
        self.tech.row = 0;
        self.tech.first = 0;
        self.tech.scroll = 0;
    }
    fn reveal_technique(&mut self) {
        let columns = self.tech_columns();
        let visible = if columns == 2 { 12 } else { 8 };
        self.tech.first = self
            .tech
            .first
            .min(self.tech.row / columns * columns)
            .max(self.tech.row.saturating_sub(visible - 1).div_ceil(columns) * columns);
    }
    pub(super) fn step_techniques(
        &mut self,
        input: crate::field::FieldInput,
        directions: [bool; 6],
    ) -> Option<i16> {
        self.tech.scroll = (self.tech.scroll + self.tech.scroll.signum()) % 5;
        if self.tech.scroll != 0 {
            return None;
        }
        if let Some(focus) = self.tech.banner_closing {
            self.tech.banner_opacity = self.tech.banner_opacity.saturating_sub(32);
            if self.tech.banner_opacity != 0 {
                return None;
            }
            self.tech.focus = focus;
            self.tech.unison = false;
            self.tech.banner_closing = None;
        } else if self.tech.banner_visible() && self.tech.banner_opacity != 255 {
            self.tech.banner_opacity = self.tech.banner_opacity.saturating_add(32);
            if self.tech.banner_opacity != 255 {
                return None;
            }
        }
        if self.tech.animating() {
            return None;
        }
        if let Focus::Forget { yes } = self.tech.focus {
            self.tech.forget_yes = yes;
        }
        let cue = self.dispatch_techniques(input, directions);
        self.tech.fade_popups();
        cue
    }
    fn dispatch_techniques(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let focus = self.tech.focus;
        let resources = self.resources.as_ref()?.clone();
        let member = self.member_index();
        let selected = self.selected_technique();
        if focus == Focus::CannotForget {
            if input.interact || input.cancel {
                self.tech.focus = self.tech.return_to;
                return Some(1);
            }
            return None;
        }
        if let Focus::Forget { yes } = focus {
            if input.cancel {
                self.tech.focus = self.tech.return_to;
                return Some(3);
            }
            if left || right {
                self.tech.focus = Focus::Forget { yes: !yes };
                return Some(1);
            }
            if input.interact {
                self.tech.focus = self.tech.return_to;
                if !yes {
                    return Some(if self.tech_auto() { 2 } else { 3 });
                }
                let result = self
                    .checkpoint
                    .as_mut()
                    .unwrap()
                    .progress
                    .party
                    .forget_technique(&resources.data, member, selected?.technique);
                self.tech.row = self
                    .tech
                    .row
                    .min(self.technique_list().len().saturating_sub(1));
                self.reveal_technique();
                return self.party_result(result);
            }
            return None;
        }
        if input.cancel {
            if focus == Focus::AssistCharacter || focus == Focus::Shortcuts && self.tech.unison {
                self.close_tech_banner(if self.tech.unison {
                    Focus::Character
                } else {
                    Focus::Shortcuts
                });
                return Some(3);
            }
            self.tech.focus = match focus {
                Focus::Character | Focus::Control => {
                    self.tech.transition.page_closing = true;
                    self.select_main(Page::Tech);
                    focus
                }
                Focus::Shortcuts => {
                    self.tech.unison = false;
                    Focus::Character
                }
                Focus::List => {
                    if self.tech_auto() && !self.tech.unison {
                        Focus::Character
                    } else {
                        Focus::Shortcuts
                    }
                }
                Focus::AssistCharacter => Focus::Shortcuts,
                Focus::AssistList => Focus::AssistCharacter,
                Focus::Target => self.tech.return_to,
                _ => unreachable!(),
            };
            return Some(3);
        }
        let count = self.party().formation.len();
        if matches!(focus, Focus::Character | Focus::Shortcuts | Focus::List)
            && !self.tech.unison
            && (input.previous_page
                || input.next_page
                || focus == Focus::Character && (left || right))
        {
            let before = self.character;
            for _ in 0..count {
                self.character = (self.character
                    + if input.previous_page || left {
                        count - 1
                    } else {
                        1
                    })
                    % count;
                if focus != Focus::Character || !self.member().knocked_out() {
                    break;
                }
            }
            if self.character == before {
                return None;
            }
            self.reset_tech_focus();
            if focus == Focus::Character {
                self.tech.focus = Focus::Character;
            }
            return Some(1);
        }
        match focus {
            Focus::Control => {
                if left || right {
                    let value = &mut self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .settings
                        .battle_controls[self.character];
                    *value = (*value + if left { 2 } else { 1 }) % 3;
                    self.tech.first = 0;
                    self.party_changed = true;
                    return Some(1);
                }
                if input.interact || down {
                    self.tech.focus = Focus::Character;
                    return Some(1);
                }
            }
            Focus::Character => {
                if input.start && self.character < VISIBLE_PARTY {
                    let control = &mut self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .settings
                        .battle_controls[self.character];
                    *control = (*control + 1) % 3;
                    self.tech.first = 0;
                    self.party_changed = true;
                    return Some(1);
                }
                if input.menu && self.tech_unison_available() {
                    self.tech.unison = true;
                    self.tech.banner_opacity = 0;
                    self.tech.focus = Focus::Shortcuts;
                    self.tech.slot = 0;
                    self.tech.row = 0;
                    self.tech.first = 0;
                    return Some(1);
                }
                if up && self.character < VISIBLE_PARTY {
                    self.tech.focus = Focus::Control;
                    return Some(1);
                }
                if input.interact || down {
                    if self.tech_auto() {
                        self.reset_tech_focus();
                    } else {
                        self.tech.focus = Focus::Shortcuts;
                        self.tech.slot = 0;
                    }
                    return Some(1);
                }
            }
            Focus::Shortcuts => {
                if input.alternate {
                    selected?;
                    self.party_changed |= self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .assign_technique(member, self.tech.slot, None)
                        .expect("validated shortcut slot");
                    return Some(1);
                }
                if input.interact {
                    self.tech.row = 0;
                    self.tech.first = 0;
                    self.tech.focus = if self.tech.slot >= 4 {
                        self.tech.assist = self.character;
                        self.tech.banner_opacity = 0;
                        Focus::AssistCharacter
                    } else {
                        Focus::List
                    };
                    if self.tech.focus == Focus::List {
                        self.tech.row = selected
                            .and_then(|s| {
                                self.technique_list()
                                    .iter()
                                    .position(|&id| id == s.technique)
                            })
                            .unwrap_or(0);
                        self.tech.first = self.tech.row.saturating_sub(7);
                    }
                    return Some(2);
                }
                if up {
                    if self.tech.slot == 0 {
                        if self.tech.unison {
                            self.tech.slot = 3;
                        } else {
                            self.tech.focus = Focus::Character;
                        }
                    } else {
                        self.tech.slot -= 1;
                    }
                    return Some(1);
                }
                if down && self.tech.unison {
                    self.tech.slot = (self.tech.slot + 1) % 4;
                    return Some(1);
                }
                if down {
                    if self.tech.slot < 5 {
                        self.tech.slot += 1;
                    } else {
                        self.tech.focus = Focus::Character;
                    }
                    return Some(1);
                }
            }
            Focus::AssistCharacter => {
                if left || right || input.previous_page || input.next_page {
                    self.tech.assist = (self.tech.assist
                        + if left || input.previous_page {
                            count - 1
                        } else {
                            1
                        })
                        % count;
                    self.tech.row = 0;
                    self.tech.first = 0;
                    return Some(1);
                }
                if input.interact || down {
                    self.tech.focus = Focus::AssistList;
                    return Some(2);
                }
            }
            Focus::Target => {
                if !self.tech_targets_all() {
                    let old = self.tech.target;
                    if up {
                        self.tech.target = old.saturating_sub(1);
                    } else if down {
                        self.tech.target = (old + 1).min(count - 1);
                    } else if left && old >= VISIBLE_PARTY {
                        self.tech.target -= VISIBLE_PARTY;
                    } else if right && old + VISIBLE_PARTY < count {
                        self.tech.target += VISIBLE_PARTY;
                    }
                    if old != self.tech.target {
                        return Some(1);
                    }
                }
                if input.interact {
                    let selected = selected?;
                    let party = &mut self.checkpoint.as_mut().unwrap().progress.party;
                    let target = usize::from(party.formation[self.tech.target] - 1);
                    return match party.cast_technique(
                        &resources.data,
                        selected.character,
                        target,
                        selected.technique,
                        self.at_save_point,
                    ) {
                        Ok(Some(cue)) => {
                            self.party_changed = true;
                            let caster = &party.members[selected.character];
                            if caster.tp
                                < caster.technique_cost(
                                    &resources.data,
                                    selected.technique,
                                    self.at_save_point,
                                )
                            {
                                self.tech.focus = self.tech.return_to;
                            }
                            Some(cue)
                        }
                        Ok(None) => Some(4),
                        Err(error) => {
                            self.notice = Some(error);
                            Some(4)
                        }
                    };
                }
            }
            Focus::List | Focus::AssistList => {
                if self.tech.unison && (input.alternate || input.menu) {
                    return None;
                }
                let list = self.technique_list();
                let owner = self.tech_member_index();
                let id = selected.map(|s| s.technique);
                if input.alternate && focus == Focus::List {
                    let id = id?;
                    if !self.member().techniques.contains(&id) {
                        return Some(4);
                    }
                    self.tech.return_to = focus;
                    self.tech.focus =
                        if resources.data.techniques[usize::from(id)].alternatives[0] == 0 {
                            Focus::CannotForget
                        } else {
                            Focus::Forget { yes: false }
                        };
                    return Some(if self.tech.focus == Focus::CannotForget {
                        4
                    } else {
                        2
                    });
                }
                if input.menu && self.tech_auto() && focus == Focus::List {
                    let id = id?;
                    let target =
                        &mut self.checkpoint.as_mut().unwrap().progress.party.members[owner];
                    if !target.techniques.contains(&id) {
                        return Some(4);
                    }
                    if !target.disabled_techniques.insert(id) {
                        target.disabled_techniques.remove(&id);
                    }
                    self.party_changed = true;
                    return Some(1);
                }
                let cast =
                    input.interact && self.tech_auto() && !self.tech.unison && focus == Focus::List
                        || input.menu;
                if cast {
                    let id = id?;
                    let target = &self.party().members[owner];
                    if resources.data.techniques[usize::from(id)]
                        .field_use
                        .is_none()
                        || !target.techniques.contains(&id)
                        || target.conditions & 0x8000_0100 != 0
                        || target.tp
                            < target.technique_cost(&resources.data, id, self.at_save_point)
                    {
                        return Some(4);
                    }
                    self.tech.return_to = focus;
                    self.tech.focus = Focus::Target;
                    return Some(2);
                }
                if input.interact {
                    let Some(selected) = selected else {
                        return Some(4);
                    };
                    if !self.party().members[owner]
                        .techniques
                        .contains(&selected.technique)
                    {
                        return Some(4);
                    }
                    let result = self
                        .checkpoint
                        .as_mut()
                        .unwrap()
                        .progress
                        .party
                        .assign_technique(member, self.tech.slot, Some(selected));
                    if result.is_ok() {
                        if focus == Focus::AssistList {
                            self.close_tech_banner(Focus::Shortcuts);
                        } else {
                            self.tech.focus = Focus::Shortcuts;
                        }
                    }
                    return self.party_result(result);
                }
                let old = self.tech.row;
                let columns = self.tech_columns();
                let visible = if columns == 2 { 12 } else { 8 };
                if page_up || page_down {
                    let before = (self.tech.row, self.tech.first);
                    if list.is_empty() {
                        return None;
                    }
                    let last = list.len() - 1;
                    if page_up {
                        let shift = self.tech.first.min(visible);
                        self.tech.first -= shift;
                        self.tech.row = if shift == 0 { 0 } else { old - shift };
                    } else if list.len().div_ceil(columns) + usize::from(columns == 1)
                        > (self.tech.first + visible) / columns
                    {
                        self.tech.first += visible;
                        self.tech.row = (old + visible).min(last);
                        if columns == 1 && old + visible >= list.len() {
                            self.tech.first = list.len().saturating_sub(visible);
                        }
                    } else {
                        self.tech.row = last;
                    }
                    return (before != (self.tech.row, self.tech.first)).then_some(38);
                }
                if up && columns == 2 && old < columns {
                    if focus == Focus::List {
                        self.tech.focus = Focus::Character;
                    }
                    return Some(1);
                }
                let delta = if up || down { columns } else { 1 };
                if up || left {
                    self.tech.row = self.tech.row.saturating_sub(delta);
                }
                if (down || right) && old + delta < list.len() {
                    self.tech.row += delta;
                }
                let first = self.tech.first;
                self.reveal_technique();
                self.tech.scroll = match self.tech.first.cmp(&first) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                return (old != self.tech.row).then_some(1);
            }
            _ => unreachable!(),
        }
        None
    }
}

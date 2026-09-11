use super::*;
use resonance_content::menu_data::{CUSTOMIZE_OPTIONS, CustomizeSettings};

pub const VISIBLE_OPTIONS: usize = 9;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Options,
    Colors,
    Volume,
    Controls,
    Position,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Customize {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub draft: CustomizeSettings,
    pub focus: Focus,
    pub row: usize,
    pub first: usize,
    pub scroll: i8,
    pub defaults: bool,
    pub color_group: usize,
    pub color_scroll: i8,
    /// Zero selects the group heading; one through four select RGBA.
    pub component: usize,
    pub channel: usize,
    pub button: usize,
    pub preview_wait: u8,
    pub preview_shown: u8,
    #[serde(skip)]
    pub preview_opacity: u8,
}
impl Customize {
    pub fn options_cursor(&self) -> [f32; 2] {
        let fade = u32::from(self.transition.page_fade);
        if self.row == CUSTOMIZE_OPTIONS {
            [
                408. + f32::from(self.defaults) * 100. + (fade * 236 / 256) as f32,
                28.,
            ]
        } else {
            [
                32. - (fade * 612 / 256) as f32,
                70. + (self.row - self.first) as f32 * 26.,
            ]
        }
    }
}
impl Menu {
    pub fn preferences(&self) -> Option<&CustomizeSettings> {
        if self.page == Page::Customize {
            Some(&self.customize.draft)
        } else {
            self.checkpoint
                .as_ref()
                .map(|c| &c.progress.party.settings.preferences)
        }
    }
    pub(super) fn open_customize(&mut self) {
        self.customize = Customize {
            transition: super::Transition::opening(),
            preview_shown: 1,
            draft: self.party().settings.preferences.clone(),
            ..Default::default()
        };
        self.entering = Some(Page::Customize);
    }
    pub(super) fn step_customize_preview(&mut self) {
        let state = &mut self.customize;
        if state.row != 0 {
            return;
        }
        state.preview_opacity = 255 - state.transition.page_fade;
        let speed = state.draft.message_speed;
        if speed == 0 {
            return;
        }
        let count = self.resources.as_ref().unwrap().data.customize.options[0]
            .description
            .chars()
            .filter(|c| !c.is_control())
            .count();
        if usize::from(state.preview_shown) >= count {
            if state.preview_wait >= 60 {
                state.preview_opacity = (u16::from(state.preview_opacity)
                    * u16::from(75 - state.preview_wait)
                    / 15) as u8;
            }
            state.preview_wait += 1;
            if state.preview_wait == 75 {
                state.preview_wait = 0;
                state.preview_shown = 0;
            }
        } else {
            state.preview_wait += 1;
            if state.preview_wait >= speed {
                state.preview_wait = 0;
                state.preview_shown += 1;
            }
        }
    }
    pub(super) fn step_customize(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let state = &mut self.customize;
        state.scroll = (state.scroll + state.scroll.signum()) % 5;
        state.color_scroll = (state.color_scroll + state.color_scroll.signum()) % 20;
        if state.transition.animating() || state.scroll != 0 || state.color_scroll != 0 {
            return None;
        }
        if input.cancel {
            if state.focus == Focus::Options {
                self.checkpoint
                    .as_mut()
                    .unwrap()
                    .progress
                    .party
                    .settings
                    .preferences = state.draft.clone();
                self.party_changed = true;
                state.transition.page_closing = true;
                self.select_main(Page::System);
            } else {
                state.focus = Focus::Options;
            }
            return Some(3);
        }
        let horizontal = left || right;
        let vertical = up || down;
        match state.focus {
            Focus::Options => {
                if vertical {
                    let old = state.row;
                    state.row = cycle(state.row, up, CUSTOMIZE_OPTIONS + 1);
                    if old == CUSTOMIZE_OPTIONS {
                        state.first = if up {
                            CUSTOMIZE_OPTIONS - VISIBLE_OPTIONS
                        } else {
                            0
                        };
                    } else if state.row < CUSTOMIZE_OPTIONS {
                        let first = state.first;
                        state.first = state
                            .first
                            .min(state.row)
                            .max(state.row.saturating_sub(VISIBLE_OPTIONS - 1));
                        state.scroll = (state.first as i32 - first as i32).signum() as i8;
                    }
                    return Some(1);
                }
                if page_up || page_down {
                    let first = if page_up {
                        0
                    } else {
                        CUSTOMIZE_OPTIONS - VISIBLE_OPTIONS
                    };
                    if first == state.first {
                        return None;
                    }
                    state.first = first;
                    if page_up {
                        if state.row > VISIBLE_OPTIONS && state.row != CUSTOMIZE_OPTIONS {
                            state.row = VISIBLE_OPTIONS - 1;
                        }
                    } else if state.row < first {
                        state.row = first;
                    }
                    return Some(38);
                }
                if state.row == CUSTOMIZE_OPTIONS {
                    if horizontal {
                        state.defaults = !state.defaults;
                        return Some(1);
                    }
                    if input.interact {
                        state.draft = if state.defaults {
                            self.resources
                                .as_ref()
                                .unwrap()
                                .data
                                .customize
                                .defaults
                                .clone()
                        } else {
                            self.checkpoint
                                .as_ref()
                                .unwrap()
                                .progress
                                .party
                                .settings
                                .preferences
                                .clone()
                        };
                        return Some(2);
                    }
                } else if input.interact {
                    state.focus = match state.row {
                        4 => {
                            state.component = 0;
                            Focus::Colors
                        }
                        5 => Focus::Volume,
                        6 => {
                            state.button = 0;
                            Focus::Controls
                        }
                        13 => Focus::Position,
                        _ => return None,
                    };
                    return Some(2);
                } else if horizontal {
                    let draft = &mut state.draft;
                    match state.row {
                        0 => {
                            let old = draft.message_speed;
                            draft.message_speed = step(old, left, 9);
                            return (old != draft.message_speed).then_some(1);
                        }
                        1 => {
                            draft.battle_rank = cycle(usize::from(draft.battle_rank), left, 2) as u8
                        }
                        2 => {
                            draft.window = cycle(usize::from(draft.window), left, 3) as u8;
                            draft.background = 5;
                            draft.colors = self.resources.as_ref().unwrap().data.customize.themes
                                [usize::from(draft.window)]
                            .clone();
                        }
                        3 => draft.background = cycle(usize::from(draft.background), left, 6) as u8,
                        7 => draft.battle_voiceover = left,
                        8 => draft.event_voiceover = left,
                        9 => draft.skit_notifications = left,
                        10 => draft.movie_subtitles = left,
                        11 => draft.battle_auto_zoom = left,
                        12 => draft.rumble = left,
                        _ => return None,
                    }
                    return Some(1);
                }
            }
            Focus::Colors => {
                if input.interact {
                    state.component = usize::from(state.component == 0);
                    return Some(2);
                }
                if vertical {
                    state.component = cycle(state.component, up, 5);
                    return Some(1);
                }
                if input.previous_page || input.next_page || horizontal && state.component == 0 {
                    let previous = left || input.previous_page;
                    state.color_group = cycle(state.color_group, previous, 7);
                    state.color_scroll = if previous { -1 } else { 1 };
                    return Some(1);
                }
                if horizontal {
                    let value =
                        &mut state.draft.colors.group_mut(state.color_group)[state.component - 1];
                    let old = *value;
                    *value = if left {
                        value.div_ceil(8).saturating_sub(1) * 8
                    } else {
                        (u16::from(*value).div_ceil(8) * 8 + 8).min(255) as u8
                    };
                    return (old != *value).then_some(1);
                }
            }
            Focus::Volume => {
                if vertical {
                    state.channel = cycle(state.channel, up, 6);
                    return Some(1);
                }
                if horizontal {
                    if state.channel == 5 {
                        state.draft.stereo = !state.draft.stereo;
                        return Some(1);
                    }
                    let value = state.draft.volumes.channel_mut(state.channel);
                    *value = if left {
                        if *value == 127 {
                            120
                        } else {
                            value.saturating_sub(8)
                        }
                    } else {
                        (*value + 8).min(127)
                    };
                }
            }
            Focus::Controls => {
                if vertical {
                    state.button = cycle(state.button, up, 7);
                    return Some(1);
                }
                if horizontal {
                    let map = &mut state.draft.button_map;
                    let action = step(map[state.button], left, 6);
                    if action != map[state.button] {
                        let other = map.iter().position(|&v| v == action).unwrap();
                        map.swap(state.button, other);
                        return Some(1);
                    }
                }
            }
            Focus::Position => {
                let old = state.draft.screen_position;
                let [x, y] = &mut state.draft.screen_position;
                if input.menu || input.start {
                    *x = 0;
                    *y = 0;
                } else if horizontal {
                    *x = (*x + if left { -1 } else { 1 }).clamp(-12, 32);
                } else if vertical {
                    *y = (*y + if up { -1 } else { 1 }).clamp(-32, 32);
                }
                return (old != state.draft.screen_position).then_some(1);
            }
        }
        None
    }
}
fn cycle(value: usize, previous: bool, count: usize) -> usize {
    (value + if previous { count - 1 } else { 1 }) % count
}
fn step(value: u8, previous: bool, maximum: u8) -> u8 {
    if previous {
        value.saturating_sub(1)
    } else {
        (value + 1).min(maximum)
    }
}

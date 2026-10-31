use super::*;
use resonance_content::menu_data::{
    ANTI_ALIASING_OPTION, AntiAliasing, COMBINED_BATTLES_OPTION, CUSTOMIZE_OPTIONS,
    CustomizeSettings, DISPLAY_MODE_OPTION, DISPLAY_RESOLUTIONS, DisplayMode,
    FLEXIBLE_BATTLE_PARTY_OPTION, LEARN_BOTH_ARTE_PATHS_OPTION, LEARN_ELEMENTAL_ARTES_OPTION,
    RESOLUTION_OPTION,
};

pub const VISIBLE_OPTIONS: usize = 9;
pub const VISIBLE_KEYMAP_ACTIONS: usize = 9;
pub const DISPLAY_CONFIRMATION_SECONDS: u8 = 15;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Options,
    AntiAliasing {
        choice: AntiAliasing,
    },
    Resolution {
        choice: [u32; 2],
    },
    DisplayConfirmation {
        previous_mode: DisplayMode,
        previous_resolution: [u32; 2],
        remaining_secs: u8,
        keep: bool,
        revision: u64,
    },
    Colors,
    Volume,
    Controls,
    ForgetArtes {
        yes: bool,
    },
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Customize {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub draft: CustomizeSettings,
    #[serde(skip)]
    anti_aliasing_choices: Vec<AntiAliasing>,
    #[serde(skip)]
    resolution_choices: Vec<[u32; 2]>,
    #[serde(skip)]
    native_resolution: Option<[u32; 2]>,
    #[serde(skip)]
    original: CustomizeSettings,
    #[serde(skip)]
    display_revision: u64,
    #[serde(skip)]
    pub(super) display_input_blocked: bool,
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
    pub button_first: usize,
    pub capture: Option<super::input_capture::Capture>,
    pub binding_message: Option<String>,
    pub binding_error: Option<String>,
    pub preview_wait: u8,
    pub preview_shown: u8,
    #[serde(skip)]
    pub preview_opacity: u8,
}
impl Customize {
    fn preview_display_change(&mut self, previous: (DisplayMode, [u32; 2])) {
        if previous != (self.draft.display_mode, self.draft.resolution) {
            self.display_revision = self.display_revision.wrapping_add(1);
            self.focus = Focus::DisplayConfirmation {
                previous_mode: previous.0,
                previous_resolution: previous.1,
                remaining_secs: DISPLAY_CONFIRMATION_SECONDS,
                keep: true,
                revision: self.display_revision,
            };
        }
    }

    /// Also called by the host's wall-clock timeout, independently of game ticks.
    pub fn revert_display_change(&mut self) {
        if let Focus::DisplayConfirmation {
            previous_mode,
            previous_resolution,
            ..
        } = self.focus
        {
            self.draft.display_mode = previous_mode;
            self.draft.resolution = previous_resolution;
            self.focus = Focus::Options;
            self.display_input_blocked = true;
        }
    }

    pub fn anti_aliasing_choices(&self) -> &[AntiAliasing] {
        if self.anti_aliasing_choices.is_empty() {
            &AntiAliasing::ALL
        } else {
            &self.anti_aliasing_choices
        }
    }
    pub fn resolution_choices(&self) -> &[[u32; 2]] {
        if self.resolution_choices.is_empty() {
            &DISPLAY_RESOLUTIONS
        } else {
            &self.resolution_choices
        }
    }
    pub fn native_resolution(&self) -> Option<[u32; 2]> {
        self.native_resolution
    }
    /// The host supplies sizes supported by the current monitor and GPU.
    /// Changing monitors updates the choices without changing the player's draft.
    pub fn set_resolution_choices(&mut self, mut choices: Vec<[u32; 2]>, native: Option<[u32; 2]>) {
        choices.sort_unstable();
        choices.dedup();
        self.native_resolution = native.filter(|size| choices.contains(size));
        self.resolution_choices = choices;
        if let Focus::Resolution { choice } = self.focus {
            self.focus = Focus::Resolution {
                choice: self.resolution_choice(choice),
            };
        }
    }
    fn resolution_choice(&self, current: [u32; 2]) -> [u32; 2] {
        let choices = self.resolution_choices();
        choices
            .iter()
            .copied()
            .find(|size| *size >= current)
            .unwrap_or(*choices.last().unwrap())
    }
    pub fn set_anti_aliasing_choices(&mut self, choices: &[AntiAliasing]) {
        self.anti_aliasing_choices = choices.to_vec();
        self.normalize_anti_aliasing();
    }
    fn normalize_anti_aliasing(&mut self) {
        self.draft.anti_aliasing = self
            .draft
            .anti_aliasing
            .supported_or_fallback(self.anti_aliasing_choices());
        self.original.anti_aliasing = self
            .original
            .anti_aliasing
            .supported_or_fallback(self.anti_aliasing_choices());
        if let Focus::AntiAliasing { choice } = self.focus {
            self.focus = Focus::AntiAliasing {
                choice: choice.supported_or_fallback(self.anti_aliasing_choices()),
            };
        }
    }
    pub fn keymap_first(&self) -> usize {
        self.button_first
            .min(self.button)
            .max(self.button.saturating_sub(VISIBLE_KEYMAP_ACTIONS - 1))
    }

    /// Returns true while physical input belongs to the recorder.
    pub fn capture_input(
        &mut self,
        held: &std::collections::BTreeSet<resonance_content::input::PhysicalInput>,
    ) -> bool {
        let Some(capture) = &mut self.capture else {
            return false;
        };
        if let Some(chord) = capture.sample(held) {
            let error = if chord.0.len() != 1 {
                Some("Choose one key, button or stick direction for a standard action.".to_string())
            } else {
                chord.validate_shape().err()
            };
            if let Some(error) = error {
                self.binding_message = Some(error.clone());
                self.binding_error = Some(error);
            } else {
                self.draft.keymap.assign(
                    resonance_content::input::StandardAction::ALL[self.button],
                    chord.0[0].clone(),
                );
                self.binding_message = Some("Binding assigned.".into());
            }
            self.capture = None;
        }
        true
    }

    pub fn forget_confirmation_key(&self) -> Option<&'static str> {
        match (
            self.original.learn_elemental_artes && !self.draft.learn_elemental_artes,
            self.original.learn_both_arte_paths && !self.draft.learn_both_arte_paths,
        ) {
            (true, true) => Some("forget_artes"),
            (true, false) => Some("forget_elemental_artes"),
            (false, true) => Some("forget_arte_paths"),
            (false, false) => None,
        }
    }

    fn cancel_forgetting(&mut self) {
        self.draft.learn_elemental_artes |= self.original.learn_elemental_artes;
        self.draft.learn_both_arte_paths |= self.original.learn_both_arte_paths;
        self.focus = Focus::Options;
    }

    fn opening(mut preferences: CustomizeSettings) -> Self {
        preferences.migrate_button_map();
        Self {
            transition: super::Transition::opening(),
            preview_shown: 1,
            original: preferences.clone(),
            draft: preferences,
            anti_aliasing_choices: AntiAliasing::ALL.to_vec(),
            ..Default::default()
        }
    }

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
    /// Open the shared editor without a field or party checkpoint.
    pub fn title_customize(
        preferences: CustomizeSettings,
        data: menu_data::CustomizeData,
    ) -> anyhow::Result<Self> {
        data.validate()?;
        let mut menu = Self::new(Page::Customize, None, false);
        menu.customize = Customize::opening(preferences);
        menu.direct = true;
        menu.customize_data = Some(data);
        Ok(menu)
    }

    pub fn customize_data(&self) -> anyhow::Result<&menu_data::CustomizeData> {
        self.customize_data
            .as_ref()
            .context("customization page has not been prepared")
    }

    pub fn preferences(&self) -> Option<&CustomizeSettings> {
        if self.page == Page::Customize {
            Some(&self.customize.draft)
        } else {
            self.checkpoint
                .as_ref()
                .map(|c| &c.progress().party.settings.preferences)
        }
    }
    pub(super) fn open_customize(&mut self) {
        self.customize = Customize::opening(self.party().settings.preferences.clone());
        self.entering = Some(Page::Customize);
    }
    pub(super) fn step_customize_preview(&mut self) {
        let Some(data) = &self.customize_data else {
            return;
        };
        let state = &mut self.customize;
        if state.row != 0 {
            return;
        }
        state.preview_opacity = 255 - state.transition.page_fade;
        let speed = state.draft.message_speed;
        if speed == 0 {
            return;
        }
        let count = data.options[0]
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
    pub(super) fn step_customize(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        if self.customize.capture.is_some() {
            return None;
        }
        let [left, right, up, down, page_up, page_down] =
            [Left, Right, Up, Down, PageUp, PageDown].map(|action| input == Some(action));
        let data = match self
            .customize_data
            .as_ref()
            .context("customization page has not been prepared")
        {
            Ok(data) => data,
            Err(error) => {
                self.report_failure("Customization is unavailable", error);
                return Some(4);
            }
        };
        let state = &mut self.customize;
        state.scroll = (state.scroll + state.scroll.signum()) % 5;
        state.color_scroll = (state.color_scroll + state.color_scroll.signum()) % 20;
        if state.transition.animating() || state.scroll != 0 || state.color_scroll != 0 {
            return None;
        }
        if input == Some(Cancel) {
            if matches!(state.focus, Focus::DisplayConfirmation { .. }) {
                state.revert_display_change();
            } else if state.focus == Focus::Options {
                if state.forget_confirmation_key().is_some() {
                    state.focus = Focus::ForgetArtes { yes: false };
                    return Some(2);
                }
                self.close_customize();
            } else {
                if matches!(state.focus, Focus::ForgetArtes { .. }) {
                    state.cancel_forgetting();
                }
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
                    if input == Some(Confirm) {
                        let previous_display = (state.draft.display_mode, state.draft.resolution);
                        state.draft = if state.defaults {
                            data.defaults.clone()
                        } else {
                            state.original.clone()
                        };
                        state.draft.migrate_button_map();
                        state.normalize_anti_aliasing();
                        state.preview_display_change(previous_display);
                        return Some(2);
                    }
                } else if input == Some(Confirm) {
                    state.focus = match state.row {
                        4 => {
                            state.component = 0;
                            Focus::Colors
                        }
                        ANTI_ALIASING_OPTION => Focus::AntiAliasing {
                            choice: state.draft.anti_aliasing,
                        },
                        RESOLUTION_OPTION => {
                            // The current config/CLI size is a choice even without a preset.
                            let current = state.draft.resolution;
                            let mut choices = state.resolution_choices().to_vec();
                            choices.push(current);
                            state.set_resolution_choices(choices, state.native_resolution);
                            Focus::Resolution { choice: current }
                        }
                        5 => Focus::Volume,
                        6 => {
                            state.button = 0;
                            state.button_first = 0;
                            Focus::Controls
                        }
                        _ => return None,
                    };
                    return Some(2);
                } else if horizontal {
                    if state.display_input_blocked && state.row == DISPLAY_MODE_OPTION {
                        return None;
                    }
                    let previous_display = (state.draft.display_mode, state.draft.resolution);
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
                            draft.colors = data.themes[usize::from(draft.window)].clone();
                        }
                        3 => draft.background = cycle(usize::from(draft.background), left, 6) as u8,
                        7 => draft.battle_voiceover = left,
                        8 => draft.event_voiceover = left,
                        9 => draft.skit_notifications = left,
                        10 => draft.movie_subtitles = left,
                        11 => draft.battle_auto_zoom = left,
                        12 => draft.rumble = left,
                        DISPLAY_MODE_OPTION => {
                            let current = DisplayMode::ALL
                                .iter()
                                .position(|m| *m == draft.display_mode)
                                .unwrap();
                            draft.display_mode =
                                DisplayMode::ALL[cycle(current, left, DisplayMode::ALL.len())];
                        }
                        FLEXIBLE_BATTLE_PARTY_OPTION => draft.flexible_battle_party = left,
                        COMBINED_BATTLES_OPTION => draft.combined_battles = left,
                        LEARN_ELEMENTAL_ARTES_OPTION => draft.learn_elemental_artes = left,
                        LEARN_BOTH_ARTE_PATHS_OPTION => draft.learn_both_arte_paths = left,
                        _ => return None,
                    }
                    state.preview_display_change(previous_display);
                    return Some(1);
                }
            }
            Focus::DisplayConfirmation { keep, .. } => {
                if up && !keep || down && keep {
                    if let Focus::DisplayConfirmation { keep, .. } = &mut state.focus {
                        *keep = up;
                    }
                    return Some(1);
                }
                if input == Some(Confirm) {
                    if keep {
                        state.focus = Focus::Options;
                        state.display_input_blocked = true;
                    } else {
                        state.revert_display_change();
                    }
                    return Some(if keep { 2 } else { 3 });
                }
            }
            Focus::Resolution { choice } => {
                if vertical {
                    let choices = state.resolution_choices();
                    let selected = choices
                        .iter()
                        .position(|value| *value == choice)
                        .unwrap_or(0);
                    state.focus = Focus::Resolution {
                        choice: choices[cycle(selected, up, choices.len())],
                    };
                    return Some(1);
                }
                if input == Some(Confirm) {
                    let previous = (state.draft.display_mode, state.draft.resolution);
                    state.draft.resolution = choice;
                    state.focus = Focus::Options;
                    state.preview_display_change(previous);
                    return Some(2);
                }
            }
            Focus::AntiAliasing { choice } => {
                if vertical {
                    let choices = state.anti_aliasing_choices();
                    let selected = choices
                        .iter()
                        .position(|value| *value == choice)
                        .unwrap_or(0);
                    state.focus = Focus::AntiAliasing {
                        choice: choices[cycle(selected, up, choices.len())],
                    };
                    return Some(1);
                }
                if input == Some(Confirm) {
                    state.draft.anti_aliasing = choice;
                    state.focus = Focus::Options;
                    return Some(2);
                }
            }
            Focus::Colors => {
                if input == Some(Confirm) {
                    state.component = usize::from(state.component == 0);
                    return Some(2);
                }
                if vertical {
                    state.component = cycle(state.component, up, 5);
                    return Some(1);
                }
                if input == Some(PreviousTab)
                    || input == Some(NextTab)
                    || horizontal && state.component == 0
                {
                    let previous = left || input == Some(PreviousTab);
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
                use resonance_content::input::StandardAction;
                if vertical {
                    state.button = cycle(state.button, up, StandardAction::ALL.len());
                    state.button_first = state.keymap_first();
                    state.binding_message = None;
                    return Some(1);
                }
                if input == Some(Confirm) {
                    state.capture = Some(super::input_capture::Capture::after_opening(
                        resonance_content::input::Keymap::default()
                            .get(StandardAction::Confirm)
                            .iter()
                            .cloned(),
                    ));
                    state.binding_message = None;
                    return Some(2);
                }
                if input == Some(Alternate) {
                    let result = state.draft.keymap.clear(StandardAction::ALL[state.button]);
                    state.binding_message = result.as_ref().err().cloned();
                    self.notice = result.as_ref().err().cloned();
                    return Some(if result.is_ok() { 1 } else { 4 });
                }
                if input == Some(Menu) {
                    state.draft.keymap = Default::default();
                    state.draft.button_map = [0, 1, 2, 3, 4, 5, 6];
                    state.binding_message = Some("Default keymap restored.".into());
                    return Some(2);
                }
            }
            Focus::ForgetArtes { yes } => {
                if horizontal || vertical {
                    state.focus = Focus::ForgetArtes { yes: !yes };
                    return Some(1);
                }
                if input == Some(Confirm) {
                    if yes {
                        self.close_customize();
                    } else {
                        state.cancel_forgetting();
                    }
                    return Some(2);
                }
            }
        }
        None
    }

    fn close_customize(&mut self) {
        if let Some(checkpoint) = &mut self.checkpoint {
            let party = &mut checkpoint.progress_mut().party;
            if self.customize.original.learn_elemental_artes
                && !self.customize.draft.learn_elemental_artes
            {
                party.forget_elemental_artes();
            }
            if self.customize.original.learn_both_arte_paths
                && !self.customize.draft.learn_both_arte_paths
            {
                party.forget_opposite_path_artes();
            }
            party.settings.preferences = self.customize.draft.clone();
            self.party_changed = true;
        }
        self.customize.focus = Focus::Options;
        self.customize.transition.page_closing = true;
        self.select_main(Page::System);
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

use super::*;
use resonance_content::menu_data::{
    ANTI_ALIASING_OPTION, COMBINED_BATTLES_OPTION, CUSTOMIZE_OPTIONS, DISPLAY_MODE_OPTION,
    FLEXIBLE_BATTLE_PARTY_OPTION, LEARN_BOTH_ARTE_PATHS_OPTION, LEARN_ELEMENTAL_ARTES_OPTION,
    RESOLUTION_OPTION,
};
use resonance_game::menu::customize::{Focus, VISIBLE_OPTIONS};

fn resolution_label(size: [u32; 2], native: Option<[u32; 2]>) -> String {
    let suffix = if native == Some(size) {
        " (Native)"
    } else {
        ""
    };
    format!("{} x {}{suffix}", size[0], size[1])
}

impl Drawing<'_> {
    fn customize_choices(
        &mut self,
        labels: &[String],
        selected: usize,
        applied: Option<usize>,
    ) -> Result<[f32; 2]> {
        let visible = labels.len().min(VISIBLE_OPTIONS);
        let first = selected
            .saturating_sub(visible / 2)
            .min(labels.len() - visible);
        let height = (12. + visible as f32 * 26.).max(238.);
        // An opaque plane above the option text keeps both dropdowns readable,
        // including when the player's menu theme is transparent.
        self.plane = 2;
        let mut background = self.menu_color();
        background[3] = 255;
        self.quad_role(
            DrawRole::Background,
            FONT,
            [320., 60., 600., 60. + height],
            [0.5; 4],
            rgba(background),
        );
        self.plane = 3;
        self.framed([320., 60., 280., height], false)?;
        for (index, label) in labels.iter().enumerate().skip(first).take(visible) {
            let y = 66. + (index - first) as f32 * 26.;
            if index == selected {
                self.highlight([328., y, 264., 24.], 255);
            }
            self.fitted_text(
                label,
                [344., y],
                [20., 24.],
                244.,
                if Some(index) == applied { WHITE } else { GOLD },
            )?;
        }
        if first > 0 {
            self.scroll_arrow(SCROLL_UP, [580., 54.])?;
        }
        if first + visible < labels.len() {
            self.scroll_arrow(SCROLL_DOWN, [580., 60. + height - 4.])?;
        }
        Ok([330., 74. + (selected - first) as f32 * 26.])
    }

    fn keymap(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        use resonance_content::input::{Keymap, StandardAction as A};
        use resonance_game::menu::customize::VISIBLE_KEYMAP_ACTIONS;
        let state = &menu.customize;
        self.opacity = 255;
        self.offset = [0.; 2];
        self.heading("Keymap")?;
        self.framed([16., 60., 604., 278.], true)?;
        let first = state.keymap_first();
        for (offset, action) in A::ALL
            .into_iter()
            .skip(first)
            .take(VISIBLE_KEYMAP_ACTIONS)
            .enumerate()
        {
            let y = 68. + offset as f32 * 28.;
            if action as usize == state.button {
                self.highlight([24., y, 588., 26.], 255);
            }
            self.fitted_text(action.label(), [40., y + 3.], [14., 20.], 280., GOLD)?;
            self.binding_icons(
                state.draft.keymap.get(action).iter(),
                "/",
                [336., y],
                28.,
                600.,
            )?;
        }
        if first > 0 {
            self.scroll_arrow(SCROLL_UP, [300., 54.])?;
        }
        if first + VISIBLE_KEYMAP_ACTIONS < A::ALL.len() {
            self.scroll_arrow(SCROLL_DOWN, [300., 326.])?;
        }
        self.framed([16., 344., 604., 84.], true)?;
        if let Some(capture) = &state.capture {
            self.fitted_text(
                "Release the opener. Hold one input, then release.",
                [28., 352.],
                [16., 20.],
                580.,
                GOLD,
            )?;
            self.binding_icons(capture.inputs(), "+", [28., 386.], 28., 600.)?;
        } else {
            let navigation = Keymap::default();
            for (action, label, x, right) in [
                (A::Confirm, "Record", 28., 160.),
                (A::Alternate, "Clear", 176., 302.),
                (A::Menu, "Defaults", 320., 464.),
                (A::Cancel, "Back", 484., 608.),
            ] {
                self.input_hint(&navigation, action, label, [x, 352.], right)?;
            }
            let message = state
                .binding_message
                .as_deref()
                .unwrap_or("Select an action to change its binding.");
            for (line, text) in self
                .wrap_notice(message, 14., 576.)?
                .iter()
                .take(2)
                .enumerate()
            {
                self.text_size(text, [28., 384. + line as f32 * 20.], [14., 20.], WHITE)?;
            }
        }
        Ok([32., 76. + (state.button - first) as f32 * 28.])
    }
    pub(super) fn customize(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let state = &menu.customize;
        if state.focus == Focus::Controls {
            return self.keymap(menu);
        }
        let settings = &state.draft;
        let data = menu.customize_data()?;
        let fade = u32::from(state.transition.page_fade);
        let opacity = 255 - state.transition.page_fade;
        let left = -((fade * 612 / 256) as f32);
        let right = (fade * 236 / 256) as f32;
        let bottom = (fade * 140 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(menu_label(&self.spec.labels, "customize")?)?;
        self.offset = [right, 0.];
        self.frame([396., 16., 224., 32.])?;
        for (index, key) in ["cancel", "default"].into_iter().enumerate() {
            let x = 408. + index as f32 * 100.;
            if state.row == CUSTOMIZE_OPTIONS && state.defaults == (index == 1) {
                self.highlight([x, 20., if index == 0 { 80. } else { 100. }, 24.], 255);
            }
            self.text(menu_label(&data.labels, key)?, [x, 20.], 20., WHITE)?;
        }
        self.offset = [left, 0.];
        self.framed([16., 60., 604., 238.], true)?;
        if state.row != CUSTOMIZE_OPTIONS {
            let y = 62. + (state.row - state.first) as f32 * 26.;
            self.highlight(
                [
                    32.,
                    y,
                    self.text_width(data.option_name(state.row), 20.)?,
                    24.,
                ],
                255,
            );
        }
        let scroll = scroll_offset(state.scroll, 26);
        let first = state.first - usize::from(state.scroll > 0);
        let starts = self.vertex_counts();
        for (index, row) in (first..CUSTOMIZE_OPTIONS)
            .take(VISIBLE_OPTIONS + usize::from(state.scroll != 0))
            .enumerate()
        {
            let y = 62. + index as f32 * 26. - scroll as f32;
            self.text(data.option_name(row), [32., y], 20., GOLD)?;
            let mut x = 320.;
            match row {
                0..=3 => {
                    let (count, selected) = match row {
                        0 => (10, settings.message_speed),
                        1 => (2, settings.battle_rank),
                        2 => (3, settings.window),
                        _ => (6, settings.background),
                    };
                    for value in 0..count {
                        let text = match row {
                            0 => value.to_string(),
                            1 => data.difficulties[usize::from(value)].clone(),
                            _ => char::from(b'A' + value).to_string(),
                        };
                        self.text(
                            &text,
                            [x, y],
                            20.,
                            if value == selected { WHITE } else { DISABLED },
                        )?;
                        x += if row == 1 {
                            self.text_width(&text, 20.)? + 20.
                        } else {
                            30.
                        };
                    }
                }
                4 | 5 => {
                    let phase = self.tick % 60;
                    let fade = if phase < 20 { phase * 512 / 20 } else { 0 };
                    let pulse = 255 - if fade >= 256 { 511 - fade } else { fade };
                    self.opacity = (u32::from(opacity) * pulse / 255) as u8;
                    self.text(
                        menu_label(
                            &data.labels,
                            match row {
                                4 => "color",
                                5 => "volume",
                                _ => unreachable!(),
                            },
                        )?,
                        [x, y],
                        24.,
                        6,
                    )?;
                    self.opacity = opacity;
                }
                ANTI_ALIASING_OPTION => {
                    self.text(settings.anti_aliasing.label(), [x, y], 20., WHITE)?;
                }
                DISPLAY_MODE_OPTION => {
                    self.text(
                        &format!("< {} >", settings.display_mode.label()),
                        [x, y],
                        20.,
                        WHITE,
                    )?;
                }
                RESOLUTION_OPTION => {
                    self.fitted_text(
                        &resolution_label(settings.resolution, state.native_resolution()),
                        [x, y],
                        [20., 24.],
                        292.,
                        WHITE,
                    )?;
                }
                6 => self.button(6, [x, y])?,
                7..=12
                | FLEXIBLE_BATTLE_PARTY_OPTION
                | COMBINED_BATTLES_OPTION
                | LEARN_ELEMENTAL_ARTES_OPTION
                | LEARN_BOTH_ARTE_PATHS_OPTION => {
                    let on = if row == FLEXIBLE_BATTLE_PARTY_OPTION {
                        settings.flexible_battle_party
                    } else if row == COMBINED_BATTLES_OPTION {
                        settings.combined_battles
                    } else if row == LEARN_ELEMENTAL_ARTES_OPTION {
                        settings.learn_elemental_artes
                    } else if row == LEARN_BOTH_ARTE_PATHS_OPTION {
                        settings.learn_both_arte_paths
                    } else {
                        [
                            settings.battle_voiceover,
                            settings.event_voiceover,
                            settings.skit_notifications,
                            settings.movie_subtitles,
                            settings.battle_auto_zoom,
                            settings.rumble,
                        ][row - 7]
                    };
                    for (index, key) in ["on", "off"].into_iter().enumerate() {
                        self.text(
                            menu_label(&data.labels, key)?,
                            [x + index as f32 * 60., y],
                            20.,
                            if on == (index == 0) { WHITE } else { DISABLED },
                        )?;
                    }
                }
                _ => unreachable!(),
            }
        }
        self.clip_rows(starts, [60., 294.]);
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [296., 52.])?;
        }
        if state.first + VISIBLE_OPTIONS < CUSTOMIZE_OPTIONS {
            self.scroll_arrow(SCROLL_DOWN, [296., 282.])?;
        }

        self.offset = [0., bottom];
        let mut cursor = state.options_cursor();
        if matches!(state.row, 2..=4) {
            for (index, color) in settings.colors.groups().into_iter().enumerate() {
                let movement = i32::from(state.color_scroll.signum()) * 320
                    - i32::from(state.color_scroll) * 16;
                let mut x = 160 + (index as i32 - state.color_group as i32) * 320 + movement;
                if x <= -1120 {
                    x += 2240;
                } else if x >= 1120 {
                    x -= 2240;
                }
                let x = x as f32;
                if x + 312. < self.screen[0] || x > self.screen[2] {
                    continue;
                }
                if matches!(index, 4 | 5) {
                    self.shade([x + 4., 308., x + 312., 432.]);
                }
                self.colored_frame(
                    [x + 8., 308., 304., 124.],
                    false,
                    if index < 4 {
                        color
                    } else {
                        settings.colors.menu
                    },
                )?;
                let title = &data.color_groups[index];
                let width = self.text_width(title, 16.)?;
                let title_x = x + 8. + ((304. - width) / 2.).trunc();
                let active = state.focus == Focus::Colors && index == state.color_group;
                if active && state.component == 0 {
                    self.highlight([title_x, 309., width, 20.], 255);
                    cursor = [title_x, 316.];
                }
                self.text_size(title, [title_x, 308.], [16., 20.], GOLD)?;
                for (component, label) in ["R", "G", "B", "A"].into_iter().enumerate() {
                    let y = 332. + component as f32 * 24.;
                    if active && state.component == component + 1 {
                        self.highlight([x + 26., y, 278., 24.], 255);
                        cursor = [x + 26., y + 8.];
                    }
                    let mut tint = [0., 0., 0., 1.];
                    if component < 3 {
                        tint[component] = 1.;
                    } else {
                        tint = [0.5, 0.5, 0.5, 1.];
                    }
                    self.quad(
                        FONT,
                        [
                            x + 48.,
                            y + 8.5,
                            x + 48. + f32::from(color[component]),
                            y + 15.5,
                        ],
                        [0.5; 4],
                        tint,
                    );
                    self.text(label, [x + 26., y], 16., WHITE)?;
                    self.text(
                        &format!("{:3}", color[component]),
                        [x + 152., y],
                        16.,
                        WHITE,
                    )?;
                }
            }
        } else {
            self.colored_frame(
                [16., 308., 604., 124.],
                false,
                if state.row == 0 {
                    settings.colors.dialogue
                } else {
                    settings.colors.menu
                },
            )?;
            match state.row {
                0 => {
                    let text = &data.options[0].description;
                    let speed = i32::from(settings.message_speed);
                    let shown = if speed == 0 {
                        255
                    } else {
                        usize::from(state.preview_shown)
                    };
                    let [mut x, mut y] = [32., 316.];
                    let mut index = 0;
                    for ch in text.chars() {
                        match ch {
                            '\n' => {
                                x = 32.;
                                y += 28.;
                            }
                            ' ' => x += 12.,
                            _ => {
                                if index >= shown + 4 {
                                    break;
                                }
                                self.opacity = if speed == 0 || index < shown {
                                    state.preview_opacity
                                } else {
                                    ((speed * 4
                                        - (speed - i32::from(state.preview_wait)
                                            + speed * (index - shown) as i32))
                                        * i32::from(state.preview_opacity)
                                        / (speed * 4)) as u8
                                };
                                self.text(&ch.to_string(), [x, y], 24., WHITE)?;
                                x += self.text_width(&ch.to_string(), 24.)?;
                                index += 1;
                            }
                        }
                    }
                    self.opacity = opacity;
                }
                5 => {
                    for (channel, label) in data.volume_channels.iter().enumerate() {
                        let y = 310. + channel as f32 * 20.;
                        if state.focus == Focus::Volume && state.channel == channel {
                            let (x, width) = if channel < 5 {
                                (48., self.text_width(label, 16.)?)
                            } else {
                                (
                                    if settings.stereo { 368. } else { 480. },
                                    self.text_width(
                                        &data.labels
                                            [if settings.stereo { "stereo" } else { "mono" }],
                                        16.,
                                    )?,
                                )
                            };
                            self.highlight(
                                [x, y + if channel < 5 { 2. } else { 0. }, width, 18.],
                                255,
                            );
                            cursor = [
                                if channel < 5 {
                                    48.
                                } else if settings.stereo {
                                    320.
                                } else {
                                    432.
                                },
                                y + 2.,
                            ];
                        }
                        self.text_size(label, [48., y], [16., 18.], GOLD)?;
                        if channel < 5 {
                            let value = settings.volumes.channels()[channel];
                            self.quad(
                                FONT,
                                [367., y + 4.5, 496., y + 13.5],
                                [0.5; 4],
                                [0., 0., 0., 1.],
                            );
                            self.quad(
                                FONT,
                                [368., y + 5.5, 368. + f32::from(value), y + 12.5],
                                [0.5; 4],
                                [0.5, 0.5, 0.5, 1.],
                            );
                            self.text_size(&format!("{value:3}"), [500., y], [16., 18.], WHITE)?;
                        } else {
                            for (key, x, active) in [
                                ("stereo", 368., settings.stereo),
                                ("mono", 480., !settings.stereo),
                            ] {
                                self.text_size(
                                    menu_label(&data.labels, key)?,
                                    [x, y],
                                    [16., 18.],
                                    if active { WHITE } else { DISABLED },
                                )?;
                            }
                        }
                    }
                }
                6 => {
                    self.text("Keymap", [32., 316.], 24., GOLD)?;
                    self.text(
                        "Assign keyboard keys, buttons, D-pad and stick directions.",
                        [32., 350.],
                        18.,
                        WHITE,
                    )?;
                    self.text(
                        "Arte combos are assigned separately in the Artes list.",
                        [32., 384.],
                        18.,
                        WHITE,
                    )?;
                }
                CUSTOMIZE_OPTIONS => self.text(
                    menu_label(
                        &data.labels,
                        if state.defaults {
                            "default_help"
                        } else {
                            "cancel_help"
                        },
                    )?,
                    [32., 316.],
                    20.,
                    WHITE,
                )?,
                _ => self.text(data.option_description(state.row), [32., 316.], 20., WHITE)?,
            }
        }
        if matches!(state.focus, Focus::Colors | Focus::Volume | Focus::Controls) {
            cursor[1] += bottom;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        if let Focus::AntiAliasing { choice } = state.focus {
            let choices = state.anti_aliasing_choices();
            let selected = choices
                .iter()
                .position(|value| *value == choice)
                .unwrap_or(0);
            let applied = choices
                .iter()
                .position(|value| *value == settings.anti_aliasing);
            let labels: Vec<_> = choices.iter().map(|mode| mode.label().to_owned()).collect();
            cursor = self.customize_choices(&labels, selected, applied)?;
        }
        if let Focus::Resolution { choice } = state.focus {
            let choices = state.resolution_choices();
            let selected = choices
                .iter()
                .position(|value| *value == choice)
                .unwrap_or(0);
            let applied = choices
                .iter()
                .position(|value| *value == settings.resolution);
            let labels: Vec<_> = choices
                .iter()
                .map(|size| resolution_label(*size, state.native_resolution()))
                .collect();
            cursor = self.customize_choices(&labels, selected, applied)?;
        }
        if let Focus::DisplayConfirmation {
            keep,
            remaining_secs,
            ..
        } = state.focus
        {
            cursor = self.popup_layout(
                &format!("Keep these display settings?\n{}: {} x {}\nReverting in {remaining_secs} seconds.",
                    settings.display_mode.label(), settings.resolution[0], settings.resolution[1]),
                Some(keep), 255, None, Some(["Keep", "Revert"]),
            )?.context("display confirmation has no choices")?;
        }
        if let Focus::ForgetArtes { yes } = state.focus {
            cursor = self
                .popup(
                    data.confirmation(
                        state
                            .forget_confirmation_key()
                            .context("missing arte confirmation")?,
                    )?,
                    Some(yes),
                    255,
                )?
                .context("arte confirmation has no choices")?;
        }
        Ok(cursor)
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires current menu artwork/options; CPU drawing only"]
fn named_options_render_every_editor_and_modded_confirmation() -> Result<()> {
    use super::test_support::Fixture;
    use resonance_content::options::{self, Document};
    let fixture = Fixture::load()?;
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let mut doc: Document = serde_json::from_slice(&std::fs::read(
        std::path::Path::new(&root).join(options::PATH),
    )?)?;
    doc.options.get_mut("combined_battles").unwrap().name = "Nearby Battles".into();
    doc.confirmations
        .insert("forget_artes".into(), "Forget both kinds?".into());
    let data = doc.prepare()?;
    let mut menu = Menu::title_customize(
        resonance_content::menu_data::CustomizeSettings {
            learn_elemental_artes: true,
            learn_both_arte_paths: true,
            ..data.defaults.clone()
        },
        data,
    )?;
    menu.customize.transition = Default::default();
    for row in 0..CUSTOMIZE_OPTIONS {
        menu.customize.row = row;
        menu.customize.first = row.saturating_sub(8);
        fixture.drawing(0).customize(&menu)?;
    }
    for display_mode in resonance_content::menu_data::DisplayMode::ALL {
        menu.customize.row = DISPLAY_MODE_OPTION;
        menu.customize.first = DISPLAY_MODE_OPTION - 8;
        menu.customize.draft.display_mode = display_mode;
        fixture.drawing(0).customize(&menu)?;
    }
    for resolution in resonance_content::menu_data::DISPLAY_RESOLUTIONS {
        menu.customize.row = RESOLUTION_OPTION;
        menu.customize.first = RESOLUTION_OPTION - 8;
        menu.customize.draft.resolution = resolution;
        fixture.drawing(0).customize(&menu)?;
    }
    for anti_aliasing in resonance_content::menu_data::AntiAliasing::ALL {
        menu.customize.row = ANTI_ALIASING_OPTION;
        menu.customize.first = ANTI_ALIASING_OPTION - 8;
        menu.customize.draft.anti_aliasing = anti_aliasing;
        menu.customize.focus = Focus::AntiAliasing {
            choice: anti_aliasing,
        };
        let mut transparent = menu.customize.draft.clone();
        transparent.colors.menu[3] = 0;
        let mut drawing = fixture.drawing(0);
        drawing.preferences = Some(&transparent);
        drawing.customize(&menu)?;
        let backing = drawing
            .batches
            .get(&(2, DrawRole::Background, FONT))
            .context("AA dropdown needs a backing above the underlying options")?;
        assert!(backing.colors.iter().all(|color| color[3] == 1.));
        assert!(backing.uv.iter().all(|uv| *uv == [0.5; 2]));
        assert_eq!(backing.positions[0][0] + 320., 320.);
        assert_eq!(backing.positions[2][0] + 320., 600.);
        let top = (240. - backing.positions[0][1]) * 448. / 480.;
        let bottom = (240. - backing.positions[2][1]) * 448. / 480.;
        let height = (12.
            + menu
                .customize
                .anti_aliasing_choices()
                .len()
                .min(VISIBLE_OPTIONS) as f32
                * 26.)
            .max(238.);
        assert!((top - 60.).abs() < 0.001);
        assert!((bottom - (60. + height)).abs() < 0.001);
        assert!(
            DepthBand::Field.at(2, DrawRole::Background.depth())
                > DepthBand::Field.at(1, DrawRole::Text.depth())
        );
        assert!(drawing.batches.contains_key(&(3, DrawRole::Text, FONT)));
    }
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    let native = [3024, 1964];
    let mut resolutions = resonance_content::menu_data::DISPLAY_RESOLUTIONS.to_vec();
    resolutions.push(native);
    menu.customize
        .set_resolution_choices(resolutions, Some(native));
    for choice in [[640, 480], native, [5120, 1440]] {
        menu.customize.focus = Focus::Resolution { choice };
        let mut drawing = fixture.drawing(0);
        let cursor = drawing.customize(&menu)?;
        assert!(
            (74. ..=282.).contains(&cursor[1]),
            "scroll the selected resolution into view"
        );
        let backing = &drawing.batches[&(2, DrawRole::Background, FONT)];
        assert!(backing.colors.iter().all(|color| color[3] == 1.));
        let top = (240. - backing.positions[0][1]) * 448. / 480.;
        let bottom = (240. - backing.positions[2][1]) * 448. / 480.;
        assert!((top - 60.).abs() < 0.001);
        assert!(
            (bottom - 306.).abs() < 0.001,
            "only nine choices fit in the dropdown"
        );
    }
    menu.customize.focus = Focus::Options;
    for window in 0..3 {
        for background in 0..6 {
            menu.customize.row = 3;
            menu.customize.first = 0;
            menu.customize.draft.window = window;
            menu.customize.draft.background = background;
            fixture.drawing(0).customize(&menu)?;
        }
    }
    menu.customize.row = 4;
    for group in 0..7 {
        for component in 0..5 {
            menu.customize.focus = Focus::Colors;
            menu.customize.color_group = group;
            menu.customize.component = component;
            fixture.drawing(0).customize(&menu)?;
        }
    }
    menu.customize.row = 5;
    for channel in 0..6 {
        menu.customize.focus = Focus::Volume;
        menu.customize.channel = channel;
        fixture.drawing(0).customize(&menu)?;
    }
    menu.customize.row = 6;
    for button in 0..7 {
        menu.customize.focus = Focus::Controls;
        menu.customize.button = button;
        fixture.drawing(0).customize(&menu)?;
    }
    menu.customize.row = 13;
    menu.customize.first = 5;
    menu.customize.focus = Focus::Options;
    fixture.drawing(0).customize(&menu)?;
    menu.customize.draft.learn_elemental_artes = false;
    menu.customize.draft.learn_both_arte_paths = false;
    menu.customize.focus = Focus::ForgetArtes { yes: false };
    assert_eq!(
        menu.customize_data()?
            .confirmation(menu.customize.forget_confirmation_key().unwrap())?,
        "Forget both kinds?"
    );
    fixture.drawing(0).customize(&menu)?;
    Ok(())
}

#[cfg(test)]
#[test]
#[ignore = "requires locally cooked menus; keymap navigation, capture and CPU drawing"]
fn keymap_scrolls_all_actions_and_records_physical_controller_axes() -> Result<()> {
    use super::test_support::Fixture;
    use resonance_content::input::{AxisDirection, PhysicalInput, StandardAction};
    let fixture = Fixture::load()?;
    let document: resonance_content::options::Document =
        fixture.files.json(resonance_content::options::PATH)?;
    let mut menu = Menu::title_customize(Default::default(), document.prepare()?)?;
    menu.customize.transition = Default::default();
    menu.customize.focus = Focus::Controls;
    menu.customize.row = 6;
    for (row, _) in StandardAction::ALL.into_iter().enumerate() {
        menu.customize.button = row;
        let mut drawing = fixture.drawing(0);
        let cursor = drawing.customize(&menu)?;
        assert!((68. ..=308.).contains(&cursor[1]));
        assert!(
            drawing
                .batches
                .keys()
                .any(|key| key.2 == MaterialKey::InputIcons)
        );
    }
    menu.customize.button = StandardAction::Skit as usize;
    menu.customize.capture = Some(Default::default());
    menu.capture_physical(&Default::default());
    let axis = PhysicalInput::Axis(AxisDirection::RightUp);
    menu.capture_physical(&[axis.clone()].into());
    menu.capture_physical(&Default::default());
    assert!(
        menu.customize
            .draft
            .keymap
            .get(StandardAction::Skit)
            .contains(&axis)
    );
    assert!(
        menu.customize
            .draft
            .keymap
            .get(StandardAction::Skit)
            .contains(&PhysicalInput::Key("KeyZ".into()))
    );
    let before = menu.customize.draft.keymap.clone();
    menu.customize.capture = Some(Default::default());
    menu.capture_physical(&Default::default());
    menu.capture_physical(
        &[
            PhysicalInput::Key("KeyA".into()),
            PhysicalInput::Key("KeyB".into()),
        ]
        .into(),
    );
    menu.capture_physical(&Default::default());
    assert!(menu.notice.as_ref().unwrap().contains("Choose one"));
    assert!(menu.popup.is_some());
    assert_eq!(menu.customize.draft.keymap, before);
    fixture.drawing(0).customize(&menu)?;
    Ok(())
}

#[cfg(test)]
#[test]
#[ignore = "requires locally cooked menus; checks keymap geometry, navigation and published cursors"]
fn keymap_is_icon_only_bounded_and_keeps_its_window_and_single_cursor() -> Result<()> {
    use super::test_support::Fixture;
    use resonance_game::field::FieldInput;
    let fixture = Fixture::load()?;
    let document: resonance_content::options::Document =
        fixture.files.json(resonance_content::options::PATH)?;
    let mut menu = Menu::title_customize(Default::default(), document.prepare()?)?;
    menu.customize.transition = Default::default();
    menu.customize.focus = Focus::Controls;
    menu.customize.row = 6;
    let move_row = |menu: &mut Menu, y| {
        menu.step(FieldInput::default());
        menu.step(FieldInput {
            direction: [0., y],
            ..Default::default()
        });
    };
    for _ in 0..14 {
        move_row(&mut menu, -1.);
    }
    assert_eq!(menu.customize.button, 14);
    assert_eq!(menu.customize.button_first, 6);
    move_row(&mut menu, 1.);
    assert_eq!(menu.customize.button, 13);
    assert_eq!(
        menu.customize.button_first, 6,
        "moving within the window must not scroll it"
    );
    for _ in 0..7 {
        move_row(&mut menu, 1.);
    }
    assert_eq!(menu.customize.button, 6);
    assert_eq!(menu.customize.button_first, 6);
    move_row(&mut menu, 1.);
    assert_eq!(menu.customize.button_first, 5);

    for row in 0..resonance_content::input::StandardAction::ALL.len() {
        menu.customize.button = row;
        menu.customize.button_first = menu.customize.keymap_first();
        let mut draw = fixture.drawing(0);
        draw.customize(&menu)?;
        let slash = glyph_uv(fixture.font.glyphs[&'/'].rect);
        let dash = glyph_uv(fixture.font.glyphs[&'-'].rect);
        let z = glyph_uv(fixture.font.glyphs[&'Z'].rect);
        for ((_, role, material), batch) in &draw.batches {
            if !matches!(material, MaterialKey::Font | MaterialKey::InputIcons) {
                continue;
            }
            for (quad, uv) in batch
                .positions
                .chunks_exact(4)
                .zip(batch.uv.chunks_exact(4))
            {
                let x = quad[0][0] + 320.;
                let y = (240. - quad[0][1]) * 448. / 480.;
                if y >= 60.
                    && (*material == MaterialKey::InputIcons
                        || *role == DrawRole::Text && uv[0] != [0.5, 0.5])
                {
                    assert!(
                        quad.iter()
                            .all(|p| p[0] + 320. >= 20. && p[0] + 320. <= 612.),
                        "horizontal overflow at row {row}"
                    );
                    assert!(
                        quad.iter().all(|p| (240. - p[1]) * 448. / 480. <= 428.01),
                        "vertical overflow at row {row}"
                    );
                }
                if *material == MaterialKey::Font
                    && *role == DrawRole::Text
                    && x >= 330.
                    && (66. ..=320.).contains(&y)
                {
                    assert!(
                        [slash, dash, z].iter().any(|g| uv[0] == [g[0], g[1]]),
                        "binding rendered as prose at row {row}"
                    );
                }
            }
        }
    }
    menu.customize.button = 19;
    menu.customize.button_first = 11;
    let mut world = World::new();
    let mut art = fixture.artwork(&mut world)?;
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut meshes = Assets::<Mesh>::default();
    art.render(
        Source::Title(Some(&menu)),
        &fixture.font,
        &fixture.dialogue,
        0,
        crate::Resolution::default(),
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    queue.apply(&mut world);
    let cursor_vertices: usize = art
        .layers
        .iter()
        .filter(|((_, role, _), layer)| *role == DrawRole::Cursor && layer.visible)
        .map(|(_, layer)| meshes.get(&layer.mesh).unwrap().count_vertices())
        .sum();
    assert_eq!(
        cursor_vertices, 8,
        "keymap must publish only one cursor and its shadow"
    );
    if let Ok(output) = std::env::var("RESONANCE_KEYMAP_PREVIEW") {
        let batches:Vec<_> = art.layers.iter().filter(|(_,l)| l.visible).map(|((plane,role,key),layer)| {
            let (batch,_) = layer.uploaded.as_ref().unwrap();
            let texture = match key {
                MaterialKey::Font=>fixture.font.texture.clone(),
                MaterialKey::Cursor=>fixture.dialogue.cursor.path.clone(),
                MaterialKey::InputIcons=>resonance_content::input_prompts::IMAGE_PATH.into(),
                MaterialKey::Texture(i)=>fixture.art.textures[i].path.clone(),
            };
            serde_json::json!({"plane":plane,"role":format!("{role:?}"),"texture":texture,"positions":batch.positions,"uv":batch.uv,"colors":batch.colors})
        }).collect();
        std::fs::write(output, serde_json::to_vec(&batches)?)?;
    }
    Ok(())
}

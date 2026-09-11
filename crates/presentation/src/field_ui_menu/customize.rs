use super::*;
use resonance_content::menu_data::CUSTOMIZE_OPTIONS;
use resonance_game::menu::customize::{Focus, VISIBLE_OPTIONS};

impl Drawing<'_> {
    pub(super) fn customize(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let state = &menu.customize;
        let settings = &state.draft;
        let data = &menu.resources.as_ref().unwrap().data.customize;
        let fade = u32::from(state.transition.page_fade);
        let opacity = 255 - state.transition.page_fade;
        let left = -((fade * 612 / 256) as f32);
        let right = (fade * 236 / 256) as f32;
        let bottom = (fade * 140 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&self.spec.labels["customize"])?;
        self.offset = [right, 0.];
        self.frame([396., 16., 224., 32.]);
        for (index, key) in ["cancel", "default"].into_iter().enumerate() {
            let x = 408. + index as f32 * 100.;
            if state.row == CUSTOMIZE_OPTIONS && state.defaults == (index == 1) {
                self.highlight([x, 20., if index == 0 { 80. } else { 100. }, 24.], 255);
            }
            self.text(&data.labels[key], [x, 20.], 20., WHITE)?;
        }
        self.offset = [left, 0.];
        self.framed([16., 60., 604., 238.], true);
        if state.row != CUSTOMIZE_OPTIONS {
            let y = 62. + (state.row - state.first) as f32 * 26.;
            self.highlight(
                [
                    32.,
                    y,
                    self.text_width(&data.options[state.row].name, 20.)?,
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
            self.text(&data.options[row].name, [32., y], 20., GOLD)?;
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
                4 | 5 | 13 => {
                    let phase = self.tick % 60;
                    let fade = if phase < 20 { phase * 512 / 20 } else { 0 };
                    let pulse = 255 - if fade >= 256 { 511 - fade } else { fade };
                    self.opacity = (u32::from(opacity) * pulse / 255) as u8;
                    self.text(
                        &data.labels[match row {
                            4 => "color",
                            5 => "volume",
                            _ => "position",
                        }],
                        [x, y],
                        24.,
                        6,
                    )?;
                    self.opacity = opacity;
                    if row == 13 {
                        let [sx, sy] = settings.screen_position;
                        self.text(&format!("X:{sx:3} Y:{sy:3}"), [x + 20., y], 20., WHITE)?;
                    }
                }
                6 => self.sprite(self.spec.sprites.buttons[6], [x, y]),
                7..=12 => {
                    let on = [
                        settings.battle_voiceover,
                        settings.event_voiceover,
                        settings.skit_notifications,
                        settings.movie_subtitles,
                        settings.battle_auto_zoom,
                        settings.rumble,
                    ][row - 7];
                    for (index, key) in ["on", "off"].into_iter().enumerate() {
                        self.text(
                            &data.labels[key],
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
            self.scroll_arrow(SCROLL_UP, [296., 52.]);
        }
        if state.first + VISIBLE_OPTIONS < CUSTOMIZE_OPTIONS {
            self.scroll_arrow(SCROLL_DOWN, [296., 282.]);
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
                if x + 312. < 0. || x > 640. {
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
                );
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
            );
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
                                    &data.labels[key],
                                    [x, y],
                                    [16., 18.],
                                    if active { WHITE } else { DISABLED },
                                )?;
                            }
                        }
                    }
                }
                6 => {
                    for button in 0..7 {
                        let [x, y] = [
                            24. + (button / 4) as f32 * 304.,
                            316. + (button % 4) as f32 * 28.,
                        ];
                        let action = settings.button_map[button];
                        if state.focus == Focus::Controls && state.button == button {
                            self.highlight([x, y, 280., 24.], 255);
                            cursor = [x, y + 8.];
                            if action > 0 {
                                self.text("<", [x + 32., y], 16., WHITE)?;
                            }
                            if action < 6 {
                                self.text(">", [x + 264., y], 16., WHITE)?;
                            }
                        }
                        self.sprite(
                            self.spec.sprites.buttons[usize::from(data.control_buttons[button])],
                            [x, y],
                        );
                        self.text(&data.actions[usize::from(action)], [x + 48., y], 20., WHITE)?;
                    }
                }
                13 if state.focus == Focus::Position => {
                    self.text(&data.labels["position_help"], [24., 316.], 20., WHITE)?;
                    self.text_size("^", [310., 356.], [16.; 2], WHITE)?;
                    self.text_size("v", [310., 372.], [16.; 2], WHITE)?;
                    self.text_size("<", [302., 364.], [16.; 2], WHITE)?;
                    self.text_size(">", [318., 364.], [16.; 2], WHITE)?;
                }
                CUSTOMIZE_OPTIONS => self.text(
                    &data.labels[if state.defaults {
                        "default_help"
                    } else {
                        "cancel_help"
                    }],
                    [32., 316.],
                    20.,
                    WHITE,
                )?,
                _ => self.text(
                    &data.options[state.row].description,
                    [32., 316.],
                    20.,
                    WHITE,
                )?,
            }
        }
        if matches!(state.focus, Focus::Colors | Focus::Volume | Focus::Controls) {
            cursor[1] += bottom;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(cursor)
    }
}

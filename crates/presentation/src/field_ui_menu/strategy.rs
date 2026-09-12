use super::*;
use resonance_game::menu::strategy::Focus;

impl Drawing<'_> {
    pub(super) fn strategy(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu
            .resources
            .as_ref()
            .context("strategy data was not prepared")?
            .data;
        let party = menu.party();
        let state = &menu.strategy;
        let focus = state.focus;
        let preset = focus.preset();
        let row_x = if preset { 264. } else { 240. };
        let slide =
            |distance| f32::from((u32::from(state.transition.page_fade) * distance / 256) as u16);
        let party_x = -slide(436);
        self.offset = [0., -slide(76)];
        self.heading(&data.labels["strategy_title"])?;
        self.offset = [slide(208), 0.];
        self.frame([440., 60., 180., 266.]);
        self.offset = [0., slide(312)];
        self.framed([16., 336., 604., 88.], true);
        self.offset = [party_x, 0.];
        let mut anchor = [
            32. + party_x,
            100. + (menu.strategy.character - state.first) as f32 * 69.,
        ];
        let starts = self.vertex_counts();
        let first = state.first - usize::from(state.scroll > 0);
        let offset = i32::from(state.scroll) * 69 / 10 + if state.scroll < 0 { 69 } else { 0 };
        for (slot, &id) in party
            .formation
            .iter()
            .enumerate()
            .skip(first)
            .take(4 + usize::from(state.scroll != 0))
        {
            let member = usize::from(id - 1);
            let y = 60. + (slot - first) as f32 * 69. - offset as f32;
            self.colored_frame([16., y, 414., 59.], false, self.party_color(slot));
            if slot == menu.strategy.character
                && (focus.character() || focus.setting() || focus.options())
            {
                self.highlight(
                    [32., y - 4., 64., 64.],
                    if focus.character() { 255 } else { 127 },
                );
            }
            self.text(
                &(slot + 1).to_string(),
                [16., y + 18.],
                16.,
                if slot < resonance_game::menu::VISIBLE_PARTY {
                    WHITE
                } else {
                    DISABLED
                },
            )?;
            self.portrait(member, party.members[member].conditions, [32., y - 4.]);
            self.text(menu.character_name(member), [96., y + 4.], 24., WHITE)?;
            for (group, option) in menu.strategy_choices(member).into_iter().enumerate() {
                let option = &data.strategy.groups[group][usize::from(option)];
                let y = y + group as f32 * 19.;
                if slot == state.character
                    && group == state.group
                    && (focus.setting() || focus.options())
                {
                    self.highlight(
                        [
                            row_x,
                            y,
                            if preset {
                                136.
                            } else {
                                self.text_width(&option.name, 17.)?
                            },
                            19.,
                        ],
                        if focus.setting() { 255 } else { 127 },
                    );
                    anchor = [
                        row_x + party_x,
                        63. + (menu.strategy.character - state.first) as f32 * 69.
                            + group as f32 * 19.,
                    ];
                }
                self.text_size(&option.name, [row_x, y], [17., 19.], WHITE)?;
            }
        }
        self.clip_rows(starts, [55., 336.]);
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [210., 48.]);
        }
        if state.first + 4 < party.formation.len() {
            self.scroll_arrow(SCROLL_DOWN, [210., 324.]);
        }
        if focus.setting() || focus.options() {
            self.offset = [slide(208), 0.];
            let options = menu.strategy_options();
            for (row, id) in options.into_iter().enumerate() {
                let y = 64. + row as f32 * 28.;
                let label = &data.strategy.groups[state.group][id].name;
                if focus.options() && row == state.option {
                    self.highlight(
                        [
                            452.,
                            y,
                            if preset {
                                136.
                            } else {
                                self.text_width(label, 17.)?
                            },
                            24.,
                        ],
                        255,
                    );
                    anchor = [452. + self.offset[0], y + 8.];
                }
                self.text(label, [452., y], 17., WHITE)?;
            }
            self.offset = [0., slide(312)];
            for (description, opacity) in [
                (state.description_previous, 255 - state.description_opacity),
                (menu.strategy_description(), state.description_opacity),
            ] {
                if let Some([group, option]) = description {
                    let option = &data.strategy.groups[group][option];
                    self.opacity = opacity;
                    self.shadowed_text(&option.name, [24., 336.], 28., 2.)?;
                    self.text(&option.description, [128., 370.], 20., WHITE)?;
                    self.text(&option.details, [128., 396.], 20., WHITE)?;
                }
            }
            self.opacity = 255;
            let label = &data.strategy.labels[state.group];
            self.text(label, [612. - self.text_width(label, 20.)?, 340.], 20., 5)?;
        } else if focus != Focus::Rename {
            self.offset = [0., slide(312)];
            self.strategy_formation(menu);
        }
        if focus == Focus::Character {
            self.offset = [0., -slide(48)];
            let label = &data.labels["strategy_orders"];
            let x = 624. - self.text_width(label, 24.)? - 24.;
            self.button(10, [x, 24.]);
            self.text(label, [x + 24., 24.], 24., WHITE)?;
        }
        if preset {
            self.offset = [0.; 2];
            if focus == Focus::Presets {
                for (key, button, y) in
                    [("strategy_rename", 10, 68.), ("strategy_default", 12, 96.)]
                {
                    self.button(button, [448., y]);
                    self.text(&data.labels[key], [472., y], 20., WHITE)?;
                }
            }
            self.plane = 2;
            self.offset = [
                0.,
                -f32::from((u16::from(255 - state.preset_opacity) * 56) / 256),
            ];
            self.opacity = state.preset_opacity;
            self.frame([100., 16., 440., 32.]);
            for (i, command) in menu.strategy_presets().iter().enumerate() {
                let x = 108. + i as f32 * 144.;
                if i == state.preset {
                    self.highlight(
                        [x, 20., 119., 24.],
                        if focus == Focus::Presets { 255 } else { 127 },
                    );
                    if matches!(focus, Focus::Presets | Focus::Rename) {
                        anchor = [x, 28. + self.offset[1]];
                    }
                }
                self.text(
                    &command.name,
                    [x, 20.],
                    17.,
                    if focus == Focus::Presets || i == state.preset {
                        WHITE
                    } else {
                        DISABLED
                    },
                )?;
            }
        }
        self.offset = [0.; 2];
        self.opacity = if matches!(focus, Focus::Presets | Focus::Rename) {
            state.preset_opacity
        } else {
            255 - state.transition.page_fade
        };
        Ok(anchor)
    }

    fn strategy_formation(&mut self, menu: &Menu) {
        let data = &menu.resources.as_ref().unwrap().data.strategy;
        let party = menu.party();
        let opacity = self.opacity;
        self.opacity = 255;
        for [x0, y0, x1, y1] in [
            [198., 400., 438., 400.],
            [150., 352., 390., 352.],
            [150., 352., 198., 400.],
            [270., 352., 318., 400.],
            [390., 352., 438., 400.],
        ] {
            // Two-pixel grid lines, expanded along their dominant screen axis.
            let shift = if x1 - x0 >= y1 - y0 {
                [0., 1.]
            } else {
                [1., 0.]
            };
            let points = [
                [x0 - shift[0], y0 - shift[1]],
                [x1 - shift[0], y1 - shift[1]],
                [x1 + shift[0], y1 + shift[1]],
                [x0 + shift[0], y0 + shift[1]],
            ];
            let layer = self.plane * self.layers_per_plane + FONT;
            let start = self.batches[layer].positions.len();
            self.quad(FONT, [0., 0., 1., 1.], [0.5; 4], [0., 0., 0., 1.]);
            for (vertex, [x, y]) in self.batches[layer].positions[start..]
                .iter_mut()
                .zip(points)
            {
                let [x, y] = [x + self.offset[0], y + self.offset[1]];
                let [x, y, _, _] = super::super::super::choice_cursor::overlay_rect([x, y, x, y]);
                *vertex = [x - 320., 240. - y, 0.];
            }
        }
        self.opacity = opacity;
        let mut counts = [0; 3];
        let mut positions = Vec::new();
        for &id in party.formation.iter().take(4) {
            let member = usize::from(id - 1);
            let lane = data.lane(member, menu.strategy_choices(member)[2]);
            let overlap = counts[lane] as f32 * 16.;
            counts[lane] += 1;
            positions.push((member, 422. - lane as f32 * 120. - overlap, 384. - overlap));
        }
        for (member, x, y) in positions.into_iter().rev() {
            let rect = self.spec.sprites.strategy_characters[member];
            self.sprite(rect, [x - (rect[2] as f32 - 32.).max(0.) / 2., y]);
        }
    }

    pub(super) fn strategy_cursors(
        &mut self,
        menu: &Menu,
        cursor: &resonance_content::font::UiTexture,
    ) {
        let focus = menu.strategy.focus;
        let y = (menu.strategy.character - menu.strategy.first) as f32 * 69.;
        if focus.setting() || focus.options() {
            self.cursor([32., 100. + y], cursor, 127);
        }
        if focus.options() {
            self.cursor(
                [
                    if focus.preset() { 264. } else { 240. },
                    63. + y + menu.strategy.group as f32 * 19.,
                ],
                cursor,
                127,
            );
        }
        if focus.preset() && !matches!(focus, Focus::Presets | Focus::Rename) {
            self.cursor(
                [108. + menu.strategy.preset as f32 * 144., 28.],
                cursor,
                127,
            );
        }
    }

    pub(super) fn strategy_rename(&mut self, menu: &Menu) -> Result<Option<[f32; 2]>> {
        let opacity = menu.strategy.rename_opacity;
        if !menu.strategy.focus.preset() || menu.strategy.focus != Focus::Rename && opacity == 0 {
            return Ok(None);
        }
        let data = &menu.resources.as_ref().unwrap().data.strategy;
        let edit = &menu.strategy.rename;
        self.plane = 2;
        self.opacity = 255;
        self.quad(
            FONT,
            self.screen,
            [0.5; 4],
            [0., 0., 0., f32::from(opacity / 2) / 255.],
        );
        self.opacity = opacity;
        self.shade([196., 48., 444., 96.]);
        self.shade([94., 116., 546., 392.]);
        self.plane = 3;
        self.frame([196., 48., 248., 48.]);
        self.highlight([208. + edit.position as f32 * 32., 60., 24., 24.], 255);
        for (i, c) in edit.value.chars().enumerate() {
            self.text(&c.to_string(), [208. + i as f32 * 32., 60.], 24., WHITE)?;
        }
        self.frame([94., 116., 452., 276.]);
        let position = |col: usize, row: usize| {
            [
                if col == 10 {
                    414.
                } else {
                    106. + col as f32 * 28. + (col / 5) as f32 * 4.
                },
                128. + row as f32 * 28.,
            ]
        };
        let [x, y] = position(edit.column, edit.row);
        self.highlight([x, y, if edit.column == 10 { 120. } else { 24. }, 24.], 255);
        for (i, c) in data.keyboard.chars().enumerate() {
            self.text(&c.to_string(), position(i % 10, i / 10), 24., WHITE)?;
        }
        for (row, label) in data.keys.iter().enumerate() {
            self.text(
                label,
                [414., 128. + row as f32 * 28.],
                24.,
                if row < 3 { DISABLED } else { WHITE },
            )?;
        }
        Ok(Some([x, y + 8.]))
    }
}

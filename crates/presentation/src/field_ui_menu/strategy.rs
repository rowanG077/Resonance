use super::*;
use anyhow::ensure;
use resonance_game::menu::strategy::{Focus, Page as StrategyPage, Strategy};

impl Drawing<'_> {
    pub(super) fn strategy(&mut self, page: StrategyPage<'_>) -> Result<[f32; 2]> {
        let data = page.data;
        let party = page.party;
        let state = page.state;
        let focus = state.focus;
        let preset = focus.preset();
        let row_x = if preset { 264. } else { 240. };
        let slide =
            |distance| f32::from((u32::from(state.transition.page_fade) * distance / 256) as u16);
        let party_x = -slide(436);
        self.offset = [0., -slide(76)];
        self.heading(
            data.presentation
                .labels
                .get("strategy_title")
                .context("Strategy title was not prepared")?,
        )?;
        self.offset = [slide(208), 0.];
        self.frame([440., 60., 180., 266.])?;
        self.offset = [0., slide(312)];
        self.framed([16., 336., 604., 88.], true)?;
        self.offset = [party_x, 0.];
        let anchor = main_anchor(state)?;
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
            self.colored_frame([16., y, 414., 59.], false, self.party_color(slot))?;
            if slot == page.state.character
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
            self.portrait(member, &party.members[member], [32., y - 4.])?;
            self.text(page.character_name(member), [96., y + 4.], 24., WHITE)?;
            for (group, option) in page.choices(member).into_iter().enumerate() {
                let option = data.strategy_option(group, usize::from(option))?;
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
                }
                self.text_size(&option.name, [row_x, y], [17., 19.], WHITE)?;
            }
        }
        self.clip_rows(starts, [55., 336.]);
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [210., 48.])?;
        }
        if state.first + 4 < party.formation.len() {
            self.scroll_arrow(SCROLL_DOWN, [210., 324.])?;
        }
        if focus.setting() || focus.options() {
            self.offset = [slide(208), 0.];
            let options = page.options();
            for (row, id) in options.into_iter().enumerate() {
                let y = 64. + row as f32 * 28.;
                let label = &data.strategy_option(state.group, id)?.name;
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
                }
                self.text(label, [452., y], 17., WHITE)?;
            }
            self.offset = [0., slide(312)];
            for (description, opacity) in [
                (state.description_previous, 255 - state.description_opacity),
                (page.description(), state.description_opacity),
            ] {
                if let Some([group, option]) = description {
                    let option = data.strategy_option(group, option)?;
                    self.opacity = opacity;
                    self.shadowed_text(&option.name, [24., 336.], 28., 2.)?;
                    self.text(&option.description, [128., 370.], 20., WHITE)?;
                    self.text(&option.details, [128., 396.], 20., WHITE)?;
                }
            }
            self.opacity = 255;
            let label = data
                .strategy_text()?
                .labels
                .as_ref()
                .context("Strategy group labels were not prepared")?
                .get(state.group)
                .context("Strategy group label was not prepared")?;
            self.text(label, [612. - self.text_width(label, 20.)?, 340.], 20., 5)?;
        } else if focus != Focus::Rename {
            self.offset = [0., slide(312)];
            self.strategy_formation(page)?;
        }
        if focus == Focus::Character {
            self.offset = [0., -slide(48)];
            let label = data
                .presentation
                .labels
                .get("strategy_orders")
                .context("Strategy orders label was not prepared")?;
            let x = 624. - self.text_width(label, 24.)? - 24.;
            self.button(10, [x, 24.])?;
            self.text(label, [x + 24., 24.], 24., WHITE)?;
        }
        if preset || state.preset_opacity != 0 {
            self.offset = [0.; 2];
            if focus == Focus::Presets {
                for (key, button, y) in
                    [("strategy_rename", 10, 68.), ("strategy_default", 12, 96.)]
                {
                    self.button(button, [448., y])?;
                    self.text(
                        data.presentation
                            .labels
                            .get(key)
                            .with_context(|| format!("Strategy label {key} was not prepared"))?,
                        [472., y],
                        20.,
                        WHITE,
                    )?;
                }
            }
            self.offset = [
                0.,
                -f32::from((u16::from(255 - state.preset_opacity) * 56) / 256),
            ];
            self.opacity = state.preset_opacity;
            // The preset backing covers the heading and lower selection indicator.
            self.plane = 2;
            self.shade([100., 16., 540., 48.]);
            self.plane = 3;
            self.frame([100., 16., 440., 32.])?;
            for (i, command) in page.presets()?.iter().enumerate() {
                let x = 108. + i as f32 * 144.;
                if i == state.preset {
                    self.highlight(
                        [x, 20., 119., 24.],
                        if focus == Focus::Presets { 255 } else { 127 },
                    );
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
        self.opacity = main_opacity(state);
        Ok(anchor)
    }

    fn strategy_formation(&mut self, page: StrategyPage<'_>) -> Result<()> {
        let data = &page.data.strategy;
        let party = page.party;
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
            let start = self.batch(FONT).positions.len();
            let offset = self.offset;
            self.quad(FONT, [0., 0., 1., 1.], [0.5; 4], [0., 0., 0., 1.]);
            for (vertex, [x, y]) in self.batch(FONT).positions[start..].iter_mut().zip(points) {
                let [x, y] = [x + offset[0], y + offset[1]];
                let [x, y, _, _] = super::super::super::ui_coordinates::overlay_rect([x, y, x, y]);
                *vertex = [x - 320., 240. - y, 0.];
            }
        }
        self.opacity = opacity;
        let mut counts = [0; 3];
        let mut positions = Vec::new();
        for &id in party.formation.iter().take(4) {
            let member = usize::from(id - 1);
            let lane = data.lane(member, page.choices(member)[2]);
            let overlap = counts[lane] as f32 * 16.;
            counts[lane] += 1;
            positions.push((member, 422. - lane as f32 * 120. - overlap, 384. - overlap));
        }
        for (member, x, y) in positions.into_iter().rev() {
            let rect = self.spec.sprite(Sprite::StrategyCharacters, member)?;
            self.sprite(rect, [x - (rect[2] as f32 - 32.).max(0.) / 2., y]);
        }
        Ok(())
    }

    pub(super) fn strategy_cursors(&mut self, page: StrategyPage<'_>) {
        let focus = page.state.focus;
        self.plane = if focus.preset() { 1 } else { 2 };
        let y = (page.state.character - page.state.first) as f32 * 69.;
        if focus.setting() || focus.options() {
            self.cursor([32., 100. + y], 127);
        }
        if focus.options() {
            self.cursor(
                [
                    if focus.preset() { 264. } else { 240. },
                    63. + y + page.state.group as f32 * 19.,
                ],
                127,
            );
        }
        if focus.preset() && !matches!(focus, Focus::Presets | Focus::Rename) {
            // Draw this cursor after the preset frame and names.
            self.plane = 3;
            self.cursor([108. + page.state.preset as f32 * 144., 28.], 127);
        }
        self.plane = main_plane(page.state);
    }

    pub(super) fn strategy_rename(&mut self, page: StrategyPage<'_>) -> Result<Option<[f32; 2]>> {
        let opacity = page.state.rename_opacity;
        if !rename_visible(page.state) {
            return Ok(None);
        }
        let data = page.data.strategy_text()?;
        let edit = &page.state.rename;
        self.plane = 4;
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
        self.plane = 5;
        self.frame([196., 48., 248., 48.])?;
        self.highlight([208. + edit.position as f32 * 32., 60., 24., 24.], 255);
        for (i, c) in edit.value.chars().enumerate() {
            self.text(&c.to_string(), [208. + i as f32 * 32., 60.], 24., WHITE)?;
        }
        self.frame([94., 116., 452., 276.])?;
        let [x, y] = key_position(edit.column, edit.row);
        self.highlight([x, y, if edit.column == 10 { 120. } else { 24. }, 24.], 255);
        for (i, c) in data.keyboard()?.chars().enumerate() {
            self.text(&c.to_string(), key_position(i % 10, i / 10), 24., WHITE)?;
        }
        for (row, label) in data.keys()?.iter().enumerate() {
            self.text(
                label,
                [414., 128. + row as f32 * 28.],
                24.,
                if row < 3 && row != usize::from(edit.mode) {
                    DISABLED
                } else {
                    WHITE
                },
            )?;
        }
        Ok(Some([x, y + 8.]))
    }
}

fn main_anchor(state: &Strategy) -> Result<[f32; 2]> {
    ensure!(
        state.first <= state.character && state.group < 3 && state.preset < 3,
        "invalid Strategy drawing selection"
    );
    let fade = u32::from(state.transition.page_fade);
    let row = (state.character - state.first) as f32 * 69.;
    Ok(if matches!(state.focus, Focus::Presets | Focus::Rename) {
        [
            108. + state.preset as f32 * 144.,
            28. - f32::from(u16::from(255 - state.preset_opacity) * 56 / 256),
        ]
    } else if state.focus.options() {
        [
            452. + (fade * 208 / 256) as f32,
            72. + state.option as f32 * 28.,
        ]
    } else if state.focus.setting() {
        [
            if state.focus.preset() { 264. } else { 240. } - (fade * 436 / 256) as f32,
            63. + row + state.group as f32 * 19.,
        ]
    } else {
        [32. - (fade * 436 / 256) as f32, 100. + row]
    })
}

fn main_opacity(state: &Strategy) -> u8 {
    if matches!(state.focus, Focus::Presets | Focus::Rename) {
        state.preset_opacity
    } else {
        255 - state.transition.page_fade
    }
}
fn main_alpha(state: &Strategy) -> u8 {
    if state.focus == Focus::Rename {
        127
    } else {
        255
    }
}
fn rename_visible(state: &Strategy) -> bool {
    state.focus.preset() && (state.focus == Focus::Rename || state.rename_opacity != 0)
}
fn main_plane(state: &Strategy) -> usize {
    if matches!(state.focus, Focus::Presets | Focus::Rename) {
        3
    } else if state.focus.preset() {
        1
    } else {
        2
    }
}
fn key_position(column: usize, row: usize) -> [f32; 2] {
    [
        if column == 10 {
            414.
        } else {
            106. + column as f32 * 28. + (column / 5) as f32 * 4.
        },
        128. + row as f32 * 28.,
    ]
}

impl MenuArtwork {
    #[allow(clippy::too_many_arguments)] // Already prepared resources and one immutable shared page.
    pub(crate) fn render_battle_strategy(
        &mut self,
        page: StrategyPage<'_>,
        font: &BitmapFont,
        dialogue: &DialogueArt,
        preferences: &resonance_content::menu_data::CustomizeSettings,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.set_page(Some(ActivePage::Menu(Page::Strategy)));
        let mut draw = self.begin_drawing(font, dialogue, Some(preferences), tick)?;
        draw.opacity = 255 - page.state.transition.page_fade;
        // Fade the gradient with the page while the captured backdrop remains opaque.
        draw.shade(draw.screen);
        draw.plane = 1;
        draw.strategy_page(page)?;
        let drawing = draw.batches;
        self.submit_drawing(drawing, commands, meshes)
    }
}

impl Drawing<'_> {
    fn strategy_page(&mut self, page: StrategyPage<'_>) -> Result<()> {
        let anchor = self.strategy(page)?;
        self.plane = main_plane(page.state);
        self.strategy_cursors(page);
        self.cursor(anchor, main_alpha(page.state));
        if let Some(anchor) = self.strategy_rename(page)? {
            self.cursor(anchor, 255);
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "strategy_tests.rs"]
mod tests;

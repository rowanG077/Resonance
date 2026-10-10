use super::*;
use anyhow::ensure;
use resonance_game::menu::unison::{Focus, Page as UnisonPage, Unison, VISIBLE_TECHNIQUES};

fn slot_cursor(state: &Unison) -> [f32; 2] {
    let x = 32 + (state.character / 2) as i32 * 312;
    [
        (x + 40) as f32,
        90. + (state.character % 2) as f32 * 130. + state.slot as f32 * 24.,
    ]
}

impl Drawing<'_> {
    pub(super) fn unison(&mut self, page: UnisonPage<'_>, at_save_point: bool) -> Result<[f32; 2]> {
        let data = page.data;
        let party = page.party;
        let state = page.state;
        let count = page.party_count();
        let anchor = main_anchor(state)?;
        self.opacity = 255;
        self.offset = [0.; 2];
        self.heading(
            data.presentation
                .labels
                .get("unison_title")
                .context("U. Attack title was not prepared")?,
        )?;
        self.framed([16., 320., 604., 108.], true)?;
        if let Some(selected) = page.selection() {
            self.plane = 3;
            self.technique_description(party, data, selected, at_save_point)?;
        }
        self.plane = 4;
        self.opacity = 255;
        for (index, &id) in party.formation.iter().take(count).enumerate() {
            let member = usize::from(id - 1);
            let selected = index == state.character;
            let x = 16. + (index / 2) as f32 * 308.;
            let y = 60. + (index % 2) as f32 * 130.;
            self.frame([x, y, 296., 120.])?;
            if selected {
                self.highlight(
                    [x + 80., y + 22. + state.slot as f32 * 24., 200., 24.],
                    if state.focus == Focus::Slots {
                        255
                    } else {
                        127
                    },
                );
            }
            self.button(6 + index * 2 - usize::from(!selected), [x + 6., y])?;
            self.text_size(
                page.character_name(member)
                    .context("U. Attack character name was not prepared")?,
                [x + 30., y + 2.],
                [20.; 2],
                WHITE,
            )?;
            let number = (index + 1).to_string();
            self.text(&number, [x, y + 40.], 16., WHITE)?;
            self.text(
                &data
                    .presentation
                    .labels
                    .get("unison_player")
                    .context("U. Attack player label was not prepared")?
                    .replace("%d", &number),
                [x, y + 40.],
                16.,
                WHITE,
            )?;
            for (slot, &id) in party.members[member].shortcuts.iter().enumerate() {
                let y = y + 22. + slot as f32 * 24.;
                let active = selected && slot == state.slot;
                if id != 0 {
                    let tech = &data.techniques[usize::from(id)];
                    let tech_text = data.technique_text(id)?;
                    self.text(
                        &tech_text.name,
                        [x + 64., y],
                        16.,
                        if tech.unison_usable { WHITE } else { DISABLED },
                    )?;
                }
                if slot == 3 {
                    self.button(if active { 35 } else { 4 }, [x + 16., y])?;
                    self.button(if active { 36 } else { 3 }, [x + 40., y])?;
                } else {
                    self.button(if active { [0, 33, 34][slot] } else { slot }, [x + 28., y])?;
                }
            }
        }
        // Draw the controller legend after the slot cursor.
        self.plane = 5;
        self.offset = [0.; 2];
        for (index, (button_position, label_position)) in [
            ([404., 24.], [408., 40.]),
            ([380., 30.], [352., 40.]),
            ([428., 20.], [448., 24.]),
            ([396., 8.], [364., 12.]),
        ]
        .into_iter()
        .enumerate()
        {
            self.button(
                5 + index * 2 + usize::from(index == state.character),
                button_position,
            )?;
            self.text_size(
                &data
                    .presentation
                    .labels
                    .get("unison_player")
                    .context("U. Attack player label was not prepared")?
                    .replace("%d", &(index + 1).to_string()),
                label_position,
                [16.; 2],
                if index < count { WHITE } else { DISABLED },
            )?;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        if state.focus == Focus::Slots {
            return Ok(anchor);
        }
        let x = if state.character < 2 { 388. } else { 16. };
        let choices = page.techniques();
        self.plane = 6;
        self.opacity = 255;
        self.shade([x, 60., x + 232., 310.]);
        self.plane = 7;
        self.frame([x, 60., 232., 250.])?;
        self.text_size(
            &format!(
                "{}/{}",
                if choices.is_empty() { 0 } else { state.row + 1 },
                choices.len()
            ),
            [x + 8., 62.],
            [16.; 2],
            WHITE,
        )?;
        let selected_y = 84. + (state.row - state.first) as f32 * 28.;
        if !choices.is_empty() {
            self.highlight([x + 8., selected_y, 208., 24.], 255);
        }
        let starts = self.vertex_counts();
        for (row, &id) in choices
            .iter()
            .skip(state.first)
            .take(VISIBLE_TECHNIQUES)
            .enumerate()
        {
            let tech = &data.techniques[usize::from(id)];
            let tech_text = data.technique_text(id)?;
            self.text(
                &tech_text.name,
                [x + 8., 84. + row as f32 * 28.],
                16.,
                if tech.unison_usable { WHITE } else { DISABLED },
            )?;
        }
        self.clip_rows(starts, [84., 308.]);
        self.opacity = 255;
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [x + 104., 66.])?;
        }
        if state.first + VISIBLE_TECHNIQUES < choices.len() {
            self.scroll_arrow(SCROLL_DOWN, [x + 104., 300.])?;
        }
        self.opacity = 255;
        Ok(anchor)
    }

    pub(super) fn unison_cursors(&mut self, page: UnisonPage<'_>) {
        // Draw the slot cursor before the controller legend and list backing.
        self.plane = 4;
        if page.state.focus == Focus::List {
            self.cursor(slot_cursor(page.state), 127);
            self.plane = 7;
        }
    }
}

fn main_anchor(state: &Unison) -> Result<[f32; 2]> {
    ensure!(
        state.character < 4 && state.slot < 4 && state.first <= state.row,
        "invalid U. Attack drawing pose"
    );
    Ok(if state.focus == Focus::Slots {
        slot_cursor(state)
    } else {
        let x = if state.character < 2 { 388. } else { 16. };
        [x + 8., 92. + (state.row - state.first) as f32 * 28.]
    })
}
impl MenuArtwork {
    #[allow(clippy::too_many_arguments)] // One borrowed page and its prepared drawing resources.
    pub(crate) fn render_battle_unison(
        &mut self,
        page: UnisonPage<'_>,
        font: &BitmapFont,
        dialogue: &DialogueArt,
        preferences: &resonance_content::menu_data::CustomizeSettings,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.set_page(Some(ActivePage::Menu(Page::Unison)));
        let mut draw = self.begin_drawing(font, dialogue, Some(preferences), tick)?;
        draw.opacity = 255;
        // A field caller already owns its background.
        draw.shade(draw.screen);
        draw.plane = 1;
        draw.unison_page(page)?;
        let drawing = draw.batches;
        self.submit_drawing(drawing, commands, meshes)
    }
}
impl Drawing<'_> {
    fn unison_page(&mut self, page: UnisonPage<'_>) -> Result<()> {
        let anchor = self.unison(page, false)?;
        self.unison_cursors(page);
        self.cursor(anchor, 255);
        Ok(())
    }
}
#[cfg(test)]
#[path = "unison_tests.rs"]
mod tests;

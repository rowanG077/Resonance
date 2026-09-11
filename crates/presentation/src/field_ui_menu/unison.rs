use super::*;
use resonance_game::menu::unison::{Focus, VISIBLE_TECHNIQUES};

fn slot_cursor(menu: &Menu) -> [f32; 2] {
    let fade = i32::from(menu.unison.transition.page_fade);
    let x = 32 + (menu.unison.character / 2) as i32 * 312;
    let x = if menu.unison.character < 2 {
        x - (x + 296) * fade / 256
    } else {
        x + (x + 16) * fade / 256
    };
    [
        (x + 40) as f32,
        90. + (menu.unison.character % 2) as f32 * 130. + menu.unison.slot as f32 * 24.,
    ]
}

impl Drawing<'_> {
    pub(super) fn unison(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu.resources.as_ref().unwrap().data;
        let party = menu.party();
        let state = &menu.unison;
        let count = menu.unison_party_count();
        let fade = u32::from(state.transition.page_fade);
        let slide = |distance| (fade * distance / 256) as f32;
        let page_opacity = 255 - state.transition.page_fade;
        self.opacity = page_opacity;
        self.offset = [0., -slide(76)];
        self.heading(&data.labels["unison_title"])?;
        self.offset = [0., slide(136)];
        self.framed([16., 320., 604., 108.], true);
        for (index, &id) in party.formation.iter().take(count).enumerate() {
            let member = usize::from(id - 1);
            let selected = index == state.character;
            let x = 16. + (index / 2) as f32 * 308.;
            let y = 60. + (index % 2) as f32 * 130.;
            self.offset = [if index < 2 { -slide(312) } else { slide(340) }, 0.];
            self.frame([x, y, 296., 120.]);
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
            self.button(6 + index * 2 - usize::from(!selected), [x + 6., y]);
            self.text_size(
                menu.character_name(member),
                [x + 30., y + 2.],
                [20.; 2],
                WHITE,
            )?;
            let number = (index + 1).to_string();
            self.text(&number, [x, y + 40.], 16., WHITE)?;
            self.text(
                &data.labels["unison_player"].replace("%d", &number),
                [x, y + 40.],
                16.,
                WHITE,
            )?;
            for (slot, &id) in party.members[member].shortcuts.iter().enumerate() {
                let y = y + 22. + slot as f32 * 24.;
                let active = selected && slot == state.slot;
                if id != 0 {
                    let tech = &data.techniques[usize::from(id)];
                    self.text(
                        &tech.name,
                        [x + 64., y],
                        16.,
                        if tech.unison_usable { WHITE } else { DISABLED },
                    )?;
                }
                if slot == 3 {
                    self.button(if active { 35 } else { 4 }, [x + 16., y]);
                    self.button(if active { 36 } else { 3 }, [x + 40., y]);
                } else {
                    self.button(if active { [0, 33, 34][slot] } else { slot }, [x + 28., y]);
                }
            }
        }
        self.offset = [0., -slide(68)];
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
            );
            self.text_size(
                &data.labels["unison_player"].replace("%d", &(index + 1).to_string()),
                label_position,
                [16.; 2],
                if index < count { WHITE } else { DISABLED },
            )?;
        }
        self.offset = [0., slide(136)];
        if state.description_opacity != 255
            && let Some(previous) = state.description_previous
        {
            self.opacity = 255 - state.description_opacity;
            self.technique_description(menu, previous)?;
        }
        if let Some(selected) = menu.unison_selection() {
            self.opacity = crossfade_opacity(state.description_opacity, page_opacity);
            self.technique_description(menu, selected)?;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        if state.focus == Focus::Slots {
            return Ok(slot_cursor(menu));
        }
        let movement = (u32::from(255 - state.list_opacity) * 248 / 256) as f32;
        let x = if state.character < 2 {
            388. + movement
        } else {
            16. - movement
        };
        let choices = menu.unison_techniques();
        self.plane = 2;
        self.opacity = state.list_opacity;
        self.shade([x, 60., x + 232., 310.]);
        self.plane = 3;
        self.frame([x, 60., 232., 250.]);
        self.text_size(
            &format!("{}/{}", state.row + 1, choices.len()),
            [x + 8., 62.],
            [16.; 2],
            WHITE,
        )?;
        let selected_y = 84. + (state.row - state.first) as f32 * 28.;
        self.highlight([x + 8., selected_y, 208., 24.], 255);
        let scroll = state.scroll;
        let movement = scroll_offset(scroll, 28);
        let starts = self.vertex_counts();
        for (row, &id) in choices
            .iter()
            .skip(state.first - usize::from(scroll > 0))
            .take(VISIBLE_TECHNIQUES + usize::from(scroll != 0))
            .enumerate()
        {
            let tech = &data.techniques[usize::from(id)];
            self.text(
                &tech.name,
                [x + 8., 84. + row as f32 * 28. - movement as f32],
                16.,
                if tech.unison_usable { WHITE } else { DISABLED },
            )?;
        }
        self.clip_rows(starts, [84., 308.]);
        self.opacity = page_opacity;
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [x + 104., 66.]);
        }
        if state.first + VISIBLE_TECHNIQUES < choices.len() {
            self.scroll_arrow(SCROLL_DOWN, [x + 104., 300.]);
        }
        self.opacity = 255;
        Ok([x + 8., selected_y + 8.])
    }

    pub(super) fn unison_cursors(
        &mut self,
        menu: &Menu,
        cursor: &resonance_content::font::UiTexture,
    ) {
        if menu.unison.focus == Focus::List {
            self.cursor(
                slot_cursor(menu),
                cursor,
                (255 - menu.unison.transition.page_fade) >> 1,
            );
        }
    }
}

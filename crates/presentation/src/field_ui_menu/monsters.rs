use super::*;
use resonance_game::menu::monsters::VISIBLE;

impl Drawing<'_> {
    pub(super) fn monsters(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu.resources.as_ref().unwrap().data;
        let labels = &data.monsters.labels;
        let state = &menu.monsters;
        let records = menu.monster_records();
        let fade = u32::from(state.view.page_fade);
        self.opacity = 255 - state.view.page_fade;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&labels["title"])?;
        self.offset = [0., -((fade * 48 / 256) as f32)];
        self.framed([380., 20., 236., 32.], false);
        let rank = menu.preferences().unwrap().battle_rank;
        self.text(&labels["battle_rank"], [388., 24.], 18., GOLD)?;
        self.text(
            &labels[["normal", "hard", "mania"][usize::from(rank)]],
            [536., 24.],
            18.,
            WHITE,
        )?;
        self.offset = [(fade * 288 / 256) as f32, 0.];
        self.text_size(
            &format!("{:3}/251 {:3}%", records.len(), records.len() * 100 / 251),
            [408., 60.],
            [16.; 2],
            WHITE,
        )?;
        let left = -((fade * 344 / 256) as f32);
        self.offset = [left, 0.];
        self.framed([16., 60., 320., 368.], true);
        let mut anchor = self.monster_details(menu)?;
        anchor[0] += left;
        self.offset = [0.; 2];
        if menu.busy {
            self.preview_loading(menu)?;
        }
        self.opacity = 255;
        Ok(anchor)
    }

    fn monster_details(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu.resources.as_ref().unwrap().data;
        let labels = &data.monsters.labels;
        let state = &menu.monsters;
        let records = menu.monster_records();
        if state.listing {
            return self.catalogue_list(
                records
                    .iter()
                    .map(|(record, _)| record.name.as_str())
                    .collect(),
                32.,
                state.list_row,
                state.first,
                state.scroll,
            );
        }
        self.highlight([32., 64., 120., 24.], 255);
        let Some((record, _)) = menu.monster() else {
            return Ok([32., 72.]);
        };
        self.text(&labels["number"], [32., 64.], 20., GOLD)?;
        self.text(
            &format!("{:3}", u16::from(record.id) + 1),
            [92., 64.],
            20.,
            WHITE,
        )?;
        self.text(&record.name, [32., 90.], 24., WHITE)?;
        let page_opacity = self.opacity;
        let (record, knowledge) = menu.displayed_monster().unwrap();
        self.opacity = state.view.model_opacity;
        let x = 320. - self.text_width(&record.category, 20.)?;
        self.text(&record.category, [x, 64.], 20., WHITE)?;
        if knowledge.scanned && knowledge.variant > 0 {
            let text = format!("({}/{})", state.variant + 1, knowledge.variant + 1);
            let x = 336. - self.text_width(&text, 16.)?;
            self.text_size(&text, [x, 102.], [16.; 2], WHITE)?;
        }
        let stats = &record.statistics[state.variant.min(record.statistics.len() - 1)];
        let rank = menu.preferences().unwrap().battle_rank;
        let hp = stats.hp * [2, 3, 4][usize::from(rank)] / 2;
        let tp = u32::from(stats.tp) * [2, 3, 4][usize::from(rank)] / 2;
        let attack = u32::from(stats.attack) * [4, 5, 6][usize::from(rank)] / 4;
        for (i, (left, right, a, b)) in [
            ("hp", "experience", hp, stats.experience),
            ("tp", "gald", tp, stats.gald),
            ("attack", "defense", attack, stats.defense.into()),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 118. + i as f32 * 24.;
            self.opacity = page_opacity;
            self.text(&labels[left], [40., y], 16., GOLD)?;
            self.text(&labels[right], [184., y], 16., GOLD)?;
            self.opacity = state.view.model_opacity;
            if knowledge.scanned {
                self.number(a, [168., y], [16., 24.], WHITE)?;
                self.number(b, [328., y], [16., 24.], WHITE)?;
            } else {
                self.text(&labels["unknown_stat"], [88., y], 16., WHITE)?;
                self.text(&labels["unknown_stat"], [232., y], 16., WHITE)?;
            }
        }
        self.opacity = page_opacity;
        for (key, y) in [
            ("drops", 190.),
            ("steal", 262.),
            ("location", 318.),
            ("attack_element", 344.),
            ("weak", 370.),
            ("strong", 396.),
        ] {
            self.text(&labels[key], [32., y], 20., GOLD)?;
        }
        self.opacity = state.view.model_opacity;
        for (id, known, y) in [
            (record.drops[0], knowledge.drops[0], 214.),
            (record.drops[1], knowledge.drops[1], 238.),
            (record.steal, knowledge.steal, 288.),
        ] {
            if let Some(id) = id {
                let item = &data.items[usize::from(id)];
                if known {
                    self.sprite(
                        self.spec.sprites.items[usize::from(item.category - 1)],
                        [40., y],
                    );
                }
                self.text(
                    if known {
                        &item.name
                    } else {
                        &labels["unknown_item"]
                    },
                    [64., y],
                    16.,
                    WHITE,
                )?;
            }
        }
        self.text(
            if knowledge.location {
                &record.location
            } else {
                &labels["unknown_item"]
            },
            [92., 318.],
            16.,
            WHITE,
        )?;
        if knowledge.scanned {
            for (elements, y) in [
                (&record.weaknesses[..], 370.),
                (&record.resistances[..], 396.),
            ] {
                for (i, &element) in elements.iter().enumerate() {
                    self.sprite(
                        self.spec.sprites.elements[element as usize],
                        [112. + i as f32 * 24., y],
                    );
                }
            }
            if let Some(element) = record.attack_element {
                self.sprite(self.spec.sprites.elements[element as usize], [112., 344.]);
            }
        }
        Ok([32., 72.])
    }

    pub(super) fn catalogue_list(
        &mut self,
        names: Vec<&str>,
        x: f32,
        row: usize,
        first: usize,
        scroll: i8,
    ) -> Result<[f32; 2]> {
        self.text_size(
            &format!("{}/{}", row + 1, names.len()),
            [x, 60.],
            [16.; 2],
            WHITE,
        )?;
        let y = 84. + (row - first) as f32 * 28.;
        self.highlight([x, y, 280., 24.], 255);
        let starts = self.vertex_counts();
        let offset = scroll_offset(scroll, 28);
        for (i, name) in names
            .iter()
            .skip(first - usize::from(scroll > 0))
            .take(VISIBLE + usize::from(scroll != 0))
            .enumerate()
        {
            self.text(name, [x, 84. + i as f32 * 28. - offset as f32], 20., WHITE)?;
        }
        self.clip_rows(starts, [84., 420.]);
        if first > 0 {
            self.scroll_arrow(SCROLL_UP, [x + 132., 68.]);
        }
        if first + VISIBLE < names.len() {
            self.scroll_arrow(SCROLL_DOWN, [x + 132., 416.]);
        }
        Ok([x, y + 8.])
    }

    /// Preparation includes asset decoding and GPU readiness, with no fixed duration.
    pub(super) fn preview_loading(&mut self, menu: &Menu) -> Result<()> {
        let previous = (self.plane, self.opacity, self.offset);
        self.plane = self.plane.max(1);
        self.opacity = 255;
        self.offset = [0.; 2];
        let result = self.text_size(
            &menu.resources.as_ref().unwrap().data.labels["preview_loading"],
            [360., 220.],
            [16., 20.],
            WHITE,
        );
        (self.plane, self.opacity, self.offset) = previous;
        result
    }
}

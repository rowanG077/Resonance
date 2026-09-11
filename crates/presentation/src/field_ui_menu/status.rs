use super::*;

impl Drawing<'_> {
    pub(super) fn status(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu
            .resources
            .as_ref()
            .context("status data was not prepared")?
            .data;
        let state = &menu.status;
        let index = menu.member_index();
        let fade = u32::from(state.page_fade);
        let opacity = 255 - state.page_fade;
        let member_opacity = if state.portrait_fade == 0 {
            opacity
        } else {
            255 - state.portrait_fade
        };
        self.offset = [0., -(((fade * 76) / 256) as f32)];
        if let Some(previous) = state.previous.filter(|_| state.portrait_fade != 0) {
            self.opacity = state.portrait_fade;
            self.portrait(
                previous,
                menu.party().members[previous].conditions,
                [328., 16.],
            );
        }
        self.opacity = member_opacity;
        self.portrait(index, menu.member().conditions, [328., 16.]);
        self.opacity = opacity;
        self.heading(&data.labels["status"])?;
        self.offset = [-(((fade * 316) / 256) as f32), 0.];
        self.framed([16., 60., 296., 368.], true);
        let anchor = if state.title_focus {
            [48., 94.]
        } else {
            [32., 68.]
        };
        self.highlight(
            [
                anchor[0],
                anchor[1],
                if state.title_focus {
                    200.
                } else {
                    self.text_width(&menu.full_name(index), 20.)?
                },
                if state.title_focus { 20. } else { 24. },
            ],
            if menu.page == Page::Titles { 127 } else { 255 },
        );
        if let Some(previous) = state.previous.filter(|_| state.portrait_fade != 0) {
            self.opacity = state.portrait_fade;
            self.status_member(menu, previous)?;
        }
        self.opacity = member_opacity;
        // Each member's bars, labels and numbers blend as one ordered layer.
        self.plane = 2;
        self.status_member(menu, index)?;
        self.plane = 1;
        let anchor = [
            anchor[0] + self.offset[0],
            anchor[1] + if state.title_focus { 2. } else { 0. },
        ];
        self.offset = [((fade * 312) / 256) as f32, 0.];
        self.status_portrait(index);
        if let Some(previous) = state.previous.filter(|_| state.portrait_fade != 0) {
            self.plane = 2;
            self.opacity = state.portrait_fade;
            self.status_portrait(previous);
            self.plane = 1;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        if menu.page == Page::Titles {
            return self.titles(menu);
        }
        Ok(anchor)
    }

    fn status_portrait(&mut self, index: usize) {
        let portrait = &self.spec.textures[19 + index];
        self.quad(
            PORTRAITS + index,
            [
                312.,
                0.,
                312. + portrait.width as f32,
                portrait.height as f32,
            ],
            [0., 0., portrait.width as f32, portrait.height as f32],
            [1.; 4],
        );
    }

    fn status_member(&mut self, menu: &Menu, index: usize) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let member = &menu.party().members[index];
        let stats = member.stats(data);
        let title = &data.titles[index][usize::from(member.title - 1)];
        self.text(&menu.full_name(index), [32., 68.], 20., WHITE)?;
        self.text_size(&title.name, [48., 94.], [20.; 2], WHITE)?;
        if menu.status.details {
            self.status_traits(menu, member)?;
        } else {
            self.text_size("Lv", [32., 116.], [20.; 2], GOLD)?;
            self.number(u32::from(member.level), [136., 116.], [16., 20.], WHITE)?;
            self.technique([172., 114.], member.technique_balance);
            self.gauge(false, [32., 136.], [16., 20.], member.hp, stats.hp, Full)?;
            self.gauge(true, [32., 156.], [16., 20.], member.tp, stats.tp, Full)?;
            let next = self
                .experience
                .get(usize::from(member.level) + 1)
                .copied()
                .unwrap_or(member.experience)
                .saturating_sub(member.experience);
            for (label, value, y) in [
                ("EXP", member.experience, 176.),
                (data.labels["next"].as_str(), next, 196.),
            ] {
                self.text_size(label, [32., y], [20.; 2], GOLD)?;
                self.number(value, [244., y], [16., 20.], WHITE)?;
            }
            for (row, columns) in [
                [("strength", stats.strength), ("defense", stats.defense)],
                [
                    (if index == 0 { "slash" } else { "attack" }, stats.slash),
                    ("accuracy", stats.accuracy),
                ],
                [("thrust", stats.thrust), ("evasion", stats.evasion)],
                [("intelligence", stats.intelligence), ("luck", stats.luck)],
            ]
            .into_iter()
            .enumerate()
            {
                for (column, (label, value)) in columns.into_iter().enumerate() {
                    if label == "thrust" && index != 0 {
                        continue;
                    }
                    let x = 32. + column as f32 * 140.;
                    let y = 216. + row as f32 * 20.;
                    self.text_size(&data.labels[label], [x, y], [20.; 2], GOLD)?;
                    let right = if column == 0 { 156. } else { 276. };
                    self.number(u32::from(value), [right, y], [16., 20.], WHITE)?;
                }
            }
            for (row, (slot, label)) in [
                (0, "weapon"),
                (1, "body"),
                (2, "head"),
                (5, "arm"),
                (3, "accessory_1"),
                (4, "accessory_2"),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 300. + row as f32 * 20.;
                self.text_size(&data.labels[label], [32., y], [20.; 2], GOLD)?;
                if member.equipment[slot] != 0 {
                    let item = &data.items[usize::from(member.equipment[slot])];
                    let icon =
                        self.spec.sprites.items[usize::from(item.category.saturating_sub(1))];
                    self.sprite_rect(icon, [112., y, 132., y + 20.], [1.; 4]);
                    self.text_size(&item.name, [136., y], [16., 20.], WHITE)?;
                }
            }
            self.scroll_arrow(SCROLL_DOWN, [152., 422.]);
        }
        Ok(())
    }

    fn status_traits(
        &mut self,
        menu: &Menu,
        member: &resonance_events::party::Member,
    ) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let traits = member.equipment_traits(data);
        self.text(&data.labels["element_attack"], [24., 116.], 16., GOLD)?;
        self.text(&data.labels["element_defense"], [24., 142.], 16., GOLD)?;
        if let Some(element) = traits.attack_element {
            self.sprite(self.spec.sprites.elements[element as usize], [104., 116.]);
        }
        let mut count: usize = 0;
        for (element, value) in traits
            .resistance
            .into_iter()
            .enumerate()
            .filter(|(_, v)| *v != 0)
        {
            let x = 104. + (count % 2) as f32 * 88.;
            let y = 142. + (count / 2) as f32 * 26.;
            self.sprite(self.spec.sprites.elements[element], [x, y]);
            let label = match value {
                ..=-2 => "weak",
                7.. => "absorb",
                5..=6 => "invalid",
                2..=4 => "reduce",
                _ => "",
            };
            if !label.is_empty() {
                self.text(&data.labels[label], [x + 24., y], 12., WHITE)?;
            }
            count += 1;
        }
        let mut y = 142. + count.max(1).div_ceil(2) as f32 * 26.;
        let conditions = data
            .status
            .conditions
            .iter()
            .enumerate()
            .filter(|(i, _)| member.conditions & (1 << i) != 0)
            .map(|(_, label)| label);
        for (label, width) in conditions.map(|s| (s, 18.)).chain(
            traits
                .effects
                .iter()
                .map(|id| (&data.status.equipment_effects[id].description, 20.)),
        ) {
            if label.is_empty() {
                continue;
            }
            let mut width = width;
            while width > 1. && self.text_width(label, width)? > 280. {
                width -= 1.;
            }
            self.text_size(label, [24., y], [width, 20.], WHITE)?;
            y += 22.;
            if y >= 398. {
                break;
            }
        }
        self.scroll_arrow(SCROLL_UP, [152., 104.]);
        Ok(())
    }

    fn titles(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let titles = menu.titles();
        let data = &menu.resources.as_ref().unwrap().data;
        let definitions = &data.titles[menu.member_index()];
        let first = menu.status.row.saturating_sub(8);
        let fade = u32::from(255 - menu.status.title_opacity);
        let list_offset = ((fade * 304) / 256) as f32;
        let card_offset = ((fade * 116) / 256) as f32;
        self.opacity = menu.status.title_opacity;
        self.offset = [list_offset, 0.];
        self.plane = 3;
        self.shade([344., 60., 624., 68. + 28. * titles.len().min(9) as f32]);
        self.offset = [0., card_offset];
        self.shade([16., 340., 624., 432.]);
        self.offset = [list_offset, 0.];
        self.plane = 4;
        self.frame([344., 60., 280., 8. + 28. * titles.len().min(9) as f32]);
        for (row, &id) in titles.iter().skip(first).take(9).enumerate() {
            let y = 64. + row as f32 * 28.;
            let title = &definitions[usize::from(id - 1)];
            if first + row == menu.status.row {
                self.highlight([352., y, 240., 24.], 255);
            }
            self.text(&title.name, [352., y], 20., WHITE)?;
        }
        let title = &definitions[usize::from(titles[menu.status.row] - 1)];
        self.offset = [0., card_offset];
        self.frame([16., 340., 608., 92.]);
        self.text(&title.description, [24., 344.], 20., WHITE)?;
        self.text(&data.labels["growth"], [24., 396.], 24., GOLD)?;
        let current = definitions[usize::from(menu.member().title - 1)].growth;
        let mut x = 36. + self.text_width(&data.labels["growth"], 24.)?;
        for (i, (name, value)) in [
            "growth_hp",
            "growth_tp",
            "growth_strength",
            "growth_defense",
            "growth_intelligence",
            "growth_evasion",
            "growth_accuracy",
        ]
        .into_iter()
        .zip(title.growth)
        .enumerate()
        {
            let name = &data.labels[name];
            let color = match value.cmp(&current[i]) {
                std::cmp::Ordering::Greater => 4,
                std::cmp::Ordering::Less => 2,
                _ => DISABLED,
            };
            self.text(name, [x, 396.], 24., color)?;
            x += self.text_width(name, 24.)? + 12.;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok([
            352. + list_offset,
            72. + (menu.status.row - first) as f32 * 28.,
        ])
    }
}

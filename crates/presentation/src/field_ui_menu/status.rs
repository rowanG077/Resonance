use super::*;
use resonance_game::menu::status::{Focus, VISIBLE_TITLES};

fn growth_bonus(stat: usize, value: u8) -> String {
    if stat < 2 || value.is_multiple_of(10) {
        format!("+{}", if stat < 2 { value } else { value / 10 })
    } else {
        format!("+{}.{}", value / 10, value % 10)
    }
}

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
        let backdrop = if menu.page == Page::Titles {
            255 - state.title_opacity
        } else {
            255
        };
        let backdrop_alpha = |opacity: u8| (u16::from(opacity) * u16::from(backdrop) / 255) as u8;
        let opacity = backdrop_alpha(255 - state.page_fade);
        let member_opacity = if state.portrait_fade == 0 {
            opacity
        } else {
            backdrop_alpha(255 - state.portrait_fade)
        };
        self.offset = [0., -(((fade * 76) / 256) as f32)];
        if let Some(previous) = state.previous.filter(|_| state.portrait_fade != 0) {
            self.opacity = backdrop_alpha(state.portrait_fade);
            self.portrait(previous, &menu.party().members[previous], [328., 16.])?;
        }
        self.opacity = member_opacity;
        self.portrait(index, menu.member(), [328., 16.])?;
        self.opacity = 255 - state.page_fade;
        self.heading(data.label("status")?)?;
        self.opacity = opacity;
        self.offset = [-(((fade * 316) / 256) as f32), 0.];
        self.framed([16., 60., 296., 368.], true)?;
        let anchor = match state.focus {
            Focus::Name => [32., 68.],
            Focus::Title => [48., 94.],
            Focus::Costume => [32., 116.],
        };
        self.highlight(
            [
                anchor[0],
                anchor[1],
                match state.focus {
                    Focus::Name => self.text_width(&menu.full_name(index)?, 20.)?,
                    Focus::Title => 248.,
                    Focus::Costume => 264.,
                },
                if state.focus == Focus::Name { 24. } else { 20. },
            ],
            if menu.page == Page::Titles { 127 } else { 255 },
        );
        if let Some(previous) = state.previous.filter(|_| state.portrait_fade != 0) {
            self.opacity = backdrop_alpha(state.portrait_fade);
            self.status_member(menu, previous)?;
        }
        self.opacity = member_opacity;
        // Each member's bars, labels and numbers blend as one ordered layer.
        self.plane = 2;
        self.status_member(menu, index)?;
        self.plane = 1;
        let anchor = [
            anchor[0] + self.offset[0],
            anchor[1] + if state.focus == Focus::Name { 0. } else { 2. },
        ];
        self.offset = [((fade * 312) / 256) as f32, 0.];
        self.status_portrait(index)?;
        if let Some(previous) = state.previous.filter(|_| state.portrait_fade != 0) {
            self.plane = 2;
            self.opacity = backdrop_alpha(state.portrait_fade);
            self.status_portrait(previous)?;
            self.plane = 1;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        if menu.page == Page::Titles {
            return self.titles(menu);
        }
        Ok(anchor)
    }

    fn status_portrait(&mut self, index: usize) -> Result<()> {
        let portrait = self
            .spec
            .texture(MenuArt::PORTRAIT_TEXTURES.start + index)?;
        self.quad_role(
            DrawRole::Content,
            MaterialKey::Texture(PORTRAITS + index),
            [
                312.,
                0.,
                312. + portrait.width as f32,
                portrait.height as f32,
            ],
            [0., 0., portrait.width as f32, portrait.height as f32],
            [1.; 4],
        );
        Ok(())
    }

    fn status_member(&mut self, menu: &Menu, index: usize) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let member = &menu.party().members[index];
        let stats = member.stats(data);
        let title = data.title_text(u8::try_from(index + 1)?, member.title)?;
        self.text(&menu.full_name(index)?, [32., 68.], 20., WHITE)?;
        self.fitted_text(&title.name, [48., 94.], [18., 20.], 248., WHITE)?;
        self.text_size("Costume", [32., 116.], [12., 18.], GOLD)?;
        // Events may select an outfit without granting its title. Ownership
        // restricts the selector, but does not change the outfit's caption.
        let costume = data.titles[index]
            .iter()
            .position(|title| {
                title
                    .costume
                    .is_some_and(|costume| costume as u8 == member.costume)
            })
            .map(|title| {
                data.title_text((index + 1) as u8, (title + 1) as u8)
                    .map(|text| text.name.as_str())
            })
            .transpose()?
            .unwrap_or("Default");
        self.fitted_text(costume, [104., 116.], [16., 18.], 192., WHITE)?;
        if menu.status.details {
            self.status_traits(menu, member)?;
        } else {
            self.text_size("Lv", [32., 136.], [20.; 2], GOLD)?;
            self.number(u32::from(member.level), [136., 136.], [16., 20.], WHITE)?;
            self.technique([172., 134.], member.technique_balance)?;
            self.gauge(false, [32., 156.], [16., 20.], member.hp, stats.hp, Full)?;
            self.gauge(true, [32., 176.], [16., 20.], member.tp, stats.tp, Full)?;
            let next = self
                .experience
                .get(usize::from(member.level) + 1)
                .copied()
                .unwrap_or(member.experience)
                .saturating_sub(member.experience);
            for (label, value, y) in [
                ("EXP", member.experience, 196.),
                (data.label("next")?, next, 216.),
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
                    let y = 236. + row as f32 * 20.;
                    self.text_size(data.label(label)?, [x, y], [20.; 2], GOLD)?;
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
                let y = 318. + row as f32 * 18.;
                self.text_size(data.label(label)?, [32., y], [16., 18.], GOLD)?;
                if member.equipment[slot] != 0 {
                    let item = &data.items[usize::from(member.equipment[slot])];
                    let item_text = data.item_text(member.equipment[slot])?;
                    let icon = self
                        .spec
                        .sprite(Sprite::Items, usize::from(item.category.saturating_sub(1)))?;
                    self.sprite_rect(icon, [112., y, 130., y + 18.], [1.; 4]);
                    self.fitted_text(&item_text.name, [136., y], [16., 18.], 160., WHITE)?;
                }
            }
            self.scroll_arrow(SCROLL_DOWN, [284., 218.])?;
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
        self.text(data.label("element_attack")?, [24., 136.], 16., GOLD)?;
        self.text(data.label("element_defense")?, [24., 162.], 16., GOLD)?;
        if let Some(element) = traits.attack_element {
            self.sprite(
                self.spec.sprite(Sprite::Elements, element as usize)?,
                [104., 136.],
            );
        }
        let mut count: usize = 0;
        for (element, value) in traits
            .resistance
            .into_iter()
            .enumerate()
            .filter(|(_, v)| *v != 0)
        {
            let x = 104. + (count % 2) as f32 * 88.;
            let y = 162. + (count / 2) as f32 * 26.;
            self.sprite(self.spec.sprite(Sprite::Elements, element)?, [x, y]);
            let label = match value {
                ..=-2 => "weak",
                7.. => "absorb",
                5..=6 => "invalid",
                2..=4 => "reduce",
                _ => "",
            };
            if !label.is_empty() {
                self.text(data.label(label)?, [x + 24., y], 12., WHITE)?;
            }
            count += 1;
        }
        let mut y = 162. + count.max(1).div_ceil(2) as f32 * 26.;
        let status_text = data.status_text()?;
        let captions = member.equipment_captions(data)?;
        use resonance_events::party::StatBuff;
        let text = status_text.conditions.as_ref();
        let buffs = &member.queued_buffs;
        let conditions = [
            (
                member.ailments.poison.has_mild(),
                text.map(|text| &text.poison),
            ),
            (
                member.ailments.poison.has_severe(),
                text.map(|text| &text.severe_poison),
            ),
            (member.ailments.paralysis, text.map(|text| &text.paralysis)),
            (member.ailments.petrified, text.map(|text| &text.petrified)),
            (member.ailments.curse, text.map(|text| &text.curse)),
            (
                buffs.contains(&StatBuff::AttackUp),
                text.map(|text| &text.attack_up),
            ),
            (
                buffs.contains(&StatBuff::AttackDown),
                text.map(|text| &text.attack_down),
            ),
            (
                buffs.contains(&StatBuff::DefenseUp),
                text.map(|text| &text.defense_up),
            ),
            (
                buffs.contains(&StatBuff::DefenseDown),
                text.map(|text| &text.defense_down),
            ),
            (
                buffs.contains(&StatBuff::AccuracyUp),
                text.map(|text| &text.accuracy_up),
            ),
            (
                buffs.contains(&StatBuff::AccuracyDown),
                text.map(|text| &text.accuracy_down),
            ),
            (
                buffs.contains(&StatBuff::MagicAttackUp),
                text.map(|text| &text.magic_attack_up),
            ),
            (
                buffs.contains(&StatBuff::MagicAttackDown),
                text.map(|text| &text.magic_attack_down),
            ),
            (
                buffs.contains(&StatBuff::MagicDefenseUp),
                text.map(|text| &text.magic_defense_up),
            ),
            (member.knocked_out(), text.map(|text| &text.knockout)),
        ]
        .into_iter()
        .filter_map(|(active, text)| active.then_some(text));
        for label in conditions
            .map(|s| {
                s.context("Status condition captions were not prepared")
                    .map(|s| (s, 18.))
            })
            .chain(captions.iter().map(|id| {
                status_text
                    .equipment_effects
                    .get(*id)
                    .context("equipment effect caption was not prepared")
                    .map(|text| (text, 20.))
            }))
        {
            let (label, width) = label?;
            if label.is_empty() {
                continue;
            }
            // Keep complete rows inside the panel's bottom edge at 428.
            if y + 20. > 424. {
                break;
            }
            let mut width = width;
            while width > 1. && self.text_width(label, width)? > 280. {
                width -= 1.;
            }
            self.text_size(label, [24., y], [width, 20.], WHITE)?;
            y += 20.;
        }
        self.scroll_arrow(SCROLL_UP, [284., 138.])?;
        Ok(())
    }

    fn titles(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let costumes = menu.status.focus == Focus::Costume;
        let (choices, equipped) = menu.status_choices();
        let data = &menu.resources.as_ref().unwrap().data;
        let character = (menu.member_index() + 1) as u8;
        let first = menu.status.title_first;
        let fade = u32::from(255 - menu.status.title_opacity);
        let list_offset = -(((fade * 304) / 256) as f32);
        let card_offset = ((fade * 336) / 256) as f32;
        self.opacity = menu.status.title_opacity;
        self.plane = 3;
        self.offset = [list_offset, 0.];
        self.shade([16., 60., 296., 428.]);
        self.offset = [card_offset, 0.];
        self.shade([304., 60., 624., 428.]);
        self.plane = 4;
        self.offset = [list_offset, 0.];
        self.frame([16., 60., 280., 368.])?;
        self.text(
            if costumes { "Costumes" } else { "Titles" },
            [32., 72.],
            20.,
            GOLD,
        )?;
        for (row, &id) in choices.iter().skip(first).take(VISIBLE_TITLES).enumerate() {
            let y = 108. + row as f32 * 28.;
            let name = if id == 0 {
                "Default"
            } else {
                &data.title_text(character, id)?.name
            };
            if first + row == menu.status.row {
                self.highlight([32., y, 248., 24.], 255);
            }
            self.fitted_text(
                name,
                [32., y],
                [18., 24.],
                248.,
                if Some(id) == equipped { GOLD } else { WHITE },
            )?;
        }
        if first > 0 {
            self.scroll_arrow(SCROLL_UP, [280., 100.])?;
        }
        if first + VISIBLE_TITLES < choices.len() {
            self.scroll_arrow(SCROLL_DOWN, [280., 364.])?;
        }
        self.text(
            &format!("{} / {}", menu.status.row + 1, choices.len()),
            [32., 396.],
            16.,
            WHITE,
        )?;
        self.text("Equipped", [192., 396.], 12., GOLD)?;

        let selected = *choices
            .get(menu.status.row)
            .context("missing status selection")?;
        let title = if selected == 0 {
            None
        } else {
            Some(data.title_text(character, selected)?)
        };
        self.offset = [card_offset, 0.];
        self.frame([304., 60., 320., 368.])?;
        self.fitted_text(
            title.map_or("Default", |title| title.name.as_str()),
            [320., 72.],
            [20., 24.],
            288.,
            WHITE,
        )?;
        let description = title.map_or("Use the character's current story outfit.", |title| {
            title.description.as_str()
        });
        let description = description.replace('\n', " ");
        let mut cell = 16.;
        let mut lines = self.wrap_notice(&description, cell, 288.)?;
        while lines.len() > 4 && cell > 1. {
            cell -= 1.;
            lines = self.wrap_notice(&description, cell, 288.)?;
        }
        for (row, line) in lines.iter().take(4).enumerate() {
            self.text_size(line, [320., 104. + row as f32 * 18.], [cell, 18.], WHITE)?;
        }
        if !costumes {
            let rules = &data.titles[menu.member_index()];
            let current = &rules[usize::from(menu.member().title - 1)].growth;
            let growth = &rules[usize::from(selected - 1)].growth;
            self.text("Bonus per level", [320., 188.], 18., GOLD)?;
            self.text("Current", [468., 218.], 12., GOLD)?;
            self.text("Selected", [546., 218.], 12., GOLD)?;
            for (stat, label) in [
                "growth_hp",
                "growth_tp",
                "growth_strength",
                "growth_defense",
                "growth_intelligence",
                "growth_evasion",
                "growth_accuracy",
            ]
            .into_iter()
            .enumerate()
            {
                let y = 244. + stat as f32 * 24.;
                let color = match growth[stat].cmp(&current[stat]) {
                    std::cmp::Ordering::Greater => 4,
                    std::cmp::Ordering::Less => 2,
                    _ => DISABLED,
                };
                self.fitted_text(data.label(label)?, [320., y], [18., 20.], 132., GOLD)?;
                self.text_size(
                    &growth_bonus(stat, current[stat]),
                    [468., y],
                    [16., 20.],
                    WHITE,
                )?;
                self.text_size(
                    &growth_bonus(stat, growth[stat]),
                    [546., y],
                    [16., 20.],
                    color,
                )?;
            }
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok([
            32. + list_offset,
            116. + (menu.status.row - first) as f32 * 28.,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::growth_bonus;

    #[test]
    fn title_growth_uses_stat_points_including_fractional_bonuses() {
        assert_eq!(growth_bonus(0, 25), "+25");
        assert_eq!(growth_bonus(1, 3), "+3");
        for stat in 2..7 {
            assert_eq!(growth_bonus(stat, 0), "+0");
            assert_eq!(growth_bonus(stat, 4), "+0.4");
            assert_eq!(growth_bonus(stat, 10), "+1");
            assert_eq!(growth_bonus(stat, 255), "+25.5");
        }
    }
}

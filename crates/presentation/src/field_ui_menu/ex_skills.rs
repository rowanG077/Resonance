use super::*;
use resonance_content::menu_data::{ExTendency, MenuSpan};
use resonance_game::menu::ex_skills::{Description, Focus, VISIBLE_CHOICES};

impl Drawing<'_> {
    pub(super) fn ex_skills(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let state = &menu.ex_skills;
        let data = &menu.resources.as_ref().unwrap().data;
        let ex = &data.ex_skills;
        let member = menu.member();
        let gems = menu.ex_gems();
        let choices = menu.ex_choices();
        let gem_list = matches!(
            state.focus,
            Focus::Character | Focus::Gems | Focus::GemList | Focus::Confirm { .. }
        );
        let choosing_gem = matches!(state.focus, Focus::GemList | Focus::Confirm { .. });
        let fade = i32::from(state.transition.page_fade);
        let opacity = 255 - state.transition.page_fade;
        let left = -(fade * 296 / 256) as f32;
        let right = (fade * 378 / 256) as f32;
        let bottom = (fade * 112 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -(fade * 76 / 256) as f32];
        self.heading(&ex.labels["title"])?;
        self.offset = [0., bottom];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true);
        self.offset = [left, 0.];
        self.frame([16., 60., 246., 140.]);
        self.frame([16., 210., 246., 116.]);
        self.portrait(menu.member_index(), member.conditions, [208., 16.]);
        self.technique([280., 16.], member.technique_balance);
        self.opacity = if state.focus == Focus::Character {
            opacity
        } else {
            opacity >> 1
        };
        self.highlight([32., 64., 144., 24.], 255);
        self.opacity = opacity;
        self.text(
            menu.character_name(menu.member_index()),
            [32., 64.],
            24.,
            WHITE,
        )?;
        let slot_x = if gem_list { 32. } else { 112. };
        let slot_y = 92. + state.slot as f32 * 26.;
        if !matches!(state.focus, Focus::Character | Focus::Compounds) {
            self.opacity = if matches!(state.focus, Focus::Gems | Focus::Skills) {
                opacity
            } else {
                opacity >> 1
            };
            self.highlight(
                [slot_x, slot_y, if gem_list { 60. } else { 140. }, 24.],
                255,
            );
            self.opacity = opacity;
        }
        let preview = menu.ex_preview();
        let compounds = preview.active_compound_ex(ex, menu.member_index());
        for slot in 0..4 {
            let y = 92. + slot as f32 * 26.;
            let level = member.ex_gems[slot];
            let label = match level {
                0 => ex.labels["gem_empty"].clone(),
                5 => ex.labels["gem_max"].clone(),
                level => ex.labels["gem_level"].replace("%u", &level.to_string()),
            };
            self.text(&label, [32., y], 20., GOLD)?;
            let id = member.ex_skills[slot];
            if id == 0 {
                self.quad(
                    FONT,
                    [112., y + 10., 252., y + 13.],
                    [0.5; 4],
                    [0., 0., 0., 1.],
                );
                self.quad(FONT, [113., y + 11., 251., y + 12.], [0.5; 4], [1.; 4]);
            } else {
                let enabled = state.focus != Focus::Compounds
                    || ex.characters[menu.member_index()].compounds
                        [usize::from(compounds[state.compound])]
                    .required
                    .contains(&id);
                self.text(
                    &ex.skills[&id].name,
                    [112., y],
                    20.,
                    if enabled { WHITE } else { DISABLED },
                )?;
            }
        }
        let first = state.first;
        let start = first - usize::from(state.scroll > 0);
        let rows = VISIBLE_CHOICES + usize::from(state.scroll != 0);
        let scroll = scroll_offset(state.scroll, 25);
        let starts = self.vertex_counts();
        if gem_list {
            if choosing_gem {
                self.opacity = if state.focus == Focus::GemList {
                    opacity
                } else {
                    opacity >> 1
                };
                self.highlight(
                    [24., 218. + (state.gem - first) as f32 * 25., 224., 24.],
                    255,
                );
                self.opacity = opacity;
            }
            let party = menu.party();
            for (row, &level) in gems.iter().skip(start).take(rows).enumerate() {
                let id = ex.gem_items[usize::from(level - 1)];
                let item = &data.items[usize::from(id)];
                let y = 218. + row as f32 * 25. - scroll as f32;
                self.sprite(
                    self.spec.sprites.items[usize::from(item.category - 1)],
                    [24., y],
                );
                self.text(&item.name, [48., y], 16., WHITE)?;
                self.text(":", [196., y], 16., 5)?;
                self.number(u32::from(party.items[&id]), [244., y], [16., 24.], 5)?;
            }
        } else if matches!(state.focus, Focus::Skills | Focus::SkillList) {
            if state.focus == Focus::SkillList {
                self.highlight(
                    [48., 218. + (state.skill - first) as f32 * 25., 160., 24.],
                    255,
                );
            }
            for (row, id) in choices.iter().skip(start).take(rows).enumerate() {
                self.text(
                    &ex.skills[id].name,
                    [48., 218. + row as f32 * 25. - scroll as f32],
                    20.,
                    if member.ex_skills.contains(id) {
                        DISABLED
                    } else {
                        WHITE
                    },
                )?;
            }
        }
        self.clip_rows(starts, [218., 318.]);
        let list_count = if state.focus == Focus::Compounds {
            0
        } else if gem_list {
            gems.len()
        } else {
            choices.len()
        };
        if first > 0 {
            self.scroll_arrow(SCROLL_UP, [127., 202.]);
        }
        if first + VISIBLE_CHOICES < list_count {
            self.scroll_arrow(SCROLL_DOWN, [127., 312.]);
        }
        self.offset = [right, 0.];
        if !compounds.is_empty() {
            self.frame([270., 60., 140., compounds.len() as f32 * 28. + 6.]);
            for (row, &i) in compounds.iter().enumerate() {
                let y = 63. + row as f32 * 28.;
                if state.focus == Focus::Compounds && row == state.compound {
                    self.highlight([278., y, 128., 24.], 255);
                }
                self.text(
                    &ex.skills[&ex.characters[menu.member_index()].compounds[usize::from(i)].skill]
                        .name,
                    [278., y],
                    16.,
                    if member.recent_compound_ex_skills.contains(&i) {
                        4
                    } else {
                        WHITE
                    },
                )?;
            }
        }
        if choosing_gem {
            self.ex_preview_offset(state.preview_opacity);
            self.frame([420., 210., 200., 116.]);
            for (row, id) in choices.iter().enumerate() {
                let (position, size) = if choices.len() > VISIBLE_CHOICES {
                    (
                        [428. + (row / 8) as f32 * 96., 218. + (row % 8) as f32 * 13.],
                        [12.; 2],
                    )
                } else {
                    ([436., 218. + row as f32 * 25.], [20., 24.])
                };
                self.text_size(&ex.skills[id].name, position, size, WHITE)?;
            }
        }
        if state.focus == Focus::SkillList {
            self.ex_preview_offset(state.preview_opacity);
            self.frame([420., 60., 200., 266.]);
            let current = member.stats(data);
            let preview = preview.stats(data);
            for (row, (key, a, b)) in [
                ("hp", current.hp, preview.hp),
                ("tp", current.tp, preview.tp),
                (
                    if menu.member_index() == 0 {
                        "slash"
                    } else {
                        "attack"
                    },
                    current.slash,
                    preview.slash,
                ),
                ("thrust", current.thrust, preview.thrust),
                ("defense", current.defense, preview.defense),
                ("accuracy", current.accuracy, preview.accuracy),
                ("evasion", current.evasion, preview.evasion),
                ("intelligence", current.intelligence, preview.intelligence),
                ("luck", current.luck, preview.luck),
            ]
            .into_iter()
            .enumerate()
            {
                if row == 3 && menu.member_index() != 0 {
                    continue;
                }
                let y = 80. + row as f32 * 26.;
                self.text(&ex.labels[key], [428., y], 16., GOLD)?;
                self.number(a.into(), [526., y], [16., 24.], WHITE)?;
                self.text(&data.labels["stat_arrow"], [526., y], 16., 5)?;
                self.number(
                    b.into(),
                    [606., y],
                    [16., 24.],
                    match b.cmp(&a) {
                        std::cmp::Ordering::Greater => 4,
                        std::cmp::Ordering::Less => 2,
                        _ => WHITE,
                    },
                )?;
            }
        }
        if state.preview_previous_opacity != 0 {
            self.ex_preview_offset(state.preview_previous_opacity);
            self.frame([420., 210., 200., 116.]);
            for (row, id) in choices.iter().take(VISIBLE_CHOICES).enumerate() {
                self.text(
                    &ex.skills[id].name,
                    [436., 218. + row as f32 * 26.],
                    20.,
                    WHITE,
                )?;
            }
            if choices.len() > VISIBLE_CHOICES {
                self.opacity = opacity;
                self.scroll_arrow(SCROLL_DOWN, [504., 312.]);
            }
        }
        self.offset = [0., bottom];
        self.opacity = 255 - state.description_opacity;
        self.ex_description(menu, state.description_previous)?;
        self.opacity = crossfade_opacity(state.description_opacity, opacity);
        self.ex_description(menu, menu.ex_description())?;
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(match state.focus {
            Focus::Character => [32. + left, 72.],
            Focus::Gems | Focus::Skills => [slot_x + left, slot_y + 8.],
            Focus::GemList | Focus::Confirm { .. } => {
                [32. + left, 226. + (state.gem - first) as f32 * 25.]
            }
            Focus::SkillList => [48. + left, 226. + (state.skill - first) as f32 * 25.],
            Focus::Compounds => [278. + right, 71. + state.compound as f32 * 28.],
        })
    }

    fn ex_preview_offset(&mut self, opacity: u8) {
        self.opacity = opacity;
        self.offset = [(u16::from(255 - opacity) * 228 / 256) as f32, 0.];
    }

    fn ex_description(&mut self, menu: &Menu, description: Description) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let ex = &data.ex_skills;
        if let Description::Skill(id) = description {
            let skill = &ex.skills[&id];
            self.shadowed_text(&skill.name, [40., 336.], 28., 2.)?;
            let activation = &ex.activation_labels[&skill.activation];
            let x = 612. - self.text_width(activation, 16.)?;
            self.text(activation, [x, 340.], 16., 5)?;
            if let Some(tendency) = skill.tendency {
                let label = match tendency {
                    ExTendency::Technical => &data.status.technical_type,
                    ExTendency::Strike => &data.status.strike_type,
                };
                self.text(
                    label,
                    [
                        x - 8. - self.text_width(&data.status.strike_type, 16.)?,
                        340.,
                    ],
                    16.,
                    WHITE,
                )?;
            }
            for (row, line) in skill.description.lines.iter().enumerate() {
                let mut x = 72.;
                let y = 372. + row as f32 * 26.;
                for span in line {
                    match span {
                        MenuSpan::Text { text, color } => {
                            self.text(text, [x, y], 20., usize::from(*color))?;
                            x += self.text_width(text, 20.)?;
                        }
                        MenuSpan::Button { sprite } => {
                            self.button(usize::from(*sprite), [x, y]);
                            x += 24.;
                        }
                    }
                }
            }
        } else if let Description::Gem(level) = description {
            self.item_description(menu, ex.gem_items[usize::from(level - 1)])?;
        }
        Ok(())
    }

    pub(super) fn ex_cursors(&mut self, menu: &Menu, cursor: &resonance_content::font::UiTexture) {
        let state = &menu.ex_skills;
        let left = -(i32::from(state.transition.page_fade) * 296 / 256) as f32;
        let alpha = (255 - state.transition.page_fade) >> 1;
        if state.focus != Focus::Character {
            self.cursor([32. + left, 72.], cursor, alpha);
        }
        if matches!(
            state.focus,
            Focus::GemList | Focus::SkillList | Focus::Confirm { .. }
        ) {
            let x = if matches!(state.focus, Focus::GemList | Focus::Confirm { .. }) {
                32.
            } else {
                112.
            };
            self.cursor([x + left, 100. + state.slot as f32 * 26.], cursor, alpha);
        }
    }

    pub(super) fn ex_popup(&mut self, menu: &Menu) -> Result<Option<[f32; 2]>> {
        if let Focus::Confirm { yes, replacing } = menu.ex_skills.focus {
            let labels = &menu.resources.as_ref().unwrap().data.ex_skills.labels;
            return self.popup_layout(
                &labels[if replacing { "replace_gem" } else { "set_gem" }],
                Some(yes),
                menu.ex_skills.popup_opacity,
                Some([284., 72., 28.]),
            );
        }
        Ok(None)
    }
}

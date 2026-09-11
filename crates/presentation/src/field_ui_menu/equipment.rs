use super::*;
use resonance_game::menu::equipment::{Focus, LABELS, SLOTS, VISIBLE_EQUIPMENT};

impl Drawing<'_> {
    pub(super) fn equipment(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu
            .resources
            .as_ref()
            .context("equipment data was not prepared")?
            .data;
        let member = menu.member();
        let state = &menu.equipment;
        let focus = state.focus;
        let fade = u32::from(state.transition.page_fade);
        let opacity = 255 - state.transition.page_fade;
        let left = -((fade * 308 / 256) as f32);
        let right_offset = (fade * 326 / 256) as f32;
        let bottom = (fade * 120 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&self.spec.labels["equip"])?;
        self.offset = [0., -((fade * 40 / 256) as f32)];
        let mut right = 624.;
        if focus == Focus::Slots && state.slot != 0 && menu.equipment_item().is_some() {
            right = self.equipment_hint(menu, "remove", 9, right)? - 8.;
        }
        if matches!(focus, Focus::Slots | Focus::List | Focus::Character) {
            self.equipment_hint(
                menu,
                if focus == Focus::Character {
                    "optimal"
                } else {
                    "change_order"
                },
                11,
                right,
            )?;
        }
        self.offset = [0., bottom];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true);
        self.offset = [left, 0.];
        self.framed([16., 60., 296., 266.], true);
        self.highlight(
            [24., 60., 120., 20.],
            if focus == Focus::Character { 255 } else { 127 },
        );
        self.portrait(menu.member_index(), member.conditions, [176., 20.]);
        self.text_size(
            menu.character_name(menu.member_index()),
            [32., 60.],
            [20.; 2],
            WHITE,
        )?;
        let [hp, tp] = member.maximum_vitals();
        self.gauge(false, [32., 86.], [16., 24.], member.hp, hp, Compact)?;
        self.gauge(true, [144., 86.], [16., 24.], member.tp, tp, Compact)?;
        let mut anchor = [32. + left, 68.];
        if focus == Focus::List {
            let slot = SLOTS[menu.equipment.slot];
            self.text_size(
                &data.labels[LABELS[menu.equipment.slot]],
                [24., 112.],
                [20.; 2],
                GOLD,
            )?;
            self.equipment_name(menu, member.equipment[slot], [104., 112.])?;
            if let Some(id) = menu.equipment_item() {
                let stats = member.stats(data);
                let preview = member.preview_equipment(data, slot, id);
                for (row, (label, current, next)) in [
                    (
                        if menu.member_index() == 0 {
                            "slash"
                        } else {
                            "attack"
                        },
                        stats.slash,
                        preview.slash,
                    ),
                    ("thrust", stats.thrust, preview.thrust),
                    ("defense", stats.defense, preview.defense),
                    ("accuracy", stats.accuracy, preview.accuracy),
                    ("evasion", stats.evasion, preview.evasion),
                    ("intelligence", stats.intelligence, preview.intelligence),
                    ("luck", stats.luck, preview.luck),
                ]
                .into_iter()
                .enumerate()
                {
                    if row == 1 && menu.member_index() != 0 {
                        continue;
                    }
                    let y = 134. + row as f32 * 22.;
                    self.text_size(&data.labels[label], [40., y], [20.; 2], GOLD)?;
                    self.number(u32::from(current), [160., y], [16., 20.], WHITE)?;
                    self.text_size(&data.labels["stat_arrow"], [160., y], [20.; 2], 5)?;
                    self.number(
                        u32::from(next),
                        [244., y],
                        [16., 20.],
                        match next.cmp(&current) {
                            std::cmp::Ordering::Greater => 4,
                            std::cmp::Ordering::Less => 2,
                            _ => WHITE,
                        },
                    )?;
                }
            }
        } else {
            for (row, (slot, label)) in SLOTS.into_iter().zip(LABELS).enumerate() {
                let y = 116. + row as f32 * 24.;
                if focus == Focus::Slots && row == menu.equipment.slot {
                    self.highlight([24., y, 264., 20.], 255);
                    anchor = [32. + left, y + 4.];
                }
                self.text_size(&data.labels[label], [24., y], [20.; 2], GOLD)?;
                self.equipment_name(menu, member.equipment[slot], [104., y])?;
            }
        }
        self.offset = [right_offset, 0.];
        self.framed([322., 60., 298., 266.], true);
        if matches!(focus, Focus::Slots | Focus::List) {
            let items = menu.equipment_items();
            let row = if focus == Focus::List {
                (menu.equipment.row + 1).to_string()
            } else {
                "-".into()
            };
            self.text_size(
                &format!("{row}/{}", items.len()),
                [330., 60.],
                [13.; 2],
                WHITE,
            )?;
            let label = &data.labels[if menu.equipment.by_parameter {
                "parameter"
            } else {
                "alphabetical"
            }];
            self.text_size(
                label,
                [604. - self.text_width(label, 13.)?, 60.],
                [13.; 2],
                4,
            )?;
            if focus == Focus::List {
                let y = 78. + (state.row - state.first) as f32 * 23.;
                self.highlight([350., y, 240., 20.], 255);
                anchor = [350. + right_offset, y + 4.];
            }
            let starts = self.vertex_counts();
            let scroll = scroll_offset(state.scroll, 23);
            for (row, &id) in items
                .iter()
                .skip(state.first - usize::from(state.scroll > 0))
                .take(VISIBLE_EQUIPMENT + usize::from(state.scroll != 0))
                .enumerate()
            {
                let y = 78. + row as f32 * 23. - scroll as f32;
                self.equipment_name(menu, id, [350., y])?;
                let count = menu.party().items[&id];
                self.text_size(&format!(":{count:2}"), [558., y], [16., 20.], 5)?;
            }
            self.clip_rows(starts, [78., 283.]);
            if menu.equipment.first != 0 {
                self.scroll_arrow(SCROLL_UP, [459., 58.]);
            }
            if menu.equipment.first + VISIBLE_EQUIPMENT < items.len() {
                self.scroll_arrow(SCROLL_DOWN, [459., 277.]);
            }
        }
        self.offset = [0., bottom];
        self.opacity = 255 - state.description_opacity;
        if let Some(id) = state.description_previous {
            self.item_description(menu, id)?;
        }
        self.opacity = crossfade_opacity(state.description_opacity, opacity);
        if let Some(id) = menu.equipment_item() {
            self.item_description(menu, id)?;
        }
        self.offset = [0.; 2];
        self.opacity = opacity;
        if let Focus::Optimal { thrust } = focus {
            let label = &data.labels["optimal_selection"];
            self.text(
                label,
                [(320. - self.text_width(label, 24.)? / 2.).floor(), 340.],
                24.,
                WHITE,
            )?;
            let x = (320. - self.text_width(&data.labels["optimal_slash"], 24.)? / 2.).floor();
            for (index, key) in ["optimal_slash", "optimal_thrust"].into_iter().enumerate() {
                let y = 372. + index as f32 * 28.;
                let label = &data.labels[key];
                if thrust == (index == 1) {
                    self.highlight([x, y, self.text_width(label, 24.)?, 24.], 255);
                    anchor = [x, y + 8.];
                }
                self.text(label, [x, y], 24., WHITE)?;
            }
        }
        self.opacity = 255;
        Ok(anchor)
    }

    fn equipment_name(&mut self, menu: &Menu, id: u16, [x, y]: [f32; 2]) -> Result<()> {
        if id != 0 {
            let item = &menu.resources.as_ref().unwrap().data.items[usize::from(id)];
            self.sprite_rect(
                self.spec.sprites.items[usize::from(item.category - 1)],
                [x, y, x + 24., y + 24.],
                [1.; 4],
            );
            self.text_size(&item.name, [x + 24., y], [16., 20.], WHITE)?;
        }
        Ok(())
    }

    fn equipment_hint(&mut self, menu: &Menu, key: &str, sprite: usize, right: f32) -> Result<f32> {
        let label = &menu.resources.as_ref().unwrap().data.labels[key];
        let x = right - self.text_width(label, 20.)? - 24.;
        self.sprite(
            self.spec.sprites.buttons[sprite + usize::from(self.tick % 40 < 20)],
            [x, 20.],
        );
        self.text_size(label, [x + 24., 20.], [20.; 2], WHITE)?;
        Ok(x)
    }
}

use super::*;
use resonance_content::menu_data::{ItemAttention, MenuSpan};
use resonance_game::menu::items::{Description, Focus, VISIBLE_ITEMS};

fn target_panel(menu: &Menu) -> (f32, f32) {
    let count = menu.party().formation.len();
    let width = if count > 4 { 384 } else { 192 };
    let x = 620 - width;
    (
        (x + (648 - x) * u32::from(255 - menu.inventory.target_opacity) / 256) as f32,
        width as f32,
    )
}

pub(super) fn target_cursor(menu: &Menu, slot: usize) -> [f32; 2] {
    [
        target_panel(menu).0 + 16. + (slot / 4) as f32 * 188.,
        110. + (slot % 4) as f32 * 65.,
    ]
}

impl Drawing<'_> {
    pub(super) fn items(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let resources = menu
            .resources
            .as_ref()
            .context("inventory data was not prepared")?;
        let data = &resources.data;
        let party = menu.party();
        let inventory = &menu.inventory;
        let items = menu.inventory_items();
        let fade = u32::from(inventory.page_fade);
        let opacity = 255 - inventory.page_fade;
        let list_offset = (fade * 628 / 256) as f32;
        let description_offset = (fade * 120 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&self.spec.labels["items"])?;
        self.offset = [list_offset, 0.];
        self.framed([16., 60., 604., 266.], true);
        self.offset = [0., description_offset];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true);
        self.offset = [list_offset, 0.];
        self.text_size(
            &if items.is_empty() {
                "---/---".into()
            } else if inventory.focus == Focus::Categories {
                format!("-/{}", items.len())
            } else {
                format!("{}/{}", inventory.row + 1, items.len())
            },
            [32., 64.],
            [16.; 2],
            WHITE,
        )?;
        let mut anchor = [330. + inventory.category as f32 * 32., 68.];
        for (index, &rect) in self.spec.sprites.item_tabs.iter().enumerate() {
            let selected = index == inventory.category;
            let x = 330. + index as f32 * 32.;
            let y = if selected { 52. } else { 56. };
            let tint = if selected { 1. } else { 192. / 255. };
            self.sprite_rect(rect, [x, y, x + 32., y + 32.], [tint, tint, tint, 1.]);
        }
        if inventory.focus != Focus::Categories {
            let cell = inventory.row - inventory.first;
            let x = 48. + (cell % 2) as f32 * 296.;
            let y = 88. + (cell / 2) as f32 * 26.;
            self.highlight(
                [x, y, 240., 24.],
                if matches!(inventory.focus, Focus::Target | Focus::Discard(_)) {
                    127
                } else {
                    255
                },
            );
            anchor = [x, y + 8.];
        }
        let starts = self.vertex_counts();
        let first = inventory.first - usize::from(inventory.scroll > 0) * 2;
        let scroll_offset = scroll_offset(inventory.scroll, 26);
        for (row, &id) in items
            .iter()
            .skip(first)
            .take(VISIBLE_ITEMS + usize::from(inventory.scroll != 0) * 2)
            .enumerate()
        {
            let item = &data.items[usize::from(id)];
            let x = 48. + (row % 2) as f32 * 296.;
            let y = 88. + (row / 2) as f32 * 26. - scroll_offset as f32;
            if item.category != 0 {
                let rect = self.spec.sprites.items[usize::from(item.category - 1)];
                let urgent = party.formation.iter().any(|&id| {
                    let member = &party.members[usize::from(id - 1)];
                    let stats = member.stats(data);
                    let low_hp = u32::from(member.hp) <= (u32::from(stats.hp) + 2) / 4;
                    let low_tp = u32::from(member.tp) <= (u32::from(stats.tp) + 2) / 4;
                    const CURABLE_CONDITIONS: u32 = 0xfe3;
                    match item.attention {
                        Some(ItemAttention::LowHp) => low_hp,
                        Some(ItemAttention::LowTp) => low_tp,
                        Some(ItemAttention::LowVitals) => low_hp || low_tp,
                        Some(ItemAttention::Knockout) => member.knocked_out(),
                        Some(ItemAttention::Ailment) => member.conditions & CURABLE_CONDITIONS != 0,
                        None => false,
                    }
                });
                self.sprite_rect(
                    rect,
                    [x, y, x + 24., y + 24.],
                    [
                        1.,
                        1.,
                        1.,
                        if urgent {
                            blink_opacity(self.tick, 60, 15)
                        } else {
                            1.
                        },
                    ],
                );
            }
            self.text(
                &item.name,
                [x + 24., y],
                16.,
                if party.recent_items.contains(&id) {
                    4
                } else {
                    WHITE
                },
            )?;
            if item.category != 45 {
                self.text(&format!(":{:2}", party.items[&id]), [x + 220., y], 16., 5)?;
            }
        }
        self.clip_rows(starts, [88., 322.]);
        if inventory.first > 0 {
            self.scroll_arrow(SCROLL_UP, [306., 72.]);
        }
        if inventory.first + VISIBLE_ITEMS < items.len() {
            self.scroll_arrow(SCROLL_DOWN, [306., 314.]);
        }
        anchor[0] += list_offset;
        if let Focus::Transform(bottle) = inventory.focus {
            self.item_transform_prompt(menu, bottle, list_offset)?;
        }
        self.offset = [0., description_offset];
        if let Some(notice) = &inventory.notice {
            self.text(
                notice,
                [(320. - self.text_width(notice, 24.)? / 2.).floor(), 364.],
                24.,
                WHITE,
            )?;
            self.offset = [0.; 2];
            self.opacity = 255;
            return Ok(anchor);
        }
        let description = menu.item_description();
        if description != Description::None {
            self.opacity = 255 - inventory.description_opacity;
            self.item_description_content(menu, inventory.description_previous)?;
            self.opacity = crossfade_opacity(inventory.description_opacity, opacity);
            self.item_description_content(menu, description)?;
            self.opacity = opacity;
        }
        if let Some(&id) = items.get(inventory.row) {
            let item = &data.items[usize::from(id)];
            match inventory.focus {
                Focus::Categories => {}
                Focus::Target => {
                    self.offset = [0.; 2];
                    self.item_target(menu, id)?;
                    anchor = target_cursor(menu, inventory.target);
                }
                Focus::Discard(yes) => {
                    let label = data.labels["confirm_discard"].replace("%s", &item.name);
                    self.text(
                        &label,
                        [320. - self.text_width(&label, 24.)? / 2., 340.],
                        24.,
                        WHITE,
                    )?;
                    for (index, key) in ["yes", "no"].into_iter().enumerate() {
                        let label = &self.spec.labels[key];
                        let x = 320. - self.text_width(label, 24.)? / 2.;
                        let y = 372. + index as f32 * 28.;
                        if yes == (index == 0) {
                            self.highlight([x, y, self.text_width(label, 24.)?, 24.], 255);
                            anchor = [x, y + 8.];
                        }
                        self.text(label, [x, y], 24., WHITE)?;
                    }
                }
                _ => {
                    if inventory.focus == Focus::List && item.price != 0 {
                        self.offset = [0., -((fade * 56 / 256) as f32)];
                        let label = &data.labels["discard"];
                        self.text(
                            label,
                            [624. - self.text_width(label, 24.)?, 24.],
                            24.,
                            WHITE,
                        )?;
                        self.sprite(
                            self.spec.sprites.buttons[if self.tick % 40 < 20 { 10 } else { 9 }],
                            [600. - self.text_width(label, 24.)?, 24.],
                        );
                    }
                }
            }
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }

    fn item_transform_prompt(&mut self, menu: &Menu, bottle: u16, list_offset: f32) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let label = &data.labels["select_item"];
        let width = self.text_width(label, 24.)?;
        let opacity = menu.inventory.target_opacity;
        let y = 20. - (u32::from(255 - opacity) * 28 / 256) as f32;
        let x = (320. - width / 2.).floor();
        self.offset = [0.; 2];
        self.opacity = opacity;
        self.frame([x - 4., y - 4., width + 8., 32.]);
        self.text(label, [x, y], 24., WHITE)?;
        self.offset = [list_offset, 0.];
        let count = menu.party().items.get(&bottle).copied().unwrap_or(0);
        let mut x = 176.;
        for span in &data.item_bottle_count.lines[0] {
            let MenuSpan::Text { text, color } = span else {
                anyhow::bail!("button in bottle count")
            };
            let text = text.replace("%d", &count.to_string());
            self.text(&text, [x, 60.], 16., usize::from(*color))?;
            x += self.text_width(&text, 16.)?;
        }
        self.opacity = 255 - menu.inventory.page_fade;
        Ok(())
    }

    pub(super) fn item_description_content(
        &mut self,
        menu: &Menu,
        description: Description,
    ) -> Result<()> {
        match description {
            Description::None => Ok(()),
            Description::Item(id) => self.item_description(menu, id),
            Description::Category(category) => {
                let label = &menu.resources.as_ref().unwrap().data.inventory_categories[category];
                self.text(
                    label,
                    [(306. - self.text_width(label, 24.)? / 2.).trunc(), 368.],
                    24.,
                    WHITE,
                )
            }
        }
    }

    pub(super) fn item_description(&mut self, menu: &Menu, id: u16) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let item = &data.items[usize::from(id)];
        self.sprite_rect(
            self.spec.sprites.item_images[usize::from(id)],
            [36., 348., 100., 412.],
            [1.; 4],
        );
        self.text(&item.description, [120., 344.], 20., WHITE)?;
        self.text(&item.details, [120., 370.], 20., WHITE)?;
        let category = &data.item_categories[usize::from(item.category)];
        self.text(
            category,
            [612. - self.text_width(category, 20.)?, 344.],
            20.,
            5,
        )
    }

    fn item_target(&mut self, menu: &Menu, id: u16) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let party = menu.party();
        let all = menu.inventory.target_all;
        let slot = if all {
            menu.inventory.target_preview
        } else {
            menu.inventory.target
        };
        let member_index = usize::from(party.formation[slot] - 1);
        let target = &party.members[member_index];
        let stats = target.stats(data);
        let opacity = menu.inventory.target_opacity;
        let base_opacity = self.opacity;
        self.opacity = opacity;
        self.portrait(member_index, target.conditions, [32., 350.]);
        self.text(menu.character_name(member_index), [96., 336.], 24., WHITE)?;
        self.opacity = base_opacity;
        self.gauge(false, [248., 336.], [16., 24.], target.hp, stats.hp, Full)?;
        self.gauge(true, [432., 336.], [16., 24.], target.tp, stats.tp, Full)?;
        let definition = &menu.resources.as_ref().unwrap().session.items[usize::from(id)];
        let equipment = definition.equipment_kind.is_some();
        let preview = definition
            .equipment_kind
            .and_then(|kind| target.preferred_equipment_slot(kind))
            .filter(|_| definition.allowed_characters & (1 << member_index) != 0)
            .map(|slot| target.preview_equipment(data, slot, id));
        let values = if let Some(next) = preview {
            [
                (
                    if member_index == 0 {
                        "item_slash"
                    } else {
                        "item_attack"
                    },
                    stats.slash,
                    next.slash,
                ),
                ("item_thrust", stats.thrust, next.thrust),
                ("item_defense", stats.defense, next.defense),
                ("item_accuracy", stats.accuracy, next.accuracy),
                ("item_evasion", stats.evasion, next.evasion),
                ("item_intelligence", stats.intelligence, next.intelligence),
                ("item_luck", stats.luck, next.luck),
                ("", 0, 0),
            ]
        } else {
            [
                ("strength", stats.strength),
                (
                    if member_index == 0 {
                        "item_slash"
                    } else {
                        "item_attack"
                    },
                    stats.slash,
                ),
                ("item_thrust", stats.thrust),
                ("defense", stats.defense),
                ("luck", stats.luck),
                ("accuracy", stats.accuracy),
                ("evasion", stats.evasion),
                ("intelligence", stats.intelligence),
            ]
            .map(|(key, value)| (key, value, value))
        };
        for (index, (key, current, value)) in values
            .into_iter()
            .take(if equipment { 7 } else { 8 })
            .enumerate()
        {
            if equipment && preview.is_none()
                || index == if equipment { 1 } else { 2 } && member_index != 0
            {
                continue;
            }
            let cell = index + usize::from(equipment);
            let x = 104. + (cell % 4) as f32 * 128.;
            let y = 364. + (cell / 4) as f32 * 28.;
            self.opacity = opacity;
            self.text(
                &data.labels[key],
                [x, y],
                if equipment { 16. } else { 24. },
                GOLD,
            )?;
            let color = match value.cmp(&current) {
                std::cmp::Ordering::Greater => 4,
                std::cmp::Ordering::Less => 2,
                _ => WHITE,
            };
            self.opacity = if equipment { opacity } else { base_opacity };
            self.number(u32::from(value), [x + 112., y], [16., 24.], color)?;
        }
        if equipment && preview.is_none() {
            // Empty slots retain their category icon in the equipment summary.
            const SLOT_CATEGORIES: [usize; 6] = [20, 23, 27, 31, 35, 35];
            for (row, (slot, category)) in resonance_game::menu::equipment::SLOTS
                .into_iter()
                .zip(SLOT_CATEGORIES)
                .enumerate()
            {
                let x = 104. + (row % 3) as f32 * 162.;
                let y = 364. + (row / 3) as f32 * 28.;
                self.sprite_rect(
                    self.spec.sprites.items[category - 1],
                    [x, y, x + 24., y + 24.],
                    [1.; 4],
                );
                if target.equipment[slot] != 0 {
                    self.text(
                        &data.items[usize::from(target.equipment[slot])].name,
                        [x + 28., y],
                        13.,
                        WHITE,
                    )?;
                }
            }
        }
        self.opacity = opacity;
        self.plane = 2;
        let (panel_x, panel_width) = target_panel(menu);
        self.shade([panel_x, 60., panel_x + panel_width, 324.]);
        self.plane = 3;
        let label = &data.labels[if menu.inventory.target_equipment {
            "equip_target"
        } else {
            "select_target"
        }];
        let spans = &data.item_group_prompt.lines[0];
        let width = if all {
            spans
                .iter()
                .map(|span| match span {
                    MenuSpan::Text { text, .. } => self.text_width(text, 24.),
                    MenuSpan::Button { .. } => Ok(24.),
                })
                .sum::<Result<f32>>()?
        } else {
            self.text_width(label, 24.)?
        };
        let y = 20. - (u32::from(255 - opacity) * 28 / 256) as f32;
        self.frame([316. - width / 2., y - 4., width + 8., 32.]);
        let mut x = 320. - width / 2.;
        if all {
            for span in spans {
                match span {
                    MenuSpan::Text { text, color } => {
                        self.text(text, [x, y], 24., usize::from(*color))?;
                        x += self.text_width(text, 24.)?;
                    }
                    MenuSpan::Button { sprite } => {
                        self.button(usize::from(*sprite), [x, y]);
                        x += 24.;
                    }
                }
            }
        } else {
            self.text(label, [x, y], 24., WHITE)?;
        }
        self.frame([panel_x, 60., panel_width, 264.]);
        for (index, &member) in party.formation.iter().enumerate() {
            let character = &party.members[usize::from(member - 1)];
            let stats = character.stats(data);
            let x = panel_x + (index / 4) as f32 * 188.;
            let y = 62. + (index % 4) as f32 * 65.;
            if index == menu.inventory.target || all {
                self.highlight([x + 16., y, 160., 64.], 255);
            }
            self.text(&(index + 1).to_string(), [x, y + 20.], 16., WHITE)?;
            self.portrait(usize::from(member - 1), character.conditions, [x + 16., y]);
            self.item_equipment_marker(menu, usize::from(member - 1), id, [x + 56., y + 40.]);
            self.gauge(
                false,
                [x + 80., y + 8.],
                [16., 16.],
                character.hp,
                stats.hp,
                Stacked,
            )?;
            self.gauge(
                true,
                [x + 80., y + 40.],
                [16., 16.],
                character.tp,
                stats.tp,
                Stacked,
            )?;
        }
        self.opacity = base_opacity;
        Ok(())
    }

    fn item_equipment_marker(&mut self, menu: &Menu, member: usize, item: u16, at: [f32; 2]) {
        let resources = menu.resources.as_ref().unwrap();
        let definition = &resources.session.items[usize::from(item)];
        let Some(kind) = definition.equipment_kind else {
            return;
        };
        let character = &menu.party().members[member];
        let (rect, duration, color) = if definition.allowed_characters & (1 << member) == 0 {
            (self.spec.sprites.tech_ranks[1], 20, WHITE)
        } else if character.equipment.contains(&item) {
            (self.spec.sprites.equipment_markers[7], 20, 4)
        } else if kind < 4 {
            let slot = character.preferred_equipment_slot(kind).unwrap();
            let stat = if kind == 0 { 0 } else { 2 };
            let old = &resources.data.items[usize::from(character.equipment[slot])];
            let new = &resources.data.items[usize::from(item)];
            let frame = match self.tick % 60 {
                9..15 => 0,
                15..21 => 1,
                _ => 2,
            };
            let (marker, duration) = match new.equipment_stats[stat].cmp(&old.equipment_stats[stat])
            {
                std::cmp::Ordering::Greater => (frame, 18),
                std::cmp::Ordering::Less => (frame + 3, 18),
                std::cmp::Ordering::Equal => (6, 20),
            };
            (self.spec.sprites.equipment_markers[marker], duration, WHITE)
        } else {
            return;
        };
        let mut tint = rgba(self.spec.palette[color]);
        tint[3] *= blink_opacity(self.tick, 60, duration);
        self.sprite_color(rect, at, tint);
    }
}

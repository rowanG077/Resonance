use super::*;
use resonance_content::menu_data::{Element, Ingredient};
use resonance_events::party::Party;
use resonance_game::{
    field::shop::{Choice, Focus, Shop, VISIBLE_ITEMS},
    menu::items::Description,
};

impl Drawing<'_> {
    pub(super) fn shop(
        &mut self,
        shop: &Shop,
        party: &Party,
        cursor: &resonance_content::font::UiTexture,
    ) -> Result<[f32; 2]> {
        let data = &shop.resources.data;
        let fade = u32::from(shop.fade);
        let opacity = 255 - shop.fade;
        let left = -((fade * 426 / 256) as f32);
        let right = (fade * 220 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.shop_heading(&data.world_map.shops[usize::from(shop.id)].name)?;
        self.offset = [left, 0.];
        self.frame([16., 60., 402., 226.]);
        self.frame([16., 296., 402., 30.]);
        self.text(&self.spec.labels["shop_gald"], [24., 298.], 14., GOLD)?;
        self.currency(party.gald, [192., 298.], [14., 24.])?;
        if shop.focus != Focus::Root {
            self.text(&self.spec.labels["shop_total"], [214., 298.], 14., GOLD)?;
            self.currency(shop.total(party), [396., 298.], [14., 24.])?;
        }
        let mut anchor = if shop.focus == Focus::Root {
            let mut x = 48.;
            let mut selected = [x + left, 72.];
            for (choice, key) in
                Choice::ALL
                    .into_iter()
                    .zip(["shop_buy", "shop_sell", "shop_equip", "shop_exit"])
            {
                let label = &self.spec.labels[key];
                let width = self.text_width(label, 24.)?;
                if choice == shop.choice {
                    self.highlight([x, 64., width, 24.], 255);
                    selected = [x + left, 72.];
                }
                self.text(label, [x, 64.], 24., WHITE)?;
                x += width + 20.;
            }
            for (row, (button, key)) in [
                (33, "shop_select"),
                (36, "shop_add"),
                (35, "shop_reduce"),
                (6, "shop_ok"),
                (12, "shop_status"),
                (18, "shop_info"),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 100. + row as f32 * 30.;
                self.button(button, [48., y]);
                if row == 0 {
                    self.button(34, [72., y]);
                }
                self.text(&self.spec.labels[key], [168., y], 24., WHITE)?;
            }
            selected
        } else {
            self.offset = [0.; 2];
            self.shop_rows(shop, party, cursor)?
        };
        self.offset = [right, 0.];
        self.frame([428., 60., 192., 266.]);
        if shop.focus == Focus::Equipment {
            anchor = self.shop_equipment(shop, party)?;
            anchor[0] += right;
        } else if shop.focus == Focus::Items
            && shop
                .selected_item()
                .is_some_and(|id| (7..=12).contains(&data.items[usize::from(id)].category))
        {
            self.shop_recipes(shop, party)?;
        } else {
            for (slot, &id) in party.formation.iter().enumerate() {
                let member = usize::from(id - 1);
                let character = &party.members[member];
                let x = 428. + (slot / 4) as f32 * 96.;
                let y = 62. + (slot % 4) as f32 * 65.;
                if shop.focus == Focus::Characters && slot == shop.character {
                    self.highlight([x + 16., y, 64., 64.], 255);
                    anchor = [x + 8. + right, y + 48.];
                }
                self.text(&(slot + 1).to_string(), [x, y + 20.], 16., WHITE)?;
                self.portrait(member, character.conditions, [x + 16., y]);
                if matches!(
                    shop.focus,
                    Focus::Items | Focus::Characters | Focus::Confirm { .. }
                ) && let Some(item) = shop.selected_item()
                {
                    self.equipment_marker_data(
                        &shop.resources,
                        party,
                        member,
                        item,
                        [x + 56., y + 40.],
                    );
                }
            }
        }
        self.offset = [0., (fade * 120 / 256) as f32];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true);
        match shop.focus {
            Focus::Root => {}
            Focus::Confirm { yes } => {
                self.highlight([284., if yes { 372. } else { 400. }, 72., 24.], 255);
                for (key, y) in [
                    ("shop_confirm", 336.),
                    ("shop_yes", 372.),
                    ("shop_no", 400.),
                ] {
                    let label = &self.spec.labels[key];
                    self.text(
                        label,
                        [((640. - self.text_width(label, 24.)?) / 2.).floor(), y],
                        24.,
                        WHITE,
                    )?;
                }
                anchor = [284., if yes { 380. } else { 408. } + self.offset[1]];
            }
            Focus::Empty => {
                let label = &self.spec.labels["shop_empty"];
                self.text(
                    label,
                    [
                        16. + ((604. - self.text_width(label, 24.)?) / 2.).floor(),
                        370.,
                    ],
                    24.,
                    WHITE,
                )?;
            }
            _ => {
                for (description, alpha) in [
                    (shop.description_previous, 255 - shop.description_opacity),
                    (
                        shop.description(),
                        crossfade_opacity(shop.description_opacity, opacity),
                    ),
                ] {
                    self.opacity = alpha;
                    match description {
                        Description::None => {}
                        Description::Category(category) => {
                            let label = &data.inventory_categories[category];
                            self.text(
                                label,
                                [(306. - self.text_width(label, 24.)? / 2.).trunc(), 368.],
                                24.,
                                WHITE,
                            )?;
                        }
                        Description::Item(id)
                            if shop.statistics
                                && shop.resources.session.items[usize::from(id)]
                                    .equipment_kind
                                    .is_some() =>
                        {
                            self.shop_item_statistics(shop, id)?
                        }
                        Description::Item(id) => self.item_description_data(data, id)?,
                    }
                }
            }
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }

    fn shop_heading(&mut self, text: &str) -> Result<()> {
        let x = if let Some(index) = self.window().heading {
            self.quad(
                texture_layer(index),
                [16., 16., 48., 48.],
                [0., 0., 32., 32.],
                [1.; 4],
            );
            48.
        } else {
            let width = self.text_width(text, 24.)?;
            self.quad(
                FONT,
                [7., 40., 25. + width, 49.],
                [0.5; 4],
                rgba([0, 0, 0, 128]),
            );
            self.quad(
                FONT,
                [8., 41., 24. + width, 48.],
                [0.5; 4],
                rgba(self.menu_color()),
            );
            16.
        };
        let opacity = self.opacity;
        self.opacity >>= 1;
        self.text_size(text, [x + 4., 20.], [24., 32.], 0)?;
        self.opacity = opacity;
        self.text_size(text, [x, 16.], [24., 32.], WHITE)
    }

    fn shop_rows(
        &mut self,
        shop: &Shop,
        party: &Party,
        cursor: &resonance_content::font::UiTexture,
    ) -> Result<[f32; 2]> {
        let category = matches!(shop.focus, Focus::Categories | Focus::Empty);
        let y = 88. + (shop.row - shop.first) as f32 * 28.;
        let mut anchor = [32., y + 8.];
        if !category && !shop.rows.is_empty() {
            self.highlight(
                [32., y, 364., 24.],
                if shop.focus == Focus::Items { 255 } else { 127 },
            );
            if shop.focus != Focus::Items {
                self.cursor(anchor, cursor, 127);
            }
        }
        let starts = self.vertex_counts();
        let first = shop.first.saturating_sub(usize::from(shop.scroll > 0));
        let offset = scroll_offset(shop.scroll, 28);
        for (row, item) in shop
            .rows
            .iter()
            .skip(first)
            .take(VISIBLE_ITEMS + usize::from(shop.scroll != 0))
            .enumerate()
        {
            let data = &shop.resources.data.items[usize::from(item.id)];
            let y = 88. + row as f32 * 28. - offset as f32;
            self.sprite_rect(
                self.spec.sprites.items[usize::from(data.category - 1)],
                [32., y, 56., y + 24.],
                [1.; 4],
            );
            self.text(&data.name, [56., y], 14., WHITE)?;
            let mut price = shop.unit_price(item.id, party).to_string();
            for at in (1..price.len())
                .rev()
                .filter(|&i| (price.len() - i).is_multiple_of(3))
                .collect::<Vec<_>>()
            {
                price.insert(at, ',');
            }
            self.text(
                &price,
                [
                    310. - (u32::from(shop.fade) * 426 / 256) as f32
                        - self.text_width(&price, 14.)?,
                    y,
                ],
                14.,
                WHITE,
            )?;
            self.text(&format!("×{:2}", item.quantity), [312., y], 14., WHITE)?;
            self.text(
                &format!("{:2}", party.items.get(&item.id).copied().unwrap_or(0)),
                [368., y],
                14.,
                5,
            )?;
        }
        self.clip_rows(starts, [88., 284.]);
        if shop.first > 0 {
            self.scroll_arrow(SCROLL_UP, [160., 72.]);
        }
        if shop.first + VISIBLE_ITEMS < shop.rows.len() {
            self.scroll_arrow(SCROLL_DOWN, [160., 276.]);
        }
        self.frame([564., 18., 56., 32.]);
        self.text(
            &self.spec.labels[if shop.choice == Choice::Buy {
                "shop_buy"
            } else {
                "shop_sell"
            }],
            [568., 22.],
            20.,
            WHITE,
        )?;
        self.text_size(
            &if category {
                format!("-/{}", shop.rows.len())
            } else {
                format!("{}/{}", shop.row + 1, shop.rows.len())
            },
            [32., 60.],
            [16.; 2],
            WHITE,
        )?;
        if shop.focus == Focus::Items {
            let label = &self.spec.labels["shop_status"];
            let x = 548. - self.text_width(label, 16.)?;
            self.text(label, [x, 24.], 16., WHITE)?;
            self.button(12, [x - 24., 24.]);
        }
        if shop.choice == Choice::Sell {
            for tab in 0..7 {
                let selected = tab == shop.category;
                let x = 190. + tab as f32 * 32.;
                let y = if selected { 52. } else { 56. };
                let tint = if selected { 1. } else { 192. / 255. };
                self.sprite_rect(
                    self.spec.sprites.item_tabs[tab + 1],
                    [x, y, x + 32., y + 32.],
                    [tint, tint, tint, 1.],
                );
                if selected && category {
                    anchor = [x, y + 16.];
                }
            }
        }
        Ok(anchor)
    }

    fn shop_equipment(&mut self, shop: &Shop, party: &Party) -> Result<[f32; 2]> {
        let resources = &shop.resources;
        let data = &resources.data;
        let member = usize::from(party.formation[shop.character] - 1);
        let character = &party.members[member];
        let stats = character.stats(data);
        let item = shop
            .selected_item()
            .context("shop status has no selected item")?;
        self.portrait(member, character.conditions, [428., 60.]);
        self.equipment_marker_data(resources, party, member, item, [468., 100.]);
        self.text(
            character
                .name
                .as_deref()
                .unwrap_or(&data.rename.initial_names[member]),
            [500., 60.],
            16.,
            WHITE,
        )?;
        self.gauge(
            false,
            [500., 87.],
            [16., 18.],
            character.hp,
            stats.hp,
            Compact,
        )?;
        self.gauge(
            true,
            [500., 107.],
            [16., 18.],
            character.tp,
            stats.tp,
            Compact,
        )?;
        let definition = &resources.session.items[usize::from(item)];
        if let Some(slot) = definition
            .equipment_kind
            .and_then(|kind| character.preferred_equipment_slot(kind))
            .filter(|_| definition.allowed_characters & (1 << member) != 0)
        {
            let next = character.preview_equipment(data, slot, item);
            for (row, (key, old, new)) in [
                (
                    if member == 0 {
                        "shop_slash"
                    } else {
                        "shop_attack"
                    },
                    stats.slash,
                    next.slash,
                ),
                ("shop_thrust", stats.thrust, next.thrust),
                ("shop_defense", stats.defense, next.defense),
                ("shop_accuracy", stats.accuracy, next.accuracy),
                ("shop_evasion", stats.evasion, next.evasion),
                ("shop_intelligence", stats.intelligence, next.intelligence),
                ("shop_luck", stats.luck, next.luck),
            ]
            .into_iter()
            .enumerate()
            {
                if row == 1 && member != 0 {
                    continue;
                }
                let y = 140. + row as f32 * 26.;
                self.text(&self.spec.labels[key], [436., y], 16., GOLD)?;
                let value = format!("{old:4}");
                self.text(&value, [470., y], 16., WHITE)?;
                self.text(
                    "→",
                    [470. + self.text_width(&value, 16.)?, y],
                    16.,
                    DISABLED,
                )?;
                self.text(
                    &format!("{new:4}"),
                    [550., y],
                    16.,
                    match new.cmp(&old) {
                        std::cmp::Ordering::Greater => 4,
                        std::cmp::Ordering::Less => 2,
                        _ => WHITE,
                    },
                )?;
            }
        } else {
            let label = &self.spec.labels["shop_cannot_equip"];
            self.text(
                label,
                [
                    428. + ((192. - self.text_width(label, 16.)?) / 2.).floor(),
                    212.,
                ],
                16.,
                DISABLED,
            )?;
        }
        Ok([500., 68.])
    }

    fn shop_recipes(&mut self, shop: &Shop, party: &Party) -> Result<()> {
        let item = shop.selected_item().unwrap();
        let data = &shop.resources.data.cooking;
        let contains = |ingredient: &Ingredient| match *ingredient {
            Ingredient::None => false,
            Ingredient::Item(id) => id == item,
            Ingredient::Any(group) => data.groups[usize::from(group)].items.contains(&item),
        };
        let mut y = 62.;
        for (id, recipe) in data
            .recipes
            .iter()
            .enumerate()
            .filter(|(id, _)| party.cooking.knows(*id as u8))
        {
            let color = if recipe.required.iter().any(contains) {
                WHITE
            } else if party.formation.iter().any(|&member| {
                let member = usize::from(member - 1);
                recipe.cooks[member].grades[usize::from(party.members[member].cooking[id] / 3)]
                    .extras
                    .iter()
                    .any(contains)
            }) {
                1
            } else {
                continue;
            };
            self.text(&recipe.name, [436., y], 16., color)?;
            y += 26.;
            if y >= 322. {
                break;
            }
        }
        Ok(())
    }

    fn shop_item_statistics(&mut self, shop: &Shop, id: u16) -> Result<()> {
        let item = &shop.resources.data.items[usize::from(id)];
        let lloyd_swords = shop.resources.session.items[usize::from(id)].equipment_kind == Some(0)
            && shop.resources.session.items[usize::from(id)].allowed_characters & 1 != 0;
        self.sprite_rect(
            self.spec.sprites.item_images[usize::from(id)],
            [36., 348., 100., 412.],
            [1.; 4],
        );
        for (key, x, y, value) in [
            (
                if lloyd_swords {
                    "shop_slash"
                } else {
                    "shop_attack"
                },
                104.,
                372.,
                Some(item.equipment_stats[0]),
            ),
            ("shop_defense", 200., 372., Some(item.equipment_stats[2])),
            ("shop_accuracy", 296., 372., Some(item.equipment_stats[4])),
            ("shop_defense", 392., 372., None),
            (
                "shop_thrust",
                104.,
                398.,
                lloyd_swords.then_some(item.equipment_stats[1]),
            ),
            (
                "shop_intelligence",
                200.,
                398.,
                Some(item.equipment_stats[3]),
            ),
            ("shop_evasion", 296., 398., Some(item.equipment_stats[5])),
            ("shop_luck", 392., 398., Some(item.equipment_stats[6])),
            ("shop_attack", 488., 398., None),
        ] {
            if key == "shop_thrust" && !lloyd_swords {
                continue;
            }
            self.text(&self.spec.labels[key], [x, y], 16., GOLD)?;
            if let Some(value) = value {
                for (column, digit) in value.to_string().chars().rev().enumerate() {
                    self.text(
                        &digit.to_string(),
                        [x + 72. - column as f32 * 16., y],
                        16.,
                        WHITE,
                    )?;
                }
            }
        }
        if let Some(element) = item.properties.attack_element {
            self.sprite(self.spec.sprites.elements[element as usize], [532., 398.]);
        }
        let opacity = self.opacity;
        for (slot, (element, &amount)) in item.properties.resistance.iter().enumerate() {
            let x = 426. + slot as f32 * 23.;
            let index = Element::ALL.iter().position(|e| e == element).unwrap();
            self.sprite(self.spec.sprites.elements[index], [x, 372.]);
            self.opacity = (f32::from(opacity) * blink_opacity(self.tick, 40, 15)) as u8;
            self.text_size(
                if amount > 0 { "+" } else { "-" },
                [x + 10., 382.],
                [16.; 2],
                WHITE,
            )?;
            self.opacity = opacity;
        }
        let category = &shop.resources.data.item_categories[usize::from(item.category)];
        self.text(
            category,
            [612. - self.text_width(category, 20.)?, 344.],
            20.,
            5,
        )
    }
}

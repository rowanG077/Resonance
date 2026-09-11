use super::*;
use resonance_content::menu_data::{Ingredient, MealEffect, RECIPE_COUNT, RECIPE_ROWS};
use resonance_game::menu::cooking::{Content, Focus};

impl Drawing<'_> {
    fn recipe_icon(&mut self, recipe: usize, [x, y]: [f32; 2], size: f32) {
        self.sprite_rect(
            self.spec.sprites.recipes[recipe],
            [x, y, x + size, y + size],
            [1.; 4],
        );
    }
    fn cooking_description(&mut self, menu: &Menu, recipe: usize) -> Result<()> {
        let party = menu.party();
        if party.cooking.knows(recipe as u8) {
            self.recipe_icon(recipe, [32., 350.], 64.);
            self.text(
                &menu.resources.as_ref().unwrap().data.cooking.recipes[recipe].description,
                [118., 344.],
                20.,
                WHITE,
            )?;
        }
        Ok(())
    }
    fn cooking_ingredient(
        &mut self,
        menu: &Menu,
        ingredient: Ingredient,
        [x, y]: [f32; 2],
        color: usize,
    ) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let (name, category) = match ingredient {
            Ingredient::None => ("", 0),
            Ingredient::Item(id) => {
                let item = &data.items[usize::from(id)];
                (item.name.as_str(), item.category)
            }
            Ingredient::Any(id) => {
                let group = &data.cooking.groups[usize::from(id)];
                (group.name.as_str(), group.category)
            }
        };
        if category > 0 {
            self.sprite(self.spec.sprites.items[usize::from(category - 1)], [x, y]);
        }
        self.text(name, [x + 24., y], 16., color)
    }
    pub(super) fn cooking(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let state = &menu.cooking;
        let data = &menu.resources.as_ref().unwrap().data;
        let catalog = &data.cooking;
        let party = menu.party();
        let (chef, recipe_id) = menu.cooking_selection();
        let recipe = &catalog.recipes[recipe_id];
        let known = party.cooking.knows(recipe_id as u8);
        let grade = usize::from(party.members[chef].cooking[recipe_id] / 3);
        let fade = u32::from(state.transition.page_fade);
        let opacity = 255 - state.transition.page_fade;
        let left = -((fade * 402 / 256) as f32);
        let right = (fade * 244 / 256) as f32;
        let bottom = (fade * 112 / 256) as f32;
        let result = state
            .popup
            .as_ref()
            .filter(|p| p.active)
            .and_then(|p| match &p.content {
                Content::Meal(meal) => Some(meal),
                _ => None,
            });
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&self.spec.labels["cooking"])?;
        self.offset = [0., -((fade * 44 / 256) as f32)];
        if state.focus == Focus::Header && state.popup.as_ref().is_none_or(|p| !p.active) {
            let label = &catalog.labels["cook"];
            let width = self.text_width(label, 24.)?;
            self.button(10, [600. - width, 20.]);
            self.text(label, [624. - width, 20.], 24., WHITE)?;
        }
        self.offset = [right, 0.];
        self.frame([404., 60., 216., 266.]);
        self.offset = [0., bottom];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true);
        if state.description_opacity != 0 && state.description_previous != 0 {
            self.opacity = state.description_opacity;
            self.cooking_description(menu, state.description_previous)?;
        }
        self.opacity = if fade == 0 {
            255 - state.description_fade
        } else {
            opacity
        };
        self.cooking_description(menu, recipe_id)?;
        self.opacity = opacity;
        self.offset = [left, 0.];
        self.frame([16., 60., 378., 48.]);
        self.frame([16., 118., 378., 208.]);
        self.highlight(
            [
                28.,
                if state.choose_recipe { 84. } else { 60. },
                if state.choose_recipe { 200. } else { 120. },
                24.,
            ],
            if state.focus == Focus::Header {
                255
            } else {
                127
            },
        );
        self.text(menu.character_name(chef), [28., 60.], 20., WHITE)?;
        self.portrait(chef, party.members[chef].conditions, [320., 46.]);
        if known {
            let stars = usize::from(recipe.cooks[chef].base_stars);
            for i in 0..stars + 1 {
                self.sprite(
                    self.spec.sprites.cooking_stars[usize::from(i >= stars - 1 + grade)],
                    [152. + i as f32 * 24., 60.],
                );
            }
        }
        let selected = party.cooking.recipe;
        self.recipe_icon(usize::from(selected), [28., 84.], 24.);
        self.text(
            &catalog.recipes[usize::from(selected)].name,
            [52., 84.],
            20.,
            if party.has_ingredients(data, selected) {
                WHITE
            } else {
                DISABLED
            },
        )?;
        self.offset = [right, 0.];
        let ingredients: Vec<_> = if let Some(meal) = result {
            meal.ingredients
                .iter()
                .copied()
                .map(Ingredient::Item)
                .collect()
        } else if known {
            recipe
                .required
                .iter()
                .chain(&recipe.cooks[chef].grades[grade].extras)
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        let mut y = 60.;
        let label = &catalog.labels["required"];
        self.text(
            label,
            [
                404. + ((212. - self.text_width(label, 20.)?) / 2.).trunc(),
                y,
            ],
            20.,
            GOLD,
        )?;
        y += 24.;
        for (i, ingredient) in ingredients.into_iter().enumerate() {
            if i == recipe.required.len() {
                self.quad(FONT, [412., y, 608., y + 1.], [0.5; 4], [0., 0., 0., 1.]);
                self.quad(
                    FONT,
                    [412., y + 1., 608., y + 2.],
                    [0.5; 4],
                    [128. / 255., 128. / 255., 128. / 255., 1.],
                );
                let label = &catalog.labels["additional"];
                let size = if result.is_some() { 20. } else { 24. };
                self.text(
                    label,
                    [
                        404. + ((212. - self.text_width(label, size)?) / 2.).trunc(),
                        y + 4.,
                    ],
                    size,
                    GOLD,
                )?;
                y += 28.;
            }
            let count = party.ingredient_count(data, ingredient);
            self.cooking_ingredient(
                menu,
                ingredient,
                [408., y],
                if count > 0 || result.is_some() {
                    WHITE
                } else {
                    DISABLED
                },
            )?;
            if result.is_none() {
                let text = format!(": {count}");
                self.text(&text, [612. - self.text_width(&text, 14.)?, y], 14., 5)?;
            }
            y += 28.;
        }
        self.offset = [left, 0.];
        let anchor = if state.choose_recipe {
            self.text_size(
                &format!("{}/{}", state.recipe + 1, RECIPE_COUNT),
                [24., 119.],
                [16.; 2],
                WHITE,
            )?;
            let row = state.recipe - state.first;
            let anchor = [32. + (row % 2) as f32 * 176., 136. + (row / 2) as f32 * 27.];
            if state.focus == Focus::Recipes {
                self.highlight(
                    [anchor[0], anchor[1], 160., 24.],
                    if state.popup.as_ref().is_some_and(|p| p.active) {
                        127
                    } else {
                        255
                    },
                );
            }
            let scroll = scroll_offset(state.scroll, 27);
            let first = state.first - 2 * usize::from(state.scroll > 0);
            let starts = self.vertex_counts();
            for (row, id) in (first..RECIPE_COUNT)
                .take(RECIPE_ROWS + 2 * usize::from(state.scroll != 0))
                .enumerate()
            {
                let known = party.cooking.knows(id as u8);
                let [x, y] = [
                    32. + (row % 2) as f32 * 176.,
                    136. + (row / 2) as f32 * 27. - scroll as f32,
                ];
                if known {
                    self.recipe_icon(id, [x, y], 24.);
                }
                self.text(
                    if known {
                        &catalog.recipes[id].name
                    } else {
                        &catalog.labels["locked"]
                    },
                    [x + 24., y],
                    16.,
                    if known && party.has_ingredients(data, id as u8) {
                        WHITE
                    } else {
                        DISABLED
                    },
                )?;
            }
            self.clip_rows(starts, [136., 325.]);
            if state.first > 0 {
                self.scroll_arrow(SCROLL_UP, [193., 120.]);
            }
            if state.first + RECIPE_ROWS < RECIPE_COUNT {
                self.scroll_arrow(SCROLL_DOWN, [193., 317.]);
            }
            if state.focus == Focus::Recipes {
                [anchor[0] + left, anchor[1] + 8.]
            } else {
                state.header_cursor()
            }
        } else {
            for (slot, &id) in party.formation.iter().enumerate() {
                let member = &party.members[usize::from(id - 1)];
                let [x, y] = [
                    24. + (slot % 4) as f32 * 90.,
                    120. + (slot / 4) as f32 * 104.,
                ];
                if state.focus == Focus::Cooks && slot == state.chef_slot {
                    self.highlight([x, y, 72., 96.], 255);
                }
                self.portrait(usize::from(id - 1), member.conditions, [x + 4., y]);
                let [hp, tp] = member.maximum_vitals();
                for (is_tp, value, maximum, top) in [
                    (false, member.hp, hp, y + 63.),
                    (true, member.tp, tp, y + 79.),
                ] {
                    self.gauge(is_tp, [x, top], [16., 18.], value, maximum, Unlabelled)?;
                }
            }
            if state.focus == Focus::Cooks {
                [
                    24. + (state.chef_slot % 4) as f32 * 90. + left,
                    168. + (state.chef_slot / 4) as f32 * 104.,
                ]
            } else {
                state.header_cursor()
            }
        };
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }
    pub(super) fn cooking_popup(&mut self, menu: &Menu) -> Result<()> {
        let Some(popup) = &menu.cooking.popup else {
            return Ok(());
        };
        let catalog = &menu.resources.as_ref().unwrap().data.cooking;
        let (title, effects, ingredients) = match &popup.content {
            Content::Notice(key) => (catalog.labels[key.label()].clone(), Vec::new(), &[][..]),
            Content::Meal(meal) => {
                let recipe = menu.party().cooking.recipe;
                let title = format!(
                    "{}{}{}",
                    catalog.labels["result_join"],
                    catalog.recipes[usize::from(recipe)].name,
                    catalog.labels[if meal.success { "success" } else { "failure" }]
                );
                let mut effects: Vec<_> = meal
                    .effects
                    .iter()
                    .map(|(&effect, amount)| {
                        let label = &catalog.effects[effect as usize];
                        if matches!(effect, MealEffect::HpRecovery | MealEffect::TpRecovery) {
                            format!("{label} {amount}%")
                        } else {
                            label.clone()
                        }
                    })
                    .collect();
                if effects.is_empty() {
                    effects.push(catalog.labels["no_effect"].clone());
                }
                (title, effects, meal.ingredients.as_slice())
            }
        };
        let mut width = self.text_width(&title, 24.)?;
        for effect in &effects {
            width = width.max(192. + self.text_width(effect, 16.)?);
        }
        let height = if effects.is_empty() {
            24.
        } else {
            effects.len().max(ingredients.len()) as f32 * 26. + 28.
        };
        let [x, y] = [
            ((640. - width) / 2.).trunc(),
            ((448. - height) / 2.).trunc(),
        ];
        self.plane = 2;
        self.quad(
            FONT,
            [0., 0., 640., 448.],
            [0.5; 4],
            [0., 0., 0., f32::from(popup.opacity >> 1) / 255.],
        );
        self.opacity = popup.opacity;
        self.shade([x - 12., y - 12., x + width + 12., y + height + 12.]);
        if matches!(popup.content, Content::Notice(_)) {
            let colors = &mut self.batches[self.plane * self.layers_per_plane + FONT].colors;
            let start = colors.len() - 4;
            colors[start..].rotate_left(2);
        }
        self.plane = 3;
        self.colored_frame(
            [x - 12., y - 12., width + 24., height + 24.],
            false,
            self.popup_color(),
        );
        self.text(&title, [x, y], 24., WHITE)?;
        for (row, &id) in ingredients.iter().enumerate() {
            self.cooking_ingredient(
                menu,
                Ingredient::Item(id),
                [x, y + 30. + row as f32 * 26.],
                WHITE,
            )?;
        }
        for (row, text) in effects.iter().enumerate() {
            self.text(text, [x + 192., y + 30. + row as f32 * 26.], 16., WHITE)?;
        }
        self.opacity = 255;
        Ok(())
    }
}

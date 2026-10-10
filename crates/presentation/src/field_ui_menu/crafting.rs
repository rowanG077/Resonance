use super::*;
use resonance_events::party::Party;
use resonance_game::field::crafting::{Crafting, Focus, VISIBLE_ROWS};
use resonance_game::menu::items::Description;

impl Drawing<'_> {
    pub(super) fn crafting(&mut self, crafting: &Crafting, party: &Party) -> Result<[f32; 2]> {
        let data = &crafting.resources.data;
        let vendor = &data.crafting.vendors[usize::from(crafting.vendor)];
        let fade = u32::from(crafting.fade);
        let opacity = 255 - crafting.fade;
        let left = -((fade * 310 / 256) as f32);
        let right = (fade * 320 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.shop_heading(&vendor.name)?;
        self.offset = [left, 0.];
        self.frame([16., 60., 286., 266.])?;
        self.text_size(
            &if vendor.recipes.is_empty() {
                "-".into()
            } else {
                (crafting.row + 1).to_string()
            },
            [32., 60.],
            [16.; 2],
            WHITE,
        )?;
        let y = 78. + (crafting.row - crafting.first) as f32 * 27.;
        let mut anchor = [40. + left, y + 8.];
        if !vendor.recipes.is_empty() {
            self.highlight(
                [40., y, 248., 24.],
                if crafting.focus == Focus::Recipes {
                    255
                } else {
                    127
                },
            );
            if crafting.focus != Focus::Recipes {
                self.offset = [0.; 2];
                self.cursor(anchor, opacity / 2);
                self.offset = [left, 0.];
            }
        }
        let starts = self.vertex_counts();
        let first = crafting
            .first
            .saturating_sub(usize::from(crafting.scroll > 0));
        let offset = scroll_offset(crafting.scroll, 27);
        for (row, &id) in vendor
            .recipes
            .iter()
            .skip(first)
            .take(VISIBLE_ROWS + usize::from(crafting.scroll != 0))
            .enumerate()
        {
            let recipe = &data.crafting.recipes[usize::from(id)];
            let item = &data.items[usize::from(recipe.result)];
            let y = 78. + row as f32 * 27. - offset as f32;
            self.sprite_rect(
                self.spec
                    .sprite(Sprite::Items, usize::from(item.category - 1))?,
                [40., y, 64., y + 24.],
                [1.; 4],
            );
            self.text(&data.item_text(recipe.result)?.name, [64., y], 16., WHITE)?;
            self.text(
                &format!(
                    "{:2}",
                    party.items.get(&recipe.result).copied().unwrap_or(0)
                ),
                [256., y],
                16.,
                5,
            )?;
        }
        self.clip_rows(starts, [78., 321.]);
        if crafting.first > 0 {
            self.scroll_arrow(SCROLL_UP, [150., 62.])?;
        }
        if crafting.first + VISIBLE_ROWS < vendor.recipes.len() {
            self.scroll_arrow(SCROLL_DOWN, [150., 313.])?;
        }

        self.offset = [right, 0.];
        self.frame([312., 60., 308., 266.])?;
        for (slot, &id) in party.formation.iter().take(8).enumerate() {
            let member = usize::from(id - 1);
            let x = 320. + (slot % 4) as f32 * 76.;
            let y = 64. + (slot / 4) as f32 * 66.;
            self.portrait(member, &party.members[member], [x, y])?;
            if let Some(item) = crafting.selected_item() {
                self.equipment_marker_data(
                    &crafting.resources,
                    party,
                    member,
                    item,
                    [x + 40., y + 40.],
                )?;
            }
        }
        if let Some(recipe) = crafting.selected() {
            for (index, (&item, &count)) in recipe.ingredients.iter().enumerate() {
                let y = 200. + index as f32 * 24.;
                let owned = u16::from(party.items.get(&item).copied().unwrap_or(0));
                let text = data.item_text(item)?;
                let item = &data.items[usize::from(item)];
                let color = if owned >= count { WHITE } else { DISABLED };
                self.sprite_rect(
                    self.spec
                        .sprite(Sprite::Items, usize::from(item.category - 1))?,
                    [324., y, 348., y + 24.],
                    [1.; 4],
                );
                self.text(&text.name, [348., y], 16., color)?;
                self.text(&format!("{owned:2}/{count:2}"), [520., y], 16., color)?;
            }
        }
        self.offset = [0., (fade * 120 / 256) as f32];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true)?;
        match crafting.focus {
            Focus::Recipes => {
                self.shop_description(
                    &crafting.resources,
                    [crafting.description_previous, crafting.selected_item()]
                        .map(|item| item.map_or(Description::None, Description::Item)),
                    crafting.description_opacity,
                    opacity,
                    crafting.statistics,
                )?;
            }
            Focus::Confirm { yes } => {
                anchor = self.shop_confirmation(
                    [
                        &data.crafting.labels.confirmation,
                        &data.crafting.labels.yes,
                        &data.crafting.labels.no,
                    ],
                    yes,
                )?;
            }
            Focus::MissingMaterials | Focus::InventoryFull => {
                let label = if crafting.focus == Focus::MissingMaterials {
                    &data.crafting.labels.missing_materials
                } else {
                    &data.crafting.labels.inventory_full
                };
                self.text(
                    label,
                    [((640. - self.text_width(label, 24.)?) / 2.).floor(), 366.],
                    24.,
                    WHITE,
                )?;
            }
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }
}

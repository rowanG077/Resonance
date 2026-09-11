use super::*;
use resonance_game::menu::world_map::{Focus, ITEM_ROWS, LOCATION_ROWS};

impl Drawing<'_> {
    pub(super) fn world_map(
        &mut self,
        menu: &Menu,
        cursor: &resonance_content::font::UiTexture,
    ) -> Result<[f32; 2]> {
        let state = &menu.world_map;
        let data = &menu.resources.as_ref().unwrap().data;
        let travel = &menu.party().travel;
        let fade = u32::from(state.page_fade);
        let opacity = 255 - state.page_fade;
        let left = -((fade * 224 / 256) as f32);
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&data.world_map.names[usize::from(state.world)])?;
        self.offset = [left, 0.];
        self.frame([16., 60., 212., 266.]);
        self.offset = [0., (fade * 120 / 256) as f32];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true);
        let current = menu.map_description();
        for (id, alpha) in [
            (state.description_previous, 255 - state.description_opacity),
            (
                current,
                crossfade_opacity(state.description_opacity, opacity),
            ),
        ] {
            if id != 0 {
                self.opacity = alpha;
                self.item_description(menu, id)?;
            }
        }
        self.opacity = opacity;
        self.offset = [(fade * 404 / 256) as f32, 0.];
        let index = WORLD_MAPS + usize::from(state.world);
        let texture = &self.spec.textures[index - 1];
        self.plane = 0;
        self.quad(
            index,
            [236., 40., 620., 328.],
            [0., 0., texture.width as f32, texture.height as f32],
            [1.; 4],
        );
        self.plane = 1;
        self.opacity = 255;
        let locations = menu.map_locations();
        let phase = self.tick % 60 * 512 / 60;
        let alpha = if phase < 256 { phase } else { 511 - phase } as f32 / 255.;
        if let Some(location) = travel
            .current_location
            .filter(|id| id / 256 == u16::from(state.world))
            .and_then(|id| data.world_map.locations.get(&id))
        {
            self.map_crosshair(location.point, [1., 128. / 255., 128. / 255., 1. - alpha]);
        }
        if let Some((_, location)) = locations.get(state.location) {
            self.map_crosshair(location.point, [1., 1., 1., alpha]);
        }
        self.offset = [left, 0.];
        self.opacity = opacity;
        self.text_size(
            &format!("{}/{}", state.location + 1, locations.len()),
            [28., 60.],
            [16.; 2],
            WHITE,
        )?;
        let y = 86. + (state.location - state.first_location) as f32 * 26.;
        self.highlight(
            [24., y, 184., 24.],
            if state.focus == Focus::Locations {
                255
            } else {
                127
            },
        );
        let starts = self.vertex_counts();
        let first = state.first_location - usize::from(state.location_scroll > 0);
        let scroll = scroll_offset(state.location_scroll, 26);
        for (row, (_, location)) in locations
            .iter()
            .skip(first)
            .take(LOCATION_ROWS + usize::from(state.location_scroll != 0))
            .enumerate()
        {
            self.text(
                &location.name,
                [24., 86. + row as f32 * 26. - scroll as f32],
                16.,
                WHITE,
            )?;
        }
        self.clip_rows(starts, [84., 318.]);
        if state.first_location > 0 {
            self.scroll_arrow(SCROLL_UP, [108., 68.]);
        }
        if state.first_location + LOCATION_ROWS < locations.len() {
            self.scroll_arrow(SCROLL_DOWN, [108., 310.]);
        }
        let mut anchor = [24. + left, y + 8.];
        if state.focus != Focus::Locations {
            self.cursor([24., y + 8.], cursor, 127);
        }
        if state.focus != Focus::Locations || state.shops_opacity != 0 {
            let shops = menu.map_shops();
            let height = shops.len() as f32 * 26. + 28.;
            self.opacity = state.shops_opacity;
            self.shade([80., 76., 288., 76. + height]);
            self.plane = 2;
            self.frame([80., 76., 208., height]);
            let y = 102. + state.shop as f32 * 26.;
            self.highlight(
                [88., y, 192., 24.],
                if state.focus == Focus::Shops {
                    255
                } else {
                    127
                },
            );
            self.text_size(
                &format!("{}/{}", state.shop + 1, shops.len()),
                [88., 76.],
                [16.; 2],
                WHITE,
            )?;
            for (row, &id) in shops.iter().enumerate() {
                self.text(
                    &data.world_map.shops[usize::from(id)].name,
                    [88., 102. + row as f32 * 26.],
                    16.,
                    if travel.visited_shops.contains(&id) {
                        WHITE
                    } else {
                        DISABLED
                    },
                )?;
            }
            if state.focus == Focus::Shops {
                anchor = [88. + left, y + 8.];
            } else {
                self.cursor([88., y + 8.], cursor, 127);
            }
            if state.focus == Focus::Items || state.items_opacity != 0 {
                self.opacity = state.items_opacity;
                self.shade([144., 92., 352., 328.]);
                self.plane = 3;
                self.frame([144., 92., 208., 236.]);
                let (_, shop) = menu.map_shop().context("missing selected map shop")?;
                let y = 118. + (state.item - state.first_item) as f32 * 26.;
                self.highlight([152., y, 184., 24.], 255);
                self.text_size(
                    &format!("{}/{}", state.item + 1, shop.items.len()),
                    [152., 92.],
                    [16.; 2],
                    WHITE,
                )?;
                let starts = self.vertex_counts();
                let first = state.first_item - usize::from(state.item_scroll > 0);
                let scroll = scroll_offset(state.item_scroll, 26);
                for (row, &id) in shop
                    .items
                    .iter()
                    .skip(first)
                    .take(ITEM_ROWS + usize::from(state.item_scroll != 0))
                    .enumerate()
                {
                    let item = &data.items[usize::from(id)];
                    let y = 118. + row as f32 * 26. - scroll as f32;
                    self.sprite_rect(
                        self.spec.sprites.items[usize::from(item.category - 1)],
                        [152., y, 176., y + 24.],
                        [1.; 4],
                    );
                    self.text(&item.name, [176., y], 16., WHITE)?;
                }
                self.clip_rows(starts, [116., 324.]);
                if state.first_item > 0 {
                    self.scroll_arrow(SCROLL_UP, [236., 100.]);
                }
                if state.first_item + ITEM_ROWS < shop.items.len() {
                    self.scroll_arrow(SCROLL_DOWN, [236., 316.]);
                }
                if state.focus == Focus::Items {
                    anchor = [152. + left, y + 8.];
                } else {
                    self.cursor([152., y + 8.], cursor, 255);
                }
            }
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }

    fn map_crosshair(&mut self, [x, y]: [i16; 2], color: [f32; 4]) {
        let [x, y] = [236. + f32::from(x), 40. + f32::from(y)];
        self.quad(FONT, [x - 1., 40., x + 1., 328.], [0.5; 4], color);
        self.quad(FONT, [236., y - 1., 620., y + 1.], [0.5; 4], color);
    }
}

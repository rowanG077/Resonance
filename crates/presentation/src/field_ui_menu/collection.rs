use super::*;
use resonance_game::menu::collection::{COLUMNS, VISIBLE};

impl Drawing<'_> {
    pub(super) fn collection(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let data = &menu
            .resources
            .as_ref()
            .context("collection data was not prepared")?
            .data;
        let party = menu.party();
        let book = &menu.collection;
        let (items, total) = menu.collection_items();
        let fade = u32::from(book.page_fade);
        let opacity = 255 - book.page_fade;
        let list_offset = -((fade * 628 / 256) as f32);
        let description_offset = (fade * 120 / 256) as f32;
        self.opacity = opacity;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&data.labels["collectors_book"])?;
        self.offset = [list_offset, 0.];
        self.framed([16., 60., 604., 264.], true);
        self.offset = [0., description_offset];
        self.frame_detail(
            [16., 336., 604., 92.],
            !book.categories && !items.is_empty(),
            self.menu_color(),
            true,
        );
        self.offset = [list_offset, 0.];
        let count = if items.is_empty() {
            "-/-".into()
        } else if book.categories {
            format!("-/{}", items.len())
        } else {
            format!("{}/{}", book.row + 1, items.len())
        };
        self.text_size(&count, [32., 64.], [16.; 2], WHITE)?;
        self.text_size(
            &format!("{}%", items.len() * 100 / total.max(1)),
            [232., 64.],
            [16.; 2],
            WHITE,
        )?;
        let mut anchor = [362. + book.category as f32 * 32., 68.];
        if !book.categories {
            let row = book.row - book.first;
            let x = 28. + (row % COLUMNS) as f32 * 202.;
            let y = 96. + (row / COLUMNS) as f32 * 28.;
            self.highlight([x, y, 168., 24.], 255);
            anchor = [x, y + 8.];
        }
        for (category, &rect) in self.spec.sprites.item_tabs[1..].iter().enumerate() {
            let selected = category == book.category;
            let x = 362. + category as f32 * 32.;
            let y = if selected { 52. } else { 56. };
            let tint = if selected { 1. } else { 192. / 255. };
            self.sprite_rect(rect, [x, y, x + 32., y + 32.], [tint, tint, tint, 1.]);
        }
        let starts = self.vertex_counts();
        let first = book.first - usize::from(book.scroll > 0) * COLUMNS;
        let scroll_offset = scroll_offset(book.scroll, 28);
        for (row_in_view, &id) in items
            .iter()
            .skip(first)
            .take(VISIBLE + usize::from(book.scroll != 0) * COLUMNS)
            .enumerate()
        {
            let column = (row_in_view % COLUMNS) as f32;
            let x = 28. + column * 197.;
            let y = 96. + (row_in_view / COLUMNS) as f32 * 28. - scroll_offset as f32;
            let item = &data.items[usize::from(id)];
            self.sprite_rect(
                self.spec.sprites.items[usize::from(item.category - 1)],
                [x, y, x + 24., y + 24.],
                [1.; 4],
            );
            self.text(
                &item.name,
                [x + 24., y],
                14.,
                if party.recent_items.contains(&id) {
                    4
                } else {
                    WHITE
                },
            )?;
        }
        self.clip_rows(starts, [96., 320.]);
        if book.first > 0 {
            self.scroll_arrow(SCROLL_UP, [306., 80.]);
        }
        if book.first + VISIBLE < items.len() {
            self.scroll_arrow(SCROLL_DOWN, [306., 312.]);
        }
        anchor[0] += list_offset;
        self.offset = [0., description_offset];
        if !items.is_empty() {
            self.opacity = 255 - book.description_opacity;
            self.item_description_content(menu, book.description_previous)?;
            self.opacity = crossfade_opacity(book.description_opacity, opacity);
            self.item_description_content(menu, menu.collection_description())?;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }
}

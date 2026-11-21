//! Sample the list viewport before choosing visible rows and clipping their glyphs.
use super::{ActivePage, Page, Source};

type Position = (usize, i8);

#[derive(Clone, Copy, Default)]
pub(super) struct ScrollMotion {
    pub primary: ListScroll,
    pub detail: ListScroll,
    pub color_center: Option<f32>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ListScroll(Option<(Position, Position, f32)>);

impl ScrollMotion {
    pub fn between(from: Source<'_>, to: Source<'_>, alpha: f32) -> Option<Self> {
        let (page, first, detail) = positions(from)?;
        let (next_page, next, next_detail) = positions(to)?;
        (page == next_page).then(|| Self {
            primary: ListScroll::between(first, next, alpha),
            detail: ListScroll::between(detail, next_detail, alpha),
            color_center: from.menu().zip(to.menu()).and_then(|(a, b)| {
                (a.page == Page::Customize && a.customize.row == b.customize.row).then(|| {
                    super::customize::sample_color_center(&a.customize, &b.customize, alpha)
                })
            }),
        })
    }
}

fn positions(source: Source<'_>) -> Option<(ActivePage, Position, Position)> {
    let Some(menu) = source.menu() else {
        let Source::Field(field) = source else {
            return None;
        };
        return field
            .shop
            .as_ref()
            .map(|s| (ActivePage::Shop, (s.first, s.scroll), (0, 0)))
            .or_else(|| {
                field
                    .crafting
                    .as_ref()
                    .map(|s| (ActivePage::Crafting, (s.first, s.scroll), (0, 0)))
            });
    };
    let mut detail = (0, 0);
    let first = match menu.page {
        Page::Customize => (menu.customize.first, menu.customize.scroll),
        Page::Items => (menu.inventory.first, menu.inventory.scroll),
        Page::Collection => (menu.collection.first, menu.collection.scroll),
        Page::Equip => (menu.equipment.first, menu.equipment.scroll),
        Page::ExSkills => (menu.ex_skills.first, menu.ex_skills.scroll),
        Page::Cooking => (menu.cooking.first, menu.cooking.scroll),
        Page::Monsters => (menu.monsters.first, menu.monsters.scroll),
        Page::Figurines => (menu.figurines.first, menu.figurines.scroll),
        Page::Strategy => (menu.strategy.first, menu.strategy.scroll),
        Page::WorldMap => {
            let map = &menu.world_map;
            detail = (map.first_item, map.item_scroll);
            (map.first_location, map.location_scroll)
        }
        Page::Synopsis => {
            let book = &menu.synopsis;
            detail = (book.line, book.text_scroll);
            (book.first, book.list_scroll)
        }
        _ => return None,
    };
    Some((ActivePage::Menu(menu.page), first, detail))
}

#[derive(Debug, PartialEq)]
pub(super) struct Rows {
    pub first: usize,
    pub count: usize,
    pub offset: f32,
}

impl ListScroll {
    pub(super) fn between(from: Position, to: Position, alpha: f32) -> Self {
        Self((from != to && (from.1 != 0 || to.1 != 0)).then_some((from, to, alpha)))
    }

    pub fn rows(
        self,
        first: usize,
        phase: i8,
        visible: usize,
        columns: usize,
        height: i32,
    ) -> Rows {
        self.sample((first, phase), visible, columns, height, 5)
    }

    pub fn sample(
        self,
        position: Position,
        visible: usize,
        columns: usize,
        height: i32,
        ticks: i32,
    ) -> Rows {
        // Preserve the integer pixel positions of the original fixed ticks.
        let pixels = |(first, phase): Position| {
            let row = first.saturating_sub(usize::from(phase > 0) * columns) / columns;
            (row as i32 + i32::from(phase < 0)) * height + i32::from(phase) * height / ticks
        };
        let top = self
            .0
            .and_then(|(from, to, alpha)| {
                let start = pixels(from);
                let travel = pixels(to) - start;
                // Page jumps and content resets have no intermediate scroll positions.
                (travel.abs() <= (height + ticks - 1) / ticks)
                    .then_some(start as f32 + travel as f32 * alpha)
            })
            .unwrap_or_else(|| pixels(position) as f32);
        let offset = top % height as f32;
        Rows {
            first: (top / height as f32) as usize * columns,
            count: visible + usize::from(offset > 0.) * columns,
            offset,
        }
    }
}

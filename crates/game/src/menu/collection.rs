use super::*;
use items::Description;

pub const COLUMNS: usize = 3;
pub const VISIBLE: usize = 24;

#[derive(Default, serde::Serialize)]
pub struct Collection {
    pub page_fade: u8,
    pub page_closing: bool,
    pub category: usize,
    pub row: usize,
    pub first: usize,
    pub categories: bool,
    pub scroll: i8,
    pub description_previous: Description,
    pub description_fade: u8,
    pub description_opacity: u8,
}

impl Menu {
    pub(super) fn open_collection(&mut self) {
        let length = self.collection_items().0.len();
        let book = &mut self.collection;
        if book.row >= length {
            book.row = 0;
            book.first = 0;
        }
        book.categories = false;
        book.scroll = 0;
        book.page_fade = 231 - MAIN_SLIDE_STEP;
        book.page_closing = false;
        book.description_previous = Description::None;
        book.description_fade = DESCRIPTION_FADE_START;
        self.fade_collection_description();
    }

    pub(super) fn advance_collection(&mut self) -> bool {
        if self.collection.description_fade == 0 {
            self.collection.description_previous = self.collection_description();
        }
        let book = &mut self.collection;
        if book.page_fade == 255 {
            self.page = Page::Items;
            self.open_items();
            self.fade_item_description();
            return true;
        }
        book.page_fade = if book.page_closing {
            book.page_fade.saturating_add(MAIN_SLIDE_STEP)
        } else {
            book.page_fade.saturating_sub(MAIN_SLIDE_STEP)
        };
        if book.page_fade == 255 {
            book.page_closing = false;
        }
        false
    }

    pub fn collection_description(&self) -> Description {
        if self.collection.categories {
            Description::Category(self.collection.category + 1)
        } else {
            self.collection_items()
                .0
                .get(self.collection.row)
                .copied()
                .map_or(Description::None, Description::Item)
        }
    }

    pub(super) fn fade_collection_description(&mut self) {
        if self.collection_items().0.is_empty() {
            return;
        }
        let description = self.collection_description();
        let book = &mut self.collection;
        book.description_opacity = fade_description(
            &mut book.description_fade,
            book.description_previous != description,
        );
    }

    pub fn collection_items(&self) -> (Vec<u16>, usize) {
        let party = self.party();
        let data = &self.resources.as_ref().unwrap().data;
        let category = self.collection.category + 1;
        let mut total = 0;
        let mut items = data
            .items
            .iter()
            .enumerate()
            .filter_map(|(id, item)| {
                if item.inventory_category() != Some(category) {
                    return None;
                }
                total += 1;
                party
                    .found_items
                    .contains(&(id as u16))
                    .then_some(id as u16)
            })
            .collect::<Vec<_>>();
        self.sort_items(&mut items, category);
        (items, total)
    }

    pub(super) fn step_collection(
        &mut self,
        input: crate::field::FieldInput,
        [left, right, up, down, page_up, page_down]: [bool; 6],
    ) -> Option<i16> {
        let length = self.collection_items().0.len();
        let book = &mut self.collection;
        book.scroll = (book.scroll + book.scroll.signum()) % 5;
        if book.scroll != 0 {
            return None;
        }
        if input.cancel {
            if book.categories {
                book.page_closing = true;
            } else {
                book.categories = true;
            }
            return Some(3);
        }
        if !book.categories
            && !(left || right || up || down)
            && (input.previous_page || input.next_page)
            || book.categories && (left || right)
        {
            let previous = if book.categories {
                left
            } else {
                input.previous_page
            };
            book.category = (book.category + if previous { 7 } else { 1 }) % 8;
            book.row = 0;
            book.first = 0;
            return Some(1);
        }
        if book.categories {
            if down || input.interact {
                book.categories = false;
                book.row = 0;
                book.first = 0;
                return Some(2);
            }
            return None;
        }
        if up && book.row < COLUMNS {
            book.categories = true;
            return Some(1);
        }
        let old = book.row;
        if page_up && book.first != 0 {
            let first = book.first.saturating_sub(VISIBLE);
            book.row -= book.first - first;
            book.first = first;
            return Some(38);
        }
        if page_down && book.first + VISIBLE < length {
            book.first += VISIBLE;
            book.row = (book.row + VISIBLE).min(length - 1);
            return Some(38);
        }
        if left {
            book.row = book.row.saturating_sub(1);
        }
        if right {
            book.row = (book.row + 1).min(length.saturating_sub(1));
        }
        if up {
            book.row = book.row.saturating_sub(COLUMNS);
        }
        if down && book.row + COLUMNS < length {
            book.row += COLUMNS;
        }
        let first = book.first;
        book.first = book
            .first
            .min(book.row / COLUMNS * COLUMNS)
            .max((book.row / COLUMNS * COLUMNS).saturating_sub(VISIBLE - COLUMNS));
        book.scroll = (book.first as isize - first as isize).signum() as i8;
        (old != book.row).then_some(1)
    }
}

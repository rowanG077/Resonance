use super::*;
use crate::field::FieldInput;

pub(super) fn page_shift(input: FieldInput, length: usize, first: usize) -> isize {
    if input.next_page {
        VISIBLE_PARTY.min(length.saturating_sub(first + VISIBLE_PARTY)) as isize
    } else if input.previous_page {
        -(VISIBLE_PARTY.min(first) as isize)
    } else {
        0
    }
}

impl Menu {
    pub fn party(&self) -> &resonance_events::party::Party {
        &self.checkpoint.as_ref().unwrap().progress.party
    }

    pub(super) fn clamp_party_view(&mut self) {
        if let Some(checkpoint) = &self.checkpoint {
            self.character = self
                .character
                .min(checkpoint.progress.party.formation.len() - 1);
            if matches!(
                self.page,
                Page::Main | Page::Party | Page::Character(_) | Page::System
            ) {
                self.first_character = self
                    .first_character
                    .min(self.character)
                    .max(self.character.saturating_sub(VISIBLE_PARTY - 1));
            }
        }
    }

    pub(super) fn page_party(&mut self, input: FieldInput) -> Option<i16> {
        let length = self.checkpoint.as_ref()?.progress.party.formation.len();
        let shift = page_shift(input, length, self.first_character);
        self.first_character = self.first_character.saturating_add_signed(shift);
        self.character = self.character.saturating_add_signed(shift);
        (shift != 0).then_some(0x26)
    }

    pub(super) fn move_party_cursor(
        &mut self,
        input: FieldInput,
        up: bool,
        down: bool,
    ) -> Option<i16> {
        let length = self.checkpoint.as_ref()?.progress.party.formation.len();
        let previous = self.character;
        if up {
            self.character = self.character.saturating_sub(1);
        } else if down {
            self.character = (self.character + 1).min(length - 1);
        } else {
            return self.page_party(input);
        }
        (previous != self.character).then_some(1)
    }

    pub(super) fn step_party(&mut self, input: FieldInput, up: bool, down: bool) -> Option<i16> {
        if input.cancel {
            if self.swap_character.take().is_none() {
                self.page = Page::Main;
            }
            return Some(3);
        }
        if input.interact || input.menu {
            let party = &mut self.checkpoint.as_mut()?.progress.party;
            if let Some(origin) = self.swap_character.take() {
                party.formation.swap(origin, self.character);
            } else if input.menu {
                self.swap_character = Some(self.character);
                return Some(2);
            } else {
                let id = party.formation[self.character];
                if party.leader_locked || !party.members[usize::from(id - 1)].can_lead_field() {
                    return Some(4);
                }
                party.field_leader = id;
            }
            self.party_changed = true;
            return Some(2);
        }
        if up && self.character == 0 && self.swap_character.is_none() {
            self.page = Page::Main;
            return Some(1);
        }
        self.move_party_cursor(input, up, down)
    }
}

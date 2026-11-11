use super::*;

pub(super) fn page_shift(input: Input, length: usize, first: usize) -> isize {
    use MenuAction::*;
    if matches!(input, Some(PageDown | NextTab)) {
        VISIBLE_PARTY.min(length.saturating_sub(first + VISIBLE_PARTY)) as isize
    } else if matches!(input, Some(PageUp | PreviousTab)) {
        -(VISIBLE_PARTY.min(first) as isize)
    } else {
        0
    }
}

impl Menu {
    pub fn party(&self) -> &resonance_events::party::Party {
        &self.checkpoint.as_ref().unwrap().progress().party
    }

    pub(super) fn clamp_party_view(&mut self) {
        if let Some(checkpoint) = &self.checkpoint {
            self.character = self
                .character
                .min(checkpoint.progress().party.formation.len() - 1);
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

    pub(super) fn page_party(&mut self, input: Input) -> Option<i16> {
        let length = self.checkpoint.as_ref()?.progress().party.formation.len();
        let shift = page_shift(input, length, self.first_character);
        self.first_character = self.first_character.saturating_add_signed(shift);
        self.character = self.character.saturating_add_signed(shift);
        (shift != 0).then_some(0x26)
    }

    pub(super) fn move_party_cursor(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let length = self.checkpoint.as_ref()?.progress().party.formation.len();
        let previous = self.character;
        if input == Some(Up) {
            self.character = self.character.saturating_sub(1);
        } else if input == Some(Down) {
            self.character = (self.character + 1).min(length - 1);
        } else {
            return self.page_party(input);
        }
        (previous != self.character).then_some(1)
    }

    pub(super) fn step_party(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        if input == Some(Cancel) {
            if self.swap_character.take().is_none() {
                self.page = Page::Main;
            }
            return Some(3);
        }
        if matches!(input, Some(Confirm | Menu)) {
            let party = &mut self.checkpoint.as_mut()?.progress_mut().party;
            if let Some(origin) = self.swap_character.take() {
                party.formation.swap(origin, self.character);
            } else if input == Some(Menu) {
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
        if input == Some(Up) && self.character == 0 && self.swap_character.is_none() {
            self.page = Page::Main;
            return Some(1);
        }
        self.move_party_cursor(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::FieldInput;
    use resonance_events::input::Button;

    fn menu(page: Page, length: usize) -> anyhow::Result<Menu> {
        let checkpoint = serde_json::from_value(serde_json::json!({
            "map_id": 330, "position": [0, 0, 0], "heading": 0, "played_ticks": 0,
            "progress": {
                "script_globals": [], "event_flags": [], "random_state": 0, "tick": 0,
                "gameplay_random": resonance_events::GameplayRandom::default(),
                "event_records": {},
                "party": {
                    "battles": resonance_events::party::BattleStatistics::default(),
                    "members": [], "formation": (1..=length).collect::<Vec<_>>(),
                    "items": {}, "found_items": [], "recent_items": [],
                    "gald": 0, "spent_gald": 0,
                    "settings": resonance_events::party::Settings::default()
                }
            }
        }))?;
        Ok(Menu::new(page, Some(Checkpoint::Field(checkpoint)), false))
    }

    fn page_input(down: bool, shoulder: bool) -> FieldInput {
        FieldInput {
            scroll_direction: if shoulder {
                0
            } else if down {
                -1
            } else {
                1
            },
            pressed_buttons: if shoulder {
                [if down {
                    Button::NextPage
                } else {
                    Button::PreviousPage
                }]
                .into()
            } else {
                Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn party_page_inputs_preserve_visible_row_and_stop_at_both_edges() -> anyhow::Result<()> {
        for shoulder in [false, true] {
            for page in [
                Page::Main,
                Page::Party,
                Page::Character(CharacterMenu::Equip),
                Page::Character(CharacterMenu::Tech),
                Page::Character(CharacterMenu::Status),
                Page::Character(CharacterMenu::ExSkills),
            ] {
                // A partial final page must shift by the number of remaining
                // members, not four, and retain the cursor's visible row.
                for length in [4, 5, 6, 8] {
                    let mut menu = menu(page, length)?;
                    menu.character = 2;
                    let shift = length - VISIBLE_PARTY;
                    let expected = (shift != 0).then_some(0x26);
                    assert_eq!(menu.step(page_input(true, shoulder)), expected);
                    assert_eq!((menu.character, menu.first_character), (2 + shift, shift));
                    menu.step(Default::default());
                    assert_eq!(menu.step(page_input(true, shoulder)), None);
                    menu.step(Default::default());
                    assert_eq!(menu.step(page_input(false, shoulder)), expected);
                    assert_eq!((menu.character, menu.first_character), (2, 0));
                    menu.step(Default::default());
                    assert_eq!(menu.step(page_input(false, shoulder)), None);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn exchange_origin_survives_page_inputs_and_cancel() -> anyhow::Result<()> {
        for shoulder in [false, true] {
            let mut menu = menu(Page::Party, 8)?;
            menu.character = 2;
            let exchange = FieldInput {
                pressed_buttons: [Button::Menu].into(),
                ..Default::default()
            };
            assert_eq!(menu.step(exchange), Some(2));
            menu.step(Default::default());
            assert_eq!(menu.step(page_input(true, shoulder)), Some(0x26));
            assert_eq!((menu.character, menu.first_character), (6, 4));
            assert_eq!(menu.swap_character, Some(2));
            menu.step(Default::default());
            assert_eq!(menu.step(exchange), Some(2));
            assert_eq!(menu.party().formation, [1, 2, 7, 4, 5, 6, 3, 8]);
            assert_eq!(menu.swap_character, None);
            menu.step(Default::default());
            assert_eq!(menu.step(exchange), Some(2));
            menu.step(Default::default());
            assert_eq!(menu.step(page_input(false, shoulder)), Some(0x26));
            assert_eq!(menu.swap_character, Some(6));
            menu.step(Default::default());
            assert_eq!(
                menu.step(FieldInput {
                    pressed_buttons: [Button::Cancel].into(),
                    ..Default::default()
                }),
                Some(3)
            );
            assert_eq!(menu.swap_character, None);
            assert_eq!(menu.page, Page::Party);
            assert_eq!(menu.party().formation, [1, 2, 7, 4, 5, 6, 3, 8]);
        }
        Ok(())
    }
}

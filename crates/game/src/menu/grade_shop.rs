use super::*;
use resonance_content::grade::Benefit;
use std::collections::BTreeSet;

pub const VISIBLE: usize = 8;
#[derive(Default)]
pub struct State {
    pub row: usize,
    pub first: usize,
    pub selected: BTreeSet<Benefit>,
    pub confirmation: Option<bool>,
}
impl Menu {
    pub fn grade_cost(&self) -> u32 {
        self.resources
            .as_ref()
            .unwrap()
            .data
            .grade_shop
            .cost(&self.grade_shop.selected)
            .unwrap()
    }

    pub(super) fn step_grade_shop(&mut self, input: Input) -> Option<i16> {
        let left = input == Some(MenuAction::Left);
        let right = input == Some(MenuAction::Right);
        let up = input == Some(MenuAction::Up);
        let down = input == Some(MenuAction::Down);
        let page_up = input == Some(MenuAction::PageUp);
        let page_down = input == Some(MenuAction::PageDown);
        let shop = &self.resources.as_ref()?.data.grade_shop;
        if let Some(yes) = &mut self.grade_shop.confirmation {
            if input == Some(MenuAction::Cancel) {
                self.grade_shop.confirmation = None;
                return Some(3);
            }
            if input == Some(MenuAction::Confirm) {
                if *yes {
                    let party = &mut self.checkpoint.as_mut()?.progress_mut().party;
                    if party
                        .buy_new_game_plus(shop, self.grade_shop.selected.clone())
                        .is_err()
                    {
                        return Some(4);
                    }
                    self.party_changed = true;
                    self.closed = true;
                }
                self.grade_shop.confirmation = None;
                return Some(2);
            }
            if left || right || up || down {
                *yes = !*yes;
                return Some(1);
            }
            return None;
        }
        if input == Some(MenuAction::Cancel) || input == Some(MenuAction::Details) {
            self.grade_shop.row = shop.options.len();
        } else if input == Some(MenuAction::Confirm) {
            if self.grade_shop.row == shop.options.len() {
                self.grade_shop.confirmation = Some(false);
            } else {
                let purchase = &shop.options[self.grade_shop.row];
                let mut selected = self.grade_shop.selected.clone();
                if !selected.remove(&purchase.benefit) {
                    selected.extend([purchase.benefit]);
                    selected.retain(|benefit| !purchase.excludes.contains(benefit));
                }
                let Ok(cost) = shop.cost(&selected) else {
                    return Some(4);
                };
                if cost > self.party().grade_hundredths {
                    return Some(4);
                }
                self.grade_shop.selected = selected;
            }
            return Some(2);
        } else if up || down || page_up || page_down {
            let delta = if page_up {
                -(VISIBLE as isize)
            } else if page_down {
                VISIBLE as isize
            } else if up {
                -1
            } else {
                1
            };
            move_list(
                &mut self.grade_shop.row,
                &mut self.grade_shop.first,
                shop.options.len() + 1,
                VISIBLE,
                delta,
            );
            return Some(1);
        } else {
            return None;
        }
        self.grade_shop.first = self.grade_shop.row.saturating_sub(VISIBLE - 1);
        Some(1)
    }
}

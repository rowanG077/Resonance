use crate::menu::{MenuAction, Transition, TransitionStatus, fade_description};
use resonance_battle::ActorId;

pub use crate::menu::Input as InventoryInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorSelection {
    pub actor: ActorId,
    pub slot: u8,
    pub name: String,
    pub eligible: bool,
}

pub use crate::menu::items::ItemRow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListFrame {
    pub rows: Vec<ItemRow>,
    pub selected: usize,
    pub first: usize,
    pub scroll: i8,
    pub fade: u8,
    pub description_previous: u16,
    pub description_blend: u8,
}

use super::{Input, InputKind, InventoryMemory, View};
use crate::battle::items::Inventory;
use anyhow::{Context, Result, ensure};
use resonance_battle::{Battle, item::Release};

#[derive(Debug, Default)]
pub(super) struct Memory {
    pub slot: usize,
    row: usize,
    first: usize,
}
impl Memory {
    pub fn inventory(&self) -> InventoryMemory {
        InventoryMemory {
            selected: self.row,
            first: self.first,
        }
    }
    pub fn restore(&mut self, memory: InventoryMemory) {
        self.row = memory.selected;
        self.first = memory.first;
    }
}

#[derive(Debug)]
pub(super) struct Owner {
    phase: Phase,
    user: Option<ActorId>,
}

#[derive(Debug)]
enum Phase {
    User(ActorSelection),
    Inventory(List),
    Ally { item: u16, selected: ActorSelection },
    Enemy { item: u16, target: ActorId },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Outcome {
    #[default]
    Stay,
    Strip,
    Close,
}

#[derive(Default)]
pub(super) struct Visit {
    pub cues: Vec<u16>,
    pub outcome: Outcome,
}

impl Owner {
    pub fn new(memory: &mut Memory, inventory: &Inventory<'_>, battle: &Battle) -> Result<Self> {
        ensure!(
            !inventory.roster.is_empty()
                && inventory.roster.len() <= resonance_battle::PARTY_CAPACITY,
            "invalid item picker roster"
        );
        memory.slot = memory.slot.min(inventory.roster.len() - 1);
        Ok(Self {
            phase: Phase::User(selection(memory, inventory, battle, None)?),
            user: None,
        })
    }

    pub fn view(&self) -> View {
        match &self.phase {
            Phase::User(selected) => View::User(selected.clone()),
            Phase::Inventory(list) => View::Inventory(list.frame()),
            Phase::Ally { selected, .. } => View::Ally(selected.clone()),
            Phase::Enemy { target, .. } => View::Enemy { target: *target },
        }
    }

    pub(super) fn remember(&self, memory: &mut Memory) {
        if let Phase::Inventory(list) = &self.phase {
            memory.row = list.row;
            memory.first = list.first;
        }
    }

    pub fn input_kind(&self) -> InputKind {
        if matches!(self.phase, Phase::Inventory(_)) {
            InputKind::SharedMenu
        } else {
            InputKind::Command
        }
    }

    pub fn step(
        &mut self,
        input: Input,
        menu_owner: ActorId,
        memory: &mut Memory,
        inventory: &Inventory<'_>,
        battle: &mut Battle,
    ) -> Result<Visit> {
        if let Phase::Inventory(list) = &mut self.phase {
            let (_, returned, cues) =
                list.step(input.shared_menu, |item| battle.item_policy(item).is_some());
            let mut visit = Visit {
                cues,
                ..Default::default()
            };
            if let Some(returned) = returned {
                memory.row = list.row;
                memory.first = list.first;
                match returned {
                    None => {
                        visit.outcome = Outcome::Strip;
                    }
                    Some(item)
                        if matches!(
                            battle.item_policy(item).map(|p| p.effect),
                            Some(resonance_battle::item::Effect::Scan)
                        ) =>
                    {
                        let preferred = battle.target(menu_owner).unwrap_or(menu_owner);
                        let target = if battle.item_target_eligible(item, preferred)? {
                            Some(preferred)
                        } else {
                            battle.cycle_item_target(item, menu_owner, preferred, 1)?
                        };
                        if let Some(target) = target {
                            self.phase = Phase::Enemy { item, target };
                        } else {
                            visit.cues.push(4);
                            self.phase = Phase::Inventory(List::open(memory, inventory, battle)?);
                        }
                    }
                    Some(item)
                        if matches!(
                            battle.item_policy(item).map(|p| p.effect),
                            Some(
                                resonance_battle::item::Effect::PartyRecover { .. }
                                    | resonance_battle::item::Effect::AllDivide
                                    | resonance_battle::item::Effect::Hourglass
                            )
                        ) =>
                    {
                        let user = self.user.context("item inventory has no selected user")?;
                        battle.queue_item(Release {
                            user,
                            target: user,
                            item,
                        })?;
                        visit.outcome = Outcome::Close;
                    }
                    Some(item) => {
                        self.phase = Phase::Ally {
                            item,
                            selected: selection(memory, inventory, battle, Some(item))?,
                        };
                    }
                }
            }
            return Ok(visit);
        }
        if matches!(self.phase, Phase::User(_) | Phase::Ally { .. }) {
            let mut cues = vec![];
            let cancelling = input.cancel_b.pressed || input.cancel_y.pressed;
            if !cancelling && !input.confirm_a.pressed {
                let step = if input.picker_directions & 1 != 0 {
                    -1
                } else if input.picker_directions & 2 != 0 {
                    1
                } else {
                    0
                };
                if step != 0 {
                    memory.slot = (memory.slot as isize + step)
                        .rem_euclid(inventory.roster.len() as isize)
                        as usize;
                    cues.push(1);
                }
            }
            let item = match self.phase {
                Phase::Ally { item, .. } => Some(item),
                _ => None,
            };
            let selected = selection(memory, inventory, battle, item)?;
            self.phase = match item {
                Some(item) => Phase::Ally {
                    item,
                    selected: selected.clone(),
                },
                None => Phase::User(selected.clone()),
            };
            let mut visit = Visit {
                cues,
                ..Default::default()
            };
            if cancelling {
                visit.cues.push(3);
                if item.is_some() {
                    let list = List::open(memory, inventory, battle)?;
                    self.phase = Phase::Inventory(list);
                } else {
                    visit.outcome = Outcome::Strip;
                }
                return Ok(visit);
            }
            if input.confirm_a.pressed {
                if selected.eligible {
                    visit.cues.push(2);
                    if let Some(item) = item {
                        battle.queue_item(Release {
                            user: self.user.context("item target has no selected user")?,
                            target: selected.actor,
                            item,
                        })?;
                        visit.outcome = Outcome::Close;
                    } else {
                        self.user = Some(selected.actor);
                        let list = List::open(memory, inventory, battle)?;
                        self.phase = Phase::Inventory(list);
                    }
                    return Ok(visit);
                }
                visit.cues.push(4);
            }
            return Ok(visit);
        }
        let Phase::Enemy { item, target } = &mut self.phase else {
            unreachable!()
        };
        let user = self
            .user
            .context("item enemy picker has no selected user")?;
        let mut cues = vec![];
        let cancelling = input.cancel_b.pressed || input.cancel_y.pressed;
        if !cancelling {
            let direction = if !battle.item_target_eligible(*item, *target)? {
                if input.step == 0 { 1 } else { input.step }
            } else if !input.confirm_a.pressed {
                input.step
            } else {
                0
            };
            if direction != 0 {
                let Some(next) = battle.cycle_item_target(*item, user, *target, direction)? else {
                    self.phase = Phase::Inventory(List::open(memory, inventory, battle)?);
                    return Ok(Visit {
                        cues: vec![4],
                        ..Default::default()
                    });
                };
                if next != *target {
                    *target = next;
                    cues.push(108);
                }
            }
        }
        let mut visit = Visit {
            cues,
            ..Default::default()
        };
        if !cancelling && input.confirm_a.pressed && battle.can_queue_item(user)? {
            battle.queue_item(Release {
                user,
                target: *target,
                item: *item,
            })?;
            visit.cues.push(2);
            visit.outcome = Outcome::Close;
            return Ok(visit);
        }
        if cancelling {
            visit.cues.push(3);
            visit.outcome = Outcome::Strip;
        }
        Ok(visit)
    }
}

fn selection(
    memory: &Memory,
    inventory: &Inventory<'_>,
    battle: &Battle,
    item: Option<u16>,
) -> Result<ActorSelection> {
    let &(actor, character) = inventory
        .roster
        .get(memory.slot)
        .context("item picker slot is absent")?;
    let character = usize::from(character)
        .checked_sub(1)
        .filter(|i| *i < 9)
        .context("invalid item picker character")?;
    let live = battle
        .actors()
        .get(actor.index())
        .context("item picker actor is absent")?;
    Ok(ActorSelection {
        actor,
        slot: memory.slot as u8,
        name: inventory.names[character].to_owned(),
        eligible: match item {
            Some(item) => battle.item_target_eligible(item, actor)?,
            None => live.available(),
        },
    })
}

#[derive(Debug)]
struct List {
    rows: Vec<ItemRow>,
    row: usize,
    first: usize,
    scroll: i8,
    transition: Transition,
    closing: Option<Option<u16>>,
    previous: u16,
    blend: u8,
}
impl List {
    fn open(memory: &Memory, inventory: &Inventory<'_>, battle: &Battle) -> Result<Self> {
        let mut rows = vec![];
        for (&id, &count) in inventory.counts {
            let definition = inventory
                .definitions
                .get(usize::from(id))
                .context("inventory item definition is absent")?;
            if count == 0
                || !definition.battle_usable
                || !matches!(definition.category, 1..=6 | 43 | 44 | 46 | 47)
            {
                continue;
            }
            rows.push(ItemRow {
                id,
                count,
                recent: inventory.recent.contains(&id),
                urgent: urgent(definition.attention, inventory, battle)?,
            });
        }
        rows.sort_by_key(|row| (inventory.definitions[usize::from(row.id)].category, row.id));
        let (row, first) = if memory.row < rows.len() {
            let first = memory.first.min(memory.row / 2 * 2) & !1;
            (
                memory.row,
                if memory.row < first + 18 {
                    first
                } else {
                    memory.row / 2 * 2
                },
            )
        } else {
            (0, 0)
        };
        Ok(Self {
            rows,
            row,
            first,
            scroll: 0,
            transition: Transition::opening(),
            closing: None,
            previous: 0,
            blend: 240,
        })
    }

    fn selected(&self) -> Option<u16> {
        self.rows.get(self.row).map(|r| r.id)
    }

    fn step(
        &mut self,
        input: InventoryInput,
        supported: impl Fn(u16) -> bool,
    ) -> (ListFrame, Option<Option<u16>>, Vec<u16>) {
        if self.blend == 0 {
            self.previous = self.selected().unwrap_or(0);
        }
        let mut returned = None;
        let mut cues = vec![];
        if self.scroll != 0 {
            self.scroll = (self.scroll + self.scroll.signum()) % 5;
        }
        match self.transition.advance() {
            TransitionStatus::Closed => returned = self.closing,
            TransitionStatus::Ready if self.scroll == 0 => match input {
                Some(MenuAction::Cancel) => {
                    self.closing = Some(None);
                    self.transition.close();
                    cues.push(3);
                }
                Some(MenuAction::Confirm) if !self.rows.is_empty() => {
                    let item = self.selected().unwrap();
                    if supported(item) {
                        self.closing = Some(Some(item));
                        self.transition.close();
                        cues.push(2);
                    } else {
                        cues.push(4);
                    }
                }
                Some(MenuAction::Details) => cues.push(1),
                action if !self.rows.is_empty() => self.navigate(action, &mut cues),
                _ => {}
            },
            _ => {}
        }
        let current = self.selected().unwrap_or(0);
        if current != 0 {
            fade_description(&mut self.blend, current != self.previous);
        }
        (self.frame(), returned, cues)
    }

    fn frame(&self) -> ListFrame {
        ListFrame {
            rows: self.rows.clone(),
            selected: self.row,
            first: self.first,
            scroll: self.scroll,
            fade: self.transition.page_fade,
            description_previous: self.previous,
            description_blend: self.blend,
        }
    }

    fn navigate(&mut self, action: Option<MenuAction>, cues: &mut Vec<u16>) {
        let before = self.row;
        match action {
            Some(MenuAction::Left) if self.row > 0 => self.row -= 1,
            Some(MenuAction::Right) if self.row + 1 < self.rows.len() => self.row += 1,
            Some(MenuAction::Up) if self.row >= 2 => self.row -= 2,
            Some(MenuAction::Down) if self.row + 2 < self.rows.len() => self.row += 2,
            Some(MenuAction::PageDown) if self.rows.len().div_ceil(2) > self.first / 2 + 9 => {
                self.first += 18;
                self.row = (self.row + 18).min(self.rows.len() - 1);
                cues.push(38);
                return;
            }
            Some(MenuAction::PageUp) if self.first != 0 => {
                let next = self.first.saturating_sub(18);
                self.row -= self.first - next;
                self.first = next;
                cues.push(38);
                return;
            }
            _ => (),
        }
        if self.row == before {
            return;
        }
        cues.push(1);
        if self.row < self.first && self.first != 0 {
            self.first -= 2;
            self.scroll = -1;
        } else if self.row >= self.first + 18 {
            self.first += 2;
            self.scroll = 1;
        }
    }
}

fn urgent(
    attention: Option<resonance_content::menu_data::ItemAttention>,
    inventory: &Inventory<'_>,
    battle: &Battle,
) -> Result<bool> {
    use resonance_battle::{ActorAvailability, conditions::Cure};
    for &(id, _) in inventory.roster {
        let actor = battle
            .actors()
            .get(id.index())
            .context("item urgency actor is absent")?;
        let knocked_out = actor.availability == ActorAvailability::Dead;
        let needs = crate::menu::items::ItemNeeds {
            hp: [i64::from(actor.hp), i64::from(actor.equipment.max_hp)],
            tp: [i64::from(actor.tp), i64::from(actor.equipment.max_tp)],
            knocked_out,
            physical_ailment: !knocked_out && actor.conditions.needs_cure(Cure::Physical),
            magical_ailment: !knocked_out && actor.conditions.needs_cure(Cure::AntiMagic),
        };
        if needs.urgent(attention) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(count: u16) -> List {
        List {
            rows: (1..=count)
                .map(|id| ItemRow {
                    id,
                    count: 2,
                    recent: false,
                    urgent: false,
                })
                .collect(),
            row: 0,
            first: 0,
            scroll: 0,
            transition: Transition::opening(),
            closing: None,
            previous: 0,
            blend: 240,
        }
    }
    fn ready(count: u16) -> List {
        let mut list = list(count);
        for _ in 0..16 {
            list.step(InventoryInput::default(), |_| true);
        }
        list
    }

    #[test]
    fn inventory_returns_the_confirmed_item_after_closing() {
        let mut list = ready(2);
        let (_, returned, cues) = list.step(Some(MenuAction::Confirm), |_| true);
        assert_eq!((returned, cues), (None, vec![2]));
        let mut choice = None;
        for _ in 0..16 {
            let (_, returned, _) = list.step(InventoryInput::default(), |_| true);
            if returned.is_some() {
                choice = returned;
                break;
            }
        }
        assert_eq!(choice, Some(Some(1)));
    }

    #[test]
    fn cancel_closes_and_confirm_keeps_the_current_selection() {
        let mut list = ready(3);
        list.step(Some(MenuAction::Cancel), |_| true);
        assert_eq!(list.closing, Some(None));
        assert_eq!(list.row, 0);
        let mut list = ready(3);
        list.step(Some(MenuAction::Confirm), |_| true);
        assert_eq!(list.closing, Some(Some(1)));
        assert_eq!(list.row, 0);
    }

    #[test]
    fn inventory_scrolling_keeps_the_selected_row_visible() {
        let mut list = ready(32);
        list.row = 17;
        list.step(Some(MenuAction::Right), |_| true);
        assert_eq!((list.row, list.first), (18, 2));
        for _ in 0..6 {
            list.step(InventoryInput::default(), |_| true);
        }
        assert_eq!(list.scroll, 0);
        list.step(Some(MenuAction::PageDown), |_| true);
        assert_eq!(list.row, 31);
        assert!((list.first..list.first + 18).contains(&list.row));
        list.step(Some(MenuAction::PageUp), |_| true);
        assert!(list.row < 31);
        assert!((list.first..list.first + 18).contains(&list.row));
    }

    #[test]
    fn anti_magic_bottle_row_enters_close_from_the_cold_battle_menu() {
        let mut list = ready(14);
        list.row = 12;
        let (frame, returned, cues) = list.step(Some(MenuAction::Confirm), |item| {
            crate::battle::items::policies().contains_key(&item)
        });
        assert_eq!(frame.rows.len(), 14);
        assert_eq!(frame.rows[frame.selected].id, 13);
        assert_eq!(cues, [2]);
        assert_eq!(returned, None);
        assert_eq!(list.closing, Some(Some(13)));
    }

    #[test]
    fn unsupported_rows_remain_visible_and_never_enter_a_close_or_queue() {
        let mut list = ready(23);
        list.row = 21;
        let (frame, returned, cues) = list.step(Some(MenuAction::Confirm), |item| {
            crate::battle::items::policies().contains_key(&item)
        });
        assert_eq!(frame.rows[frame.selected].id, 22);
        assert_eq!(cues, [4]);
        assert!(returned.is_none() && list.closing.is_none());
        let (_, _, cues) = list.step(Some(MenuAction::Details), |item| {
            crate::battle::items::policies().contains_key(&item)
        });
        assert_eq!(cues, [1]);
    }

    #[test]
    fn details_action_does_not_also_move_the_inventory_selection() {
        let mut list = ready(32);
        let (_, _, cues) = list.step(Some(MenuAction::Details), |_| true);
        assert_eq!(cues, [1]);
        assert_eq!(list.row, 0);
    }

    #[test]
    fn empty_inventory_can_cancel_but_cannot_confirm_or_age_description() {
        let mut list = ready(0);
        let (frame, _, cues) = list.step(Some(MenuAction::Confirm), |_| true);
        assert_eq!(frame.description_blend, 240);
        assert!(cues.is_empty() && list.closing.is_none());
        let (_, _, cues) = list.step(Some(MenuAction::Cancel), |_| true);
        assert_eq!(cues, [3]);
        assert_eq!(list.closing, Some(None));
    }
    #[test]
    fn shared_cursor_survives_a_new_owner_and_clamps_to_filtered_inventory() {
        use resonance_content::menu_data::Item;
        use std::collections::BTreeMap;
        let definitions: Vec<_> = (0..=32)
            .map(|_| Item {
                category: 1,
                field_usable: false,
                battle_usable: true,
                view: None,
                equipment_stats: [0; 7],
                properties: Default::default(),
                price: 0,
                transforms_to: 0,
                field_use: None,
                attention: None,
            })
            .collect();
        let counts: BTreeMap<_, _> = (1..=32).map(|id| (id, 1)).collect();
        let (mut battle, setup) = super::super::tests::battle();
        let id = setup.actors[0];
        let roster = [(id, 1)];
        let inventory = Inventory {
            counts: &counts,
            definitions: &definitions,
            recent: &[1],
            roster: &roster,
            names: ["Party"; 9],
        };
        let mut list = ready(32);
        list.row = 20;
        list.first = 4;
        let mut inner = Owner {
            phase: Phase::Inventory(list),
            user: Some(id),
        };
        let mut memory = Memory::default();
        inner
            .step(
                Input {
                    shared_menu: Some(MenuAction::Cancel),
                    ..Default::default()
                },
                id,
                &mut memory,
                &inventory,
                &mut battle,
            )
            .unwrap();
        let mut closed = false;
        for _ in 0..32 {
            if inner
                .step(Input::default(), id, &mut memory, &inventory, &mut battle)
                .unwrap()
                .outcome
                == Outcome::Strip
            {
                closed = true;
                break;
            }
        }
        assert!(closed, "inventory should finish its closing animation");
        let first_owner = super::super::Owner {
            item_memory: memory,
            ..Default::default()
        };
        let retained = first_owner.memory().inventory;
        assert_eq!(
            retained,
            InventoryMemory {
                selected: 20,
                first: 4
            }
        );
        let mut next_owner = super::super::Owner::default();
        next_owner.restore_memory(super::super::Memory {
            inventory: retained,
            ..Default::default()
        });
        let reopened = List::open(&next_owner.item_memory, &inventory, &battle).unwrap();
        assert_eq!((reopened.row, reopened.first), (20, 4));
        assert!(reopened.rows[0].recent);
        assert!(!reopened.rows[0].urgent);
        let fewer = BTreeMap::from([(1, 1), (2, 1)]);
        let inventory = Inventory {
            counts: &fewer,
            ..inventory
        };
        let reopened = List::open(&next_owner.item_memory, &inventory, &battle).unwrap();
        assert_eq!((reopened.row, reopened.first), (0, 0));
    }
    #[test]
    #[ignore = "requires current prepared battle assets; CPU only"]
    fn lens_picker_returns_to_inventory_when_its_last_target_becomes_ineligible() -> Result<()> {
        let (mut candidate, mut battle, _) = crate::battle::results::item_tests::fixture(&[4], 2)?;
        let inventory = candidate.items();
        let stock = inventory.counts.clone();
        let user = inventory.roster[0].0;
        let target = battle
            .actor_ids()
            .find(|&id| battle.item_target_eligible(37, id).unwrap())
            .context("fixture has no eligible Lens target")?;
        let mut owner = Owner {
            phase: Phase::Enemy { item: 37, target },
            user: Some(user),
        };
        let mut memory = Memory::default();
        assert!(matches!(owner.view(), View::Enemy { target: id } if id == target));
        assert!(battle.pending_item().is_none());

        // Cleanup invalidates every enemy while the picker still holds its prior target.
        battle.recognize_escape(true)?;
        assert!(battle.recognize_result().is_some());
        battle.retire_combat()?;
        battle.hide_result_enemies()?;
        assert_eq!(battle.cycle_item_target(37, user, target, 1)?, None);
        let visit = owner.step(
            Input {
                confirm_a: resonance_battle::ButtonInput {
                    pressed: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            user,
            &mut memory,
            &inventory,
            &mut battle,
        )?;
        assert_eq!(visit.outcome, Outcome::Stay);
        assert!(visit.cues.contains(&4));
        assert!(matches!(owner.view(), View::Inventory(_)));
        assert!(battle.pending_item().is_none());

        let mut cancelled = false;
        for _ in 0..64 {
            let visit = owner.step(
                Input {
                    shared_menu: Some(MenuAction::Cancel),
                    ..Default::default()
                },
                user,
                &mut memory,
                &inventory,
                &mut battle,
            )?;
            if visit.outcome == Outcome::Strip {
                cancelled = true;
                break;
            }
        }
        assert!(
            cancelled,
            "inventory should remain cancellable after losing the Lens target"
        );
        assert!(battle.pending_item().is_none());
        crate::battle::results::item_tests::step(&mut candidate, &mut battle)?;
        assert_eq!(candidate.items().counts, &stock);
        assert!(battle.pending_item().is_none());
        assert!(!battle.enemy_scanned(target)?);
        Ok(())
    }

    #[test]
    fn urgency_uses_live_cure_policy_and_wide_vitals() -> Result<()> {
        use resonance_battle::{
            PreparedBattle,
            conditions::{
                Condition::{AttackDown, AttackUp, Petrified, Weak},
                ConditionSet, Conditions, Layers,
            },
        };
        use resonance_content::menu_data::ItemAttention;
        use std::collections::BTreeMap;
        let (initial, setup) = super::super::tests::battle();
        let roster = [(setup.actors[0], 1)];
        let inventory = Inventory {
            counts: &BTreeMap::new(),
            definitions: &[],
            recent: &[],
            roster: &roster,
            names: ["Party"; 9],
        };
        for (base, intrinsic, petrified, expected) in [
            (
                Petrified.into(),
                ConditionSet::EMPTY,
                false,
                [true, true, false],
            ),
            (ConditionSet::EMPTY, Petrified.into(), true, [false; 3]),
            (Weak.into(), ConditionSet::EMPTY, false, [false, true, true]),
            (
                AttackDown.into(),
                ConditionSet::EMPTY,
                false,
                [false, true, true],
            ),
            (AttackUp.into(), ConditionSet::EMPTY, false, [false; 3]),
        ] {
            let mut actors = initial.actors().to_vec();
            actors[0].conditions = Conditions::new(Layers {
                base,
                intrinsic,
                ..Default::default()
            });
            actors[0].availability = if petrified {
                resonance_battle::ActorAvailability::Petrified
            } else {
                resonance_battle::ActorAvailability::Active
            };
            let live = PreparedBattle::new(
                (actors)
                    .into_iter()
                    .map(|actor| (actor, Default::default()))
                    .collect(),
                Default::default(),
                1,
            )?
            .finish()?;
            for (attention, expected) in [
                ItemAttention::Ailment,
                ItemAttention::AllAilments,
                ItemAttention::MagicalAilment,
            ]
            .into_iter()
            .zip(expected)
            {
                assert_eq!(urgent(Some(attention), &inventory, &live)?, expected);
            }
        }
        for low in [false, true] {
            let mut actors = initial.actors().to_vec();
            actors[0].equipment.max_hp = i32::MAX;
            actors[0].hp = if low { 1 } else { i32::MAX };
            actors[0].equipment.max_tp = u16::MAX;
            actors[0].tp = if low { 1 } else { u16::MAX };
            let live = PreparedBattle::new(
                (actors)
                    .into_iter()
                    .map(|actor| (actor, Default::default()))
                    .collect(),
                Default::default(),
                1,
            )?
            .finish()?;
            for attention in [
                ItemAttention::LowHp,
                ItemAttention::LowTp,
                ItemAttention::LowVitals,
            ] {
                assert_eq!(urgent(Some(attention), &inventory, &live)?, low);
            }
        }
        Ok(())
    }
}

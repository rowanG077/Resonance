//! Queued item use and the Magic Lens panel.
//! The caller lends inventory and knowledge for one synchronous release.
use crate::state::ActorTask;
use crate::{ActorId, Battle, Cue, PreparedBattle, Side};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, btree_map::OccupiedEntry},
    sync::Arc,
};

/// Shared delay between successful item releases, in simulation ticks.
pub const ITEM_COOLDOWN_TICKS: u16 = 120;

mod release;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Release {
    pub user: ActorId,
    pub target: ActorId,
    pub item: u16,
}

/// Acquisition is the release's final fallible operation. A scan loan may
/// insert its persistent knowledge row, so the caller must finish immediately.
pub trait Provider {
    fn acquire_item(&mut self, request: Release) -> Result<ItemLoan<'_>>;
    fn acquire_scan(&mut self, request: Release) -> Result<ScanLoan<'_>>;
}

pub struct Unavailable;
impl Provider for Unavailable {
    fn acquire_item(&mut self, _: Release) -> Result<ItemLoan<'_>> {
        anyhow::bail!("battle item storage is not available")
    }
    fn acquire_scan(&mut self, _: Release) -> Result<ScanLoan<'_>> {
        anyhow::bail!("battle scan storage is not available")
    }
}

#[must_use = "a validated release must consume its inventory loan"]
pub struct ItemLoan<'a> {
    stack: OccupiedEntry<'a, u16, u8>,
    remaining: u8,
    gel_history: Option<&'a mut bool>,
}
impl<'a> ItemLoan<'a> {
    pub fn new(
        stack: OccupiedEntry<'a, u16, u8>,
        gel_history: Option<&'a mut bool>,
    ) -> Result<Self> {
        let remaining = stack
            .get()
            .checked_sub(1)
            .context("empty battle item stack")?;
        Ok(Self {
            stack,
            remaining,
            gel_history,
        })
    }
    pub fn record_gel_use(&mut self) {
        if let Some(history) = &mut self.gel_history {
            **history = true;
        }
    }
    pub fn consume(mut self) {
        if self.remaining == 0 {
            self.stack.remove();
        } else {
            *self.stack.get_mut() = self.remaining;
        }
    }
}

#[must_use = "a validated scan must consume its inventory loan"]
pub struct ScanLoan<'a> {
    item: ItemLoan<'a>,
    scanned: &'a mut bool,
    location: Option<&'a mut bool>,
}
impl<'a> ScanLoan<'a> {
    pub fn new(item: ItemLoan<'a>, scanned: &'a mut bool, location: Option<&'a mut bool>) -> Self {
        Self {
            item,
            scanned,
            location,
        }
    }
    pub fn scan(&mut self) -> bool {
        let mut learned = !*self.scanned;
        *self.scanned = true;
        if let Some(location) = &mut self.location {
            learned |= !**location;
            **location = true;
        }
        learned
    }
    pub fn consume(self) {
        self.item.consume();
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActorDefinition {
    /// Requested visual pose; item timing and release do not depend on it.
    pub motion: Option<crate::MotionBinding>,
    pub quick: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TargetDefinition {
    pub inactive_selectable: bool,
    pub excluded: bool,
}

/// Behavior prepared by the game boundary. Item IDs remain inventory/notice keys only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Recover { hp: u8, tp: u8 },
    PartyRecover { hp: u8, tp: u8 },
    FullRecovery,
    Revive,
    Cure(crate::conditions::Cure),
    Buff(crate::conditions::Buff),
    Scan,
    AllDivide,
    Hourglass,
}

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub effect: Effect,
    pub records_gel_use: bool,
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub policies: Arc<BTreeMap<u16, Policy>>,
    pub actors: Vec<Option<ActorDefinition>>,
    pub targets: Vec<TargetDefinition>,
}

#[derive(Clone, Copy)]
pub(crate) struct Use {
    pub action: crate::ActionId,
    pub elapsed: u32,
    duration: u32,
}

const RELEASE_TICKS: u32 = 20;
const USE_TICKS: u32 = 60;
const QUICK_USE_TICKS: u32 = 30;
const HOURGLASS_TICKS: u16 = 300;

#[derive(Default)]
pub(crate) struct State {
    pub pending: Option<Release>,
    pub cooldown: u16,
    /// All-Divide remains active until the battle ends.
    pub all_divide: bool,
    pub revealed: u16,
}

impl PreparedBattle {
    pub fn with_items(mut self, definition: Definition) -> Result<Self> {
        ensure!(self.resources.items.is_none(), "duplicate item preparation");
        ensure!(
            definition.actors.len() == self.actors.len()
                && definition.targets.len() == self.actors.len(),
            "item/actor count differs"
        );
        for (&item, policy) in definition.policies.iter() {
            ensure!(item != 0, "empty inventory item identity");
            if let Effect::Recover { hp, tp } | Effect::PartyRecover { hp, tp } = policy.effect {
                ensure!(
                    hp <= 100 && tp <= 100 && (hp != 0 || tp != 0),
                    "invalid item recovery percentages"
                );
            }
        }
        for (actor, binding) in self.actors.iter().zip(&definition.actors) {
            ensure!(
                binding.is_some() == (actor.side == Side::Party),
                "item user binding differs from side"
            );
        }
        self.resources.items = Some(definition);
        Ok(self)
    }
}

impl Battle {
    pub fn item_policy(&self, item: u16) -> Option<&Policy> {
        self.prepared.items.as_ref()?.policies.get(&item)
    }
    pub fn pending_item(&self) -> Option<Release> {
        self.items.pending
    }
    pub fn item_cooldown(&self) -> u16 {
        self.items.cooldown
    }
    pub fn all_divide_active(&self) -> bool {
        self.items.all_divide
    }
    /// Longest remaining actor freeze; no separate shared clock.
    pub fn hourglass_remaining(&self) -> u16 {
        self.actors
            .iter()
            .map(|actor| actor.time_stop)
            .max()
            .unwrap_or(0)
    }
    pub fn enemy_scanned(&self, actor: ActorId) -> Result<bool> {
        ensure!(
            self.actor(actor)?.side == Side::Enemy,
            "scan reveal needs an enemy"
        );
        Ok(self.items.revealed & (1 << actor.index()) != 0)
    }
    pub fn can_queue_item(&self, user: ActorId) -> Result<bool> {
        let actor = self.actor(user)?;
        Ok(self.phase() == crate::BattlePhase::Combat
            && actor.side == Side::Party
            && actor.available()
            && self.items.cooldown == 0
            && self.items.pending.is_none()
            && self
                .prepared
                .items
                .as_ref()
                .is_some_and(|items| items.actors[user.index()].is_some()))
    }
    pub fn item_target_eligible(&self, item: u16, target: ActorId) -> Result<bool> {
        let actor = self.actor(target)?;
        let effect = self
            .item_policy(item)
            .context("unprepared battle item")?
            .effect;
        Ok(match effect {
            Effect::Scan => {
                let flags = self.prepared.items.as_ref().unwrap().targets[target.index()];
                actor.side == Side::Enemy
                    && !flags.excluded
                    && (actor.available() || flags.inactive_selectable)
            }
            Effect::Cure(cure) => {
                actor.side == Side::Party
                    && matches!(
                        actor.availability,
                        crate::ActorAvailability::Active | crate::ActorAvailability::Petrified
                    )
                    && cure.eligible_base(actor.conditions.base())
            }
            Effect::Revive => {
                actor.side == Side::Party && actor.availability == crate::ActorAvailability::Dead
            }
            _ => actor.side == Side::Party && actor.available(),
        })
    }

    pub fn queue_item(&mut self, request: Release) -> Result<()> {
        ensure!(
            self.can_queue_item(request.user)?,
            "battle item user or reservation is unavailable"
        );
        ensure!(
            self.item_target_eligible(request.item, request.target)?,
            "battle item target is ineligible"
        );
        self.items.pending = Some(request);
        Ok(())
    }
    pub fn cycle_item_target(
        &self,
        item: u16,
        user: ActorId,
        current: ActorId,
        step: i8,
    ) -> Result<Option<ActorId>> {
        ensure!(matches!(step, -1 | 1), "item target step must be -1 or 1");
        ensure!(
            self.actor(user)?.side == Side::Party,
            "item selector needs a party user"
        );
        self.actor(current)?;
        for offset in 1..=self.actors.len() {
            let index = (current.index() as isize + offset as isize * isize::from(step))
                .rem_euclid(self.actors.len() as isize) as usize;
            let target = ActorId(index as u8);
            if self.item_target_eligible(item, target)? {
                return Ok(Some(target));
            }
        }
        Ok(None)
    }
    pub(crate) fn has_pending_item(&self, actor: ActorId) -> bool {
        self.items
            .pending
            .is_some_and(|pending| pending.user == actor)
    }
    pub(crate) fn cancel_pending_item(&mut self, actor: ActorId) {
        if self.has_pending_item(actor) {
            self.items.pending = None;
        }
    }
    pub(crate) fn start_pending_item(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let index = actor.index();
        if !self.has_pending_item(actor)
            || self.phase() != crate::BattlePhase::Combat
            || !self.actors[index].available()
            || self.activity(ActorId(index as u8)) != crate::Activity::Idle
            || !matches!(self.runtime[index].task(), ActorTask::None)
        {
            return Ok(());
        }
        let binding = self
            .prepared
            .items
            .as_ref()
            .and_then(|items| items.actors[actor.index()])
            .context("missing item user binding")?;
        let action = self.allocate_action_id()?;
        self.interrupt_actor(actor, cues);
        let owner = &mut self.actors[actor.index()];

        owner.reaction.protection.item();
        owner.movement.gravity = -f32::from(u8::from(!owner.movement.flying));
        owner.movement.locomotion = crate::Locomotion::Action;
        self.set_task(
            actor.index(),
            ActorTask::Item(Use {
                action,
                elapsed: 0,
                duration: if binding.quick {
                    QUICK_USE_TICKS
                } else {
                    USE_TICKS
                },
            }),
        );
        cues.push(Cue::Started {
            action,
            actor,
            definition: None,
        });
        self.request_pose(actor, binding.motion, crate::Pose::default());
        Ok(())
    }

    pub(crate) fn advance_item(
        &mut self,
        actor: ActorId,
        mut item: Use,
        provider: &mut dyn Provider,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        if !self.actors[index].available() {
            self.interrupt_actor(actor, cues);
            return Ok(());
        }
        item.elapsed += 1;
        self.set_task(index, ActorTask::Item(item));
        if item.elapsed == RELEASE_TICKS
            && let Err(error) = self.release_item(actor, provider, cues)
        {
            self.interrupt_actor(actor, cues);
            self.enter_idle(actor);
            self.diagnostic = true;
            self.diagnostics.report("battle item", error)?;
            return Ok(());
        }
        if item.elapsed >= item.duration {
            self.enter_idle(actor);
            cues.push(Cue::Completed {
                action: item.action,
            });
        }
        Ok(())
    }

    /// Clear frozen actors when leaving combat.
    pub(crate) fn clear_hourglass(&mut self) {
        for actor in &mut self.actors {
            actor.time_stop = 0;
        }
    }
}

#[cfg(test)]
mod tests;

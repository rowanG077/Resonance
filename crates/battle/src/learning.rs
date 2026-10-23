//! Learning rules evaluated after a technique use. Only prepared actions can be published.
use crate::TechniqueId;
use anyhow::{Context, Result, ensure};
use resonance_content::arte::{Catalogue, LearningRoute, LearningRules};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const BASE_LEARNING_ODDS: u32 = 8;
const MIN_SUCCESSOR_CHANCE_PERCENT: u32 = 5;

#[derive(Debug, Clone)]
pub struct LearningCatalogue {
    data: Arc<Catalogue>,
    learn_elemental_artes: bool,
    learn_both_arte_paths: bool,
}

impl LearningCatalogue {
    pub fn new(catalogue: Arc<Catalogue>) -> Self {
        Self {
            data: catalogue,
            learn_elemental_artes: false,
            learn_both_arte_paths: false,
        }
    }

    pub fn with_elemental_learning(mut self, enabled: bool) -> Self {
        self.learn_elemental_artes = enabled;
        self
    }

    pub fn with_both_arte_paths(mut self, enabled: bool) -> Self {
        self.learn_both_arte_paths = enabled;
        self
    }

    pub fn prepare_member(&self, mut entry: LearningEntry) -> Result<LearningMember> {
        let allowed: BTreeSet<TechniqueId> = self
            .data
            .validate_learning(entry.character as u8)?
            .iter()
            .copied()
            .map(TechniqueId)
            .collect();
        ensure!(
            entry.level != 0 && entry.current.is_subset(&allowed),
            "invalid learning member"
        );
        ensure!(
            entry
                .counts
                .iter()
                .all(|(id, count)| allowed.contains(id)
                    && *count <= resonance_content::arte::MAX_USES),
            "invalid technique usage history"
        );
        for &id in &entry.current {
            entry.counts.entry(id).or_insert(0);
        }
        Ok(LearningMember {
            catalogue: self.clone(),
            entry,
            allowed,
        })
    }

    fn row(&self, id: TechniqueId) -> &resonance_content::arte::Definition {
        &self.data.definitions[id.index()]
    }
}

#[derive(Debug, Clone)]
pub struct LearningEntry {
    pub character: crate::Character,
    pub level: u8,
    pub balance: i8,
    pub story_unlocked: bool,
    pub current: BTreeSet<TechniqueId>,
    pub counts: BTreeMap<TechniqueId, u16>,
}

#[derive(Debug, Clone)]
pub struct LearningMember {
    catalogue: LearningCatalogue,
    entry: LearningEntry,
    allowed: BTreeSet<TechniqueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearningMode {
    Martial,
    Casting,
}

#[derive(Debug, Clone, Copy)]
pub struct LearningAttempt {
    pub mode: LearningMode,
    pub current: Option<TechniqueId>,
    pub airborne: bool,
}

impl LearningMember {
    pub fn current(&self) -> &BTreeSet<TechniqueId> {
        &self.entry.current
    }
    pub fn counts(&self) -> &BTreeMap<TechniqueId, u16> {
        &self.entry.counts
    }
    pub(crate) fn record_use(&mut self, id: TechniqueId) -> Option<()> {
        if !self.allowed.contains(&id) {
            return None;
        }
        let count = self.entry.counts.entry(id).or_default();
        *count = count
            .saturating_add(1)
            .min(resonance_content::arte::MAX_USES);
        Some(())
    }
    pub(crate) fn mark_acquired(&mut self, id: TechniqueId) -> Result<bool> {
        ensure!(
            self.allowed.contains(&id),
            "acquired technique is outside learning list"
        );
        self.entry.counts.entry(id).or_insert(0);
        Ok(self.entry.current.insert(id))
    }
    pub(crate) fn forget(&mut self, id: TechniqueId) -> Result<bool> {
        ensure!(
            self.allowed.contains(&id),
            "forgotten technique is outside learning list"
        );
        Ok(self.entry.current.remove(&id))
    }
    fn learned(&self, id: TechniqueId) -> bool {
        self.entry.current.contains(&id)
    }
    fn count(&self, id: TechniqueId) -> u16 {
        self.entry.counts.get(&id).copied().unwrap_or(0)
    }
    fn eligible(&self, id: TechniqueId, attempt: LearningAttempt) -> bool {
        let row = self.catalogue.row(id);
        let capabilities = row.capabilities;
        self.allowed.contains(&id)
            && !self.entry.current.contains(&id)
            && (!row.learning.requires_story_unlock || self.entry.story_unlocked)
            && row.required_level != 0
            && u16::from(self.entry.level) >= row.required_level
            && capabilities.spell == (attempt.mode == LearningMode::Casting)
            && (capabilities.regal_family == Some(crate::RegalArteFamily::Aerial))
                == attempt.airborne
    }

    /// Select from eligible rules before consuming randomness. Candidate order is stable by ID.
    pub fn select_after_count(
        &self,
        attempt: LearningAttempt,
        available: impl Fn(TechniqueId) -> bool,
        mut random: impl FnMut() -> u32,
    ) -> Result<Option<TechniqueId>> {
        if let Some(current) = attempt.current {
            ensure!(self.allowed.contains(&current), "unknown current technique");
            let row = self.catalogue.row(current);
            let spell = row.capabilities.spell;
            if spell != (attempt.mode == LearningMode::Casting) {
                return Ok(None);
            }
            if self.catalogue.learn_elemental_artes
                && resonance_content::arte::elemental::find(current.0).is_some()
                && !self.learned(current)
                && self.count(current) >= resonance_content::arte::elemental::LEARNING_USES
                && available(current)
            {
                return Ok(Some(current));
            }
            let (next, opposite) = if self.entry.balance <= 0 {
                (
                    row.learning.technical_successor.map(TechniqueId),
                    row.learning.strike_successor.map(TechniqueId),
                )
            } else {
                (
                    row.learning.strike_successor.map(TechniqueId),
                    row.learning.technical_successor.map(TechniqueId),
                )
            };
            let both_paths = self.catalogue.learn_both_arte_paths;
            for next in [next, opposite.filter(|_| both_paths)]
                .into_iter()
                .flatten()
            {
                let requirements = &self.catalogue.row(next).learning;
                if !available(next)
                    || !self.eligible(next, attempt)
                    || self.count(current) < requirements.parent_uses
                    || if both_paths {
                        !self.base_eligible(requirements)
                    } else {
                        opposite.is_some_and(|id| self.learned(id))
                    }
                {
                    continue;
                }
                let chance = (u32::from(self.entry.balance.unsigned_abs()) / 2)
                    .max(MIN_SUCCESSOR_CHANCE_PERCENT);
                if both_paths || random() % 100 < chance {
                    return Ok(Some(next));
                }
            }
            if attempt.mode == LearningMode::Martial {
                return Ok(None);
            }
        }
        for &id in &self.allowed {
            let row = self.catalogue.row(id);
            if available(id)
                && row.learning.parent.is_none()
                && self.eligible(id, attempt)
                && self.base_eligible(&row.learning)
                && (self.catalogue.learn_both_arte_paths
                    || random().is_multiple_of(BASE_LEARNING_ODDS))
            {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    fn base_eligible(&self, rule: &LearningRules) -> bool {
        let route_matches = match rule.route {
            Some(LearningRoute::Technical) => self.entry.balance <= 0,
            Some(LearningRoute::Strike) => self.entry.balance > 0,
            None => true,
        };
        (route_matches || self.catalogue.learn_both_arte_paths)
            && !rule.excludes.iter().any(|&id| {
                let id = TechniqueId(id);
                let opposite_path = rule
                    .route
                    .zip(self.catalogue.row(id).learning.route)
                    .is_some_and(|(candidate, excluded)| candidate != excluded);
                self.learned(id) && !(self.catalogue.learn_both_arte_paths && opposite_path)
            })
            && rule.prerequisites.iter().all(|group| {
                group.any_of.iter().any(|&id| {
                    self.learned(TechniqueId(id))
                        && self.count(TechniqueId(id)) >= group.minimum_uses
                })
            })
    }
}

/// Learned membership and learning operands for one actor. Prepared action
/// capacity is separate; catalogue metadata alone does not make an action usable.
#[derive(Debug, Clone)]
pub struct TechniqueLearningMember {
    pub actor: crate::ActorId,
    pub member: LearningMember,
}

/// A successful acquisition published by the battle owner. The action handle
/// names the existing actor-owned prepared technique; the event creates no executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechniqueAcquisition {
    pub actor: crate::ActorId,
    pub catalogue: TechniqueId,
    pub action: crate::ActionKey,
}

impl crate::PreparedBattle {
    /// Join saved-party learning parameters. Acquisitions update the battle-local copy and are
    /// persisted with results.
    pub fn with_technique_learning_members(
        mut self,
        rows: Vec<TechniqueLearningMember>,
    ) -> Result<Self> {
        let mut actors = BTreeSet::new();
        for row in &rows {
            ensure!(
                self.actors
                    .get(row.actor.index())
                    .is_some_and(|actor| actor.side == crate::Side::Party),
                "technique learning member needs a prepared party actor"
            );
            ensure!(
                actors.insert(row.actor),
                "duplicate technique learning member"
            );
        }
        self.technique_learning_members = rows;
        Ok(self)
    }
}

impl crate::Battle {
    /// Read the current live membership for a prepared usage identity. A
    /// missing owner means this encounter has no learning projection for it.
    /// Historical ledger rows remain valid after the live member forgets one.
    pub fn current_techniques(&self, actor: crate::ActorId) -> Option<&BTreeSet<TechniqueId>> {
        self.learning_members
            .iter()
            .find(|row| row.actor == actor)
            .map(|row| row.member.current())
    }

    pub fn technique_is_current(
        &self,
        actor: crate::ActorId,
        catalogue: TechniqueId,
    ) -> Option<bool> {
        self.learning_members
            .iter()
            .find(|row| row.actor == actor)
            .map(|row| row.member.current().contains(&catalogue))
    }

    /// Publish one selected acquisition only when its complete action body was
    /// prepared in this encounter. Learning selection filters to this capacity;
    /// direct callers cannot publish an unavailable action.
    pub fn record_technique_acquisition(
        &mut self,
        actor: crate::ActorId,
        catalogue: TechniqueId,
    ) -> Result<crate::ActionKey> {
        ensure!(!self.ended, "technique acquisition after battle completion");
        let action = self
            .prepared
            .actor_setup
            .get(actor.index())
            .and_then(|setup| {
                setup
                    .techniques
                    .iter()
                    .find(|row| row.catalogue == catalogue)
            })
            .map(|row| row.action)
            .with_context(|| {
                format!(
                    "unprepared learning action for actor {} technique {}",
                    actor.index(),
                    catalogue
                )
            })?;
        let row = self
            .learning_members
            .iter_mut()
            .find(|row| row.actor == actor)
            .context("technique acquisition needs a learning member")?;
        if !row.member.mark_acquired(catalogue)? {
            return Ok(action);
        }
        if let Some(control) = self
            .runtime
            .get_mut(actor.index())
            .and_then(|state| state.control.as_mut())
        {
            control.disabled_techniques.remove(&action);
            if let Some(slot) = control.shortcuts.iter_mut().find(|id| id.is_none()) {
                *slot = Some(catalogue);
            }
        }
        self.ledger
            .technique_acquisitions
            .push(TechniqueAcquisition {
                actor,
                catalogue,
                action,
            });
        self.pending_cues.push(crate::Cue::TechniqueAcquired {
            actor,
            action,
            spell: matches!(
                &self.prepared.actions[action].execution,
                crate::ActionExecution::Casting(_)
            ),
        });
        Ok(action)
    }

    /// Ordered successful acquisition events for notices and observations.
    pub fn technique_acquisitions(&self) -> &[TechniqueAcquisition] {
        &self.ledger.technique_acquisitions
    }
}

#[cfg(test)]
mod tests;

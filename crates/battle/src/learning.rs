//! Learning rules evaluated after a technique use. Only prepared actions can be published.
use anyhow::{Context, Result, ensure};
use resonance_content::arte::{Catalogue, LearningRoute, LearningRules};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const BASE_LEARNING_ODDS: u32 = 8;
const MIN_SUCCESSOR_CHANCE_PERCENT: u32 = 5;

#[derive(Debug, Clone)]
pub struct LearningCatalogue(Arc<Catalogue>);

impl LearningCatalogue {
    pub fn new(catalogue: Arc<Catalogue>) -> Self {
        Self(catalogue)
    }

    pub fn prepare_member(&self, mut entry: LearningEntry) -> Result<LearningMember> {
        let allowed: BTreeSet<u16> = self
            .0
            .validate_learning(entry.character)?
            .iter()
            .copied()
            .map(u16::from)
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

    fn row(&self, id: u16) -> &resonance_content::arte::Definition {
        &self.0.definitions[usize::from(id)]
    }
}

#[derive(Debug, Clone)]
pub struct LearningEntry {
    pub character: u8,
    pub level: u8,
    pub balance: i8,
    pub story_unlocked: bool,
    pub current: BTreeSet<u16>,
    pub counts: BTreeMap<u16, u16>,
}

#[derive(Debug, Clone)]
pub struct LearningMember {
    catalogue: LearningCatalogue,
    entry: LearningEntry,
    allowed: BTreeSet<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearningMode {
    Martial,
    Casting,
}

#[derive(Debug, Clone, Copy)]
pub struct LearningAttempt {
    pub mode: LearningMode,
    pub current: Option<u16>,
    pub airborne: bool,
}

impl LearningMember {
    pub fn character(&self) -> u8 {
        self.entry.character
    }
    pub fn current(&self) -> &BTreeSet<u16> {
        &self.entry.current
    }
    pub fn techniques(&self) -> &[u8] {
        self.catalogue
            .0
            .learned_by(self.entry.character)
            .expect("validated learning member")
    }
    pub fn counts(&self) -> &BTreeMap<u16, u16> {
        &self.entry.counts
    }
    pub(crate) fn record_use(&mut self, id: u16) -> Option<()> {
        if !self.allowed.contains(&id) {
            return None;
        }
        let count = self.entry.counts.entry(id).or_default();
        *count = count
            .saturating_add(1)
            .min(resonance_content::arte::MAX_USES);
        Some(())
    }
    pub(crate) fn mark_acquired(&mut self, id: u16) -> Result<bool> {
        ensure!(
            self.allowed.contains(&id),
            "acquired technique is outside learning list"
        );
        self.entry.counts.entry(id).or_insert(0);
        Ok(self.entry.current.insert(id))
    }
    pub(crate) fn forget(&mut self, id: u16) -> Result<bool> {
        ensure!(
            self.allowed.contains(&id),
            "forgotten technique is outside learning list"
        );
        Ok(self.entry.current.remove(&id))
    }
    fn learned(&self, id: u16) -> bool {
        self.entry.current.contains(&id)
    }
    fn count(&self, id: u16) -> u16 {
        self.entry.counts.get(&id).copied().unwrap_or(0)
    }
    fn eligible(&self, id: u16, attempt: LearningAttempt) -> bool {
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
        available: impl Fn(u16) -> bool,
        mut random: impl FnMut() -> u32,
    ) -> Result<Option<u16>> {
        if let Some(current) = attempt.current {
            ensure!(self.allowed.contains(&current), "unknown current technique");
            let row = self.catalogue.row(current);
            let spell = row.capabilities.spell;
            if spell != (attempt.mode == LearningMode::Casting) {
                return Ok(None);
            }
            let (next, opposite) = if self.entry.balance <= 0 {
                (
                    row.learning.technical_successor,
                    row.learning.strike_successor,
                )
            } else {
                (
                    row.learning.strike_successor,
                    row.learning.technical_successor,
                )
            };
            if let Some(next) = next
                && available(next)
                && !opposite.is_some_and(|id| self.learned(id))
                && self.eligible(next, attempt)
                && self.count(current) >= self.catalogue.row(next).learning.parent_uses
            {
                let chance = (u32::from(self.entry.balance.unsigned_abs()) / 2)
                    .max(MIN_SUCCESSOR_CHANCE_PERCENT);
                if random() % 100 < chance {
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
                && random().is_multiple_of(BASE_LEARNING_ODDS)
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
        route_matches
            && !rule.excludes.iter().any(|&id| self.learned(id))
            && rule.prerequisites.iter().all(|group| {
                group
                    .any_of
                    .iter()
                    .any(|&id| self.learned(id) && self.count(id) >= group.minimum_uses)
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
    pub catalogue: u16,
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
            ensure!(
                row.member.character() >= 1 && row.member.character() <= 9,
                "invalid technique learning character"
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
    pub fn current_techniques(&self, actor: crate::ActorId) -> Option<&BTreeSet<u16>> {
        self.learning_members
            .iter()
            .find(|row| row.actor == actor)
            .map(|row| row.member.current())
    }

    pub fn technique_is_current(&self, actor: crate::ActorId, catalogue: u16) -> Option<bool> {
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
        catalogue: u16,
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
            if let Some(slot) = control.shortcuts.iter_mut().find(|id| **id == 0) {
                *slot = catalogue;
            }
        }
        self.ledger
            .technique_acquisitions
            .push(TechniqueAcquisition {
                actor,
                catalogue,
                action,
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

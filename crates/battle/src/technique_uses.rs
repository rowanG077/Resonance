use crate::{ActorId, Battle, TechniqueId};
use anyhow::{Result, ensure};

impl Battle {
    pub(crate) fn technique_catalogue_for_action(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Option<TechniqueId> {
        self.prepared
            .technique(actor, action)
            .map(|row| row.catalogue)
    }

    pub fn technique_counts(
        &self,
        actor: ActorId,
    ) -> Option<&std::collections::BTreeMap<TechniqueId, u16>> {
        self.learning_members
            .iter()
            .find(|row| row.actor == actor)
            .map(|row| row.member.counts())
    }

    pub fn technique_uses(&self, actor: ActorId, catalogue: TechniqueId) -> Option<u16> {
        self.technique_counts(actor)?.get(&catalogue).copied()
    }

    fn record_technique_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        casting: bool,
    ) -> Option<TechniqueId> {
        let catalogue = self.technique_catalogue_for_action(actor, action)?;
        let is_casting = self.prepared.actions.get(action).is_some_and(|definition| {
            matches!(&definition.execution, crate::ActionExecution::Casting(_))
        });
        if is_casting != casting {
            return None;
        }
        self.learning_members
            .iter_mut()
            .find(|row| row.actor == actor)?
            .member
            .record_use(catalogue)?;
        let elements = self
            .prepared
            .technique(actor, action)
            .and_then(|row| row.element);
        self.ledger.achievements[actor.index()].acted = true;
        self.ledger.achievements[actor.index()].used_arte = true;
        self.ledger.achievements[actor.index()].technique(catalogue, elements, casting);
        Some(catalogue)
    }

    /// Unison records the selected arte at launch, without rolling a new technique.
    pub(crate) fn record_unison_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<()> {
        // Elemental variants track ordinary uses for permanent learning.
        if self
            .technique_catalogue_for_action(actor, action)
            .is_some_and(|id| resonance_content::arte::elemental::find(id.0).is_some())
        {
            return Ok(());
        }
        let casting = matches!(
            &self.prepared.actions[action].execution,
            crate::ActionExecution::Casting(_)
        );
        ensure!(
            self.record_technique_use(actor, action, casting).is_some(),
            "Unison arte has no learning member"
        );
        Ok(())
    }

    pub(crate) fn record_immediate_spell_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<()> {
        ensure!(
            self.record_technique_use(actor, action, true).is_some(),
            "immediate spell has no learning member"
        );
        Ok(())
    }

    fn learn_after_use(
        &mut self,
        actor: ActorId,
        current_catalogue: Option<TechniqueId>,
        mode: crate::learning::LearningMode,
    ) -> Result<Option<crate::ActionKey>> {
        if self.phase() != crate::BattlePhase::Combat {
            return Ok(None);
        }
        let Some(row) = self.learning_members.iter().find(|row| row.actor == actor) else {
            return Ok(None);
        };
        let attempt = crate::learning::LearningAttempt {
            mode,
            current: current_catalogue,
            airborne: self.actors[actor.index()].airborne(),
        };
        let random = &mut self.random;
        let capacity = &self.prepared.actor_setup[actor.index()].techniques;
        let selected = row.member.select_after_count(
            attempt,
            |id| capacity.iter().any(|technique| technique.catalogue == id),
            || u32::from(random.next_u16()),
        )?;
        selected
            .map(|selected| self.record_technique_acquisition(actor, selected))
            .transpose()
    }

    /// A successful acquisition replaces this admitted attack's first use.
    pub(crate) fn commit_martial_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<Option<crate::ActionKey>> {
        let current = self.record_technique_use(actor, action, false);
        self.learn_after_use(actor, current, crate::learning::LearningMode::Martial)
    }

    /// Return only the acquisition selected by this completed chant.
    pub(crate) fn commit_cast_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<Option<crate::ActionKey>> {
        let current = self.record_technique_use(actor, action, true);
        self.learn_after_use(actor, current, crate::learning::LearningMode::Casting)
    }

    pub(crate) fn snapshot_technique_proficiency(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) {
        let Some(catalogue) = self.technique_catalogue_for_action(actor, action) else {
            return;
        };
        let catalogue = resonance_content::arte::elemental::find(catalogue.0)
            .map_or(catalogue, |variant| TechniqueId(variant.base));
        self.actors[actor.index()].proficiency =
            (self.technique_uses(actor, catalogue).unwrap_or(0) / 50).min(5) as u8;
    }
}

#[cfg(test)]
mod tests;

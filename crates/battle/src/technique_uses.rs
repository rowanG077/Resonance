use crate::{ActorId, Battle};
use anyhow::{Result, ensure};

impl Battle {
    pub(crate) fn technique_catalogue_for_action(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Option<u16> {
        self.prepared
            .technique(actor, action)
            .map(|row| row.catalogue)
    }

    pub fn technique_counts(
        &self,
        actor: ActorId,
    ) -> Option<&std::collections::BTreeMap<u16, u16>> {
        self.learning_members
            .iter()
            .find(|row| row.actor == actor)
            .map(|row| row.member.counts())
    }

    pub fn technique_uses(&self, actor: ActorId, catalogue: u16) -> Option<u16> {
        self.technique_counts(actor)?.get(&catalogue).copied()
    }

    fn record_technique_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        casting: bool,
    ) -> Option<u16> {
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
        Some(catalogue)
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
        current_catalogue: Option<u16>,
        mode: crate::learning::LearningMode,
    ) -> Result<()> {
        if self.phase() != crate::BattlePhase::Combat {
            return Ok(());
        }
        let Some(row) = self.learning_members.iter().find(|row| row.actor == actor) else {
            return Ok(());
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
        if let Some(selected) = selected {
            self.record_technique_acquisition(actor, selected)?;
        }
        Ok(())
    }

    /// Learning updates future commands; the selected attack retains its identity and cost.
    pub(crate) fn commit_martial_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<()> {
        let current = self.record_technique_use(actor, action, false);
        self.learn_after_use(actor, current, crate::learning::LearningMode::Martial)
    }

    /// Learning changes the next command; the paid cast keeps its selected spell.
    pub(crate) fn commit_cast_use(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<()> {
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
        self.actors[actor.index()].proficiency =
            (self.technique_uses(actor, catalogue).unwrap_or(0) / 50).min(5) as u8;
    }
}

#[cfg(test)]
mod tests;

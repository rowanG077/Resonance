use crate::{ActorId, Battle, Control, PreparedBattle};
use anyhow::{Context, Result, ensure};

pub use resonance_content::arte::{
    ArteFamily, RegalArteFamily, TechniqueCapabilities, TechniqueTarget,
};

/// Immutable technique metadata shared by player commands and autonomous policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedTechnique {
    pub action: crate::ActionKey,
    pub catalogue: u16,
    pub player_range: [f32; 2],
    pub ai_range: [f32; 2],
    pub capabilities: TechniqueCapabilities,
    pub element: u8,
}

pub type AssistShortcuts = [Option<(ActorId, crate::ActionKey)>; 2];

impl crate::prepare::BattleResources {
    pub(crate) fn technique(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Option<&PreparedTechnique> {
        self.actor_setup
            .get(actor.index())?
            .techniques
            .iter()
            .find(|row| row.action == action)
    }
}

impl PreparedBattle {
    pub(crate) fn validate_actor_techniques(&self) -> Result<()> {
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            ensure!(
                setup.techniques.is_empty() || self.actors[index].side == crate::Side::Party,
                "techniques require a party actor"
            );
            let mut actions = std::collections::BTreeSet::new();
            let mut catalogues = std::collections::BTreeSet::new();
            for row in &setup.techniques {
                ensure!(row.catalogue > 0, "invalid technique catalogue identity");
                ensure!(
                    row.capabilities.target != TechniqueTarget::Unavailable,
                    "technique target is unavailable"
                );
                ensure!(
                    actions.insert(row.action) && catalogues.insert(row.catalogue),
                    "duplicate prepared technique identity"
                );
                ensure!(row.element < 9, "invalid technique element");
                for [minimum, maximum] in [row.player_range, row.ai_range] {
                    ensure!(
                        minimum.is_finite()
                            && maximum.is_finite()
                            && minimum >= 0.
                            && (maximum > minimum
                                || row.capabilities.target == TechniqueTarget::SelfTarget
                                    && minimum == 0.
                                    && maximum == 0.),
                        "invalid prepared technique range"
                    );
                }
                ensure!(
                    self.resources
                        .actions
                        .get(row.action)
                        .is_some_and(|action| action.normal.is_none()),
                    "technique needs a prepared action distinct from normal attacks"
                );
            }
        }
        Ok(())
    }
}

impl Battle {
    pub fn prepared_techniques(&self, actor: ActorId) -> &[PreparedTechnique] {
        self.prepared
            .actor_setup
            .get(actor.index())
            .map_or(&[], |setup| &setup.techniques)
    }

    pub fn prepared_technique(&self, actor: ActorId, catalogue: u16) -> Option<&PreparedTechnique> {
        self.prepared_techniques(actor)
            .iter()
            .find(|row| row.catalogue == catalogue)
    }

    /// Shared by menu confirmation and command admission. Revival selects a
    /// fallen ally; other techniques require an available recipient.
    pub fn technique_target_eligible(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
        target: ActorId,
    ) -> bool {
        let Some(row) = self.learned_technique(actor, action) else {
            return false;
        };
        let Some(owner) = self.actors.get(actor.index()) else {
            return false;
        };
        let Some(recipient) = self.actors.get(target.index()) else {
            return false;
        };
        let side_matches = match row.capabilities.target {
            TechniqueTarget::Unavailable => false,
            TechniqueTarget::Enemy => recipient.side != owner.side,
            TechniqueTarget::Ally => recipient.side == owner.side,
            TechniqueTarget::SelfTarget => target == actor,
        };
        side_matches
            && if row.capabilities.revives {
                recipient.availability == crate::ActorAvailability::Dead
            } else {
                recipient.available()
            }
    }

    /// Current membership selects from the actor's immutable action capacity.
    pub(crate) fn learned_technique(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Option<PreparedTechnique> {
        let row = self.prepared.technique(actor, action)?;
        if self
            .learning_members
            .iter()
            .find(|member| member.actor == actor)
            .is_some_and(|member| !member.member.current().contains(&row.catalogue))
        {
            return None;
        }
        Some(*row)
    }

    pub(crate) fn technique_available(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        self.learned_technique(actor, action).is_some()
    }

    pub fn pending_technique(&self, actor: ActorId) -> Option<crate::ActionKey> {
        Some(
            self.runtime
                .get(actor.index())?
                .control
                .as_ref()?
                .queued_technique?
                .action,
        )
    }

    pub fn pending_technique_issuer(&self, actor: ActorId) -> Option<u8> {
        self.runtime
            .get(actor.index())?
            .control
            .as_ref()?
            .queued_technique?
            .issuer
    }

    /// Target retained by an explicit Tech command, kept separate from the
    /// actor's ordinary decision target until the command is released.
    pub fn pending_technique_target(&self, actor: ActorId) -> Option<ActorId> {
        self.runtime
            .get(actor.index())?
            .control
            .as_ref()?
            .queued_technique?
            .target
    }

    pub(crate) fn clear_technique_command(&mut self, actor: ActorId) {
        if let Some(control) = &mut self.runtime[actor.index()].control {
            control.queued_technique = None;
        }
    }

    pub(crate) fn clear_matching_technique_command(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) {
        if self
            .pending_technique(actor)
            .is_some_and(|pending| pending == action)
        {
            self.clear_technique_command(actor);
        }
    }

    pub(crate) fn request_queued_technique(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        if self.actors[index].control != Control::Auto || !self.companion_mode_ready(actor) {
            return Ok(false);
        }
        let Some(control) = self.runtime[index].control.as_ref() else {
            return Ok(false);
        };
        let Some(pending) = control.queued_technique else {
            return Ok(false);
        };
        let action = pending.action;
        let Some(binding) = self.learned_technique(actor, action) else {
            return Ok(false);
        };
        let target = if let Some(target) = pending.target {
            target
        } else if binding.capabilities.target == TechniqueTarget::Enemy {
            let policy = self
                .companion_policy(actor)
                .context("queued command has no policy")?;
            let strategy = if policy.choices[0] == 0 {
                self.prepared.actor_setup[index]
                    .companion
                    .as_ref()
                    .context("queued command has no companion")?
                    .defaults[0]
            } else {
                policy.choices[0]
            };
            let Some(target) =
                self.decision_target(actor, crate::TargetPolicy::try_from(strategy)?)?
            else {
                return Ok(false);
            };
            target
        } else {
            let target = self
                .actors
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    self.technique_target_eligible(actor, action, ActorId(*index as u8))
                })
                .min_by_key(|(_, candidate)| candidate.hp_percent())
                .map(|(index, _)| ActorId(index as u8));
            let Some(target) = target else {
                return Ok(false);
            };
            target
        };
        if !self.technique_target_eligible(actor, action, target) {
            self.clear_technique_command(actor);
            return Ok(false);
        }
        if self.prepared.special_guard(actor) == Some(action) {
            return self.start_special_guard(actor, action, cues);
        }
        let Ok(candidate) = self.action_candidate(actor, action, binding.player_range) else {
            return Ok(false);
        };
        if binding.capabilities.target != TechniqueTarget::Enemy {
            let request = crate::ActionRequest {
                actor,
                target,
                action,
            };
            if self.action_rejection(request)?.is_some() {
                return Ok(false);
            }
            if crate::conditions::paralysis_controller_roll(&self.actors[index], &mut self.random) {
                self.clear_technique_command(actor);
                self.begin_paralysis(actor, cues)?;
                return Ok(true);
            }
            self.runtime[index].support_target = target;
            self.actors[index].movement.forward = 0.;
            self.start_admitted_action(request, cues)?;
            return Ok(true);
        }
        let definition = self.prepared.actor_setup[index]
            .control
            .as_ref()
            .context("queued command has no controller")?;
        let parameters = crate::ApproachParameters {
            minimum: candidate.range[0],
            maximum: candidate.range[1],
            motion: definition.motions.map(|motions| motions.run),
            motion_rate: 0.5,
            speed: definition.run_speed,
            turn_ticks: definition.turn_ticks,
        };
        let accepted = self.request_selected_approach(actor, target, action, parameters)?;
        if accepted {
            self.set_decision_target(actor, target)?;
        }
        Ok(accepted)
    }

    pub(crate) fn queue_pending_technique_chain(&mut self, actor: ActorId) -> Result<Option<bool>> {
        let Some(action) = self.pending_technique(actor) else {
            return Ok(None);
        };
        if !self.actors[actor.index()].conditions.arte_queue_allowed()
            || !self.technique_available(actor, action)
            || self.prepared.actions.get(action).is_some_and(|definition| {
                matches!(&definition.execution, crate::ActionExecution::Casting(_))
            })
        {
            return Ok(Some(false));
        }
        Ok(Some(self.queue_companion_chain(actor, action)?))
    }

    pub(crate) fn consume_assist_input(
        &mut self,
        owner: ActorId,
        input: crate::ControlInput,
    ) -> Result<()> {
        if self.phase() != crate::BattlePhase::Combat {
            return Ok(());
        }
        let control = self.runtime[owner.index()].control.as_ref().unwrap();
        let selected = if input.assist[1].pressed {
            control.assist_shortcuts[1]
        } else if input.assist[0].pressed {
            control.assist_shortcuts[0]
        } else {
            None
        };
        let Some((actor, binding)) = selected else {
            return Ok(());
        };
        if actor == owner
            || self.actors[actor.index()].control != Control::Auto
            || !self.actors[actor.index()].available()
        {
            return Ok(());
        }
        if u32::from(self.actors[actor.index()].tp) < self.action_quote(actor, binding) {
            return Ok(());
        }
        if self.queue_technique_from(actor, binding, owner)? {
            self.runtime[actor.index()]
                .control
                .as_mut()
                .unwrap()
                .queued_technique
                .as_mut()
                .unwrap()
                .target = None;
            self.ledger.assist_commands = self.ledger.assist_commands.saturating_add(1).min(10);
        }
        Ok(())
    }
}

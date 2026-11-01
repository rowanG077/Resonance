//! Companion policy using prepared character defaults and skill limits.
use crate::{ActorId, Battle, Side};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_recoil::strategy_guard_recovery;

mod selection;
use selection::{OpeningAction, OpeningDecision};

const NORMAL_OPENING_MIN_HP_PERCENT: i16 = 30;

pub use resonance_content::battle_profile::CompanionLimits as PolicyLimits;

#[derive(Debug, Clone, Copy)]
pub struct StrategyRefresh {
    pub actor: ActorId,
    pub choices: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct CompanionDefinition {
    /// Native admission has character-specific opening and combo rules.
    pub character: crate::Character,
    pub initial_policy: [u8; 3],
    pub defaults: [u8; 3],
    pub target_strategies: [crate::TargetPolicy; 8],
    pub positioning: resonance_content::battle_profile::CompanionPositioning,
    pub normal_selection: resonance_content::battle_profile::CompanionNormalSelection,
    pub limits: [PolicyLimits; 9],
}

pub(crate) use resonance_content::strategy::PositionStrategy;
use resonance_content::strategy::{ResolvedChoices, TechniqueStrategy};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedPolicy {
    raw: [u8; 3],
    character: crate::Character,
    normal_selection: resonance_content::battle_profile::CompanionNormalSelection,
    pub(crate) positioning: resonance_content::battle_profile::CompanionPositioning,
    pub(crate) target: crate::TargetPolicy,
    pub(crate) position: PositionStrategy,
    opening_weapon: Option<bool>,
    opening_spell: Option<bool>,
    idle_techniques: bool,
    reduced_offense: bool,
    limits: PolicyLimits,
}

impl CompanionDefinition {
    pub(crate) fn resolve(&self, choices: [u8; 3]) -> Result<ResolvedPolicy> {
        self.positioning.validate()?;
        self.normal_selection.validate()?;
        ensure!(
            self.target_strategies
                .iter()
                .all(|target| crate::TargetPolicy::PARTY.contains(target)),
            "companion strategy needs living-opponent target handlers"
        );
        for limits in self.limits {
            limits.validate()?;
        }
        let raw = choices;
        let choices = ResolvedChoices::resolve(choices, self.defaults)?;
        Ok(ResolvedPolicy {
            raw,
            character: self.character,
            normal_selection: self.normal_selection,
            positioning: self.positioning,
            target: self.target_strategies[usize::from(choices.target - 1)],
            opening_weapon: match choices.position {
                PositionStrategy::Frontlines => Some(true),
                position if position.is_ranged() => Some(false),
                _ => None,
            },
            opening_spell: (choices.explicit_position && choices.position.is_ranged())
                .then_some(choices.position == PositionStrategy::LongRangeMagic),
            position: choices.position,
            idle_techniques: choices.techniques != TechniqueStrategy::DoNotUse,
            reduced_offense: matches!(
                choices.techniques,
                TechniqueStrategy::Moderation
                    | TechniqueStrategy::Heal
                    | TechniqueStrategy::Support
            ),
            limits: self.limits[choices.techniques as usize],
        })
    }
}

impl Battle {
    /// Raw saved choices, including Default values.
    pub fn companion_choices(&self, actor: ActorId) -> Option<[u8; 3]> {
        self.companion_policy(actor).map(|policy| policy.raw)
    }

    pub(crate) fn companion_policy(&self, actor: ActorId) -> Option<ResolvedPolicy> {
        self.runtime.get(actor.index())?.companion_policy
    }

    pub fn refresh_strategy(&mut self, rows: &[StrategyRefresh]) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "strategy refresh requires active combat"
        );
        let party_count = self.actors.iter().filter(|a| a.side == Side::Party).count();
        ensure!(
            rows.len() == party_count,
            "strategy refresh needs every party actor"
        );
        let mut seen = std::collections::BTreeSet::new();
        let mut policies = Vec::with_capacity(rows.len());
        for row in rows {
            ensure!(
                self.actor(row.actor)?.side == crate::Side::Party && seen.insert(row.actor),
                "strategy refresh needs unique party actors"
            );
            let definition = self.prepared.actor_setup[row.actor.index()]
                .companion
                .as_ref()
                .context("actor has no companion policy")?;
            policies.push((row.actor, definition.resolve(row.choices)?));
        }
        for (actor, policy) in policies {
            self.runtime[actor.index()].companion_policy = Some(policy);
            self.actors[actor.index()].guard.recovery_bonus =
                strategy_guard_recovery(policy.position as u8);
        }
        Ok(())
    }

    pub(crate) fn advance_companion_ai(
        &mut self,
        owner: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        if self.actors[owner.index()].control != crate::Control::Auto || !self.decision_ready(owner)
        {
            return Ok(());
        }
        if self.pending_technique(owner).is_some() {
            self.request_queued_technique(owner, cues)?;
            return Ok(());
        }
        let policy = self
            .companion_policy(owner)
            .context("actor has no companion policy")?;
        let mut random = self.random;
        let decision = selection::Selection {
            battle: self,
            owner,
            policy,
            random: &mut random,
        }
        .opening();
        self.random = random;
        self.apply_companion_decision(owner, decision?)
    }

    fn apply_companion_decision(
        &mut self,
        owner: ActorId,
        decision: OpeningDecision,
    ) -> Result<()> {
        for target in decision.target_visits.into_iter().flatten() {
            self.set_decision_target(owner, target)?;
        }
        match decision.action {
            OpeningAction::None => Ok(()),
            OpeningAction::Formation => self.maintain_companion_formation(owner),
            OpeningAction::Action { opponent, request } => {
                self.runtime[owner.index()].support_target = request.target;
                let range = self
                    .prepared
                    .technique(owner, request.action)
                    .map(|row| row.ai_range)
                    .unwrap_or_else(|| self.selection_range(owner, request.action));
                let definition = self.prepared.actor_setup[owner.index()]
                    .control
                    .as_ref()
                    .unwrap();
                self.request_approach(
                    request,
                    opponent,
                    crate::ApproachParameters {
                        minimum: range[0],
                        maximum: range[1],
                        motion: definition.motions.map(|motions| motions.run),
                        motion_rate: 0.5,
                        speed: definition.run_speed,
                        turn_ticks: definition.turn_ticks,
                    },
                )?;
                Ok(())
            }
        }
    }

    fn maintain_companion_formation(&mut self, owner: ActorId) -> Result<()> {
        let index = owner.index();
        let moving = self.return_position(owner)?;
        let definition = self.prepared.actor_setup[index].control.as_ref().unwrap();
        if moving {
            let actor = &mut self.actors[index];
            actor.movement.forward = actor.walk_speed(
                definition.walk_speed,
                actor.body.scale,
                actor.conditions.effective(),
            );
            actor.facing_direction = actor.movement.direction;
            crate::control::face(
                actor,
                actor.facing_direction,
                180. / f32::from(definition.turn_ticks),
            );
            self.request_pose(
                owner,
                definition.motions.map(|motions| motions.walk),
                crate::Pose {
                    repeat: true,
                    restart: false,
                    ..Default::default()
                },
            );
        } else {
            self.runtime[index].idle_timer = 30;
            self.maintain_ai_idle(owner);
        }
        Ok(())
    }

    pub(crate) fn advance_ai_combo(
        &mut self,
        owner: ActorId,
        normal: Option<crate::control::NormalState>,
    ) -> Result<bool> {
        if self.actors[owner.index()].control != crate::Control::Auto {
            return Ok(false);
        }
        if let Some(accepted) = self.queue_pending_technique_chain(owner)? {
            return Ok(accepted);
        }
        let policy = self
            .companion_policy(owner)
            .context("actor has no companion policy")?;
        let mut random = self.random;
        let next = selection::Selection {
            battle: self,
            owner,
            policy,
            random: &mut random,
        }
        .combo(normal);
        self.random = random;
        self.runtime[owner.index()].combo.buffered = next;
        Ok(next.is_some())
    }

    pub(crate) fn advance_ai_approach(&mut self, owner: ActorId) -> Result<()> {
        if self.prepared.actor_setup[owner.index()].companion.is_none() {
            return Ok(());
        }
        let Some(previous) = self.approach_normal(owner) else {
            return Ok(());
        };
        let policy = self
            .companion_policy(owner)
            .context("actor has no companion policy")?;
        let mut random = self.random;
        let target = self.runtime[owner.index()].target;
        let next = selection::approach(
            &self.actors[owner.index()],
            &self.actors[target.index()],
            policy,
            previous,
            &mut random,
            || self.body_gap(owner, target),
        );
        self.random = random;
        if let Some(next) = next {
            self.reselect_approach_normal(owner, next)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;

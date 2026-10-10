//! Companion policy using prepared character defaults and skill limits.
use crate::{ActorId, Battle, NormalAttack, PreparedBattle, PreparedTechnique};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_recoil::strategy_guard_recovery;

/// Raw saved choices; zero resolves through the prepared character defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompanionPolicy {
    pub choices: [u8; 3],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyLimits {
    pub tp: u8,
    pub healing: u8,
    pub support_level: i8,
}

#[derive(Debug, Clone, Copy)]
pub struct StrategyRefresh {
    pub actor: ActorId,
    pub choices: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct CompanionDefinition {
    pub initial_policy: [u8; 3],
    pub defaults: [u8; 3],
    pub limits: [PolicyLimits; 9],
    pub level: u8,
    pub level_difference: i8,
}

impl CompanionDefinition {
    fn resolve(&self, choices: [u8; 3]) -> Result<[u8; 3]> {
        ensure!(
            self.defaults
                .iter()
                .zip([8, 8, 6])
                .all(|(&value, maximum)| { (1..=maximum).contains(&value) })
                && choices
                    .iter()
                    .zip([8, 8, 6])
                    .all(|(&value, maximum)| value <= maximum),
            "invalid companion strategy choices"
        );
        Ok(std::array::from_fn(|i| {
            if choices[i] == 0 {
                self.defaults[i]
            } else {
                choices[i]
            }
        }))
    }

    fn validate(&self) -> Result<()> {
        self.resolve(self.initial_policy)?;
        ensure!(
            self.level != 0
                && (-8..=8).contains(&self.level_difference)
                && self
                    .limits
                    .iter()
                    .all(|row| row.tp <= 100 && row.healing <= 100),
            "invalid companion policy parameters"
        );
        Ok(())
    }
}

impl PreparedBattle {
    pub(crate) fn validate_actor_companions(&self) -> Result<()> {
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            let Some(definition) = &setup.companion else {
                continue;
            };
            ensure!(
                index < self.actors.len() && self.actors[index].side == crate::Side::Party,
                "companion policy needs a party actor"
            );
            ensure!(
                self.resources.actor_setup[index].control.is_some(),
                "companion policy needs unique normal bindings"
            );
            definition.validate()?;
        }
        Ok(())
    }
}

impl Battle {
    pub(crate) fn companion_mode_ready(&self, actor: ActorId) -> bool {
        self.actors[actor.index()].control == crate::Control::Auto && self.decision_ready(actor)
    }

    fn companion(&self, owner: ActorId) -> Result<&CompanionDefinition> {
        self.prepared
            .actor_setup
            .get(owner.index())
            .and_then(|setup| setup.companion.as_ref())
            .context("actor has no companion policy")
    }

    fn companion_techniques(&self, owner: ActorId) -> impl Iterator<Item = &PreparedTechnique> {
        let setup = &self.prepared.actor_setup[owner.index()];
        setup.techniques.iter().filter(move |row| {
            setup.special_guard.is_none_or(|guard| guard != row.action)
                && self.technique_available(owner, row.action)
        })
    }

    pub fn companion_policy(&self, actor: ActorId) -> Option<CompanionPolicy> {
        self.runtime.get(actor.index())?.companion_policy
    }

    fn resolved_companion_policy(&self, owner: ActorId) -> Result<[u8; 3]> {
        let policy = self
            .companion_policy(owner)
            .context("actor has no live companion policy")?;
        self.companion(owner)?.resolve(policy.choices)
    }

    pub(crate) fn companion_position(&self, owner: ActorId) -> Result<u8> {
        Ok(self.resolved_companion_policy(owner)?[2])
    }

    pub fn refresh_strategy(&mut self, rows: &[StrategyRefresh]) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "strategy refresh requires active combat"
        );
        ensure!(
            rows.len()
                == self
                    .actors
                    .iter()
                    .filter(|actor| actor.side == crate::Side::Party)
                    .count(),
            "strategy refresh needs every party actor"
        );
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            ensure!(
                self.actor(row.actor)?.side == crate::Side::Party && seen.insert(row.actor),
                "strategy refresh needs unique party actors"
            );
            self.companion(row.actor)?.resolve(row.choices)?;
            ensure!(
                self.companion_policy(row.actor).is_some(),
                "actor has no live companion policy"
            );
        }
        for row in rows {
            let position = self.companion(row.actor)?.resolve(row.choices)?[2];
            self.runtime[row.actor.index()].companion_policy = Some(CompanionPolicy {
                choices: row.choices,
            });
            self.actors[row.actor.index()].guard.recovery_bonus = strategy_guard_recovery(position);
        }
        Ok(())
    }

    fn usable_companion_technique(&self, owner: ActorId, technique: &PreparedTechnique) -> bool {
        let actor = &self.actors[owner.index()];
        self.technique_available(owner, technique.action)
            && self.technique_enabled(owner, technique.action)
            && actor.conditions.arte_queue_allowed()
            && self
                .prepared
                .actions
                .get(technique.action)
                .is_some_and(|_| u32::from(actor.tp) >= self.action_quote(owner, technique.action))
    }

    fn support_choice(&self, owner: ActorId, threshold: u8) -> Option<CompanionAction> {
        let side = self.actors[owner.index()].side;
        for (revival, target) in [
            (
                true,
                self.actors
                    .iter()
                    .enumerate()
                    .find(|(_, actor)| {
                        actor.side == side && actor.availability == crate::ActorAvailability::Dead
                    })
                    .map(|(i, _)| ActorId(i as u8)),
            ),
            (
                false,
                self.actors
                    .iter()
                    .enumerate()
                    .filter(|(_, actor)| {
                        actor.side == side
                            && actor.available()
                            && actor.hp_percent() <= i16::from(threshold)
                    })
                    .min_by_key(|(_, actor)| actor.hp_percent())
                    .map(|(i, _)| ActorId(i as u8)),
            ),
        ] {
            if let Some(target) = target
                && let Some(technique) = self.companion_techniques(owner).find(|row| {
                    (if revival {
                        row.capabilities.revives
                    } else {
                        row.capabilities.healing
                    }) && self.usable_companion_technique(owner, row)
                })
            {
                return Some(CompanionAction::technique(technique, target));
            }
        }
        None
    }

    fn offensive_choice(
        &mut self,
        owner: ActorId,
        target: ActorId,
        strategy: [u8; 3],
    ) -> Result<Option<CompanionAction>> {
        let limits = self.companion(owner)?.limits[usize::from(strategy[1])];
        if strategy[1] == 5
            || strategy[2] != 6 && self.actors[owner.index()].tp_percent() <= i16::from(limits.tp)
        {
            return Ok(None);
        }
        let mut choices: Vec<_> = self
            .companion_techniques(owner)
            .filter(|row| row.capabilities.offensive && self.usable_companion_technique(owner, row))
            .filter(|row| match strategy[2] {
                1 => row.capabilities.uses_weapon_reach,
                4 => !row.capabilities.uses_weapon_reach && !row.capabilities.spell,
                5 => row.capabilities.spell,
                _ => true,
            })
            .filter(|row| {
                !matches!(
                    self.actors[target.index()].equipment.affinities[usize::from(row.element)],
                    crate::Affinity::Absorb | crate::Affinity::Immune
                )
            })
            .cloned()
            .collect();
        if choices.iter().any(|row| {
            self.actors[target.index()].equipment.affinities[usize::from(row.element)]
                == crate::Affinity::Weak
        }) {
            choices.retain(|row| {
                self.actors[target.index()].equipment.affinities[usize::from(row.element)]
                    == crate::Affinity::Weak
            });
        }
        if choices.is_empty() {
            return Ok(None);
        }
        let choice = &choices[usize::from(self.random.next_u16()) % choices.len()];
        Ok(Some(CompanionAction::technique(choice, target)))
    }

    fn normal_choice(&mut self, owner: ActorId, target: ActorId) -> Result<CompanionAction> {
        let definition = self.prepared.actor_setup[owner.index()]
            .control
            .as_ref()
            .context("companion has no normal bindings")?;
        use NormalAttack::*;
        let selection = if self.actors[target.index()].position[1] >= 100. {
            Rising
        } else {
            [Neutral, Thrust, Low, Finisher][usize::from(self.random.next_u16()) % 4]
        };
        let normal = definition.normals[selection as usize];
        Ok(CompanionAction {
            action: normal.action,
            target,
            range: [normal.minimum_reach, normal.reach],
        })
    }

    pub(crate) fn advance_companion_ai(
        &mut self,
        owner: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        let index = owner.index();
        if !self.companion_mode_ready(owner) {
            return Ok(());
        }
        if self.pending_technique(owner).is_some() {
            self.request_queued_technique(owner, cues)?;
            return Ok(());
        }
        let strategy = self.resolved_companion_policy(owner)?;
        let Some(target) =
            self.decision_target(owner, crate::TargetPolicy::try_from(strategy[0])?)?
        else {
            return Ok(());
        };
        self.set_decision_target(owner, target)?;
        let threshold = self.companion(owner)?.limits[usize::from(strategy[1])].healing;
        let action = if strategy[1] != 5 {
            self.support_choice(owner, threshold)
        } else {
            None
        };
        let action = match action {
            Some(action) => Some(action),
            None => self.offensive_choice(owner, target, strategy)?,
        };
        let defensive = matches!(strategy[2], 4 | 5);
        let action = match action {
            Some(action) => action,
            None if defensive
                && self
                    .actors
                    .iter()
                    .filter(|actor| actor.side == crate::Side::Party && actor.available())
                    .count()
                    > 1 =>
            {
                let moving = self.return_position(owner)?;
                let definition = self.prepared.actor_setup[index].control.as_ref().unwrap();
                let motions = definition.motions;
                let walk_speed = definition.walk_speed;
                let turn_ticks = definition.turn_ticks;
                if moving {
                    self.request_pose(
                        owner,
                        motions.map(|motions| motions.walk),
                        crate::Pose {
                            repeat: true,
                            restart: false,
                            ..Default::default()
                        },
                    );
                    let actor = &mut self.actors[index];
                    actor.movement.forward = actor.walk_speed(
                        walk_speed,
                        actor.body.scale,
                        actor.conditions.effective(),
                    );
                    actor.facing_direction = actor.movement.direction;
                    crate::control::face(
                        actor,
                        actor.facing_direction,
                        180. / f32::from(turn_ticks),
                    );
                } else {
                    self.runtime[index].idle_timer = 30;
                    self.maintain_ai_idle(owner);
                }
                return Ok(());
            }
            None => self.normal_choice(owner, target)?,
        };
        self.runtime[index].support_target = action.target;
        let definition = self.prepared.actor_setup[index].control.as_ref().unwrap();
        let parameters = crate::ApproachParameters {
            minimum: action.range[0],
            maximum: action.range[1],
            motion: definition.motions.map(|motions| motions.run),
            motion_rate: 0.5,
            speed: definition.run_speed,
            turn_ticks: definition.turn_ticks,
        };
        if self.request_approach(owner, target, action.action, parameters)?
            && action.target != target
            && self.runtime[index].task().approach().is_some()
        {
            self.set_approach_action_target(owner, action.target)?;
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
        let strategy = self.resolved_companion_policy(owner)?;
        let target = self
            .target(owner)
            .context("companion combo has no target")?;
        if let Some(action) = self.offensive_choice(owner, target, strategy)?
            && self
                .prepared
                .actions
                .get(action.action)
                .is_some_and(|row| matches!(&row.execution, crate::ActionExecution::Attack(_)))
            && self.queue_companion_chain(owner, action.action)?
        {
            return Ok(true);
        }
        let Some(normal) = normal else {
            return Ok(false);
        };
        let index = owner.index();
        let equipment = &self.actors[index].equipment;
        let limit = equipment.normal_combo_limit;
        if self.runtime[index].combo.normal_links + 1 >= limit
            || normal.airborne() && !equipment.combo_traits.sky_combo
        {
            return Ok(false);
        }
        let (allowed, fallback) = normal.continuations(equipment.combo_traits.jump_combo);
        let choices: Vec<_> = allowed
            .iter()
            .copied()
            .filter(|&choice| {
                choice != NormalAttack::Rising || self.actors[target.index()].position[1] >= 100.
            })
            .collect();
        let next = if choices.is_empty() {
            fallback
        } else {
            Some(choices[usize::from(self.random.next_u16()) % choices.len()])
        };
        if let Some(next) = next {
            let action = self.prepared.actor_setup[owner.index()]
                .control
                .as_ref()
                .unwrap()
                .normals[next as usize]
                .action;
            return self.queue_companion_chain(owner, action);
        }
        Ok(false)
    }

    pub(crate) fn advance_ai_approach(&mut self, owner: ActorId) -> Result<()> {
        let Some(previous) = self.approach_normal(owner) else {
            return Ok(());
        };
        let target = self
            .target(owner)
            .context("normal approach has no target")?;
        let airborne = self.actors[target.index()].position[1] >= 100.;
        if airborne && previous != NormalAttack::Rising {
            self.reselect_approach_normal(owner, NormalAttack::Rising)?;
        } else if !airborne && previous == NormalAttack::Rising {
            self.reselect_approach_normal(owner, NormalAttack::Neutral)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct CompanionAction {
    action: crate::ActionKey,
    target: ActorId,
    range: [f32; 2],
}
impl CompanionAction {
    fn technique(technique: &PreparedTechnique, target: ActorId) -> Self {
        Self {
            action: technique.action,
            target,
            range: technique.ai_range,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;

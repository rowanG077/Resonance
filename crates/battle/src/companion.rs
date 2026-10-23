//! Companion policy using prepared character defaults and skill limits.
use crate::{ActorId, Battle, NormalAttack, PreparedBattle, PreparedTechnique};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_recoil::strategy_guard_recovery;

const NORMAL_OPENING_MIN_HP_PERCENT: i16 = 30;

/// Raw saved choices; zero resolves through the prepared character defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompanionPolicy {
    pub choices: [u8; 3],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyLimits {
    pub tp: u8,
    pub healing: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct StrategyRefresh {
    pub actor: ActorId,
    pub choices: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct CompanionDefinition {
    /// Start with normal attacks unless hurt or assigned a ranged/unrestricted strategy.
    pub normal_attack_opening: bool,
    pub initial_policy: [u8; 3],
    pub defaults: [u8; 3],
    pub limits: [PolicyLimits; 9],
}

/// Resolved behavior; numeric choices stay at the save/menu boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PositionStrategy {
    Melee = 1,
    Balanced,
    Guarded,
    Ranged,
    Magic,
    Unrestricted,
}

impl PositionStrategy {
    fn ranged(self) -> bool {
        matches!(self, Self::Ranged | Self::Magic)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OffensiveUse {
    Never,
    AboveReserve,
    ReducedBelowReserve,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedPolicy {
    target: crate::TargetPolicy,
    position: PositionStrategy,
    offense: OffensiveUse,
    limits: PolicyLimits,
}

impl CompanionDefinition {
    fn resolve(&self, choices: [u8; 3]) -> Result<ResolvedPolicy> {
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
        let [target, techniques, position] = std::array::from_fn(|i| {
            if choices[i] == 0 {
                self.defaults[i]
            } else {
                choices[i]
            }
        });
        Ok(ResolvedPolicy {
            target: crate::TargetPolicy::try_from(target)?,
            position: match position {
                1 => PositionStrategy::Melee,
                2 => PositionStrategy::Balanced,
                3 => PositionStrategy::Guarded,
                4 => PositionStrategy::Ranged,
                5 => PositionStrategy::Magic,
                6 => PositionStrategy::Unrestricted,
                _ => unreachable!("validated position strategy"),
            },
            offense: match techniques {
                1..=4 => OffensiveUse::AboveReserve,
                5 => OffensiveUse::Never,
                _ => OffensiveUse::ReducedBelowReserve,
            },
            limits: self.limits[usize::from(techniques)],
        })
    }

    fn validate(&self) -> Result<()> {
        self.resolve(self.initial_policy)?;
        ensure!(
            self.limits
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

    fn resolved_companion_policy(&self, owner: ActorId) -> Result<ResolvedPolicy> {
        let policy = self
            .companion_policy(owner)
            .context("actor has no live companion policy")?;
        self.companion(owner)?.resolve(policy.choices)
    }

    pub(crate) fn companion_position(&self, owner: ActorId) -> Result<PositionStrategy> {
        Ok(self.resolved_companion_policy(owner)?.position)
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
            let position = self.companion(row.actor)?.resolve(row.choices)?.position;
            self.runtime[row.actor.index()].companion_policy = Some(CompanionPolicy {
                choices: row.choices,
            });
            self.actors[row.actor.index()].guard.recovery_bonus =
                strategy_guard_recovery(position as u8);
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

    fn support_choice(&mut self, owner: ActorId, threshold: u8) -> Option<CompanionAction> {
        let side = self.actors[owner.index()].side;
        let wounded = self.actors.iter().enumerate().filter(|(_, actor)| {
            actor.side == side && actor.available() && actor.hp_percent() <= i16::from(threshold)
        });
        let needs_group_heal = wounded.clone().count() >= 2;
        let healing_target = wounded
            .min_by_key(|(_, actor)| actor.hp_percent())
            .map(|(i, _)| ActorId(i as u8));
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
            (false, healing_target),
        ] {
            let Some(target) = target else { continue };
            let mut choices: Vec<_> = self
                .companion_techniques(owner)
                .filter(|row| {
                    (if revival {
                        row.capabilities.revives
                    } else {
                        row.capabilities.healing
                            && self.prepared.casting(row.action).is_some_and(|cast| {
                                cast.release.recovery.iter().any(|pulse| {
                                    matches!(
                                        pulse.benefit,
                                        crate::SpellBenefit::Hp(_)
                                            | crate::SpellBenefit::HpByDistance { .. }
                                    )
                                })
                            })
                    }) && self.usable_companion_technique(owner, row)
                })
                .map(|row| {
                    let group = !revival
                        && self.prepared.casting(row.action).is_some_and(|cast| {
                            cast.release.recovery.iter().any(|pulse| {
                                matches!(
                                    pulse.targets,
                                    crate::RecoveryTargets::LivingAllies
                                        | crate::RecoveryTargets::Area { .. }
                                )
                            })
                        });
                    (CompanionAction::technique(row, target), group)
                })
                .collect();
            // Prefer group healing for multiple injured allies, with single-target
            // healing as the fallback when no group spell is usable.
            let group = needs_group_heal && choices.iter().any(|(_, group)| *group);
            choices.retain(|(_, is_group)| *is_group == group);
            if !choices.is_empty() {
                return Some(choices[usize::from(self.random.next_u16()) % choices.len()].0);
            }
        }
        None
    }

    fn condition_choice(&self, owner: ActorId, allow_buffs: bool) -> Option<CompanionAction> {
        use crate::{RecoveryTargets, SpellBenefit, TechniqueTarget};
        let side = self.actors[owner.index()].side;
        let mut best = None;
        let mut best_score = (false, 0, std::cmp::Reverse(u32::MAX));
        for row in self.companion_techniques(owner).filter(|row| {
            matches!(
                row.capabilities.target,
                TechniqueTarget::Ally | TechniqueTarget::SelfTarget
            ) && self.usable_companion_technique(owner, row)
        }) {
            let Some(cast) = self.prepared.casting(row.action) else {
                continue;
            };
            for pulse in &cast.release.recovery {
                let needed = |actor: &crate::Actor| {
                    actor.side == side
                        && pulse.benefit.accepts(actor)
                        && match pulse.benefit {
                            SpellBenefit::Cure(cure) => actor.conditions.needs_cure(cure),
                            SpellBenefit::Stat { condition, .. } if allow_buffs => {
                                !actor.conditions.rejects(condition)
                                    && !actor.conditions.effective().contains(condition)
                            }
                            SpellBenefit::Buff(buff) if allow_buffs => {
                                actor.conditions.needs_buff(buff)
                            }
                            _ => false,
                        }
                };
                for (index, actor) in self
                    .actors
                    .iter()
                    .enumerate()
                    .filter(|(_, actor)| needed(actor))
                {
                    let target = ActorId(index as u8);
                    if (row.capabilities.target == TechniqueTarget::SelfTarget
                        || pulse.targets == RecoveryTargets::Caster)
                        && target != owner
                    {
                        continue;
                    }
                    let recipients = match pulse.targets {
                        RecoveryTargets::LivingOpponents => continue,
                        RecoveryTargets::Target | RecoveryTargets::Caster => 1,
                        RecoveryTargets::LivingAllies => {
                            self.actors.iter().filter(|ally| needed(ally)).count()
                        }
                        RecoveryTargets::Area { radius, inclusive } => self
                            .actors
                            .iter()
                            .filter(|ally| {
                                let distance = (ally.position[0] - actor.position[0])
                                    .hypot(ally.position[2] - actor.position[2]);
                                needed(ally)
                                    && if inclusive {
                                        distance <= radius
                                    } else {
                                        distance < radius
                                    }
                            })
                            .count(),
                    };
                    let score = (
                        matches!(pulse.benefit, SpellBenefit::Cure(_)),
                        recipients,
                        std::cmp::Reverse(self.action_quote(owner, row.action)),
                    );
                    if score > best_score {
                        best = Some(CompanionAction::technique(row, target));
                        best_score = score;
                    }
                }
            }
        }
        best
    }

    fn offensive_choice(
        &mut self,
        owner: ActorId,
        target: ActorId,
        strategy: ResolvedPolicy,
    ) -> Option<CompanionAction> {
        let limits = strategy.limits;
        let reserve_reached = strategy.position != PositionStrategy::Unrestricted
            && self.actors[owner.index()].tp_percent() <= i16::from(limits.tp);
        if strategy.offense == OffensiveUse::Never
            || strategy.offense == OffensiveUse::AboveReserve && reserve_reached
        {
            return None;
        }
        let mut choices: Vec<_> = self
            .companion_techniques(owner)
            .filter(|row| row.capabilities.offensive && self.usable_companion_technique(owner, row))
            .filter(|row| match strategy.position {
                PositionStrategy::Melee => row.capabilities.uses_weapon_reach,
                PositionStrategy::Ranged => {
                    !row.capabilities.uses_weapon_reach && !row.capabilities.spell
                }
                PositionStrategy::Magic => row.capabilities.spell,
                _ => true,
            })
            .filter(|row| {
                !matches!(
                    self.actors[target.index()].affinity(row.element),
                    crate::Affinity::Absorb | crate::Affinity::Immune
                )
            })
            .cloned()
            .collect();
        if choices
            .iter()
            .any(|row| self.actors[target.index()].affinity(row.element) == crate::Affinity::Weak)
        {
            choices.retain(|row| {
                self.actors[target.index()].affinity(row.element) == crate::Affinity::Weak
            });
        }
        if choices.is_empty() {
            return None;
        }
        let choice = &choices[usize::from(self.random.next_u16()) % choices.len()];
        // Moderation, healing and support policies reduce offensive use below
        // their reserve instead of refusing every affordable technique.
        if reserve_reached && self.random.next_u16() % 100 >= 25 {
            return None;
        }
        Some(CompanionAction::technique(choice, target))
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
        let Some(target) = self.decision_target(owner, strategy.target)? else {
            return Ok(());
        };
        self.set_decision_target(owner, target)?;
        let limits = strategy.limits;
        let action = if strategy.offense != OffensiveUse::Never {
            self.support_choice(owner, limits.healing).or_else(|| {
                let allow_buffs = strategy.position == PositionStrategy::Unrestricted
                    || self.actors[index].tp_percent() > i16::from(limits.tp);
                self.condition_choice(owner, allow_buffs)
            })
        } else {
            None
        };
        let action = match action {
            Some(action) => Some(action),
            None if self.companion(owner)?.normal_attack_opening
                && !strategy.position.ranged()
                && strategy.position != PositionStrategy::Unrestricted
                && self.actors[index].hp_percent() >= NORMAL_OPENING_MIN_HP_PERCENT =>
            {
                None
            }
            None => self.offensive_choice(owner, target, strategy),
        };
        let defensive = strategy.position.ranged();
        let action = match action {
            Some(action) => action,
            None if defensive
                && self.actors[index].tp_percent() > i16::from(limits.tp)
                && self
                    .companion_techniques(owner)
                    .any(|row| self.usable_companion_technique(owner, row))
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
        if let Some(action) = self.offensive_choice(owner, target, strategy)
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
        let definition = self.prepared.actor_setup[index]
            .control
            .as_ref()
            .context("companion combo has no normal controls")?;
        let continuation = normal.continuations(definition, equipment.combo_traits.jump_combo);
        let choices: Vec<_> = NormalAttack::ALL
            .into_iter()
            .filter(|&choice| continuation.directions & (1 << choice as u8) != 0)
            .filter(|&choice| {
                choice != NormalAttack::Rising || self.actors[target.index()].position[1] >= 100.
            })
            .collect();
        let next = if choices.is_empty() {
            continuation
                .fallback
                .map(|index| NormalAttack::ALL[index as usize])
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

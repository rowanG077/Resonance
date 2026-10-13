//! Weighted enemy choices and native action dispatch.
use crate::{ActorId, Battle, PreparedBattle, TargetPolicy};
use anyhow::{Context, Result, ensure};

pub use resonance_content::battle_action::EnemyRequirements;

#[derive(Debug, Clone)]
pub struct EnemyChoice {
    pub action: crate::ActionKey,
    pub weight: u8,
    pub chain_group: u8,
    pub followup_chance: u8,
    pub requirements: EnemyRequirements,
    pub target_policy: Option<TargetPolicy>,
    pub target_allies: bool,
    pub return_to_formation: bool,
    /// Selected-row contact guard chance, including approach and idle visits.
    pub guard_chance: i8,
    pub range: [i16; 2],
    pub tp: u16,
    pub approach_minimum: f32,
    pub approach_range: f32,
    pub approach: Option<crate::ApproachParameters>,
}

#[derive(Debug, Clone)]
pub struct EnemyDecisionDefinition {
    pub enemy_id: u8,
    pub strategy: TargetPolicy,
    pub difficulty: u8,
    pub choices: Vec<EnemyChoice>,
    pub disabled_choices: Vec<u8>,
    pub back_row: Vec<(u8, u8)>,
    pub walk_speed: f32,
    pub walk_override: Option<resonance_content::battle_action::EnemyWalkOverride>,
    pub behavior: Option<crate::EnemyBehavior>,
    pub walk_motion: Option<crate::MotionBinding>,
    pub turn_ticks: u8,
}

impl PreparedBattle {
    pub(crate) fn validate_actor_enemy_decisions(&self) -> Result<()> {
        self.validate_enemy_behaviors()?;
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            let Some(definition) = &setup.enemy_decision else {
                continue;
            };
            ensure!(
                index < self.actors.len() && self.actors[index].control == crate::Control::Enemy,
                "enemy choice owner is not an enemy"
            );

            ensure!(
                definition.difficulty <= 2,
                "unsupported ordinary enemy strategy"
            );
            ensure!(!definition.choices.is_empty(), "empty enemy choice table");
            ensure!(
                definition
                    .disabled_choices
                    .iter()
                    .all(|&row| usize::from(row) < definition.choices.len()),
                "disabled enemy choice is out of range"
            );
            ensure!(
                definition.walk_speed.is_finite()
                    && definition.walk_speed > 0.
                    && definition.turn_ticks != 0
                    && definition.walk_override.is_none_or(|value| {
                        value.hp_percent <= 100 && value.speed.is_finite() && value.speed > 0.
                    }),
                "invalid enemy movement parameters"
            );
            for row in &definition.choices {
                let action = self
                    .resources
                    .actions
                    .get(row.action)
                    .context("enemy row action is not prepared")?;
                ensure!(
                    *row.requirements.difficulty.start() <= 2
                        && *row.requirements.difficulty.end() <= 2
                        && row.requirements.hp_percent.is_none_or(|value| value <= 100),
                    "invalid enemy eligibility conditions"
                );
                ensure!(
                    matches!(action.execution, crate::ActionExecution::LeaveArena)
                        || row.approach_minimum.is_finite()
                            && row.approach_range.is_finite()
                            && row.approach_minimum >= 0.
                            && row.approach_range >= 0.,
                    "invalid enemy approach thresholds"
                );
            }
            ensure!(
                definition
                    .back_row
                    .iter()
                    .all(|&(row, _)| usize::from(row) < definition.choices.len()),
                "invalid back-row choice"
            );
        }
        Ok(())
    }
}

impl Battle {
    pub fn enemy_choices(&self, actor: ActorId) -> Option<&[EnemyChoice]> {
        self.prepared
            .actor_setup
            .get(actor.index())?
            .enemy_decision
            .as_ref()
            .map(|definition| definition.choices.as_slice())
    }

    pub(crate) fn enemy_decision(&self, actor: ActorId) -> Result<&EnemyDecisionDefinition> {
        self.prepared.actor_setup[actor.index()]
            .enemy_decision
            .as_ref()
            .context("missing enemy choice table")
    }

    fn enemy_waiting_rank(&self, owner: ActorId) -> usize {
        const ENGAGEMENT_DISTANCE: f32 = 300.;
        let actor = &self.actors[owner.index()];
        if self.actors.iter().any(|other| {
            other.side != actor.side
                && other.available()
                && crate::distance::length([
                    other.position[0] - actor.position[0],
                    0.,
                    other.position[2] - actor.position[2],
                ]) <= ENGAGEMENT_DISTANCE
        }) {
            return 0;
        }
        self.actors
            .iter()
            .enumerate()
            .filter(|&(index, other)| {
                index != owner.index()
                    && other.side == actor.side
                    && other.available()
                    && other.position[0] < actor.position[0]
            })
            .count()
    }

    fn enemy_choice_eligible(
        &self,
        owner: ActorId,
        index: usize,
        row: &EnemyChoice,
        followup_hit: Option<bool>,
    ) -> bool {
        let actor = &self.actors[owner.index()];
        let definition = self.prepared.actor_setup[owner.index()]
            .enemy_decision
            .as_ref()
            .unwrap();
        if definition
            .disabled_choices
            .iter()
            .any(|&row| usize::from(row) == index)
            || self.defense_arm_waiting(owner)
        {
            return false;
        }
        let difficulty = definition.difficulty;
        if row.target_policy.unwrap_or(definition.strategy) == TargetPolicy::NearestIncapacitated
            && !self.actors.iter().any(|candidate| {
                (candidate.side == actor.side) == row.target_allies
                    && candidate.hp <= 0
                    && candidate.availability == crate::ActorAvailability::Dead
            })
        {
            return false;
        }
        let Some(target) = self.target(owner) else {
            return false;
        };
        let gap = self.body_gap(owner, target);
        let maximum = if row.range[1] == 0 {
            f32::INFINITY
        } else {
            f32::from(row.range[1])
        };
        self.enemy_behavior_allows_choice(owner, index)
            && gap >= f32::from(row.range[0])
            && gap <= maximum
            && !(row.requirements.once_per_battle
                && self.runtime[owner.index()]
                    .used_enemy_choices
                    .contains(&index))
            && actor.tp >= row.tp
            && u32::from(actor.tp) >= self.action_quote(owner, row.action)
            && (!row.requirements.target_guarding
                || self.activity(target) == crate::Activity::Guarding)
            && (!row.requirements.followup_only || followup_hit.is_some())
            && (!row.requirements.followup_requires_hit || followup_hit != Some(false))
            && row.requirements.max_enemies.is_none_or(|maximum| {
                self.actors
                    .iter()
                    .filter(|other| other.side == actor.side && other.available())
                    .count()
                    <= usize::from(maximum)
            })
            && row.requirements.difficulty.contains(&difficulty)
            && row
                .requirements
                .hp_percent
                .is_none_or(|maximum| actor.hp_percent() <= i16::from(maximum))
            && row.requirements.predicates.iter().all(|predicate| {
                use resonance_content::battle_action::EnemyPredicate;
                match *predicate {
                    EnemyPredicate::DoNotRepeat => {
                        self.runtime[owner.index()].last_executed_enemy_row != Some(index)
                    }
                    EnemyPredicate::OriginDistance { minimum, maximum } => {
                        let distance = actor.position[0].hypot(actor.position[2]);
                        minimum.is_none_or(|value| distance >= f32::from(value))
                            && maximum.is_none_or(|value| distance <= f32::from(value))
                    }
                    EnemyPredicate::OpponentCount { minimum, maximum } => {
                        let count = self
                            .actors
                            .iter()
                            .filter(|other| other.side != actor.side && other.available())
                            .count();
                        minimum.is_none_or(|value| count >= usize::from(value))
                            && maximum.is_none_or(|value| count <= usize::from(value))
                    }
                    EnemyPredicate::TargetAttacking => {
                        self.activity(target) == crate::Activity::Action
                    }
                    EnemyPredicate::AllyAbsent(enemy) => {
                        !self.actors.iter().enumerate().any(|(other_index, other)| {
                            other_index != owner.index()
                                && other.side == actor.side
                                && other.available()
                                && self.prepared.actor_setup[other_index]
                                    .enemy_decision
                                    .as_ref()
                                    .is_some_and(|definition| {
                                        u16::from(definition.enemy_id) == enemy
                                    })
                        })
                    }
                }
            })
    }

    pub(crate) fn choose_enemy_action(&mut self, owner: ActorId) -> Result<Option<usize>> {
        if self.target(owner).is_none_or(|target| {
            self.actors[target.index()].side == self.actors[owner.index()].side
        }) {
            let Some(opponent) = self.decision_target(owner, TargetPolicy::Nearest)? else {
                return Ok(None);
            };
            self.set_decision_target(owner, opponent)?;
        }
        let definition = self.enemy_decision(owner)?;
        let party_count = self
            .actors
            .iter()
            .filter(|actor| actor.side == crate::Side::Party && actor.available())
            .count();
        let cap = if definition.difficulty >= 2 {
            8
        } else {
            (party_count + 1).min(3)
        };
        let waiting = self.enemy_waiting_rank(owner) >= cap;
        let candidates: Vec<_> = if waiting {
            definition
                .back_row
                .iter()
                .filter_map(|&(index, weight)| {
                    self.enemy_choice_eligible(
                        owner,
                        usize::from(index),
                        &definition.choices[usize::from(index)],
                        None,
                    )
                    .then_some((usize::from(index), u64::from(weight)))
                })
                .collect()
        } else {
            definition
                .choices
                .iter()
                .enumerate()
                .filter(|(index, row)| self.enemy_choice_eligible(owner, *index, row, None))
                .map(|(index, row)| (index, u64::from(row.weight)))
                .collect()
        };
        let selected = self.weighted_enemy_choice(owner, candidates, waiting)?;
        self.runtime[owner.index()].enemy_choice = selected;
        if let Some(index) = selected {
            let definition = self.enemy_decision(owner)?;
            let row = &definition.choices[index];
            let strategy = row.target_policy.unwrap_or(definition.strategy);
            let target_allies = row.target_allies;
            let guard_chance = row.guard_chance;
            let Some(target) = self.decision_target_pool(owner, strategy, target_allies)? else {
                self.runtime[owner.index()].enemy_choice = None;
                return Ok(None);
            };
            if self.actors[target.index()].side == self.actors[owner.index()].side {
                self.runtime[owner.index()].support_target = target;
            } else {
                self.set_decision_target(owner, target)?;
            }
            self.actors[owner.index()].guard.enemy_chance = guard_chance;
            let direction = crate::distance::planar_direction(
                self.actors[target.index()].position,
                self.actors[owner.index()].position,
                self.actors[owner.index()].movement.target_direction,
            );
            self.actors[owner.index()].movement.target_direction = direction;
        }
        Ok(selected)
    }

    pub(crate) fn refresh_enemy_choice(&mut self, owner: ActorId) -> Result<()> {
        let runtime = &self.runtime[owner.index()];
        if runtime.task().action().is_some()
            || runtime
                .enemy_choice
                .is_none_or(|row| self.enemy_behavior_allows_choice(owner, row))
        {
            return Ok(());
        }
        let approaching = runtime.task().approach().is_some();
        self.runtime[owner.index()].enemy_choice = None;
        if approaching {
            self.cancel_approach(owner.index())?;
        }
        Ok(())
    }

    fn weighted_enemy_choice(
        &mut self,
        owner: ActorId,
        candidates: Vec<(usize, u64)>,
        waiting: bool,
    ) -> Result<Option<usize>> {
        let definition = self.enemy_decision(owner)?;
        let priority = candidates
            .iter()
            .find(|&&(index, _)| definition.choices[index].requirements.priority)
            .map(|&(index, _)| index);
        let total = candidates.iter().try_fold(0_u64, |sum, (_, weight)| {
            sum.checked_add(*weight)
                .context("enemy choice weight overflow")
        })?;
        Ok(if let Some(index) = priority {
            Some(index)
        } else if total == 0 {
            if waiting {
                None
            } else {
                candidates.first().map(|&(index, _)| index)
            }
        } else {
            let draw = (0..4).fold(0_u64, |value, _| {
                (value << 16) | u64::from(self.random.next_u16())
            });
            let mut threshold = draw % if waiting { total.max(100) } else { total };
            candidates.into_iter().find_map(|(index, weight)| {
                if threshold < weight {
                    Some(index)
                } else {
                    threshold -= weight;
                    None
                }
            })
        })
    }

    pub(crate) fn record_enemy_action(&mut self, actor: ActorId, action: crate::ActionKey) {
        let Some(definition) = &self.prepared.actor_setup[actor.index()].enemy_decision else {
            return;
        };
        let runtime = &mut self.runtime[actor.index()];
        let selected = runtime
            .enemy_choice
            .filter(|&index| definition.choices[index].action == action)
            .or_else(|| {
                definition
                    .choices
                    .iter()
                    .position(|row| row.action == action)
            });
        runtime.enemy_choice = selected;
        runtime.last_executed_enemy_row = selected;
        runtime.enemy_effective_hit = false;
        if let Some(index) = selected
            && definition.choices[index].requirements.once_per_battle
            && !runtime.used_enemy_choices.contains(&index)
        {
            runtime.used_enemy_choices.push(index);
        }
    }

    pub(crate) fn chain_enemy(
        &mut self,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<bool> {
        let actor = sequence.actor;
        if self.actors[actor.index()].control != crate::Control::Enemy
            || sequence.started_at == self.update
            || sequence
                .combo
                .is_none_or(|window| sequence.age < window.opens_at)
        {
            return Ok(false);
        }
        // Each action gets one attempt, including when no successor is eligible.
        sequence.combo = None;
        let Some(index) = self.runtime[actor.index()].enemy_choice else {
            return Ok(false);
        };
        let row = &self.enemy_decision(actor)?.choices[index];
        let chance = row.followup_chance;
        let Some(group) = row.chain_group.checked_add(1) else {
            return Ok(false);
        };
        if chance == 0 || self.random.next_u16() % 100 >= u16::from(chance) {
            return Ok(false);
        }
        let candidates = self
            .enemy_decision(actor)?
            .choices
            .iter()
            .enumerate()
            .filter(|(index, row)| {
                row.chain_group == group
                    && self.enemy_choice_eligible(
                        actor,
                        *index,
                        row,
                        Some(self.runtime[actor.index()].enemy_effective_hit),
                    )
            })
            .map(|(index, row)| (index, u64::from(row.weight)))
            .collect();
        let Some(index) = self.weighted_enemy_choice(actor, candidates, false)? else {
            return Ok(false);
        };
        let row = &self.enemy_decision(actor)?.choices[index];
        let action = row.action;
        let guard_chance = row.guard_chance;
        self.runtime[actor.index()].enemy_choice = Some(index);
        self.start_admitted_action(
            crate::ActionRequest {
                actor,
                action,
                target: sequence.target,
            },
            cues,
        )?;
        if matches!(
            self.prepared.actions[action].execution,
            crate::ActionExecution::Attack(_)
        ) {
            let owner = &mut self.actors[actor.index()];
            owner.attack_power = owner
                .attack_power
                .saturating_sub(crate::damage::CHAIN_POWER_REDUCTION)
                .max(crate::damage::MIN_ATTACK_POWER);
        }
        self.actors[actor.index()].guard.enemy_chance = guard_chance;
        cues.push(crate::Cue::Completed { action: id });
        Ok(true)
    }

    pub(crate) fn record_enemy_contact(
        &mut self,
        owner: ActorId,
        result: crate::HitResult,
        defeated: bool,
    ) {
        self.runtime[owner.index()].enemy_effective_hit |=
            (result.is_unblocked_damage() && !result.suppresses_reaction()) || defeated;
    }

    pub(crate) fn update_enemy_walk_override(&mut self, actor: ActorId) {
        let index = actor.index();
        if let Some(value) = self.prepared.actor_setup[index]
            .enemy_decision
            .as_ref()
            .and_then(|definition| definition.walk_override)
            && self.actors[index].hp_percent() <= i16::from(value.hp_percent)
        {
            self.runtime[index].enemy_walk_override = Some(value.speed);
        }
    }

    pub(crate) fn advance_enemy_ai(
        &mut self,
        owner: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        if self.defense_arm_waiting(owner)
            || !self.decision_ready(owner)
            || !self.actors[owner.index()].movement.hover_ready()
        {
            return Ok(());
        }
        let Some(selected) = self.choose_enemy_action(owner)? else {
            self.begin_taunt(owner)?;
            return Ok(());
        };
        let definition = self.enemy_decision(owner)?;
        let row = &definition.choices[selected];
        let action = row.action;
        let target = self.target(owner).context("enemy has no target")?;
        if matches!(
            self.prepared.actions[action].execution,
            crate::ActionExecution::LeaveArena
        ) {
            return self.start_admitted_action(
                crate::ActionRequest {
                    actor: owner,
                    target,
                    action,
                },
                cues,
            );
        }
        let parameters = row.approach.unwrap_or(crate::ApproachParameters {
            minimum: row.approach_minimum,
            maximum: row.approach_range,
            motion: definition.walk_motion,
            motion_rate: 0.5,
            speed: definition.walk_speed,
            turn_ticks: definition.turn_ticks,
        });
        let allied_recipient = row.target_allies
            || row.target_policy.unwrap_or(definition.strategy) == TargetPolicy::SelfTarget;
        if self.request_approach(owner, target, action, parameters)?
            && allied_recipient
            && self.runtime[owner.index()].task().approach().is_some()
        {
            self.set_approach_action_target(owner, self.runtime[owner.index()].support_target)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDefinition, Control, Side};

    fn choice(action: crate::ActionKey, weight: u8) -> EnemyChoice {
        EnemyChoice {
            action,
            weight,
            chain_group: 0,
            followup_chance: 0,
            requirements: EnemyRequirements::default(),
            target_policy: None,
            target_allies: false,
            return_to_formation: true,
            guard_chance: 20,
            range: [0, 0],
            tp: 0,
            approach_minimum: 0.,
            approach_range: 80.,
            approach: None,
        }
    }

    fn prepared(choices: Vec<EnemyChoice>, enemies: usize) -> Result<PreparedBattle> {
        let mut actors = vec![crate::tests::actor(Side::Party)];
        for index in 0..enemies {
            let mut enemy = crate::tests::actor(Side::Enemy);
            enemy.control = Control::Enemy;
            enemy.position[0] = 500. + index as f32 * 100.;

            actors.push(enemy);
        }
        let actions: Vec<_> = choices
            .iter()
            .map(|row| ActionDefinition {
                normal: None,
                execution: crate::ActionExecution::Attack(crate::tests::attack(30)),
                tp_cost: row.tp,
            })
            .collect();
        let mut setup = vec![crate::ActorSetup::default(); actors.len()];
        for row in setup.iter_mut().skip(1) {
            *row = crate::ActorSetup {
                enemy_decision: Some(EnemyDecisionDefinition {
                    enemy_id: 0,
                    disabled_choices: Vec::new(),
                    strategy: TargetPolicy::Nearest,
                    difficulty: 0,
                    choices: choices.clone(),
                    back_row: vec![],
                    walk_speed: 2.,
                    walk_override: None,
                    behavior: None,
                    walk_motion: None,
                    turn_ticks: 8,
                }),
                ..Default::default()
            };
        }
        PreparedBattle::new(actors.into_iter().zip(setup).collect(), actions.into(), 7)
    }

    fn battle(choices: Vec<EnemyChoice>) -> Result<Battle> {
        prepared(choices, 1)?.finish()
    }

    #[test]
    fn allied_recipient_is_distinct_from_the_opponent_used_for_approach() -> Result<()> {
        for (policy, target_allies, recipient) in [
            (TargetPolicy::LowestHp, true, ActorId(2)),
            (TargetPolicy::SelfTarget, false, ActorId(1)),
            (TargetPolicy::NearestIncapacitated, true, ActorId(2)),
        ] {
            let mut row = choice(crate::ActionKey(0), 1);
            row.target_allies = target_allies;
            row.target_policy = Some(policy);
            let mut prepared = prepared(vec![row], 2)?;
            let mut release = crate::tests::volley();
            if policy == TargetPolicy::NearestIncapacitated {
                release.projectiles.clear();
                release.recovery = vec![crate::SpellRecovery {
                    at: 1,
                    benefit: crate::SpellBenefit::Resurrect(30),
                    targets: crate::RecoveryTargets::Target,
                    effect: None,
                }];
            }
            prepared.resources.actions.entries[0] = std::sync::Arc::new(ActionDefinition {
                normal: None,
                tp_cost: 0,
                execution: crate::ActionExecution::Casting(std::sync::Arc::new(
                    crate::CastingDefinition {
                        chant: vec![],
                        release_pose: Default::default(),
                        hide_primary_weapons: false,
                        duration: 1,
                        recovery: 1,
                        release: std::sync::Arc::new(release),
                        threat: None,
                    },
                )),
            });
            prepared.resources.actor_setup[2].enemy_decision = None;
            let mut battle = prepared.finish()?;
            let owner = ActorId(1);
            battle.actors[1].position = [30., 0., 0.];
            battle.actors[2].hp = 1;
            if policy == TargetPolicy::NearestIncapacitated {
                assert_eq!(battle.choose_enemy_action(owner)?, None);
                battle.actors[2].hp = 0;
                battle.actors[2].availability = crate::ActorAvailability::Dead;
            }
            battle.runtime[1].target = owner;
            battle.advance_enemy_ai(owner, &mut Vec::new())?;
            assert_eq!(battle.target(owner), Some(ActorId(0)));
            assert_eq!(battle.runtime[1].support_target, recipient);
            let mut released = false;
            for _ in 0..40 {
                battle.step(crate::BattleInput::default())?;
                if let Some(volley) = battle.volleys.values().find(|volley| volley.actor == owner) {
                    assert_eq!(volley.target, recipient);
                    released = true;
                    break;
                }
            }
            assert!(released, "allied action did not release after approach");
        }
        Ok(())
    }

    #[test]
    fn enemy_approach_releases_at_the_authored_upper_distance() -> Result<()> {
        for (minimum, maximum) in [(200., 150.), (500., 500.), (4000., 0.), (0., 0.)] {
            let mut row = choice(crate::ActionKey(0), 1);
            row.approach_minimum = minimum;
            row.approach_range = maximum;
            battle(vec![row])?;
        }
        for (minimum, maximum) in [(-1., 100.), (0., -1.), (f32::NAN, 1.), (0., f32::INFINITY)] {
            let mut row = choice(crate::ActionKey(0), 1);
            row.approach_minimum = minimum;
            row.approach_range = maximum;
            assert!(battle(vec![row]).is_err());
        }
        for (minimum, maximum) in [(4000., 0.), (0., 0.)] {
            let mut row = choice(crate::ActionKey(0), 1);
            row.approach_minimum = minimum;
            row.approach_range = maximum;
            row.tp = 7;
            let mut battle = battle(vec![row])?;
            let owner = ActorId(1);
            battle.actors[1].movement.turning_disabled = true;
            battle.actors[1].heading = 30.;
            let position = battle.actors[1].position;
            let tp = battle.actors[1].tp;
            battle.advance_enemy_ai(owner, &mut Vec::new())?;
            assert!(battle.approach_within_range(owner.index()));
            let frame = battle.step(crate::BattleInput::default())?;
            assert_eq!(battle.activity(owner), crate::Activity::Action);
            assert_eq!(battle.actors[1].position, position);
            assert_eq!(battle.actors[1].heading, 30.);
            assert_eq!(battle.actors[1].tp, tp - 7);
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue,
                        crate::Cue::Started { actor, .. } if *actor == owner
                    ))
                    .count(),
                1,
            );
        }
        let owner = ActorId(1);
        let mut row = choice(crate::ActionKey(0), 1);
        row.approach_range = 200.;
        let mut battle = battle(vec![row])?;
        let radii = battle.actors[0].body_radius() + battle.actors[1].body_radius();
        battle.actors[1].position[0] = radii + 201.;
        battle.actors[1].heading = -90.;
        battle.actors[1].movement.direction = [-1., 0., 0.];
        battle.advance_enemy_ai(owner, &mut Vec::new())?;
        assert!(!battle.approach_within_range(owner.index()));

        battle.actors[1].position[0] = radii + 195.;
        assert!(battle.approach_within_range(owner.index()));
        let position = battle.actors[1].position;
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.activity(owner), crate::Activity::Action);
        assert_eq!(battle.actors[1].position, position);
        Ok(())
    }

    #[test]
    fn approach_alignment_survives_action_entry() -> Result<()> {
        let mut battle = battle(vec![choice(crate::ActionKey(0), 1)])?;
        let enemy = &mut battle.actors[1];
        enemy.position = [30., 0., 0.];
        enemy.heading = 0.;
        enemy.movement.direction = [0., 0., 1.];
        battle.advance_enemy_ai(ActorId(1), &mut Vec::new())?;
        for _ in 0..8 {
            battle.step(crate::BattleInput::default())?;
            if battle.activity(ActorId(1)) == crate::Activity::Action {
                break;
            }
        }
        assert_eq!(battle.activity(ActorId(1)), crate::Activity::Action);
        assert_eq!(battle.actors[1].movement.direction, [-1., 0., 0.]);
        assert!((battle.actors[1].heading + 90.).abs() < 0.001);
        battle.step(crate::BattleInput::default())?;
        assert!((battle.actors[1].heading + 90.).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn low_health_walk_override_updates_an_active_approach_and_survives_healing() -> Result<()> {
        let mut prepared = prepared(vec![choice(crate::ActionKey(0), 1)], 1)?;
        prepared.actors[1].equipment.max_hp = 4200;
        prepared.actors[1].hp = 1092;
        prepared.resources.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .walk_override = Some(resonance_content::battle_action::EnemyWalkOverride {
            hp_percent: 25,
            speed: 2.83,
        });
        let mut battle = prepared.finish()?;
        battle.advance_enemy_ai(ActorId(1), &mut Vec::new())?;
        battle.actors[1].movement.forward = 2.;
        assert!(battle.runtime[1].task().approach().is_some());

        // Integer HP percent is 26 at 1092 HP and 25 at 1091 HP.
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.);
        battle.actors[1].hp = 1091;
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.);

        // The existing acceleration observes the new limit on the next update.
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.5);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.83);
        battle.actors[1].hp = 4200;
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.83);
        assert!(battle.runtime[1].task().approach().is_some());
        Ok(())
    }

    #[test]
    fn enemy_followup_attempts_once_and_starts_the_next_group_on_the_next_update() -> Result<()> {
        for (chance, group, disabled, chains) in [
            (100, 2, false, true),
            (0, 2, false, false),
            (100, 1, false, false),
            (100, 3, false, false),
            (100, 2, true, false),
        ] {
            let mut first = choice(crate::ActionKey(0), 1);
            first.chain_group = 1;
            first.followup_chance = chance;
            first.requirements.priority = true;
            let mut next = choice(crate::ActionKey(1), 1);
            next.chain_group = group;
            let mut prepared = prepared(vec![first, next], 1)?;
            if disabled {
                prepared.resources.actor_setup[1]
                    .enemy_decision
                    .as_mut()
                    .unwrap()
                    .disabled_choices
                    .push(1);
            }
            prepared.actors[1].movement.turning_disabled = true;
            for (index, definition) in prepared.resources.actions.entries.iter_mut().enumerate() {
                std::sync::Arc::make_mut(definition).execution =
                    crate::ActionExecution::Attack(crate::PreparedAttack {
                        contact_recovery: None,
                        opening: None,
                        chain_at: (index == 0).then_some(44),
                        end_at: 49,
                        recovery: 30,
                        events: vec![(
                            0,
                            crate::AttackEvent::Armor(if index == 0 { 1 } else { 7 }),
                        )],
                    });
            }
            let mut battle = prepared.finish()?;
            battle.start_admitted_action(
                crate::ActionRequest {
                    actor: ActorId(1),
                    action: crate::ActionKey(0),
                    target: ActorId(0),
                },
                &mut Vec::new(),
            )?;
            // Recorded handoff: action time 40 + blend 4 + two held visits.
            for tick in 1..=47 {
                if tick == 20 {
                    battle.actors[1].hit_stop = 2;
                }
                let frame = battle.step(crate::BattleInput::default())?;
                let started = frame.cues.iter().any(|cue| {
                    matches!(
                        cue,
                        crate::Cue::Started {
                            definition: Some(crate::ActionKey(1)),
                            ..
                        }
                    )
                });
                assert_eq!(
                    started,
                    chains && tick == 47,
                    "chance={chance}, group={group}, tick={tick}"
                );
            }
            assert_eq!(battle.actors[1].reaction.armor.threshold, 1);
            assert_eq!(battle.actors[1].attack_power, if chains { 85 } else { 100 });
            // A failed roll or missing group never retries the consumed window.
            battle.prepared.actor_setup[1]
                .enemy_decision
                .as_mut()
                .unwrap()
                .choices[0]
                .followup_chance = 100;
            let frame = battle.step(crate::BattleInput::default())?;
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, crate::Cue::Started { .. }))
            );
            assert_eq!(
                battle.actors[1].reaction.armor.threshold,
                if chains { 7 } else { 1 }
            );
            assert_eq!(battle.activity(ActorId(1)), crate::Activity::Action);
        }
        Ok(())
    }

    #[test]
    fn guarding_targets_and_followup_only_actions_gate_ordinary_selection() -> Result<()> {
        let mut counter = choice(crate::ActionKey(0), 1);
        counter.requirements.priority = true;
        counter.requirements.target_guarding = true;
        let mut followup = choice(crate::ActionKey(1), 255);
        followup.requirements.priority = true;
        followup.requirements.followup_only = true;
        let mut battle = battle(vec![counter, followup, choice(crate::ActionKey(2), 1)])?;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(2));
        battle.actors[0].guard.active = true;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        battle.actors[0].guard.active = false;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(2));
        Ok(())
    }

    #[test]
    fn hit_followups_exclude_blocks_and_armor_and_reset_on_next_action() -> Result<()> {
        use crate::{GuardResult as G, HitProtection as P};
        let mut next = choice(crate::ActionKey(1), 1);
        next.requirements.followup_only = true;
        next.requirements.followup_requires_hit = true;
        let mut battle = battle(vec![choice(crate::ActionKey(0), 1), next.clone()])?;
        for (guard, protection, hp_change, defeated, eligible) in [
            (G::None, P::None, -1, false, true),
            (
                G::Blocked {
                    first: true,
                    special: false,
                },
                P::None,
                -1,
                false,
                false,
            ),
            (G::None, P::Armored, -1, false, false),
            (G::None, P::None, 1, false, false),
            (G::None, P::Armored, -1, true, true),
        ] {
            battle.record_enemy_action(ActorId(1), crate::ActionKey(0));
            battle.record_enemy_contact(
                ActorId(1),
                crate::HitResult {
                    amount: 1,
                    hp_change,
                    critical: false,
                    boosted: false,
                    reaction_suppressed: false,
                    reaction_forced: false,
                    affinity: crate::Affinity::Normal,
                    guard,
                    protection,
                },
                defeated,
            );
            assert_eq!(
                battle.enemy_choice_eligible(
                    ActorId(1),
                    1,
                    &next,
                    Some(battle.runtime[1].enemy_effective_hit)
                ),
                eligible
            );
            assert!(!battle.enemy_choice_eligible(ActorId(1), 1, &next, None));
            battle.record_enemy_action(ActorId(1), crate::ActionKey(1));
            assert!(!battle.runtime[1].enemy_effective_hit);
        }
        Ok(())
    }

    #[test]
    fn enemy_count_limit_opens_after_allies_are_defeated() -> Result<()> {
        let mut limited = choice(crate::ActionKey(0), 1);
        limited.requirements.priority = true;
        limited.requirements.max_enemies = Some(2);
        let mut battle = prepared(vec![limited, choice(crate::ActionKey(1), 1)], 3)?.finish()?;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(1));
        battle.actors[3].availability = crate::ActorAvailability::Dead;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .choices[0]
            .requirements
            .max_enemies = Some(1);
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(1));
        battle.actors[2].availability = crate::ActorAvailability::Dead;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        let owner = battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap();
        owner.enemy_id = 10;
        owner.choices[0].requirements.max_enemies = None;
        owner.choices[0].requirements.predicates =
            vec![resonance_content::battle_action::EnemyPredicate::AllyAbsent(10)];
        battle.prepared.actor_setup[2]
            .enemy_decision
            .as_mut()
            .unwrap()
            .enemy_id = 10;
        battle.actors[2].availability = crate::ActorAvailability::Active;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(1));
        battle.actors[2].availability = crate::ActorAvailability::Dead;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        Ok(())
    }

    #[test]
    fn living_departure_keeps_weighted_admission_and_excludes_only_departing_rewards() -> Result<()>
    {
        use resonance_content::battle_action::EnemyPredicate;
        let mut departure = choice(crate::ActionKey(1), 100);
        departure.requirements.predicates = vec![EnemyPredicate::AllyAbsent(30)];
        let mut prepared = prepared(vec![choice(crate::ActionKey(0), 100), departure], 3)?;
        std::sync::Arc::make_mut(&mut prepared.resources.actions.entries[1]).execution =
            crate::ActionExecution::LeaveArena;
        for (index, setup) in prepared
            .resources
            .actor_setup
            .iter_mut()
            .enumerate()
            .skip(1)
        {
            let decision = setup.enemy_decision.as_mut().unwrap();
            decision.enemy_id = if index == 3 { 30 } else { 31 };
            decision.walk_speed = 1.8;
            decision.turn_ticks = 20;
            decision.choices[1].approach_range = 0.;
            prepared.actors[index].hp = prepared.actors[index].equipment.max_hp;
        }
        prepared.actors[1].position = [897., 0., 0.];
        prepared.actors[2].position = [870., 0., 0.];
        prepared.actors[3].position = [1000., 0., 0.];
        let mut battle = prepared.with_arena_boundary().finish()?;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        battle.actors[3].hp = 0;
        battle.actors[3].availability = crate::ActorAvailability::Dead;
        let mut selected = std::collections::BTreeSet::new();
        for _ in 0..32 {
            selected.insert(battle.choose_enemy_action(ActorId(1))?.unwrap());
        }
        assert_eq!(selected, [0, 1].into_iter().collect());

        // Select departures for this lifecycle check; the weighted competition above stays intact.
        for owner in [ActorId(1), ActorId(2)] {
            battle.prepared.actor_setup[owner.index()]
                .enemy_decision
                .as_mut()
                .unwrap()
                .choices[0]
                .weight = 0;
            battle.advance_enemy_ai(owner, &mut Vec::new())?;
            assert_eq!(battle.activity(owner), crate::Activity::Escaping);
            assert_eq!(battle.actors[owner.index()].movement.forward, 0.);
        }
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].position[0], 897.);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].position[0], 897.5);
        assert_eq!(battle.actors[1].heading, 9.);
        battle.step(crate::BattleInput::default())?;
        assert!(battle.actors[1].available());
        battle.step(crate::BattleInput::default())?;
        assert_eq!(
            battle.actors[1].availability,
            crate::ActorAvailability::Absent
        );
        assert!(battle.actors[2].available());
        assert_eq!(battle.recognize_result(), None);
        assert!(!battle.ledger.reward_eligible(ActorId(1)));
        assert!(battle.ledger.reward_eligible(ActorId(2)));

        for _ in 0..20 {
            if !battle.actors[2].available() {
                break;
            }
            battle.step(crate::BattleInput::default())?;
        }
        assert_eq!(
            battle.actors[2].availability,
            crate::ActorAvailability::Absent
        );
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        assert!(battle.ledger.reward_eligible(ActorId(3)));
        for owner in [ActorId(1), ActorId(2)] {
            let actor = &battle.actors[owner.index()];
            assert_eq!(actor.hp, actor.equipment.max_hp);
            assert!(!battle.ledger.reward_eligible(owner));
        }
        Ok(())
    }

    #[test]
    fn once_per_battle_is_consumed_on_start_and_survives_interruption() -> Result<()> {
        let mut first = choice(crate::ActionKey(0), 1);
        first.requirements.priority = true;
        first.requirements.once_per_battle = true;
        let mut battle = battle(vec![first, choice(crate::ActionKey(1), 1)])?;
        battle.advance_enemy_ai(ActorId(1), &mut Vec::new())?;
        assert_eq!(battle.activity(ActorId(1)), crate::Activity::Approaching);
        battle.cancel_approach(1)?;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        battle.start_admitted_action(
            crate::ActionRequest {
                actor: ActorId(1),
                action: crate::ActionKey(0),
                target: ActorId(0),
            },
            &mut Vec::new(),
        )?;
        battle.interrupt_actor(ActorId(1), &mut Vec::new());
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(1));
        Ok(())
    }

    #[test]
    fn expanded_choice_tables_keep_wide_weights_and_validate_back_row_indices() -> Result<()> {
        let mut candidate = prepared(
            (0..300)
                .map(|index| choice(crate::ActionKey(index), 255))
                .collect(),
            3,
        )?;
        candidate.resources.actor_setup[3]
            .enemy_decision
            .as_mut()
            .unwrap()
            .back_row = vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 255)];
        let mut battle = candidate.finish()?;
        for _ in 0..16 {
            assert!(
                battle
                    .choose_enemy_action(ActorId(1))?
                    .is_some_and(|index| index < 300)
            );
        }
        assert_eq!(battle.choose_enemy_action(ActorId(3))?, Some(4));
        battle.prepared.actor_setup[3]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .push(4);
        assert_eq!(battle.choose_enemy_action(ActorId(3))?, None);
        let mut invalid = prepared(vec![choice(crate::ActionKey(0), 1)], 1)?;
        invalid.resources.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .back_row = vec![(1, 1)];
        assert!(invalid.finish().is_err());
        Ok(())
    }

    #[test]
    fn enemy_range_choices_use_current_geometry_at_entry_and_after_movement() -> Result<()> {
        let mut nearby = choice(crate::ActionKey(0), 1);
        nearby.range = [0, 100];
        let mut distant = choice(crate::ActionKey(1), 1);
        distant.range = [101, 1000];
        let mut battle = battle(vec![nearby, distant])?;
        assert_eq!(
            battle.runtime[1].enemy_choice,
            Some(1),
            "entry uses the real initial gap"
        );
        for (x, expected) in [(50., 0), (500., 1)] {
            let enemy = &mut battle.actors[1];
            enemy.position[0] = x;

            assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(expected));
        }
        Ok(())
    }

    #[test]
    fn waiting_enemy_completes_a_taunt_without_artwork_or_party_rewards() -> Result<()> {
        let mut battle = prepared(vec![choice(crate::ActionKey(0), 1)], 3)?.finish()?;
        battle.prepared.unison_available = true;
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.activity(ActorId(3)), crate::Activity::Taunting);
        for _ in 0..120 {
            battle.step(crate::BattleInput::default())?;
            if battle.actor_command_ready(ActorId(3)) {
                break;
            }
        }
        assert!(battle.actor_command_ready(ActorId(3)));
        assert_eq!(battle.unison_gauge(), 0);
        Ok(())
    }

    #[test]
    fn weighted_choices_exclude_unaffordable_and_unmet_requirements() -> Result<()> {
        let mut costly = choice(crate::ActionKey(0), 100);
        costly.tp = 40;
        let mut hard = choice(crate::ActionKey(1), 100);
        hard.requirements.difficulty = 2..=2;
        let mut battle = battle(vec![costly, hard, choice(crate::ActionKey(2), 1)])?;
        battle.actors[1].tp = 0;
        for _ in 0..10 {
            assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(2));
        }
        assert_eq!(battle.runtime[1].enemy_choice, Some(2));
        assert_eq!(battle.actors[1].guard.enemy_chance, 20);
        assert_eq!(battle.target(ActorId(1)), Some(ActorId(0)));
        assert_eq!(battle.actors[1].movement.target_direction, [-1., 0., 0.]);
        // A row can omit a separate TP requirement while its shared arte still costs TP.
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .choices[0]
            .tp = 0;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(2));
        battle.actors[0].availability = crate::ActorAvailability::Dead;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, None);
        assert_eq!(battle.runtime[1].enemy_choice, None);
        Ok(())
    }

    #[test]
    fn priority_requires_eligibility_and_no_eligible_choice_waits() -> Result<()> {
        let mut priority = choice(crate::ActionKey(0), 1);
        priority.requirements.priority = true;
        priority.requirements.hp_percent = Some(25);
        let mut battle = battle(vec![priority, choice(crate::ActionKey(1), 1)])?;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(1));
        battle.actors[1].hp = 1;
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(0));
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .push(0);
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, Some(1));
        battle.actors[1].tp = 0;
        let prepared = &mut battle.prepared;
        for row in &mut prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .choices
        {
            row.tp = 1;
        }
        assert_eq!(battle.choose_enemy_action(ActorId(1))?, None);
        Ok(())
    }

    #[test]
    fn nonrepeat_tracks_executed_rows_and_shared_predicates_observe_live_roster() -> Result<()> {
        use resonance_content::battle_action::EnemyPredicate;
        let mut primary = choice(crate::ActionKey(0), 1);
        primary.requirements.priority = true;
        primary.requirements.predicates = vec![
            EnemyPredicate::DoNotRepeat,
            EnemyPredicate::OriginDistance {
                minimum: Some(500),
                maximum: Some(600),
            },
            EnemyPredicate::OpponentCount {
                minimum: Some(1),
                maximum: Some(1),
            },
        ];
        let mut battle = battle(vec![primary, choice(crate::ActionKey(1), 1)])?;
        let owner = ActorId(1);
        // Reconsidering an approach does not consume the repeat restriction.
        assert_eq!(battle.choose_enemy_action(owner)?, Some(0));
        assert_eq!(battle.choose_enemy_action(owner)?, Some(0));
        battle.record_enemy_action(owner, crate::ActionKey(0));
        assert_eq!(battle.choose_enemy_action(owner)?, Some(1));
        battle.record_enemy_action(owner, crate::ActionKey(1));
        battle.actors[1].position[0] = 600.;
        assert_eq!(battle.choose_enemy_action(owner)?, Some(0));
        battle.actors[1].position[0] = 601.;
        assert_eq!(battle.choose_enemy_action(owner)?, Some(1));
        battle.actors[1].position[0] = 499.;
        assert_eq!(battle.choose_enemy_action(owner)?, Some(1));
        battle.actors[1].position[0] = 500.;
        battle.actors[0].availability = crate::ActorAvailability::Dead;
        assert_eq!(battle.choose_enemy_action(owner)?, None);
        Ok(())
    }
}

//! Weighted enemy choices and native action dispatch.
use crate::{ActorId, Battle, Difficulty, PreparedBattle, TargetPolicy};
use anyhow::{Context, Result, ensure};

pub use resonance_content::battle_action::EnemyRequirements;

/// An entry in the enemy catalogue, independent of its live actor slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyId(pub u8);

/// An actor's prepared enemy-choice row, distinct from its executable action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnemyChoiceId(pub usize);

impl std::fmt::Display for EnemyChoiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Finite branch draws for an observed enemy action sequence.
#[cfg(feature = "test-support")]
#[derive(Clone, Copy, Debug)]
pub struct FixtureEnemyChainInput {
    pub clock: u32,
    pub actor: ActorId,
    pub row: EnemyChoiceId,
    pub chance: u16,
    pub selection: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct EnemyChoice {
    pub action: crate::ActionKey,
    /// Physical artes consume retained movement on their admission visit.
    pub physical_arte: bool,
    pub weight: u8,
    pub chain_group: u8,
    pub followup_chance: u8,
    pub requirements: EnemyRequirements,
    pub target_policy: Option<TargetPolicy>,
    pub target_allies: bool,
    pub return_to_formation: bool,
    pub returning: Option<crate::MotionBinding>,
    /// A running return overrides the actor's ordinary walking speed.
    pub return_speed: Option<f32>,
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
    pub enemy_id: EnemyId,
    pub strategy: TargetPolicy,
    pub difficulty: Difficulty,
    pub choices: Vec<EnemyChoice>,
    pub disabled_choices: Vec<EnemyChoiceId>,
    pub back_row: Vec<(EnemyChoiceId, u8)>,
    pub walk_speed: f32,
    pub walk_override: Option<resonance_content::battle_action::EnemyWalkOverride>,
    pub behavior: Option<crate::EnemyBehavior>,
    pub walk_motion: Option<crate::MotionBinding>,
    pub run_motion: Option<crate::MotionBinding>,
    pub turn_ticks: u8,
}

impl EnemyDecisionDefinition {
    fn approach_parameters(
        &self,
        row: &EnemyChoice,
        walk_override: Option<f32>,
    ) -> crate::ApproachParameters {
        let walking = crate::ApproachParameters {
            minimum: row.approach_minimum,
            maximum: row.approach_range,
            motion: self.walk_motion,
            motion_rate: 0.5,
            speed: walk_override.unwrap_or(self.walk_speed),
            turn_ticks: self.turn_ticks,
        };
        if walk_override.is_some() {
            walking
        } else {
            row.approach.unwrap_or(walking)
        }
    }
}

impl PreparedBattle {
    /// Scale distant enemy engagement with the number of combined formations.
    pub fn with_formation_count(mut self, count: usize) -> Result<Self> {
        ensure!(count > 0, "empty battle encounter");
        self.resources.formation_count = count;
        Ok(self)
    }

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

            ensure!(!definition.choices.is_empty(), "empty enemy choice table");
            ensure!(
                definition
                    .disabled_choices
                    .iter()
                    .all(|&row| row.0 < definition.choices.len()),
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
                    row.return_speed
                        .is_none_or(|speed| speed.is_finite() && speed > 0.),
                    "invalid enemy return speed"
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
                    .all(|&(row, _)| row.0 < definition.choices.len()),
                "invalid back-row choice"
            );
        }
        Ok(())
    }
}

impl Battle {
    /// Replay an observed enemy entry without rerunning weighted selection.
    #[cfg(feature = "test-support")]
    pub fn request_enemy_approach(
        &mut self,
        request: crate::ActionRequest,
        combat_target: ActorId,
    ) -> Result<bool> {
        let definition = self.enemy_decision(request.actor)?;
        let row = definition
            .choices
            .iter()
            .find(|row| row.action == request.action)
            .context("requested enemy action has no authored entry")?;
        let parameters = definition
            .approach_parameters(row, self.runtime[request.actor.index()].enemy_walk_override);
        let allied_recipient = row.target_allies
            || row.target_policy.unwrap_or(definition.strategy) == TargetPolicy::SelfTarget;
        ensure!(
            allied_recipient || request.target == combat_target,
            "hostile action recipient differs from its approach target"
        );
        let accepted = self.request_approach(
            crate::ActionRequest {
                actor: request.actor,
                target: combat_target,
                action: request.action,
            },
            combat_target,
            parameters,
        )?;
        if accepted {
            self.set_decision_target(request.actor, combat_target)?;
            if allied_recipient {
                self.set_approach_action_target(request.actor, request.target)?;
            }
        }
        Ok(accepted)
    }

    pub(crate) fn enemy_physical_arte(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        self.enemy_choices(actor).is_some_and(|choices| {
            choices
                .iter()
                .any(|row| row.action == action && row.physical_arte)
        })
    }

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
        index: EnemyChoiceId,
        row: &EnemyChoice,
        followup_hit: Option<bool>,
    ) -> bool {
        let Some(target) = self.target(owner) else {
            return false;
        };
        let gap = self.body_gap(owner, target);
        let maximum = if row.range[1] == 0 {
            f32::INFINITY
        } else {
            f32::from(row.range[1])
        };
        gap >= f32::from(row.range[0])
            && gap <= maximum
            && self.enemy_choice_available(owner, index, row, followup_hit)
    }

    fn enemy_choice_available(
        &self,
        owner: ActorId,
        index: EnemyChoiceId,
        row: &EnemyChoice,
        followup_hit: Option<bool>,
    ) -> bool {
        let actor = &self.actors[owner.index()];
        let definition = self.prepared.actor_setup[owner.index()]
            .enemy_decision
            .as_ref()
            .unwrap();
        if definition.disabled_choices.contains(&index) || self.defense_arm_waiting(owner) {
            return false;
        }
        let difficulty = definition.difficulty as u8;
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
        self.enemy_behavior_allows_choice(owner, index)
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
                                        u16::from(definition.enemy_id.0) == enemy
                                    })
                        })
                    }
                }
            })
    }

    pub(crate) fn choose_enemy_action(&mut self, owner: ActorId) -> Result<Option<EnemyChoiceId>> {
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
        let cap = if definition.difficulty == Difficulty::Mania {
            8
        } else {
            (party_count + 1).min(3)
        };
        let cap = cap.saturating_mul(self.prepared.formation_count);
        let waiting = self.enemy_waiting_rank(owner) >= cap;
        let mut candidates: Vec<_> = if waiting {
            definition
                .back_row
                .iter()
                .map(|&(index, weight)| (index, u64::from(weight)))
                .collect()
        } else {
            definition
                .choices
                .iter()
                .enumerate()
                .map(|(index, row)| (EnemyChoiceId(index), u64::from(row.weight)))
                .collect()
        };
        candidates.retain(|&(index, _)| {
            self.enemy_choice_eligible(owner, index, &definition.choices[index.0], None)
        });
        // The first action is the basic fallback. Its approach closes the gap
        // when no action is currently in range; rear-row enemies still wait.
        let fallback = (!waiting
            && definition.choices.first().is_some_and(|row| {
                self.enemy_choice_available(owner, EnemyChoiceId(0), row, None)
            }))
        .then_some(EnemyChoiceId(0));
        let selected = self
            .weighted_enemy_choice(owner, candidates, waiting)?
            .or(fallback);
        self.runtime[owner.index()].enemy_choice = selected;
        if let Some(index) = selected {
            let definition = self.enemy_decision(owner)?;
            let row = &definition.choices[index.0];
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

    pub(crate) fn refresh_enemy_choice(&mut self, owner: ActorId) {
        let runtime = &self.runtime[owner.index()];
        if runtime.task().action().is_some()
            || runtime
                .enemy_choice
                .is_none_or(|row| self.enemy_behavior_allows_choice(owner, row))
        {
            return;
        }
        let approaching = runtime.task().approach().is_some();
        self.runtime[owner.index()].enemy_choice = None;
        if approaching {
            self.cancel_approach(owner.index());
        }
    }

    fn weighted_enemy_choice(
        &mut self,
        owner: ActorId,
        candidates: Vec<(EnemyChoiceId, u64)>,
        waiting: bool,
    ) -> Result<Option<EnemyChoiceId>> {
        let definition = self.enemy_decision(owner)?;
        let priority = candidates
            .iter()
            .find(|&&(index, _)| definition.choices[index.0].requirements.priority)
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
            .filter(|&index| definition.choices[index.0].action == action)
            .or_else(|| {
                definition
                    .choices
                    .iter()
                    .position(|row| row.action == action)
                    .map(EnemyChoiceId)
            });
        runtime.enemy_choice = selected;
        runtime.last_executed_enemy_row = selected;
        runtime.enemy_effective_hit = false;
        if let Some(index) = selected
            && definition.choices[index.0].requirements.once_per_battle
        {
            runtime.used_enemy_choices.insert(index);
        }
    }

    #[cfg(feature = "test-support")]
    pub fn install_fixture_enemy_chains(
        &mut self,
        actor: ActorId,
        origin: u32,
        mut inputs: Vec<FixtureEnemyChainInput>,
    ) -> Result<()> {
        ensure!(
            self.fixture_enemy_chains.is_none(),
            "chain inputs already installed"
        );
        let count = self.enemy_decision(actor)?.choices.len();
        let mut previous = 0;
        for input in &mut inputs {
            ensure!(
                input.actor == actor && input.row.0 < count && input.clock > previous,
                "invalid ordered enemy chain input"
            );
            previous = input.clock;
            input.clock = input
                .clock
                .checked_add(origin)
                .context("chain input clock overflow")?;
        }
        self.prepared.actor_setup[actor.index()]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .clear();
        self.fixture_enemy_chains = Some(inputs.into());
        Ok(())
    }

    #[cfg(feature = "test-support")]
    pub fn fixture_enemy_choice(&self, actor: ActorId) -> Option<EnemyChoiceId> {
        self.runtime[actor.index()].enemy_choice
    }

    #[cfg(feature = "test-support")]
    pub fn select_fixture_enemy_row(
        &mut self,
        actor: ActorId,
        row: EnemyChoiceId,
    ) -> Result<crate::ActionKey> {
        let action = self
            .enemy_decision(actor)?
            .choices
            .get(row.0)
            .context("unknown observed enemy row")?
            .action;
        self.runtime[actor.index()].enemy_choice = Some(row);
        // An independent selection begins a new attack chain, even without travel.
        self.actors[actor.index()].attack_power = 100;
        Ok(action)
    }

    #[cfg(feature = "test-support")]
    pub fn finish_fixture_enemy_chains(&self) -> Result<()> {
        ensure!(
            self.fixture_enemy_chains
                .as_ref()
                .is_none_or(|inputs| inputs.is_empty()),
            "unused enemy chain inputs"
        );
        Ok(())
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
        let row = &self.enemy_decision(actor)?.choices[index.0];
        let chance = row.followup_chance;
        let followup = row.chain_group != 0;
        let Some(group) = row.chain_group.checked_add(1) else {
            return Ok(false);
        };
        #[cfg(feature = "test-support")]
        let recorded = if let Some(inputs) = &mut self.fixture_enemy_chains {
            let input = inputs
                .pop_front()
                .context("unobserved enemy chain decision")?;
            ensure!(
                input.actor == actor
                    && input.row == index
                    && input.clock == self.ledger.combat_ticks,
                "enemy chain decision differs: expected actor {:?}, row {}, combat tick {}; observed actor {:?}, row {}, combat tick {}",
                input.actor,
                input.row,
                input.clock,
                actor,
                index,
                self.ledger.combat_ticks
            );
            Some(input)
        } else {
            None
        };
        #[cfg(not(feature = "test-support"))]
        let recorded: Option<(u16, Option<u16>)> = None;
        #[cfg(feature = "test-support")]
        let recorded = recorded.map(|input| (input.chance, input.selection));
        let draw = recorded.map_or_else(|| self.random.next_u16(), |input| input.0);
        if draw % 100 >= u16::from(chance) {
            ensure!(
                recorded.is_none_or(|input| input.1.is_none()),
                "unused chain selection draw"
            );
            return Ok(false);
        }
        let candidates: Vec<_> = self
            .enemy_decision(actor)?
            .choices
            .iter()
            .enumerate()
            .filter(|(index, row)| {
                (!followup || row.chain_group == group)
                    && self.enemy_choice_eligible(
                        actor,
                        EnemyChoiceId(*index),
                        row,
                        followup.then_some(self.runtime[actor.index()].enemy_effective_hit),
                    )
            })
            .map(|(index, row)| {
                (
                    EnemyChoiceId(index),
                    if followup { 1 } else { u64::from(row.weight) },
                )
            })
            .collect();
        let total: u64 = candidates.iter().map(|(_, weight)| weight).sum();
        if total == 0 {
            ensure!(
                recorded.is_none_or(|input| input.1.is_none()),
                "selection draw has no eligible chain"
            );
            return Ok(false);
        }
        let draw = match recorded {
            Some((_, draw)) => draw.context("enemy chain selection lacks its observed draw")?,
            None => self.random.next_u16(),
        };
        // Priority applies when the weighted scan reaches that row.
        let mut threshold = u64::from(draw) % total;
        let definition = self.enemy_decision(actor)?;
        let index = candidates
            .into_iter()
            .find_map(|(index, weight)| {
                if threshold < weight || definition.choices[index.0].requirements.priority {
                    Some(index)
                } else {
                    threshold -= weight;
                    None
                }
            })
            .expect("positive candidate total must select a row");
        let row = &self.enemy_decision(actor)?.choices[index.0];
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
            && self.runtime[index].enemy_walk_override.is_none()
            && self.actors[index].hp_percent() <= i16::from(value.hp_percent)
        {
            self.runtime[index].enemy_walk_override = Some(value.speed);
            if self.prepared.actor_setup[index]
                .enemy_decision
                .as_ref()
                .is_some_and(|definition| definition.difficulty == Difficulty::Hard)
                && self
                    .actors
                    .iter()
                    .filter(|owner| owner.side == crate::Side::Enemy && owner.available())
                    .count()
                    == 1
            {
                self.runtime[index].enemy_cast_base_ticks = value.last_enemy_hard_cast_ticks;
            }
            let (clip, replacement) = value.motion_alias;
            self.model_requests
                .push(crate::ModelRequest::MotionAliases {
                    actor,
                    aliases: [(clip, Some(replacement))].into(),
                });
            if self.runtime[index].task().approach().is_some()
                && let Some(motion) = self.prepared.actor_setup[index]
                    .enemy_decision
                    .as_ref()
                    .and_then(|definition| definition.walk_motion)
            {
                let owner = &self.actors[index];
                let rate = owner.motion_rate(0.5, owner.conditions.effective());
                self.request_pose(
                    actor,
                    Some(motion),
                    crate::Pose {
                        rate,
                        repeat: true,
                        ..Default::default()
                    },
                );
            }
        }
    }

    pub(crate) fn advance_enemy_ai(
        &mut self,
        owner: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        if self.defense_arm_waiting(owner)
            || self.is_hosted(owner)
            || !self.decision_ready(owner)
            || !self.actors[owner.index()].movement.hover_ready()
        {
            return Ok(());
        }
        let Some(selected) = self.choose_enemy_action(owner)? else {
            self.begin_taunt(owner);
            return Ok(());
        };
        let definition = self.enemy_decision(owner)?;
        let row = &definition.choices[selected.0];
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
        let parameters =
            definition.approach_parameters(row, self.runtime[owner.index()].enemy_walk_override);
        let allied_recipient = row.target_allies
            || row.target_policy.unwrap_or(definition.strategy) == TargetPolicy::SelfTarget;
        if self.request_approach(
            crate::ActionRequest {
                actor: owner,
                target: target,
                action: action,
            },
            target,
            parameters,
        )? && allied_recipient
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
            weight,
            return_to_formation: true,
            guard_chance: 20,
            approach_range: 80.,
            ..crate::tests::enemy_choice(action)
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
                enemy_decision: Some(crate::tests::enemy_decision(choices.clone())),
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
        for (policy, target_allies, recipient, benefit) in [
            (
                TargetPolicy::LowestHp,
                true,
                ActorId(2),
                Some(crate::SpellBenefit::Hp(30)),
            ),
            (
                TargetPolicy::SelfTarget,
                false,
                ActorId(1),
                Some(crate::SpellBenefit::Hp(30)),
            ),
            (
                TargetPolicy::NearestIncapacitated,
                true,
                ActorId(2),
                Some(crate::SpellBenefit::Resurrect(30)),
            ),
            (TargetPolicy::SelfTarget, true, ActorId(1), None),
        ] {
            let mut row = choice(crate::ActionKey(0), 1);
            row.target_allies = target_allies;
            row.target_policy = Some(policy);
            let mut prepared = prepared(vec![row], 2)?;
            let mut release = crate::tests::volley();
            if let Some(benefit) = benefit {
                release.projectiles.clear();
                release.recovery = vec![crate::SpellRecovery {
                    duration: 1,
                    effect_placement: Default::default(),
                    flash: None,
                    at: 1,
                    benefit,
                    targets: crate::RecoveryTargets::Target,
                    effect: None,
                }];
            }
            prepared.resources.actions.entries[0] = std::sync::Arc::new(ActionDefinition {
                normal: None,
                tp_cost: 0,
                execution: crate::ActionExecution::Casting(std::sync::Arc::new(
                    crate::CastingDefinition {
                        armor: None,
                        incantation_lead: None,
                        chant: vec![],
                        release_pose: Default::default(),
                        hide_primary_weapons: false,
                        duration: 1,
                        profile_ticks: 1,
                        spell_ticks: 0,
                        fixed_timing: false,
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
                    assert_eq!(
                        volley.target,
                        if benefit.is_some() {
                            recipient
                        } else {
                            ActorId(0)
                        }
                    );
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
            // Selection remains pending for its first visit, even without turning.
            let entry = battle.step(crate::BattleInput::default())?;
            assert_eq!(battle.activity(owner), crate::Activity::Approaching);
            assert_eq!(battle.actors[1].position, position);
            assert_eq!(battle.actors[1].heading, 30.);
            assert_eq!(battle.actors[1].tp, tp);
            assert!(!entry.cues.iter().any(|cue| matches!(cue,
                crate::Cue::Started { actor, .. } if *actor == owner
            )));
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

        let mut started = false;
        for _ in 0..300 {
            let frame = battle.step(crate::BattleInput::default())?;
            if frame.cues.iter().any(|cue| {
                matches!(cue,
                    crate::Cue::Started { actor, .. } if *actor == owner
                )
            }) {
                started = true;
                break;
            }
        }
        assert!(started, "enemy never reached its action range");
        assert!(battle.body_gap(owner, ActorId(0)) <= 200.);
        assert_eq!(battle.activity(owner), crate::Activity::Action);
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
        for (physical_arte, casting) in [(false, false), (true, false), (false, true)] {
            let mut row = choice(crate::ActionKey(0), 1);
            row.physical_arte = physical_arte;
            let mut prepared = prepared(vec![row], 1)?;
            if casting {
                prepared.resources.actions.entries[0] =
                    std::sync::Arc::new(crate::tests::cast(60, 0));
            }
            let mut subject = prepared.finish()?;
            subject.actors[1].heading = -90.;
            subject.actors[1].movement.direction = [-1., 0., 0.];
            subject.advance_enemy_ai(ActorId(1), &mut Vec::new())?;
            let mut admitted = false;
            for _ in 0..300 {
                let position = subject.actors[1].position[0];
                let forward = subject.actors[1].movement.forward;
                subject.step(crate::BattleInput::default())?;
                if matches!(
                    subject.activity(ActorId(1)),
                    crate::Activity::Action | crate::Activity::Casting { .. }
                ) {
                    assert!(forward > 0.);
                    let expected = if physical_arte || casting {
                        position - forward
                    } else {
                        position
                    };
                    assert_eq!(subject.actors[1].position[0], expected);
                    admitted = true;
                    break;
                }
            }
            assert!(admitted);
        }
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
            .difficulty = Difficulty::Hard;
        prepared.resources.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .walk_override = Some(resonance_content::battle_action::EnemyWalkOverride {
            hp_percent: 25,
            speed: 2.83,
            motion_alias: (1, 45),
            last_enemy_hard_cast_ticks: Some(0),
        });
        let mut battle = prepared.finish()?;
        battle.advance_enemy_ai(ActorId(1), &mut Vec::new())?;
        battle.actors[1].movement.forward = 2.;
        assert!(battle.runtime[1].task().approach().is_some());

        // Integer HP percent is 26 at 1092 HP and 25 at 1091 HP.
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.);
        assert_eq!(battle.runtime[1].enemy_cast_base_ticks, None);
        battle.actors[1].hp = 1091;
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.);
        assert_eq!(battle.runtime[1].enemy_cast_base_ticks, Some(0));

        // The existing acceleration observes the new limit on the next update.
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.5);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.83);
        battle.actors[1].hp = 4200;
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[1].movement.forward, 2.83);
        assert!(battle.runtime[1].task().approach().is_some());
        assert_eq!(battle.runtime[1].enemy_cast_base_ticks, Some(0));
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
                    .push(EnemyChoiceId(1));
            }
            prepared.actors[1].movement.turning_disabled = true;
            for (index, definition) in prepared.resources.actions.entries.iter_mut().enumerate() {
                std::sync::Arc::make_mut(definition).execution =
                    crate::ActionExecution::Attack(crate::PreparedAttack {
                        contact_recovery: None,
                        opening: None,
                        chain_at: (index == 0).then_some(44),
                        chain_retry_at: None,
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
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(2))
        );
        battle.actors[0].guard.active = true;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
        battle.actors[0].guard.active = false;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(2))
        );
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
                    EnemyChoiceId(1),
                    &next,
                    Some(battle.runtime[1].enemy_effective_hit)
                ),
                eligible
            );
            assert!(!battle.enemy_choice_eligible(ActorId(1), EnemyChoiceId(1), &next, None));
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
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(1))
        );
        battle.actors[3].availability = crate::ActorAvailability::Dead;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .choices[0]
            .requirements
            .max_enemies = Some(1);
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(1))
        );
        battle.actors[2].availability = crate::ActorAvailability::Dead;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
        let owner = battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap();
        owner.enemy_id = EnemyId(10);
        owner.choices[0].requirements.max_enemies = None;
        owner.choices[0].requirements.predicates =
            vec![resonance_content::battle_action::EnemyPredicate::AllyAbsent(10)];
        battle.prepared.actor_setup[2]
            .enemy_decision
            .as_mut()
            .unwrap()
            .enemy_id = EnemyId(10);
        battle.actors[2].availability = crate::ActorAvailability::Active;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(1))
        );
        battle.actors[2].availability = crate::ActorAvailability::Dead;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
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
            decision.enemy_id = if index == 3 { EnemyId(30) } else { EnemyId(31) };
            decision.walk_speed = 1.8;
            decision.turn_ticks = 20;
            decision.choices[1].approach_range = 0.;
            prepared.actors[index].hp = prepared.actors[index].equipment.max_hp;
        }
        prepared.actors[1].position = [897., 0., 0.];
        prepared.actors[2].position = [870., 0., 0.];
        prepared.actors[3].position = [1000., 0., 0.];
        let mut battle = prepared.with_arena_boundary().finish()?;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
        battle.actors[3].hp = 0;
        battle.actors[3].availability = crate::ActorAvailability::Dead;
        let mut selected = std::collections::BTreeSet::new();
        for _ in 0..32 {
            selected.insert(battle.choose_enemy_action(ActorId(1))?.unwrap());
        }
        assert_eq!(selected, [EnemyChoiceId(0), EnemyChoiceId(1)].into());

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
        assert_eq!(battle.actors[1].heading, 18.);
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
        battle.cancel_approach(1);
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
        battle.start_admitted_action(
            crate::ActionRequest {
                actor: ActorId(1),
                action: crate::ActionKey(0),
                target: ActorId(0),
            },
            &mut Vec::new(),
        )?;
        battle.interrupt_actor(ActorId(1), &mut Vec::new());
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(1))
        );
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
            .back_row = vec![
            (EnemyChoiceId(0), 0),
            (EnemyChoiceId(1), 0),
            (EnemyChoiceId(2), 0),
            (EnemyChoiceId(3), 0),
            (EnemyChoiceId(4), 255),
        ];
        let mut battle = candidate.finish()?;
        for _ in 0..16 {
            assert!(
                battle
                    .choose_enemy_action(ActorId(1))?
                    .is_some_and(|index| index.0 < 300)
            );
        }
        assert_eq!(
            battle.choose_enemy_action(ActorId(3))?,
            Some(EnemyChoiceId(4))
        );
        battle.prepared.actor_setup[3]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .push(EnemyChoiceId(4));
        assert_eq!(battle.choose_enemy_action(ActorId(3))?, None);
        let mut invalid = prepared(vec![choice(crate::ActionKey(0), 1)], 1)?;
        invalid.resources.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .back_row = vec![(EnemyChoiceId(1), 1)];
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
            Some(EnemyChoiceId(1)),
            "entry uses the real initial gap"
        );
        for (x, expected) in [(50., 0), (500., 1), (1400., 0)] {
            let enemy = &mut battle.actors[1];
            enemy.position[0] = x;

            assert_eq!(
                battle.choose_enemy_action(ActorId(1))?,
                Some(EnemyChoiceId(expected))
            );
        }
        Ok(())
    }

    #[test]
    fn basic_attack_fallback_moves_into_range_and_respects_requirements() -> Result<()> {
        let mut basic = choice(crate::ActionKey(0), 1);
        basic.range = [0, 100];
        basic.tp = 1;
        let mut battle = battle(vec![basic])?;
        let enemy = ActorId(1);
        battle.actors[1].tp = 0;
        assert_eq!(battle.choose_enemy_action(enemy)?, None);
        battle.actors[1].tp = 1;
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .push(EnemyChoiceId(0));
        assert_eq!(battle.choose_enemy_action(enemy)?, None);
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .clear();
        battle.advance_enemy_ai(enemy, &mut vec![])?;
        assert_eq!(battle.activity(enemy), crate::Activity::Approaching);
        for _ in 0..400 {
            let frame = battle.step(crate::BattleInput::default())?;
            if frame.actions.iter().any(|(_, actor, _)| *actor == enemy) {
                assert!(battle.body_gap(enemy, ActorId(0)) <= 100.);
                return Ok(());
            }
        }
        anyhow::bail!("enemy did not approach and attack")
    }

    #[test]
    fn combined_formations_scale_engagement_without_restricting_nearby_enemies() -> Result<()> {
        for (difficulty, party_count, allowance) in [
            (Difficulty::Normal, 1, 2),
            (Difficulty::Normal, 4, 3),
            (Difficulty::Hard, 4, 3),
            (Difficulty::Mania, 4, 8),
        ] {
            for formations in 1..=3 {
                let source = prepared(vec![choice(crate::ActionKey(0), 1)], 25)?;
                let mut participants: Vec<_> = source
                    .actors
                    .into_iter()
                    .zip(source.resources.actor_setup)
                    .collect();
                for (_, setup) in participants.iter_mut().skip(1) {
                    setup.enemy_decision.as_mut().unwrap().difficulty = difficulty;
                }
                participants.extend(
                    (1..party_count)
                        .map(|_| (crate::tests::actor(Side::Party), Default::default())),
                );
                let mut battle = PreparedBattle::new(participants, source.resources.actions, 7)?
                    .with_formation_count(formations)?
                    .finish()?;
                let active = allowance * formations;
                for index in 1..=25 {
                    assert_eq!(
                        battle.choose_enemy_action(ActorId(index))?,
                        (usize::from(index) <= active).then_some(EnemyChoiceId(0)),
                        "difficulty={difficulty:?}, party={party_count}, formations={formations}, actor={index}"
                    );
                }
                battle.actors[25].position = [0., 0., 250.];
                assert_eq!(
                    battle.choose_enemy_action(ActorId(25))?,
                    Some(EnemyChoiceId(0))
                );
            }
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
            assert_eq!(
                battle.choose_enemy_action(ActorId(1))?,
                Some(EnemyChoiceId(2))
            );
        }
        assert_eq!(battle.runtime[1].enemy_choice, Some(EnemyChoiceId(2)));
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
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(2))
        );
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
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(1))
        );
        battle.actors[1].hp = 1;
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(0))
        );
        battle.prepared.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .disabled_choices
            .push(EnemyChoiceId(0));
        assert_eq!(
            battle.choose_enemy_action(ActorId(1))?,
            Some(EnemyChoiceId(1))
        );
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
        assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(0)));
        assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(0)));
        battle.record_enemy_action(owner, crate::ActionKey(0));
        assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(1)));
        battle.record_enemy_action(owner, crate::ActionKey(1));
        battle.actors[1].position[0] = 600.;
        assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(0)));
        battle.actors[1].position[0] = 601.;
        assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(1)));
        battle.actors[1].position[0] = 499.;
        assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(1)));
        battle.actors[1].position[0] = 500.;
        battle.actors[0].availability = crate::ActorAvailability::Dead;
        assert_eq!(battle.choose_enemy_action(owner)?, None);
        Ok(())
    }
}

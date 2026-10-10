//! Weighted enemy choices and native action dispatch.
use crate::{ActorId, Battle, PreparedBattle, TargetPolicy};
use anyhow::{Context, Result, ensure};

pub use resonance_content::battle_action::EnemyRequirements;

#[derive(Debug, Clone)]
pub struct EnemyChoice {
    pub action: crate::ActionKey,
    pub weight: u8,
    pub requirements: EnemyRequirements,
    pub target_policy: Option<TargetPolicy>,
    pub return_to_formation: bool,
    /// Selected-row contact guard chance, including approach and idle visits.
    pub guard_chance: i8,
    pub range: [i16; 2],
    pub tp: u16,
    pub approach_minimum: f32,
    pub approach_range: f32,
}

#[derive(Debug, Clone)]
pub struct EnemyDecisionDefinition {
    pub strategy: TargetPolicy,
    pub difficulty: u8,
    pub choices: Vec<EnemyChoice>,
    pub back_row: Vec<(u8, u8)>,
    pub walk_speed: f32,
    pub walk_motion: Option<crate::MotionBinding>,
    pub turn_ticks: u8,
}

impl PreparedBattle {
    pub(crate) fn validate_actor_enemy_decisions(&self) -> Result<()> {
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
                definition.walk_speed.is_finite()
                    && definition.walk_speed > 0.
                    && definition.turn_ticks != 0,
                "invalid enemy movement parameters"
            );
            for row in &definition.choices {
                ensure!(
                    *row.requirements.difficulty.start() <= 2
                        && *row.requirements.difficulty.end() <= 2
                        && row.requirements.hp_percent.is_none_or(|value| value <= 100),
                    "invalid enemy eligibility conditions"
                );
                ensure!(
                    row.approach_minimum.is_finite()
                        && row.approach_range.is_finite()
                        && row.approach_minimum >= 0.
                        && row.approach_range > row.approach_minimum,
                    "invalid enemy approach interval"
                );
                ensure!(
                    self.resources.actions.get(row.action).is_some(),
                    "enemy row action is not prepared"
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

    fn enemy_choice_eligible(&self, owner: ActorId, row: &EnemyChoice) -> bool {
        let actor = &self.actors[owner.index()];
        let difficulty = self.prepared.actor_setup[owner.index()]
            .enemy_decision
            .as_ref()
            .unwrap()
            .difficulty;
        let Some(target) = self.target(owner) else {
            return false;
        };
        let gap = crate::control::body_gap(actor, &self.actors[target.index()]);
        let maximum = if row.range[1] == 0 {
            f32::INFINITY
        } else {
            f32::from(row.range[1])
        };
        gap >= f32::from(row.range[0])
            && gap <= maximum
            && actor.tp >= row.tp
            && row.requirements.difficulty.contains(&difficulty)
            && row
                .requirements
                .hp_percent
                .is_none_or(|maximum| actor.hp_percent() <= i16::from(maximum))
    }

    pub(crate) fn choose_enemy_action(&mut self, owner: ActorId) -> Result<Option<usize>> {
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
                    self.enemy_choice_eligible(owner, &definition.choices[usize::from(index)])
                        .then_some((usize::from(index), u64::from(weight)))
                })
                .collect()
        } else {
            definition
                .choices
                .iter()
                .enumerate()
                .filter(|(_, row)| self.enemy_choice_eligible(owner, row))
                .map(|(index, row)| (index, u64::from(row.weight)))
                .collect()
        };
        let priority = candidates
            .iter()
            .find(|&&(index, _)| definition.choices[index].requirements.priority)
            .map(|&(index, _)| index);
        let total = candidates.iter().try_fold(0_u64, |sum, (_, weight)| {
            sum.checked_add(*weight)
                .context("enemy choice weight overflow")
        })?;
        let selected = if let Some(index) = priority {
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
        };
        self.runtime[owner.index()].enemy_choice = selected;
        if let Some(index) = selected {
            let definition = self.enemy_decision(owner)?;
            let row = &definition.choices[index];
            let strategy = row.target_policy.unwrap_or(definition.strategy);
            let guard_chance = row.guard_chance;
            let Some(target) = self.decision_target(owner, strategy)? else {
                self.runtime[owner.index()].enemy_choice = None;
                return Ok(None);
            };
            self.set_decision_target(owner, target)?;
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

    pub(crate) fn advance_enemy_ai(&mut self, owner: ActorId) -> Result<()> {
        if !self.decision_ready(owner) || !self.actors[owner.index()].movement.hover_ready() {
            return Ok(());
        }
        let Some(selected) = self.choose_enemy_action(owner)? else {
            self.begin_taunt(owner)?;
            return Ok(());
        };
        let definition = self.enemy_decision(owner)?;
        let row = &definition.choices[selected];
        let parameters = crate::ApproachParameters {
            minimum: row.approach_minimum,
            // Choose an interval inside the action's reach. Movement and admission
            // must use that same interval, so braking aims inside it too.
            maximum: row.approach_minimum.midpoint(row.approach_range),
            motion: definition.walk_motion,
            motion_rate: 0.5,
            speed: definition.walk_speed,
            turn_ticks: definition.turn_ticks,
        };
        let action = row.action;
        let target = self.target(owner).context("enemy has no target")?;
        self.request_approach(owner, target, action, parameters)?;
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
            requirements: EnemyRequirements::default(),
            target_policy: None,
            return_to_formation: true,
            guard_chance: 20,
            range: [0, 0],
            tp: 0,
            approach_minimum: 0.,
            approach_range: 80.,
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
                    strategy: TargetPolicy::Nearest,
                    difficulty: 0,
                    choices: choices.clone(),
                    back_row: vec![],
                    walk_speed: 2.,
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
}

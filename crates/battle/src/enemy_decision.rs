//! Prepared ordinary enemy rows; the maintained decision program owns policy.
use crate::{ActorId, Battle, PreparedBattle};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone)]
pub struct EnemyChoice {
    pub action: u16,
    pub weight: i8,
    pub requirements: u32,
    pub target_policy: u8,
    /// Selected-row contact guard chance, including approach and idle visits.
    pub guard_chance: i8,
    /// Source normal callback age and signed follow-up chance.
    pub combo_at: u16,
    pub followup_chance: i8,
    pub range: [i16; 2],
    pub tp: u16,
    pub approach_minimum: f32,
    pub approach_range: f32,
}

#[derive(Debug, Clone)]
pub struct EnemyDecisionDefinition {
    pub actor: ActorId,
    pub strategy: u8,
    pub difficulty: u8,
    pub choices: Vec<EnemyChoice>,
    pub back_row: Vec<(u8, u8)>,
    pub walk_speed: f32,
    pub turn_ticks: u8,
    pub body_flags: u16,
}

impl PreparedBattle {
    pub fn with_enemy_decisions(
        mut self,
        definitions: Vec<EnemyDecisionDefinition>,
    ) -> Result<Self> {
        for definition in definitions {
            let index = definition.actor.index();
            ensure!(
                index < self.actors.len() && self.actors[index].control == crate::Control::Enemy,
                "enemy choice owner is not an enemy"
            );
            ensure!(
                self.enemy_decisions[index].is_none(),
                "duplicate enemy decision table"
            );
            ensure!(
                matches!(definition.strategy, 1 | 3 | 4 | 9) && definition.difficulty <= 2,
                "unsupported ordinary enemy strategy"
            );
            let party_count = self
                .actors
                .iter()
                .filter(|a| a.side == crate::Side::Party)
                .count();
            let cap = if definition.difficulty >= 2 {
                8
            } else if party_count <= 2 {
                party_count + 1
            } else {
                3
            };
            let enemy_count = self
                .actors
                .iter()
                .filter(|a| a.side == crate::Side::Enemy)
                .count();
            ensure!(
                enemy_count <= cap,
                "enemy back-row waiting controller is not prepared"
            );
            ensure!(
                !definition.choices.is_empty()
                    && definition.choices.len() <= 12
                    && definition.back_row.len() <= 4,
                "invalid ordinary enemy choice count"
            );
            ensure!(
                definition.walk_speed.is_finite()
                    && definition.walk_speed > 0.
                    && definition.turn_ticks != 0,
                "invalid enemy movement parameters"
            );
            for row in &definition.choices {
                ensure!(
                    row.weight >= 0 && row.requirements & !0x2018_002f == 0,
                    "unsupported ordinary enemy eligibility flags"
                );
                ensure!(
                    row.combo_at <= i16::MAX as u16 && row.followup_chance <= 0,
                    "enemy follow-up selection is not prepared"
                );
                ensure!(
                    matches!(row.target_policy, 0 | 1 | 3 | 4 | 9),
                    "unsupported enemy row targeting"
                );
                ensure!(
                    row.approach_minimum.is_finite()
                        && row.approach_range.is_finite()
                        && row.approach_minimum >= 0.
                        && row.approach_range > row.approach_minimum,
                    "invalid enemy approach interval"
                );
                ensure!(
                    self.actions.iter().any(|a| a.id == row.action
                        && matches!(
                            a.phase,
                            crate::ActionPhase::Actor | crate::ActionPhase::Casting
                        )),
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
            self.enemy_decisions[index] = Some(definition);
        }
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn enemy_decision(&self, actor: ActorId) -> Result<&EnemyDecisionDefinition> {
        self.prepared.enemy_decisions[actor.index()]
            .as_ref()
            .context("missing enemy choice table")
    }

    pub(crate) fn enemy_parameters(&self, owner: ActorId) -> Result<Vec<i32>> {
        let definition = self.enemy_decision(owner)?;
        let actor = &self.actors[owner.index()];
        // 31C88 writes actor+18ac after controls, so this is the previous
        // visit's body-volume separation, independently of this row's target.
        let distance = i32::from(self.target_gaps[owner.index()] as i16);
        let party_count = self
            .actors
            .iter()
            .filter(|a| a.side == crate::Side::Party)
            .count();
        let cap = if definition.difficulty >= 2 {
            8
        } else if party_count <= 2 {
            party_count + 1
        } else {
            3
        };
        let mut rank = self
            .actors
            .iter()
            .enumerate()
            .filter(|&(i, other)| {
                i != owner.index()
                    && other.side == actor.side
                    && other.available()
                    && self.prepared.enemy_decisions[i]
                        .as_ref()
                        .is_none_or(|d| d.body_flags & 0x24 != 0x24)
                    && other.position[0] < actor.position[0]
            })
            .count();
        let position = self.target_positions[owner.index()];
        if self
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.side != actor.side)
            .any(|(index, _)| {
                crate::distance::length([
                    position[0] - self.target_positions[index][0],
                    0.,
                    position[2] - self.target_positions[index][2],
                ]) <= 300.
            })
        {
            rank = 0;
        }
        Ok(vec![
            definition.choices.len() as i32,
            i32::from(definition.strategy),
            i32::from(definition.difficulty),
            (i64::from(actor.hp) * 100 / i64::from(actor.max_hp)) as i32,
            i32::from(actor.tp),
            distance,
            rank as i32,
            cap as i32,
            definition.back_row.len() as i32,
            definition.walk_speed.to_bits() as i32,
            i32::from(definition.turn_ticks),
        ])
    }

    pub(crate) fn enemy_choice(&self, actor: ActorId, index: i32) -> Result<Vec<i32>> {
        let row = usize::try_from(index)
            .ok()
            .and_then(|index| self.enemy_decision(actor).ok()?.choices.get(index))
            .context("invalid enemy choice index")?;
        Ok(vec![
            i32::from(row.action),
            i32::from(row.weight),
            row.requirements as i32,
            i32::from(row.target_policy),
            i32::from(row.range[0]),
            if row.range[1] == 0 {
                2500
            } else {
                i32::from(row.range[1])
            },
            i32::from(row.tp),
            row.approach_minimum.to_bits() as i32,
            row.approach_range.to_bits() as i32,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDefinition, ActionPhase, Control, Side};
    use std::{collections::BTreeMap, sync::Arc};

    fn source_action() -> ActionDefinition {
        let sources = BTreeMap::from([
            ("selection".into(), "script battle; use battle; use battle::enemy_ai; pub task select() { let choice = enemy_ai::choose(); battle::ai_set_idle_timer(choice); }".into()),
            ("battle::enemy_ai".into(), include_str!("../../../scripts/battle/enemy_ai.sym").into()),
        ]);
        let compilation = symphonia_script_compiler::compile(
            "selection",
            &sources,
            &crate::native_declarations(),
        )
        .unwrap();
        let entry = compilation
            .program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name.ends_with("::select") || f.name == "select")
            .unwrap()
            .entry;
        ActionDefinition {
            id: 100,
            phase: ActionPhase::Decision,
            program: Arc::new(compilation.program),
            entry,
            duration: 0,
            tp_cost: 0,
            resources: vec![],
        }
    }

    fn choice(index: u16, weight: i8, requirements: u32) -> EnemyChoice {
        EnemyChoice {
            action: index,
            weight,
            requirements,
            target_policy: 0,
            guard_chance: 0,
            combo_at: i16::MAX as u16,
            followup_chance: 0,
            range: [0, 0],
            tp: 0,
            approach_minimum: 0.,
            approach_range: 80.,
        }
    }

    fn prepared(choices: Vec<EnemyChoice>, strategy: u8, difficulty: u8) -> PreparedBattle {
        let mut actors = vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
            crate::tests::actor(Side::Enemy),
        ];
        actors[0].control = Control::Manual;
        actors[1].control = Control::Enemy;
        actors[2].control = Control::Enemy;
        actors[1].position[0] = 500.;
        actors[2].position[0] = 600.;
        for actor in &mut actors {
            actor.body.approach_points.push(crate::HurtPoint {
                center: actor.position,
                radius: 10.,
            });
        }
        let selector = source_action();
        let mut actions: Vec<_> = choices
            .iter()
            .map(|row| {
                let mut action = selector.clone();
                action.id = row.action;
                action.phase = ActionPhase::Actor;
                action
            })
            .collect();
        actions.push(selector);
        PreparedBattle::new(actors, actions, 0, vec![], vec![])
            .unwrap()
            .with_enemy_decisions(vec![EnemyDecisionDefinition {
                actor: ActorId(1),
                strategy,
                difficulty,
                choices,
                back_row: vec![],
                walk_speed: 2.,
                turn_ticks: 8,
                body_flags: 0,
            }])
            .unwrap()
    }

    fn select(prepared: PreparedBattle) -> Battle {
        let mut battle = Battle::new(Arc::new(prepared));
        let definition = battle
            .prepared
            .actions
            .iter()
            .position(|a| a.id == 100)
            .unwrap();
        let (id, mut sequence) = battle
            .allocate_sequence(definition, ActorId(1), ActorId(0))
            .unwrap();
        crate::script::step_sequence(&mut battle, id, &mut sequence, &mut Vec::new()).unwrap();
        assert!(sequence.tasks_complete());
        battle
    }

    #[test]
    fn proximity_rank_uses_planar_visit_start_distance_even_for_a_flying_enemy() {
        let mut battle = Battle::new(Arc::new(prepared(vec![choice(0, 1, 0)], 1, 0)));
        battle.actors[1].position = [1000., 1000., 0.];
        battle.actors[2].position = [100., 0., 0.];
        battle.target_positions[1] = [300., 1000., 0.];
        assert_eq!(
            battle.enemy_parameters(ActorId(1)).unwrap()[6],
            0,
            "34BD4 reads planar1084 and includes the exact300 boundary"
        );
        battle.target_positions[1][0] = 301.;
        assert_eq!(
            battle.enemy_parameters(ActorId(1)).unwrap()[6],
            1,
            "outside the cached planar radius the other enemy contributes rank"
        );
    }

    #[test]
    fn zombie_hard_priority_precedes_weights_and_normal_rejects_it() {
        let rows = vec![
            choice(0, 35, 0),
            choice(1, 45, 0),
            choice(2, 20, 0),
            choice(3, 10, 0x80004),
            choice(4, 0, 0),
        ];
        let hard = select(prepared(rows.clone(), 1, 1));
        assert_eq!(hard.idle_timers[1], 3);
        assert_eq!(
            hard.random_state(),
            0,
            "priority admission must not draw weighted fallback"
        );
        let normal = select(prepared(rows, 1, 0));
        assert_eq!(normal.idle_timers[1], 0);
        assert_eq!(
            normal.random_state(),
            0x0012_d687,
            "Normal consumes one weighted draw"
        );
    }

    #[test]
    fn opening_selected_row_can_guard_during_approach_before_attack_admission() -> Result<()> {
        // Source04 C21 ends at draw117; C22's weighted draw118 is50296%100=96.
        // Normal difficulty admits rows0/1/2 with weights35/45/20, selecting2.
        let mut rows = vec![
            choice(0, 35, 0),
            choice(1, 45, 0),
            choice(2, 20, 0),
            choice(3, 10, 0x80004),
            choice(4, 0, 0),
        ];
        for (row, chance) in rows.iter_mut().zip([0, 15, 33, 50, 0]) {
            row.guard_chance = chance;
        }
        let mut prepared = prepared(rows, 1, 0);
        prepared.random_seed = 1_139_493_870;
        let mut battle = select(prepared);
        assert_eq!(battle.enemy_selected[1], Some(2));
        assert_eq!(battle.actors[1].guard.enemy_chance, 33);
        assert_eq!(battle.random_state(), 3_296_243_421);
        assert!(battle.sequences.is_empty());
        assert!(battle.request_approach(
            ActorId(1),
            ActorId(0),
            2,
            crate::ApproachParameters {
                minimum: 0.,
                maximum: 80.,
                motion: None,
                stop_motion: None,
                motion_rate: 0.5,
                speed: 2.,
                turn_ticks: 8,
            }
        )?);
        assert_eq!(battle.actors[1].activity, crate::Activity::Approaching);
        assert_eq!(battle.actors[1].guard.auto_chance, 49);
        assert_eq!(battle.actors[1].guard.enemy_chance, 33);
        assert_eq!(battle.random_state(), 3_366_625_440); // Source C22 draw119.
        assert!(battle.sequences.is_empty());
        // Source04 C110's physical contact has already consumed variation and
        // two critical draws; guard draw343 gives4, clamped to0 after approach-40.
        battle.random = crate::state::Random(2_541_216_701);
        battle.actors[1].guard.break_pressure = 10;
        assert!(crate::guard::attempt(
            &mut battle.actors[1],
            crate::Affinity::Normal,
            &mut battle.random
        ));
        assert_eq!(battle.random_state(), 649_094_144);
        assert!(battle.actors[1].guard.active);
        Ok(())
    }

    #[test]
    fn selected_guard_chance_keeps_signed_byte_and_does_not_draw() {
        let mut row = choice(0, 0, 0);
        row.guard_chance = -128;
        let mut prepared = prepared(vec![row], 1, 0);
        prepared.actors[1].guard.enemy_chance = 33;
        let battle = select(prepared);
        assert_eq!(battle.enemy_selected[1], Some(0));
        assert_eq!(battle.actors[1].guard.enemy_chance, -128);
        assert_eq!(battle.random_state(), 0);
    }

    #[test]
    fn ghost_eligibility_and_commit_each_keep_spread_target_draws() {
        let battle = select(prepared(vec![choice(0, 60, 0), choice(1, 50, 0x20)], 4, 0));
        // Other enemy claims the only opponent: 16 rejected draws for each
        // eligible row, one weighted draw, then 16 for the committed target.
        assert_eq!(battle.random_state(), 0x8a49_e237);
        assert_eq!(battle.targets[1], ActorId(0));
        assert_eq!(battle.idle_timers[1], 1);
    }

    #[test]
    fn zero_weight_fallback_keeps_row_zero_without_weight_draw() {
        let battle = select(prepared(vec![choice(0, 0, 0), choice(1, 0, 0)], 1, 0));
        assert_eq!(battle.idle_timers[1], 0);
        assert_eq!(battle.random_state(), 0);
    }
}

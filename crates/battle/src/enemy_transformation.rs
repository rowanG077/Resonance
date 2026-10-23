//! Health stages retain their choices and capability changes after healing.
use crate::{ActorId, Battle, EnemyBehavior, EnemyChoiceId, ModelRequest};
use anyhow::Result;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Grounded,
    Revealed,
    Airborne,
}

impl Battle {
    pub(crate) fn enemy_behavior_allows_choice(&self, owner: ActorId, row: EnemyChoiceId) -> bool {
        let Some(decision) = &self.prepared.actor_setup[owner.index()].enemy_decision else {
            return true;
        };
        let runtime = &self.runtime[owner.index()].enemy_behavior;
        let (initial_actions, final_actions, shifted) = match &decision.behavior {
            Some(EnemyBehavior::Transformation {
                initial_actions,
                final_actions,
                ..
            }) => (
                initial_actions,
                final_actions,
                runtime.transformation == State::Airborne,
            ),
            Some(EnemyBehavior::HealthStage {
                initial_actions,
                final_actions,
                ..
            }) => (initial_actions, final_actions, runtime.health_stage_shifted),
            Some(EnemyBehavior::ReplaceWeapon {
                disabled_action, ..
            }) => {
                return !runtime.weapon_replaced || row != *disabled_action;
            }
            _ => return true,
        };
        let actions = if shifted {
            final_actions
        } else {
            initial_actions
        };
        actions.contains(&row)
    }

    /// Called with ordinary actor updates, after scene and hit-stop gates.
    pub(crate) fn advance_enemy_transformation(&mut self, index: usize) -> Result<()> {
        if matches!(
            self.prepared.actor_setup[index]
                .enemy_decision
                .as_ref()
                .and_then(|decision| decision.behavior.as_ref()),
            Some(EnemyBehavior::Transformation { .. })
        ) {
            self.advance_enemy_behavior(index, false)?;
        }
        let Some(decision) = &self.prepared.actor_setup[index].enemy_decision else {
            return Ok(());
        };
        if let Some(EnemyBehavior::HealthStage {
            hp_percent,
            initial_station,
            ..
        }) = &decision.behavior
        {
            if !self.runtime[index].enemy_behavior.health_stage_shifted {
                self.actors[index].position = initial_station.position;
                self.actors[index].heading = initial_station.heading;
            }
            if !self.runtime[index].enemy_behavior.health_stage_shifted
                && self.actors[index].hp_percent() <= i16::from(*hp_percent)
            {
                self.runtime[index].enemy_behavior.health_stage_shifted = true;
                self.actors[index].movement.turning_disabled = false;
                self.actors[index].movement.steering.push_immovable = false;
                self.unison.release_immunity(ActorId(index as u8));
                // Future choices change; a committed approach or action keeps its recipe.
            }
            return Ok(());
        }
        let Some(EnemyBehavior::Transformation {
            reveal_hp_percent,
            revealed_bones,
            airborne_hp_percent,
            airborne_walk_speed,
            airborne_motions,
            hover_height,
            ..
        }) = &decision.behavior
        else {
            return Ok(());
        };
        let health = self.actors[index].hp_percent();
        match self.runtime[index].enemy_behavior.transformation {
            State::Grounded if health <= i16::from(*reveal_hp_percent) => {
                self.model_requests.push(ModelRequest::BoneScales {
                    actor: ActorId(index as u8),
                    scales: revealed_bones.iter().map(|&bone| (bone, [1.; 3])).collect(),
                });
                self.runtime[index].enemy_behavior.transformation = State::Revealed;
            }
            State::Revealed if health <= i16::from(*airborne_hp_percent) => {
                self.runtime[index].enemy_behavior.transformation = State::Airborne;
                self.runtime[index].enemy_walk_override = Some(*airborne_walk_speed);
                self.model_requests.push(ModelRequest::MotionAliases {
                    actor: ActorId(index as u8),
                    aliases: airborne_motions.iter().copied().collect(),
                });
                let movement = &mut self.actors[index].movement;
                movement.flying = true;
                movement.hover_height = *hover_height;
                movement.reset_hover();
                self.refresh_enemy_choice(ActorId(index as u8));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionKey, ActionRequest, Activity, Side};

    #[test]
    fn anchored_health_stage_starts_at_its_station_before_entry() -> Result<()> {
        let mut enemy = crate::tests::actor(Side::Enemy);
        enemy.position = [350., 0., 0.];
        enemy.movement.turning_disabled = true;
        let mut prepared =
            crate::tests::prepared(vec![crate::tests::actor(Side::Party), enemy], 30);
        crate::tests::assign_action(&mut prepared, 1, ActionKey(0));
        let final_action = prepared.resources.actions.insert(crate::tests::action(30));
        crate::tests::assign_action(&mut prepared, 1, final_action);
        let station = resonance_content::battle_action::EnemyStation {
            position: [700., 0., 100.],
            heading: -90.,
        };
        prepared.resources.actor_setup[1]
            .enemy_decision
            .as_mut()
            .unwrap()
            .behavior = Some(EnemyBehavior::HealthStage {
            hp_percent: 25,
            initial_station: station,
            initial_actions: EnemyChoiceId(0)..EnemyChoiceId(1),
            final_actions: EnemyChoiceId(1)..EnemyChoiceId(2),
        });
        let prepared = crate::PreparedBattle::new(
            prepared
                .actors
                .into_iter()
                .zip(prepared.resources.actor_setup)
                .collect(),
            prepared.resources.actions,
            1,
        )?
        .with_entry_choices(
            (0..2)
                .map(|index| crate::EntryChoice {
                    actor: ActorId(index),
                    strategy: crate::TargetPolicy::Nearest,
                })
                .collect(),
        )?
        .with_entry();
        assert_eq!(prepared.actors[1].position, station.position);
        assert_eq!(prepared.actors[1].heading, station.heading);
        assert_eq!(prepared.actors[1].movement.steering.home, [350., 0., 0.]);
        let mut battle = prepared.finish()?;
        for _ in 0..crate::prepare::ENTRY_RESUME_UPDATE + 1 {
            let frame = battle.step(crate::BattleInput::default())?;
            assert_eq!(frame.actors[1].position, station.position);
            assert_eq!(frame.actors[1].heading, station.heading);
        }
        Ok(())
    }

    #[test]
    fn health_stage_latches_without_interrupting_the_current_action() -> Result<()> {
        let owner = ActorId(1);
        for maximum_hp in [36_000, 72_000] {
            let mut enemy = crate::tests::actor(Side::Enemy);
            enemy.equipment.max_hp = maximum_hp;
            enemy.hp = maximum_hp * 51 / 100;
            enemy.movement.turning_disabled = true;
            enemy.movement.steering.push_immovable = true;
            enemy.reaction.profile.can_knock_down = false;
            let mut prepared =
                crate::tests::prepared(vec![crate::tests::actor(Side::Party), enemy], 30);
            let final_action = prepared.resources.actions.insert(crate::tests::action(30));
            crate::tests::assign_action(&mut prepared, owner.index(), ActionKey(0));
            crate::tests::assign_action(&mut prepared, owner.index(), final_action);
            prepared.resources.actor_setup[owner.index()]
                .enemy_decision
                .as_mut()
                .unwrap()
                .behavior = Some(EnemyBehavior::HealthStage {
                hp_percent: 50,
                initial_station: resonance_content::battle_action::EnemyStation {
                    position: [850., 0., 0.],
                    heading: -90.,
                },
                initial_actions: EnemyChoiceId(0)..EnemyChoiceId(1),
                final_actions: EnemyChoiceId(1)..EnemyChoiceId(2),
            });
            let mut battle = prepared.finish()?;
            battle.advance_enemy_transformation(owner.index())?;
            assert!(battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(0)));
            assert!(!battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(1)));
            assert!(battle.actors[owner.index()].movement.turning_disabled);
            assert_eq!(battle.actors[owner.index()].position, [850., 0., 0.]);
            assert_eq!(battle.actors[owner.index()].heading, -90.);

            battle.start_admitted_action(
                ActionRequest {
                    actor: owner,
                    action: ActionKey(0),
                    target: ActorId(0),
                },
                &mut Vec::new(),
            )?;
            let action = *battle.runtime[owner.index()].task().action().unwrap().0;
            battle.actors[owner.index()].hp -= 1;
            battle.advance_enemy_transformation(owner.index())?;
            assert_eq!(battle.activity(owner), Activity::Action);
            assert_eq!(
                battle.runtime[owner.index()].task().action().unwrap().0,
                &action
            );
            assert!(!battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(0)));
            assert!(battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(1)));
            let actor = &battle.actors[owner.index()];
            assert!(!actor.movement.turning_disabled);
            assert!(!actor.movement.steering.push_immovable);
            assert!(!actor.reaction.profile.can_knock_down);

            battle.actors[owner.index()].hp = maximum_hp;
            battle.actors[owner.index()].position = [200., 0., 30.];
            battle.advance_enemy_transformation(owner.index())?;
            battle.advance_enemy_behavior(owner.index(), false)?;
            assert_eq!(battle.actors[owner.index()].position, [200., 0., 30.]);
            assert!(!battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(0)));
            assert!(battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(1)));
        }
        Ok(())
    }

    #[test]
    fn transformation_replaces_pending_choices_and_retains_running_actions() -> Result<()> {
        let owner = ActorId(1);
        for running in [false, true] {
            let mut enemy = crate::tests::actor(Side::Enemy);
            enemy.hp = enemy.equipment.max_hp;
            enemy.position[0] = 500.;
            let mut prepared =
                crate::tests::prepared(vec![crate::tests::actor(Side::Party), enemy], 30);
            let final_action = prepared.resources.actions.insert(crate::tests::action(30));
            crate::tests::assign_action(&mut prepared, owner.index(), ActionKey(0));
            crate::tests::assign_action(&mut prepared, owner.index(), final_action);
            let decision = prepared.resources.actor_setup[owner.index()]
                .enemy_decision
                .as_mut()
                .unwrap();
            for choice in &mut decision.choices {
                choice.requirements.hp_percent = None;
            }
            decision.behavior = Some(EnemyBehavior::Transformation {
                reveal_hp_percent: 75,
                revealed_bones: vec![0],
                airborne_hp_percent: 50,
                airborne_walk_speed: 6.5,
                airborne_motions: vec![(0, Some(1))],
                hover_height: 25.,
                initial_actions: EnemyChoiceId(0)..EnemyChoiceId(1),
                final_actions: EnemyChoiceId(1)..EnemyChoiceId(2),
            });
            let mut battle = prepared.finish()?;
            assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(0)));
            if running {
                battle.start_admitted_action(
                    ActionRequest {
                        actor: owner,
                        action: ActionKey(0),
                        target: ActorId(0),
                    },
                    &mut Vec::new(),
                )?;
            } else {
                battle.advance_enemy_ai(owner, &mut Vec::new())?;
                assert_eq!(battle.activity(owner), Activity::Approaching);
            }

            battle.model_requests.clear();
            battle.actors[owner.index()].hp = 50;
            battle.advance_enemy_transformation(owner.index())?;
            let scales: Vec<_> = battle
                .model_requests
                .iter()
                .filter_map(|request| match request {
                    ModelRequest::BoneScales { actor, scales } if *actor == owner => {
                        Some(scales.clone())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(scales, vec![vec![(0, [0.; 3])], vec![(0, [1.; 3])]]);
            assert!(!battle.actors[owner.index()].movement.flying);
            assert!(battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(0)));

            battle.advance_enemy_transformation(owner.index())?;
            assert!(battle.actors[owner.index()].movement.flying);
            assert_eq!(battle.actors[owner.index()].movement.hover_height, 25.);
            assert!(!battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(0)));
            assert!(battle.enemy_behavior_allows_choice(owner, EnemyChoiceId(1)));
            if running {
                assert_eq!(battle.activity(owner), Activity::Action);
            } else {
                assert_eq!(battle.activity(owner), Activity::Idle);
                assert_eq!(battle.runtime[owner.index()].enemy_choice, None);
            }

            battle.actors[owner.index()].hp = 100;
            battle.advance_enemy_transformation(owner.index())?;
            assert_eq!(battle.choose_enemy_action(owner)?, Some(EnemyChoiceId(1)));
            assert!(battle.actors[owner.index()].movement.flying);
            assert_eq!(battle.runtime[owner.index()].enemy_walk_override, Some(6.5));
        }
        Ok(())
    }
}

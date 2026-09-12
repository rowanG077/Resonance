use crate::{Actor, ActorId};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, sync::Arc};

pub use resonance_content::battle_voice::Sound;

#[derive(Debug, Clone)]
pub enum ActionExecution {
    Attack(crate::PreparedAttack),
    Casting(Arc<crate::CastingDefinition>),
}

/// Index of an immutable definition within one prepared battle.
/// Catalogue IDs and live action handles have separate lifetimes and types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionKey(pub usize);

impl std::fmt::Display for ActionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Default)]
pub struct ActionDefinitions {
    pub(crate) entries: Vec<Arc<ActionDefinition>>,
}

impl ActionDefinitions {
    pub fn insert(&mut self, definition: ActionDefinition) -> ActionKey {
        let key = ActionKey(self.entries.len());
        self.entries.push(Arc::new(definition));
        key
    }

    pub fn get(&self, key: ActionKey) -> Option<&Arc<ActionDefinition>> {
        self.entries.get(key.0)
    }
}

impl From<Vec<ActionDefinition>> for ActionDefinitions {
    fn from(definitions: Vec<ActionDefinition>) -> Self {
        Self {
            entries: definitions.into_iter().map(Arc::new).collect(),
        }
    }
}

impl std::ops::Index<ActionKey> for ActionDefinitions {
    type Output = Arc<ActionDefinition>;
    fn index(&self, key: ActionKey) -> &Self::Output {
        &self.entries[key.0]
    }
}

#[derive(Debug, Clone)]
pub struct ActionDefinition {
    pub normal: Option<crate::NormalAttack>,
    pub execution: ActionExecution,
    pub tp_cost: u16,
}

#[derive(Debug, Clone, Default)]
pub struct ActorSetup {
    pub techniques: Vec<crate::PreparedTechnique>,
    /// Maximum normal-chain count before another arte needs Super Blast.
    pub arte_chain_limit: Option<u8>,
    pub control: Option<Arc<crate::ControlDefinition>>,
    /// Initial autonomous-use policy, moved into the live controller at activation.
    pub disabled_techniques: BTreeSet<ActionKey>,
    pub decision: Option<crate::DecisionDefinition>,
    pub overlimit_gain: u8,
    pub extended_overlimit: bool,
    pub companion: Option<crate::CompanionDefinition>,
    pub enemy_decision: Option<crate::EnemyDecisionDefinition>,
    pub contact_recovery: bool,
    pub assist_shortcuts: crate::AssistShortcuts,
    pub special_guard: Option<ActionKey>,
    pub spell_charge: Option<crate::SpellChargeDefinition>,
    pub aerial_spells: bool,
    pub recovery_return: Option<crate::RecoveryReturnDefinition>,
}

impl ActorSetup {
    pub(crate) fn turn_ticks(&self) -> Option<u8> {
        self.control
            .as_ref()
            .map(|row| row.turn_ticks)
            .or_else(|| self.enemy_decision.as_ref().map(|row| row.turn_ticks))
    }

    pub(crate) fn action_ids(&self) -> impl Iterator<Item = ActionKey> + '_ {
        self.control
            .iter()
            .flat_map(|control| control.normals.iter().map(|row| row.action))
            .chain(self.techniques.iter().map(|row| row.action))
            .chain(
                self.enemy_decision
                    .iter()
                    .flat_map(|decision| decision.choices.iter().map(|row| row.action)),
            )
    }
}

/// Owned inputs for one battle, validated and activated by `finish`.
/// This type is deliberately not serializable: executables never enter cooked data.
#[derive(Debug)]
pub struct PreparedBattle {
    pub(crate) technique_learning_members: Vec<crate::learning::TechniqueLearningMember>,
    pub(crate) targets: Vec<ActorId>,
    pub(crate) camera: Option<crate::camera::Camera>,
    pub(crate) entry_remaining: u8,
    pub(crate) actors: Vec<Actor>,
    pub(crate) random_seed: u64,
    pub(crate) resources: BattleResources,
}

#[derive(Debug)]
pub(crate) struct BattleResources {
    pub(crate) actor_setup: Vec<ActorSetup>,
    pub(crate) unison_gauge: i16,
    pub(crate) unison_available: bool,
    pub(crate) escape: Option<crate::EscapeDefinition>,
    pub(crate) overlimit_boosted_gain: bool,
    pub(crate) items: Option<crate::item::Definition>,
    pub(crate) grade_rank: u8,
    pub(crate) arena_boundary: bool,
    pub(crate) actions: ActionDefinitions,
}

impl PreparedBattle {
    pub fn finish(mut self) -> Result<crate::Battle> {
        self.validate_actions()?;
        self.validate_actor_setup()?;
        self.validate_escape()?;
        self.validate_actor_techniques()?;
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            if setup.control.is_some() {
                self.actors[index].movement.braking = 0.55;
                self.actors[index].movement.gravity = if self.actors[index].movement.flying {
                    0.
                } else {
                    -1.
                };
            }
        }
        let mut battle = crate::Battle::from_prepared(self);
        battle.initialize_entry_state()?;
        Ok(battle)
    }

    fn validate_actions(&self) -> Result<()> {
        for action in &self.resources.actions.entries {
            match &action.execution {
                ActionExecution::Attack(attack) => attack.validate()?,
                ActionExecution::Casting(cast) => cast.release.validate()?,
            }
        }
        Ok(())
    }

    fn validate_actor_setup(&self) -> Result<()> {
        self.validate_actor_controls()?;
        self.validate_actor_enemy_decisions()?;
        self.validate_actor_companions()?;
        self.validate_actor_special_guards()?;
        self.validate_actor_recovery_returns()?;
        for setup in &self.resources.actor_setup {
            if setup.assist_shortcuts.iter().any(Option::is_some) {
                ensure!(
                    setup.control.is_some(),
                    "assist commands require party controls"
                );
            }
            for (owner, shortcut) in setup.assist_shortcuts.into_iter().flatten() {
                let technique = self
                    .resources
                    .technique(owner, shortcut)
                    .context("initial assist technique is not prepared")?;
                ensure!(
                    self.technique_learning_members
                        .iter()
                        .find(|member| member.actor == owner)
                        .is_none_or(|member| member
                            .member
                            .current()
                            .contains(&technique.catalogue)),
                    "initial assist technique is not learned"
                );
            }
            if setup.aerial_spells {
                ensure!(
                    setup
                        .spell_charge
                        .as_ref()
                        .is_some_and(|stored| stored.enabled),
                    "Aerial Spell needs Spell Charge"
                );
                ensure!(
                    setup
                        .techniques
                        .iter()
                        .any(|command| command.capabilities.spell
                            && command.capabilities.family == Some(crate::ArteFamily::Basic)
                            && self.resources.casting(command.action).is_some()),
                    "Aerial Spell has no prepared basic spell"
                );
            }
        }
        Ok(())
    }

    pub fn with_overlimit_boost(mut self, boosted_gain: bool) -> Self {
        self.resources.overlimit_boosted_gain = boosted_gain;
        self
    }

    pub(crate) fn validate_actor_special_guards(&self) -> Result<()> {
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            let Some(definition) = &setup.special_guard else {
                continue;
            };
            ensure!(
                self.resources
                    .technique(ActorId(index as u8), *definition)
                    .is_some(),
                "Special Guard has no prepared technique metadata"
            );
            let actor = &self.actors[index];

            ensure!(
                actor.side == crate::Side::Party,
                "Special Guard requires a party actor"
            );
            ensure!(
                self.resources
                    .actions
                    .get(*definition)
                    .is_some_and(|action| {
                        matches!(&action.execution, crate::ActionExecution::Attack(_))
                    }),
                "Special Guard action is not a prepared actor action"
            );
        }
        Ok(())
    }

    pub fn with_camera(mut self, definition: crate::CameraDefinition) -> Result<Self> {
        let target = *self
            .targets
            .get(definition.leader.index())
            .context("invalid camera leader")?;
        self.camera = Some(crate::camera::Camera::new(
            definition,
            &self.actors,
            target,
        )?);
        Ok(self)
    }

    /// A half-second introduction at 60 updates/second, independent of camera or artwork.
    pub fn with_entry(mut self) -> Self {
        self.entry_remaining = 30;
        self
    }

    pub fn new(
        participants: Vec<(Actor, ActorSetup)>,
        actions: ActionDefinitions,
        random_seed: u64,
    ) -> Result<Self> {
        let (mut actors, actor_setup): (Vec<_>, Vec<_>) = participants.into_iter().unzip();
        ensure!(
            !actors.is_empty()
                && actors
                    .iter()
                    .filter(|a| a.side == crate::Side::Party)
                    .count()
                    <= crate::PARTY_CAPACITY
                && actors
                    .iter()
                    .filter(|a| a.side == crate::Side::Enemy)
                    .count()
                    <= crate::ENEMY_CAPACITY,
            "battle roster exceeds participant limits or is empty"
        );
        for actor in &mut actors {
            actor.validate()?;
            ensure!(
                actor.reaction.recoil.kind == crate::RecoilKind::Normal,
                "initial actor recoil must be neutral"
            );
            if actor.hp == 0 && actor.availability != crate::ActorAvailability::Absent {
                actor.availability = crate::ActorAvailability::Dead;
            }
        }
        crate::steering::return_position::initialize(&mut actors);
        Ok(Self {
            targets: actors
                .iter()
                .enumerate()
                .map(|(index, actor)| {
                    ActorId(
                        actors
                            .iter()
                            .position(|a| a.side != actor.side)
                            .unwrap_or(index) as u8,
                    )
                })
                .collect(),
            technique_learning_members: Vec::new(),
            camera: None,
            entry_remaining: 0,
            actors,
            random_seed,
            resources: BattleResources {
                actor_setup,
                items: None,
                grade_rank: 0,
                overlimit_boosted_gain: false,
                escape: None,
                unison_gauge: 0,
                unison_available: false,
                arena_boundary: false,
                actions,
            },
        })
    }

    /// Handles remain valid for the life of this encounter; actor slots are never reused.
    pub fn actor_ids(&self) -> impl Iterator<Item = ActorId> + '_ {
        (0..self.actors.len()).map(|index| ActorId(index as u8))
    }
}

impl BattleResources {
    pub fn special_guard(&self, actor: ActorId) -> Option<crate::ActionKey> {
        self.actor_setup
            .get(actor.index())
            .and_then(|setup| setup.special_guard.as_ref())
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conditions::Condition;
    use crate::{Battle, BattleInput, ButtonInput, Control, ControlDefinition, ControlInput, Side};
    use std::sync::Arc;

    #[test]
    fn activation_validates_the_final_actor_and_action_inventory() {
        let candidate = || crate::tests::prepared(vec![crate::tests::actor(Side::Party)], 1);
        candidate().finish().unwrap();

        let mut edited = candidate();
        crate::tests::attack_mut(Arc::make_mut(&mut edited.resources.actions.entries[0])).events =
            vec![(
                0,
                crate::AttackEvent::Move {
                    forward: Some(f32::NAN),
                    vertical: None,
                },
            )];
        assert!(edited.finish().is_err());
    }

    fn action() -> ActionDefinition {
        ActionDefinition {
            normal: None,
            tp_cost: 2,
            execution: ActionExecution::Attack(crate::PreparedAttack {
                opening: None,
                events: vec![(0, crate::AttackEvent::SpecialGuard)],
                chain_at: None,
                end_at: 2,
                recovery: 0,
            }),
        }
    }

    fn special_battle() -> Battle {
        let mut owner = crate::tests::actor(Side::Party);
        owner.control = Control::Manual;
        owner.tp = 40;
        owner.equipment.max_tp = 40;
        owner.movement.turning_disabled = true;
        owner.movement.target_direction = [1., 0., 0.];
        let mut target = crate::tests::actor(Side::Enemy);
        target.position[0] = 100.;
        let mut actions = ActionDefinitions::default();
        let normals =
            crate::tests::normal_controls(&mut actions, crate::tests::attack(2), [0., 120.]);
        actions.insert(action());
        let prepared = PreparedBattle::new(
            (vec![owner, target])
                .into_iter()
                .zip(vec![
                    ActorSetup {
                        techniques: vec![crate::PreparedTechnique {
                            player_range: [0.; 2],
                            ai_range: [0.; 2],
                            capabilities: crate::TechniqueCapabilities {
                                family: Some(crate::ArteFamily::Arcane),
                                target: crate::TechniqueTarget::SelfTarget,
                                ..Default::default()
                            },
                            ..crate::tests::technique(crate::ActionKey(7), 34)
                        }],
                        special_guard: Some(crate::ActionKey(7)),
                        control: Some(Arc::new(ControlDefinition {
                            normals,
                            shortcuts: [0; 4],
                            walk_speed: 5.,
                            run_speed: 10.,
                            turn_ticks: 8,
                            motions: None,
                        })),
                        ..Default::default()
                    },
                    ActorSetup::default(),
                ])
                .collect(),
            actions,
            0,
        )
        .unwrap()
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[34],
            &[],
        )])
        .unwrap();
        prepared.finish().unwrap()
    }

    #[test]
    fn special_guard_native_admission_pays_once_and_rejects_before_stance() -> Result<()> {
        for tp in [3, 4] {
            let mut battle = special_battle();
            battle.actors[0].tp = tp;
            let frame = battle.step(BattleInput {
                actions: vec![crate::ActionRequest {
                    actor: ActorId(0),
                    action: crate::ActionKey(7),
                    target: ActorId(1),
                }],
                ..Default::default()
            })?;
            if tp == 3 {
                assert!(frame.cues.contains(&crate::Cue::Rejected {
                    actor: ActorId(0),
                    reason: crate::Rejection::InsufficientTp
                }));
                assert!(frame.actions.is_empty());
                assert!(!battle.actors[0].guard.active);
                assert_eq!(battle.actors[0].tp, 3);
                assert_eq!(battle.technique_uses(ActorId(0), 34), Some(0));
            } else {
                assert_eq!(battle.actors[0].tp, 0);
                assert_eq!(battle.actors[0].guard.kind, crate::GuardKind::Special);
                assert!(battle.actors[0].guard.active);
                assert_eq!(battle.technique_uses(ActorId(0), 34), Some(1));
                battle.step(BattleInput {
                    paused: true,
                    ..Default::default()
                })?;
                battle.step(BattleInput {
                    interrupt: vec![frame.actions[0].0],
                    ..Default::default()
                })?;
                assert!(!battle.actors[0].guard.active);
                assert_eq!(battle.actors[0].tp, 0);
                assert_eq!(battle.technique_uses(ActorId(0), 34), Some(1));
            }
        }
        Ok(())
    }

    #[test]
    fn special_guard_binding_accepts_a_prepared_catalogue_outside_the_initial_roster() {
        let mut owner = crate::tests::actor(Side::Party);
        owner.equipment.max_tp = 100;
        owner.tp = 100;
        let mut target = crate::tests::actor(Side::Enemy);
        target.position[0] = 100.;
        let prepared = PreparedBattle::new(
            (vec![owner, target])
                .into_iter()
                .zip(vec![
                    ActorSetup {
                        techniques: vec![crate::PreparedTechnique {
                            player_range: [0.; 2],
                            ai_range: [0.; 2],
                            capabilities: crate::TechniqueCapabilities {
                                target: crate::TechniqueTarget::SelfTarget,
                                ..Default::default()
                            },
                            ..crate::tests::technique(crate::ActionKey(0), 90)
                        }],
                        special_guard: Some(crate::ActionKey(0)),
                        ..Default::default()
                    },
                    ActorSetup::default(),
                ])
                .collect(),
            (vec![action()]).into(),
            0,
        )
        .unwrap()
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[90],
            &[],
        )])
        .unwrap();
        let battle = prepared.finish().unwrap();
        assert_eq!(
            battle.prepared.special_guard(ActorId(0)).unwrap(),
            crate::ActionKey(0)
        );
        assert_eq!(battle.prepared.special_guard(ActorId(1)), None);
    }

    #[test]
    fn automatic_special_guard_threat_is_targeted_and_policy_gated() {
        let mut battle = special_battle();
        battle.actors[0].control = Control::Auto;
        battle.publish_special_guard_threat(
            ActorId(1),
            ActorId(0),
            crate::CastingThreat {
                // This is the enemy spell's catalogue, not the target's
                // character-specific guard catalogue.
                catalogue: 66,
                offensive: true,
                element: 1,
            },
        );
        assert_eq!(battle.runtime[0].special_guard_pending, Some(34));
        assert_eq!(battle.runtime[1].special_guard_pending, Some(34));

        battle.runtime[0].special_guard_pending = None;
        battle.runtime[1].special_guard_pending = None;
        battle.actors[0].equipment.affinities[1] = crate::Affinity::Resistant;
        battle.publish_special_guard_threat(
            ActorId(1),
            ActorId(0),
            crate::CastingThreat {
                catalogue: 66,
                offensive: true,
                element: 1,
            },
        );
        assert_eq!(battle.runtime[0].special_guard_pending, None);

        let mut battle = special_battle();
        battle.actors[0].control = Control::Auto;
        assert!(
            battle
                .set_technique_enabled(ActorId(0), crate::ActionKey(7), false)
                .unwrap()
        );
        battle.publish_special_guard_threat(
            ActorId(1),
            ActorId(0),
            crate::CastingThreat {
                catalogue: 66,
                offensive: true,
                element: 1,
            },
        );
        assert_eq!(battle.runtime[0].special_guard_pending, None);

        let mut battle = special_battle();
        battle.actors[0].control = Control::Auto;
        battle
            .start(
                crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: crate::ActionKey(0),
                },
                &mut vec![],
            )
            .unwrap();
        battle.publish_special_guard_threat(
            ActorId(1),
            ActorId(0),
            crate::CastingThreat {
                catalogue: 66,
                offensive: true,
                element: 1,
            },
        );
        assert_eq!(battle.runtime[0].special_guard_pending, None);

        battle.interrupt_actor(ActorId(0), &mut vec![]);
        battle.publish_special_guard_threat(
            ActorId(1),
            ActorId(0),
            crate::CastingThreat {
                catalogue: 66,
                offensive: true,
                element: 1,
            },
        );
        assert_eq!(battle.runtime[0].special_guard_pending, Some(34));
    }

    #[test]
    fn automatic_special_guard_consumer_requires_learned_membership_and_unused_family() -> Result<()>
    {
        let mut battle = special_battle();
        battle.actors[0].control = Control::Auto;
        battle.learning_members[0].member.forget(34)?;
        battle.runtime[0].special_guard_pending = Some(34);
        assert!(!battle.consume_auto_special_guard(ActorId(0), &mut vec![])?);
        assert_eq!(battle.runtime[0].special_guard_pending, None);

        let mut battle = special_battle();
        assert!(battle.start_actor_command(
            crate::ActionRequest {
                actor: ActorId(0),
                target: ActorId(0),
                action: crate::ActionKey(7)
            },
            &mut vec![]
        )?);
        assert!(!battle.special_guard_family_available(ActorId(0)));
        battle.actors[0].control = Control::Auto;
        battle.runtime[0].special_guard_pending = Some(34);
        assert!(!battle.consume_auto_special_guard(ActorId(0), &mut vec![])?);
        battle.interrupt_actor(ActorId(0), &mut vec![]);
        assert!(battle.special_guard_family_available(ActorId(0)));
        Ok(())
    }

    #[test]
    fn paralysis_interrupts_automatic_special_guard_without_spending_tp_or_counting_use()
    -> Result<()> {
        let mut battle = special_battle();
        battle.enter_player_guard(0);
        let actor = &mut battle.actors[0];
        actor.control = Control::Auto;
        actor.guard.recovery = 15;
        actor.reaction.direction = [-1., 0., 0.];
        actor.movement.direction = [1., 0., 0.];
        actor.facing_direction = [1., 0., 0.];
        actor.equipment.luck = 0;
        actor.conditions.apply_hit(crate::HitCondition {
            condition: resonance_content::battle_action::Condition::Paralysis,
            chance: 100,
            value: 0,
        });
        battle.actors[0].body.collider = Some(crate::Collider::sphere(1.));
        battle.actors[1].body.collider = Some(crate::Collider::sphere(1.));
        battle.runtime[0].special_guard_pending = Some(34);
        assert!(
            battle.actors[0]
                .conditions
                .effective()
                .contains(Condition::Paralysis)
        );
        let tp = battle.actors[0].tp;
        const ACTION_START_BUDGET: usize = 60;
        for _ in 0..ACTION_START_BUDGET {
            let frame = battle.step(BattleInput::default())?;
            assert!(!frame.cues.iter().any(|cue| matches!(
                cue,
                crate::Cue::Started {
                    actor: ActorId(0),
                    ..
                }
            )));
            if battle.activity(ActorId(0)) == crate::Activity::Hurt {
                break;
            }
        }
        let actor = &battle.actors[0];
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Hurt);
        assert!(actor.position.iter().all(|value| value.is_finite()));
        assert_eq!(actor.tp, tp);
        assert_eq!(battle.technique_uses(ActorId(0), 34), Some(0));
        assert_eq!(battle.runtime[0].special_guard_pending, None);
        Ok(())
    }

    #[test]
    fn automatic_special_guard_consumes_latch_before_strategy() -> Result<()> {
        let mut battle = special_battle();
        // A self action has no dependency on a retained opponent or approach range.
        battle.runtime[0].target = ActorId(0);
        battle.actors[1].position[0] = 10_000.;
        battle.actors[0].control = Control::Auto;
        battle.runtime[0].special_guard_pending = Some(34);
        assert!(battle.consume_auto_special_guard(ActorId(0), &mut vec![])?);
        assert_eq!(battle.runtime[0].special_guard_pending, None);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);

        battle.runtime[0].special_guard_pending = Some(34);
        battle.actors[0].conditions =
            crate::conditions::Conditions::new(crate::conditions::Layers {
                base: Condition::Curse.into(),
                ..Default::default()
            });
        assert!(!battle.consume_auto_special_guard(ActorId(0), &mut vec![])?);
        assert_eq!(battle.runtime[0].special_guard_pending, None);

        let mut battle = special_battle();
        battle.actors[0].control = Control::Auto;
        battle.actors[0].conditions =
            crate::conditions::Conditions::new(crate::conditions::Layers {
                equipment_overlay: Condition::ReduceItemEffect.into(),
                ..Default::default()
            });
        battle.runtime[0].special_guard_pending = Some(34);
        assert!(battle.consume_auto_special_guard(ActorId(0), &mut vec![])?);

        let mut battle = special_battle();
        battle.actors[0].control = Control::Auto;
        assert!(
            battle
                .set_technique_enabled(ActorId(0), crate::ActionKey(7), false)
                .unwrap()
        );
        battle.runtime[0].special_guard_pending = Some(34);
        assert!(battle.consume_auto_special_guard(ActorId(0), &mut vec![])?);
        Ok(())
    }

    #[test]
    fn special_guard_input_enters_action_and_cleans_up_on_finish() -> Result<()> {
        let mut battle = special_battle();
        let binding = battle.prepared.special_guard(ActorId(0)).unwrap();
        let raw_cost = battle.prepared.actions.get(binding).unwrap().tp_cost;
        let expected_tp = battle.actors[0].tp
            - crate::tp::special_guard_debit(&battle.actors[0], raw_cost) as u16;
        let guard = || BattleInput {
            controllers: vec![ControlInput {
                stick: [0, 0],
                guard: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        };
        let input = || BattleInput {
            controllers: vec![ControlInput {
                stick: [0, -49],
                vertical_pressed: -1,
                guard: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        };
        battle.step(guard())?;
        let mut released = input();
        released.controllers[0].guard = ButtonInput::default();
        let frame = battle.step(released)?;
        assert_eq!(frame.actors[0].activity, crate::Activity::Idle);
        assert_eq!(battle.technique_uses(ActorId(0), 34), Some(0));
        battle.step(guard())?;
        battle.step(input())?;
        const ACTION_COMPLETION_BUDGET: usize = 60;
        for _ in 0..ACTION_COMPLETION_BUDGET {
            if battle.actors[0].guard.kind == crate::GuardKind::Special {
                break;
            }
            battle.step(BattleInput::default())?;
        }
        assert!(battle.actors[0].guard.active);
        assert_eq!(battle.actors[0].guard.kind, crate::GuardKind::Special);
        assert_eq!(battle.actors[0].tp, expected_tp);
        assert_eq!(battle.technique_uses(ActorId(0), 34), Some(1));
        for _ in 0..ACTION_COMPLETION_BUDGET {
            battle.step(BattleInput::default())?;
            if !battle.actors[0].guard.active {
                break;
            }
        }
        assert!(!battle.actors[0].guard.active);
        assert_eq!(battle.actors[0].guard.kind, crate::GuardKind::Normal);
        assert_eq!(battle.actors[0].tp, expected_tp);
        assert_eq!(battle.technique_uses(ActorId(0), 34), Some(1));
        Ok(())
    }

    #[test]
    fn special_guard_manual_selection_ignores_automatic_policy_disable() -> Result<()> {
        let mut battle = special_battle();
        assert!(
            battle
                .set_technique_enabled(ActorId(0), crate::ActionKey(7), false)
                .unwrap()
        );
        let guard = || BattleInput {
            controllers: vec![ControlInput {
                guard: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        };
        let input = || BattleInput {
            controllers: vec![ControlInput {
                stick: [0, -49],
                vertical_pressed: -1,
                guard: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        };
        battle.step(guard())?;
        battle.step(input())?;
        assert_eq!(battle.actors[0].guard.kind, crate::GuardKind::Special);
        assert!(battle.actors[0].guard.active);
        assert_eq!(battle.technique_uses(ActorId(0), 34), Some(1));
        Ok(())
    }

    #[test]
    fn rejected_special_guard_keeps_its_family_available_without_queuing_an_action() -> Result<()> {
        let mut battle = special_battle();
        let guard = || BattleInput {
            controllers: vec![ControlInput {
                guard: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        };
        battle.step(guard())?;
        let tp = battle.actors[0].tp;
        battle.actors[0].hit_stop = 2;
        battle.step(BattleInput {
            controllers: vec![ControlInput {
                stick: [0, -49],
                vertical_pressed: -1,
                guard: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        })?;
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Guarding);
        assert!(battle.special_guard_family_available(ActorId(0)));
        battle.step(BattleInput::default())?;
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Guarding);
        assert!(battle.special_guard_family_available(ActorId(0)));
        assert_eq!(battle.actors[0].tp, tp);
        Ok(())
    }
}

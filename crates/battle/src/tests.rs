use super::*;
use crate::conditions::{Condition, ConditionSet};
use std::sync::Arc;

pub(crate) fn technique(action: crate::ActionKey, catalogue: u16) -> PreparedTechnique {
    PreparedTechnique {
        action,
        catalogue,
        player_range: [0., 120.],
        ai_range: [0., 120.],
        capabilities: TechniqueCapabilities::default(),
        element: 0,
    }
}

/// Give a focused fixture a real technique or enemy choice. Enemy choices stay
/// ineligible for automatic selection so the test controls when they start.
pub(crate) fn assign_action(prepared: &mut PreparedBattle, owner: usize, action: crate::ActionKey) {
    let setup = &mut prepared.resources.actor_setup[owner];
    if setup.action_ids().any(|assigned| assigned == action) {
        return;
    }
    if prepared.actors[owner].side == Side::Party {
        setup.techniques.push(technique(
            action,
            u16::try_from(setup.techniques.len() + 1).unwrap(),
        ));
    } else {
        prepared.actors[owner].control = Control::Enemy;
        setup.decision.get_or_insert(DecisionDefinition {
            idle_ticks: u32::MAX,
            idle_variation: 0,
        });
        let definition = setup
            .enemy_decision
            .get_or_insert_with(|| EnemyDecisionDefinition {
                strategy: TargetPolicy::Nearest,
                difficulty: 0,
                choices: vec![],
                back_row: vec![],
                walk_speed: 1.,
                walk_motion: None,
                turn_ticks: 1,
            });
        definition.choices.push(EnemyChoice {
            action,
            weight: 1,
            requirements: EnemyRequirements {
                hp_percent: Some(0),
                ..Default::default()
            },
            target_policy: None,
            return_to_formation: false,
            guard_chance: 0,
            range: [0, 0],
            tp: 0,
            approach_minimum: 0.,
            approach_range: 120.,
        });
    }
}

pub(crate) fn revive(
    battle: &mut Battle,
    actor: ActorId,
    percent: i16,
    cues: &mut Vec<Cue>,
) -> anyhow::Result<()> {
    let dead = battle.actor(actor)?.availability == ActorAvailability::Dead;
    battle.recover_vitals(actor, percent, false, cues)?;
    if dead {
        battle.enter_revival(actor, cues);
    }
    Ok(())
}

#[track_caller]
pub(crate) fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} ± {tolerance}, got {actual}"
    );
}

#[track_caller]
pub(crate) fn assert_vector_close<const N: usize>(
    actual: [f32; N],
    expected: [f32; N],
    tolerance: f32,
) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_close(actual, expected, tolerance);
    }
}

#[test]
fn equipment_refresh_preserves_live_vitals_and_condition_base() {
    let mut current = actor(Side::Party);
    current.hp = 37;
    current.tp = 9;
    current.species = 6;
    current.equipment.damage.weapon_species = Some(2);
    current.equipment.damage.critical_chance_bonus = 1;
    current.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: ConditionSet::of(&[Condition::Weak]),
        ..Default::default()
    });
    let prepared =
        PreparedBattle::new(vec![(current, Default::default())], Default::default(), 1).unwrap();
    let id = prepared.actor_ids().next().unwrap();
    let mut battle = prepared.finish().unwrap();
    let mut replacement = actor(Side::Party);
    replacement.equipment.max_hp = 200;
    replacement.equipment.max_tp = 80;
    //A freshly derived gear row has no actor-template species. Only that
    //profile field survives; weapon/EX damage traits must still be rebuilt.
    assert_eq!(replacement.species, 0);
    replacement.equipment.damage.weapon_species = Some(9);
    replacement.equipment.damage.critical_chance_bonus = 7;
    replacement.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: battle.actors()[id.index()].conditions.base(),
        intrinsic: ConditionSet::of(&[Condition::Acuity]),
        equipment_overlay: ConditionSet::of(&[Condition::ReduceItemEffect]),
        immunity: ConditionSet::of(&[Condition::Guard]),
    });
    crate::tests::equip(&mut battle, id, replacement).unwrap();
    let live = &battle.actors()[id.index()];
    assert_eq!(
        (
            live.hp,
            live.tp,
            live.equipment.max_hp,
            live.equipment.max_tp
        ),
        (37, 9, 200, 80)
    );
    assert_eq!(live.species, 6);
    assert_eq!(live.equipment.damage.weapon_species, Some(9));
    assert_eq!(live.equipment.damage.critical_chance_bonus, 7);
    assert_eq!(live.conditions.base(), ConditionSet::of(&[Condition::Weak]));
    assert_eq!(
        live.conditions.effective(),
        ConditionSet::of(&[
            Condition::Weak,
            Condition::Acuity,
            Condition::ReduceItemEffect
        ])
    );
    assert_eq!(
        live.conditions.immunity(),
        ConditionSet::of(&[Condition::Guard])
    );
}

#[test]
fn equipment_batch_preflight_keeps_earlier_actor_unpublished_on_late_failure() {
    let first = actor(Side::Party);
    let second = actor(Side::Party);
    let prepared = PreparedBattle::new(
        vec![(first, Default::default()), (second, Default::default())],
        Default::default(),
        1,
    )
    .unwrap();
    let mut battle = prepared.finish().unwrap();
    let mut changed = actor(Side::Party);
    changed.equipment.max_hp = 200;
    let mut invalid = actor(Side::Party);
    invalid.equipment.max_hp = 0;
    let error = battle
        .replace_equipment_batch(vec![
            EquipmentReplacement {
                actor: ActorId(0),
                attributes: changed.equipment.clone(),
                conditions: changed.conditions.clone(),
                equipment: None,
            },
            EquipmentReplacement {
                actor: ActorId(1),
                attributes: invalid.equipment.clone(),
                conditions: invalid.conditions.clone(),
                equipment: None,
            },
        ])
        .unwrap_err();
    assert!(error.to_string().contains("invalid battle HP"));
    assert_eq!(battle.actors()[0].equipment.max_hp, 100);
    assert_eq!(battle.actors()[1].side, Side::Party);
}

#[path = "diagnostic_tests.rs"]
mod diagnostic_tests;

pub(super) fn actor(side: Side) -> Actor {
    Actor {
        side,
        species: 0,
        equipment: crate::EquipmentAttributes {
            max_hp: 100,
            max_tp: 40,
            tp_cost_reduction: false,
            quick_escape: false,
            taunt_enabled: false,
            taunt_guard: false,
            taunt_cancel: false,
            control_ex: Default::default(),
            quick_turn: false,
            backstep_guard: false,
            casting: Default::default(),
            dagger_reach: false,
            contact: Default::default(),
            normal_combo_limit: 3,
            luck: 0,
            stats: crate::CombatStats::default(),
            affinities: [crate::Affinity::Normal; 9],
            damage: Default::default(),
            recovery: RecoveryTraits::default(),
            base_element: None,
            combo_traits: Default::default(),
            normal_guard: false,
            speed_multiplier: 1.,
            reaction_ex: Default::default(),
            stun_ex_bonus: false,
            spell_revenge: false,
        },
        control: Default::default(),
        availability: Default::default(),
        overlimit: crate::OverLimit::new(0).unwrap(),
        guard: Default::default(),
        hp: 50,
        tp: 40,
        control_ex_state: Default::default(),
        casting_state: Default::default(),
        stored_spell: None,
        control_slot: 0,
        elements: Default::default(),
        attack_power: 100,
        proficiency: 0,
        input: Default::default(),
        conditions: Default::default(),
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        movement: Default::default(),
        hit_stop: 0,
        time_stop: 0,
        reaction: Default::default(),
        body: Body::default(),
    }
}
pub(crate) fn particle_definition(resource: u32, member: u16) -> ParticleDefinition {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/stun-particle-source.json")).unwrap();
    let declaration: resonance_content::battle_effect::declaration::Declaration =
        serde_json::from_value(fixture["declaration"].clone()).unwrap();
    ParticleDefinition {
        resource,
        member,
        model: None,
        data: declaration.particle().unwrap(),
    }
}

/// A single-root model for fixtures whose behavior does not depend on animation tracks.
pub(crate) fn model_definition(
    resource: u32,
    motions: impl IntoIterator<Item = (u16, f32)>,
) -> ModelDefinition {
    use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
    let motions: Vec<_> = motions.into_iter().collect();
    ModelDefinition {
        tint: [64, 64, 64, 255],
        fade_on_defeat: false,
        resource,
        initial: Playback {
            clip: motions[0].0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(8),
                bind: Transform::default(),
            }],
        },
        motions: motions
            .into_iter()
            .map(|(clip, duration_frames)| {
                (
                    clip,
                    Motion {
                        duration_frames,
                        tracks: vec![],
                    },
                )
            })
            .collect(),
        secondary_motion: vec![],
        reactions: Default::default(),
        idle_motions: [None; 2],
        idle_expression: [0; 4],
        blink: None,
        weapons: vec![],

        shadow: None,
        suppress_root_translation: [false; 3],
    }
}

pub(crate) fn attack(end_at: u16) -> PreparedAttack {
    PreparedAttack {
        opening: None,
        events: vec![],
        chain_at: None,
        end_at,
        recovery: 0,
    }
}

pub(crate) fn action(end_at: u16) -> ActionDefinition {
    ActionDefinition {
        normal: None,
        tp_cost: 0,
        execution: ActionExecution::Attack(attack(end_at)),
    }
}

pub(crate) fn cast(duration: u32, cost: u16) -> ActionDefinition {
    ActionDefinition {
        normal: None,
        tp_cost: cost,
        execution: ActionExecution::Casting(Arc::new(CastingDefinition {
            duration,

            recovery: 0,
            release: Arc::new(crate::tests::volley()),
            threat: None,
        })),
    }
}

pub(crate) fn normal_controls(
    actions: &mut ActionDefinitions,
    attack: PreparedAttack,
    range: [f32; 2],
) -> [NormalControl; 7] {
    NormalAttack::ALL.map(|kind| NormalControl {
        action: actions.insert(ActionDefinition {
            normal: Some(kind),
            tp_cost: 0,
            execution: ActionExecution::Attack(attack.clone()),
        }),
        minimum_reach: range[0],
        reach: range[1],
    })
}

pub(crate) fn attack_mut(action: &mut ActionDefinition) -> &mut PreparedAttack {
    let ActionExecution::Attack(attack) = &mut action.execution else {
        panic!("fixture requires a native attack")
    };
    attack
}

pub(super) fn prepared(actors: Vec<Actor>, end_at: u16) -> PreparedBattle {
    let mut prepared = PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        (vec![action(end_at)]).into(),
        1,
    )
    .unwrap();
    crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
    prepared
}

fn request(actor: u8) -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(actor),
            action: crate::ActionKey(0),
            target: ActorId(actor),
        }],
        ..Default::default()
    }
}
fn started(frame: &BattleFrame) -> ActionId {
    frame
        .cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap()
}

#[test]
fn direct_requests_require_owned_roots_before_any_batch_payment_or_execution() {
    let mut prepared = prepared(vec![actor(Side::Party), actor(Side::Enemy)], 1).with_entry();
    Arc::make_mut(&mut prepared.resources.actions.entries[0]).tp_cost = 28;
    let mut battle = prepared.finish().unwrap();
    let mut input = request(0);
    input.actions.extend(request(1).actions);
    let error = battle.step(input).unwrap_err();
    assert!(error.to_string().contains("not assigned to actor 1"));
    assert_eq!(
        battle
            .actors
            .iter()
            .map(|actor| (actor.hp, actor.tp))
            .collect::<Vec<_>>(),
        [(50, 40); 2]
    );
    assert_eq!(battle.random_state(), 1);
    assert!(!battle.is_diagnostic());
    assert!(
        battle
            .step(BattleInput::default())
            .unwrap()
            .actions
            .is_empty()
    );
    for _ in 0..60 {
        if battle.phase() == crate::BattlePhase::Combat {
            break;
        }
        let frame = battle.step(request(0)).unwrap();
        assert!(frame.actions.is_empty());
        assert_eq!(frame.actors[0].tp, 40);
    }
    assert_eq!(battle.phase(), crate::BattlePhase::Combat);

    let frame = battle.step(request(0)).unwrap();
    assert_eq!(frame.actors[0].tp, 12);
    assert_eq!(frame.actors[0].hp, 50);
    assert_eq!(frame.actors[1].hp, 50);
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| matches!(
                cue,
                Cue::Started {
                    actor: ActorId(0),
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn insufficient_tp_and_busy_admission_do_not_repeat_costs() {
    let mut actors = vec![actor(Side::Party); 2];
    actors[1].tp = 27;
    let mut p = prepared(actors, 5);
    Arc::make_mut(&mut p.resources.actions.entries[0]).tp_cost = 28;
    crate::tests::assign_action(&mut p, 1, crate::ActionKey(0));
    let mut battle = p.finish().unwrap();
    let mut input = request(0);
    input.actions.extend(request(0).actions);
    input.actions.extend(request(1).actions);
    let frame = battle.step(input).unwrap();
    assert_eq!(
        &frame.cues[1..],
        &[
            Cue::Rejected {
                actor: ActorId(0),
                reason: Rejection::Busy
            },
            Cue::Rejected {
                actor: ActorId(1),
                reason: Rejection::InsufficientTp
            },
        ]
    );
    assert_eq!((battle.actors()[0].tp, battle.actors()[1].tp), (12, 27));
}

#[test]
fn native_sounds_follow_action_pauses_and_interruption_without_consuming_rng() {
    let mut prepared = prepared(vec![actor(Side::Party)], 10);
    let sound = Sound::Cue(60);
    attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])).events = [0, 2, 4]
        .map(|at| (at, AttackEvent::Sound(Some(sound))))
        .into();
    let mut battle = prepared.finish().unwrap();
    let random = battle.random.state();
    let first = battle.step(request(0)).unwrap();
    let id = started(&first);
    assert!(first.cues.contains(&Cue::Sound {
        actor: ActorId(0),
        sound,
        position: [0.; 3],
        priority: 1,
    }));
    let paused = battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert!(paused.cues.is_empty());
    battle.actors[0].hit_stop = 2;
    for _ in 0..2 {
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    }
    assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    battle.actors[0].position = [9., 0., 4.];
    let second = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        second.cues,
        [Cue::Sound {
            actor: ActorId(0),
            sound,
            position: [9., 0., 4.],
            priority: 1,
        }]
    );
    assert_eq!(battle.random.state(), random);
    assert_eq!(
        battle
            .step(BattleInput {
                interrupt: vec![id],
                ..Default::default()
            })
            .unwrap()
            .cues,
        [Cue::Interrupted { action: id }]
    );
    for _ in 0..5 {
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    }
}

#[test]
fn boosted_recovery_reports_full_amount_without_signed_narrowing() {
    let mut recipient = actor(Side::Party);
    recipient.equipment.max_hp = 100_000;
    recipient.hp = 50_000;
    recipient.equipment.recovery.boost = true;
    let mut battle =
        PreparedBattle::new(vec![(recipient, Default::default())], Default::default(), 1)
            .unwrap()
            .finish()
            .unwrap();
    let mut cues = vec![];
    battle.recover(ActorId(0), 40, &mut cues).unwrap();
    assert!(matches!(
        cues.as_slice(),
        [Cue::Recovered {
            nominal: 48_000,
            applied: 48_000,
            ..
        }]
    ));
    assert_eq!(battle.actors()[0].hp, 98_000);
}

#[test]
fn recognition_keeps_stepping_until_the_owner_finishes_once() {
    let mut enemy = actor(Side::Enemy);
    enemy.hp = 0;
    let mut battle = prepared(vec![actor(Side::Party), enemy], 8)
        .finish()
        .unwrap();
    let frame = battle.step(request(0)).unwrap();
    assert_eq!(frame.recognized_result, Some(BattleResult::Victory));
    assert!(frame.outcome.is_none());
    assert!(battle.step(BattleInput::default()).is_ok());
    let frame = battle.finish_result().unwrap();
    assert_eq!(frame.outcome.unwrap().result, BattleResult::Victory);
    assert!(frame.actions.is_empty());
    assert!(battle.finish_result().is_err());
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn invalid_external_input_is_atomic_and_does_not_fault_the_running_battle() {
    let mut battle = prepared(vec![actor(Side::Party)], 3).finish().unwrap();
    let mut input = request(0);
    input.actions.push(ActionRequest {
        actor: ActorId(0),
        target: ActorId(0),
        action: crate::ActionKey(1234),
    });
    assert!(battle.step(input).is_err());
    assert_eq!(battle.actors()[0].tp, 40);
    assert_eq!(battle.step(BattleInput::default()).unwrap().update, 1);
}

#[test]
fn petrified_actors_cannot_begin_an_action_or_pay_its_cost() {
    let mut caster = actor(Side::Party);
    caster.availability = crate::ActorAvailability::Petrified;
    let mut battle = prepared(
        // A living ally keeps this an admission test rather than all-party defeat.
        vec![caster, actor(Side::Party), actor(Side::Enemy)],
        1,
    )
    .finish()
    .unwrap();
    assert_eq!(
        battle.step(request(0)).unwrap().cues,
        [Cue::Rejected {
            actor: ActorId(0),
            reason: Rejection::Petrified
        }]
    );
    assert_eq!((battle.actors()[0].hp, battle.actors()[0].tp), (50, 40));
}

fn projectile_definition() -> ProjectileDefinition {
    ProjectileDefinition {
        motion: Default::default(),
        effects: Default::default(),
        lifetime: Some(20),
        velocity: [0.; 3],
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: true,
        active: None,
        birth: Some(EffectAppearance {
            resource: 37,
            member: 28,
        }),
        contact: None,
    }
}

pub(crate) fn volley() -> crate::PreparedVolley {
    let mut projectile = projectile_definition();
    projectile.birth = None;
    crate::PreparedVolley {
        projectile: Arc::new(projectile),
        shots: 3,
        interval: 8,
        startup: None,
        sound: None,
    }
}

pub(super) fn projectile_battle(
    owner: usize,
    mut prepared: PreparedBattle,
    projectile: ProjectileDefinition,
    at: u16,
) -> Battle {
    prepared.resources.actor_setup[0].techniques.clear();
    prepared.resources.actor_setup[0].enemy_decision = None;
    crate::tests::assign_action(&mut prepared, owner, crate::ActionKey(0));
    attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])).events =
        vec![(at, AttackEvent::Projectile(Arc::new(projectile)))];
    PreparedBattle::new(
        (prepared.actors)
            .into_iter()
            .zip(prepared.resources.actor_setup)
            .collect(),
        prepared.resources.actions,
        prepared.random_seed,
    )
    .unwrap()
    .finish()
    .unwrap()
}

#[test]
fn prepared_attack_pays_once_and_owns_only_unreleased_projectiles() {
    let prepare = |wave| {
        let mut owner = actor(Side::Party);
        owner.equipment.normal_guard = true;
        PreparedBattle::new(
            vec![(
                owner,
                ActorSetup {
                    techniques: vec![crate::tests::technique(crate::ActionKey(0), 100)],
                    ..Default::default()
                },
            )],
            (vec![ActionDefinition {
                normal: None,
                tp_cost: 4,
                execution: ActionExecution::Attack(PreparedAttack {
                    opening: None,
                    chain_at: Some(5),
                    end_at: 6,
                    recovery: 2,
                    events: vec![
                        (0, AttackEvent::Notice { duration: 10 }),
                        (3, AttackEvent::Projectile(Arc::new(wave))),
                    ],
                }),
            }])
            .into(),
            1,
        )
        .unwrap()
    };
    let mut wave = projectile_definition();
    wave.birth = None;
    wave.velocity[0] = f32::NAN;
    assert!(prepare(wave.clone()).finish().is_err());
    wave.velocity[0] = 0.;

    // Cancel before release, interrupt a released wave, then finish normally.
    for interrupt_at in [Some(1), Some(5), None] {
        let mut battle = prepare(wave.clone()).finish().unwrap();
        let first = battle.step(request(0)).unwrap();
        let id = started(&first);
        assert_eq!(first.actors[0].tp, 36);
        assert_ne!(
            first.actors[0].reaction.protection.mode,
            ProtectionMode::Armor
        );
        assert!(first.cues.iter().any(|cue| matches!(
            cue,
            Cue::Notice {
                action: crate::ActionKey(0),
                ..
            }
        )));
        let age = battle.action_age(id);
        assert!(
            battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap()
                .projectiles
                .is_empty()
        );
        assert_eq!(battle.action_age(id), age);
        let mut emissions = 0;
        let mut recovered = false;
        let mut completed = false;
        for tick in 1..12 {
            let frame = battle
                .step(BattleInput {
                    interrupt: if interrupt_at == Some(tick) {
                        vec![id]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                })
                .unwrap();
            emissions += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::ProjectileStarted { action, .. } if *action == id))
                .count();
            completed |= frame.cues.contains(&Cue::Completed { action: id });
            recovered |= frame.actors[0].activity == Activity::Recovering;
            if interrupt_at == Some(5) && tick >= 5 {
                assert_eq!(
                    frame.projectiles.len(),
                    1,
                    "released wave survives its owner action"
                );
            }
            assert_eq!(frame.actors[0].tp, 36);
        }
        assert_eq!(emissions, usize::from(interrupt_at != Some(1)));
        assert_eq!(completed, interrupt_at.is_none());
        assert_eq!(recovered, interrupt_at.is_none());
        assert_eq!(battle.action_age(id), None);
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    }
}

fn clash_battle(
    party: ProjectileDefinition,
    enemy: ProjectileDefinition,
    enemy_position: [f32; 3],
) -> Battle {
    let mut actors = vec![actor(Side::Enemy), actor(Side::Party)];
    actors[0].position = enemy_position;
    let mut battle = PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )
    .unwrap()
    .finish()
    .unwrap();
    for (projectile, owner, target) in [
        (party, ActorId(1), ActorId(0)),
        (enemy, ActorId(0), ActorId(1)),
    ] {
        battle
            .emit(
                Arc::new(projectile),
                ActionId(1),
                owner,
                target,
                battle.actors[owner.index()].position,
            )
            .unwrap();
    }
    battle.step(BattleInput::default()).unwrap();
    battle
}

pub(crate) fn contact_projectile(clashes: bool, survives_contact: bool) -> ProjectileDefinition {
    let mut definition = projectile_definition();
    definition.birth = None;
    definition.contact = Some(ProjectileContact {
        hit: crate::HitRule {
            overlimit_pause: false,
            condition: None,
            arte: false,
            reaction: Default::default(),
            kind: crate::DamageKind::Slash,
            power: crate::Power::Normal,
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            guard: crate::GuardRule::default(),
        },
        cooldown: 120,
        repeat_limit: 0,
        radius: 5.,
        height: 5.,
        shape: HitShape::Sphere,
        offset: [0.; 3],
        radius_growth: 0.,
        height_growth: 0.,
        survives_contact,
        clashes,
    });
    definition
}

fn clashes(frame: &BattleFrame) -> Vec<(ProjectileId, ProjectileId, [f32; 3])> {
    frame
        .cues
        .iter()
        .filter_map(|c| match c {
            Cue::ProjectileClashed {
                projectile,
                other: ContactSource::Projectile(other),
                position,
            } => Some((*projectile, *other, *position)),
            _ => None,
        })
        .collect()
}

#[test]
fn opposing_projectiles_disarm_together_before_actor_hits() {
    for (party_clashes, enemy_clashes, artwork) in [
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let mut party = contact_projectile(party_clashes, true);
        let mut enemy = contact_projectile(enemy_clashes, false);
        party.lifetime = Some(4);
        enemy.lifetime = Some(4);
        if artwork {
            party.effects.clash = Some(EffectAppearance {
                resource: 19,
                member: 11,
            });
            enemy.effects.clash = party.effects.clash;
        }
        let mut battle = clash_battle(party, enemy, [0.; 3]);
        for actor in &mut battle.actors {
            actor.body.collider = Some(crate::Collider::sphere(1.));
        }
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(clashes(&frame).len(), 1);

        assert_eq!(battle.random_state(), 1);
        assert!(hits(&frame).is_empty());
        assert!(
            frame
                .projectiles
                .iter()
                .all(|p| p.disarmed && !p.contact_active)
        );
        assert!(
            battle
                .projectiles
                .values()
                .all(|p| !p.can_hit(ActorId(0)) && !p.can_hit(ActorId(1)))
        );
        let paused = battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(paused.projectiles, frame.projectiles);
        for _ in 0..6 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert!(clashes(&frame).is_empty() && hits(&frame).is_empty());
        }
        assert!(battle.projectiles.is_empty());
        assert!(battle.actors.iter().all(|actor| actor.hp == 50));
    }
}

#[test]
fn clashes_require_overlapping_volumes_and_active_contact_windows() {
    for (height, enabled) in [(8., true), (12., true), (8., false)] {
        let mut party = contact_projectile(enabled, true);
        party.effects.clash = Some(EffectAppearance {
            resource: 19,
            member: 11,
        });
        party.contact.as_mut().unwrap().offset = [0., height, 0.];
        let mut enemy = contact_projectile(false, true);
        enemy.active = Some([2, 3]);
        let mut battle = clash_battle(party, enemy, [0.; 3]);
        assert!(clashes(&battle.step(BattleInput::default()).unwrap()).is_empty());
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(clashes(&frame).len(), usize::from(enabled && height < 10.));
    }
}

#[test]
fn all_projectile_contacts_remain_eligible_after_many_newer_submissions() {
    let mut battle = PreparedBattle::new(
        vec![
            (actor(Side::Party), Default::default()),
            (actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        1,
    )
    .unwrap()
    .finish()
    .unwrap();
    let party = Arc::new(contact_projectile(false, true));
    let enemy = Arc::new(contact_projectile(true, true));
    // The last submitted contact overlaps after forty newer contacts miss.
    battle
        .emit(party.clone(), ActionId(1), ActorId(0), ActorId(1), [0.; 3])
        .unwrap();
    for _ in 0..40 {
        battle
            .emit(
                party.clone(),
                ActionId(1),
                ActorId(0),
                ActorId(1),
                [100., 0., 0.],
            )
            .unwrap();
    }
    battle
        .emit(enemy.clone(), ActionId(2), ActorId(1), ActorId(0), [0.; 3])
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        clashes(&frame),
        [(ProjectileId(42), ProjectileId(1), [0.; 3])]
    );
}

#[test]
fn invalid_contact_parameters_fail_preparation_and_overflow_faults_the_battle() {
    let valid = contact_projectile(true, true);
    for value in [f32::NAN, f32::INFINITY, -1.] {
        let mut definition = valid.clone();
        definition.contact.as_mut().unwrap().radius = value;
        assert!(definition.validate().is_err());
    }
    let mut party = valid;
    party.contact.as_mut().unwrap().radius_growth = f32::MAX;
    let mut battle = clash_battle(party, contact_projectile(false, true), [0.; 3]);
    assert_eq!(
        battle.step(BattleInput::default()).unwrap_err().to_string(),
        "projectile contact growth overflow"
    );
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn projectile_hits_first_eligible_actor_and_can_hit_another_target_before_retiring() {
    let mut actors = vec![
        actor(Side::Party),
        actor(Side::Enemy),
        actor(Side::Party),
        actor(Side::Enemy),
    ];
    actors[0].body.scale = 2.;
    for actor in &mut actors[1..] {
        actor.body = Body {
            scale: 0.5,
            collider: Some(Collider::sphere(2.)),
            ..Default::default()
        };
    }
    let mut projectile = contact_projectile(false, false);
    projectile.contact.as_mut().unwrap().shape = HitShape::Box;
    projectile.contact.as_mut().unwrap().height = 1.;
    projectile.active = Some([1, 2]);
    let mut battle = projectile_battle(0, prepared(actors, 0), projectile, 0);
    let first = battle.step(request(0)).unwrap();
    assert!(hits(&first).is_empty());
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(1))]);
    assert_eq!(frame.actors[1].hp, 49);
    assert_eq!(frame.actors[3].hp, 50);
    let paused = battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert!(hits(&paused).is_empty());
    assert_eq!(paused.update, frame.update);
    battle.actors[1].body.collider = None;
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(3))]);
    assert_eq!(frame.actors[3].hp, 49);
    assert!(hits(&battle.step(BattleInput::default()).unwrap()).is_empty());
}

#[test]
fn contact_height_grows_before_the_actor_geometry_phase() {
    let mut party = contact_projectile(false, true);
    let contact = party.contact.as_mut().unwrap();
    contact.shape = HitShape::Cylinder;
    contact.height = 0.;
    contact.height_growth = 1.;
    let mut battle = clash_battle(party, contact_projectile(false, true), [20., 0., 0.]);
    battle.actors[0].position = [0., 2., 0.];
    battle.actors[0].body.collider = Some(crate::Collider::sphere(0.));
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(0))]);
}

#[test]
fn invalid_hurt_poses_and_shapes_fail_before_activation() {
    for scale in [0., -1., f32::NAN, f32::INFINITY] {
        let mut actor = actor(Side::Party);
        actor.body.scale = scale;
        assert!(
            PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1).is_err()
        );
    }
    for collider in [Collider::sphere(-1.), Collider::standing(1., f32::NAN)] {
        let mut actor = actor(Side::Party);
        actor.body.collider = Some(collider);
        assert!(
            PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1).is_err()
        );
    }
    let mut definition = contact_projectile(false, true);
    definition.contact.as_mut().unwrap().shape = HitShape::Ring { width: f32::NAN };
    assert!(definition.validate().is_err());
    definition.contact.as_mut().unwrap().shape = HitShape::Box;
    definition.contact.as_mut().unwrap().height = f32::INFINITY;
    assert!(definition.validate().is_err());
}

fn hits(frame: &BattleFrame) -> Vec<(ProjectileId, ActorId)> {
    frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Hit {
                source: ContactSource::Projectile(projectile),
                actor,
                ..
            } => Some((*projectile, *actor)),
            _ => None,
        })
        .collect()
}

fn vulnerable(side: Side) -> Actor {
    let mut actor = actor(side);
    actor.body.collider = Some(crate::Collider::sphere(1.));
    actor
}

fn firing_battle(actors: Vec<Actor>, projectile: ProjectileDefinition) -> Battle {
    let prepared = prepared(actors, 30);

    let mut battle = prepared.finish().unwrap();
    battle.start(request(0).actions[0], &mut vec![]).unwrap();
    battle
        .emit(
            Arc::new(projectile),
            ActionId(1),
            ActorId(0),
            ActorId(1),
            battle.actors[0].position,
        )
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    battle
}

#[test]
fn projectile_cooldowns_and_repeat_limits_bound_hits_and_hold_during_pause() {
    for timed in [false, true] {
        let mut projectile = contact_projectile(false, true);
        let contact = projectile.contact.as_mut().unwrap();
        contact.cooldown = 3;
        contact.repeat_limit = 2;
        let mut battle = firing_battle(
            vec![actor(Side::Party), vulnerable(Side::Enemy)],
            projectile,
        );
        assert_eq!(hits(&battle.step(BattleInput::default()).unwrap()).len(), 1);
        let random = battle.random_state();
        if timed {
            battle.request_timed_hold(4, None);
        }
        for _ in 0..4 {
            assert!(
                hits(
                    &battle
                        .step(BattleInput {
                            paused: !timed,
                            ..Default::default()
                        })
                        .unwrap()
                )
                .is_empty()
            );
        }
        assert_eq!(battle.random_state(), random);
        for age in 2..=10 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert_eq!(hits(&frame).len(), usize::from(age == 4), "age {age}");
        }
        assert_eq!(battle.actors[1].hp, 48);
    }
}

#[test]
fn dead_and_petrified_targets_are_skipped() {
    let mut dead = vulnerable(Side::Enemy);
    dead.hp = 0;
    let mut stone = vulnerable(Side::Enemy);
    stone.availability = crate::ActorAvailability::Petrified;
    let mut battle = firing_battle(
        vec![actor(Side::Party), dead, stone, vulnerable(Side::Enemy)],
        contact_projectile(false, true),
    );
    assert_eq!(
        hits(&battle.step(BattleInput::default()).unwrap()),
        [(ProjectileId(1), ActorId(3))]
    );

    assert!(hits(&battle.step(BattleInput::default()).unwrap()).is_empty());
}

#[test]
fn death_cancels_pending_attack_events_before_their_next_emission_and_delivers_one_outcome() {
    let mut projectile = contact_projectile(false, true);
    projectile.contact.as_mut().unwrap().hit.power = Power::Fixed(100);
    let mut battle = projectile_battle(
        1,
        prepared(vec![actor(Side::Party), vulnerable(Side::Enemy)], 30),
        projectile.clone(),
        3,
    );
    battle.step(request(1)).unwrap();
    battle
        .emit(
            Arc::new(projectile),
            ActionId(77),
            ActorId(0),
            ActorId(1),
            [0.; 3],
        )
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(1))]);
    assert!(frame.cues.contains(&Cue::Interrupted {
        action: ActionId(1)
    }));
    assert!(
        !frame
            .cues
            .iter()
            .any(|c| matches!(c, Cue::ProjectileStarted { .. }))
    );
    assert!(frame.outcome.is_none());
    assert_eq!(frame.recognized_result, None);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.recognized_result, Some(BattleResult::Victory));
    assert!(frame.outcome.is_none());
    battle.retire_combat().unwrap();
    let frame = battle.finish_result().unwrap();
    let outcome = frame.outcome.unwrap();
    assert_eq!(outcome.result, BattleResult::Victory);
    assert_eq!(battle.actors()[1].hp, 0);
    assert!(battle.owns_outcome(&outcome));
    assert!(frame.actions.is_empty() && frame.projectiles.is_empty());
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn surviving_projectile_inherits_the_live_owners_element_at_contact() {
    for (element, expected) in [
        (HitElement::Inherited, Affinity::Absorb),
        (HitElement::Neutral, Affinity::Normal),
        (HitElement::Element(Element::Lightning), Affinity::Weak),
    ] {
        let mut owner = actor(Side::Party);
        owner.equipment.base_element = Some(Element::Fire);
        let mut target = vulnerable(Side::Enemy);
        target.equipment.affinities[Element::Fire as usize + 1] = Affinity::Immune;
        target.equipment.affinities[Element::Water as usize + 1] = Affinity::Absorb;
        target.equipment.affinities[Element::Lightning as usize + 1] = Affinity::Weak;
        let mut projectile = contact_projectile(false, true);
        projectile.contact.as_mut().unwrap().hit.element = element;
        let mut battle = firing_battle(vec![owner, target], projectile);
        battle.actors[0].elements.enchantment = Some(Element::Water);
        let frame = battle
            .step(BattleInput {
                interrupt: vec![ActionId(1)],
                ..Default::default()
            })
            .unwrap();
        let result = frame
            .cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit { result, .. } => Some(result),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.affinity, expected);
        assert_eq!(result.hp_change > 0, expected == Affinity::Absorb);
    }
}

#[test]
fn guard_uses_post_acceleration_velocity_and_preserves_ordinary_projectile_retirement() {
    for (acceleration, expected_guard, hp) in [
        (
            0.3,
            GuardResult::Blocked {
                first: true,
                special: false,
            },
            45,
        ),
        (0.4, GuardResult::Broken, 30),
    ] {
        let mut target = vulnerable(Side::Enemy);
        target.guard = Guard {
            active: true,
            break_pressure: 10,
            reduction: 75,
            ..Default::default()
        };
        let mut projectile = contact_projectile(false, false);
        projectile.acceleration = [0., 0., acceleration];
        let hit = &mut projectile.contact.as_mut().unwrap().hit;
        hit.power = Power::Fixed(20);
        hit.guard.pressure = 1;
        let mut battle = firing_battle(vec![actor(Side::Party), target], projectile);
        let paused = battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            (paused.actors[1].hp, paused.actors[1].guard.pressure),
            (50, 0)
        );
        assert_eq!(battle.random_state(), 1);
        let frame = battle.step(BattleInput::default()).unwrap();
        let Some(Cue::Hit { result, .. }) =
            frame.cues.iter().find(|c| matches!(c, Cue::Hit { .. }))
        else {
            panic!("missing contact")
        };
        assert_eq!(result.guard, expected_guard);
        assert_eq!(frame.actors[1].hp, hp);
        assert!(!frame.projectiles[0].disarmed);
        // A guard result does not itself deflect/disarm an ordinary projectile.
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.projectiles.len(), 1);
        assert!(frame.projectiles[0].contact_active);
        assert!(!frame.projectiles[0].disarmed);
        assert!(hits(&frame).is_empty());
        assert!(
            battle
                .step(BattleInput::default())
                .unwrap()
                .projectiles
                .is_empty()
        );
    }
}

#[test]
fn contact_ex_equipment_rebuild_is_repeatable_and_preserves_live_hurt() {
    let mut current = actor(Side::Party);
    current.reaction.combo_hits = 11;
    current.equipment.normal_combo_limit = 4;
    let prepared =
        PreparedBattle::new(vec![(current, Default::default())], Default::default(), 1).unwrap();
    let mut battle = prepared.finish().unwrap();
    battle.begin_hurt(ActorId(0), 42, &mut vec![]);
    let random = battle.random_state();
    for enabled in [true, true, false, true] {
        let before = battle.actors[0].clone();
        let mut replacement = actor(Side::Party);
        replacement.equipment.contact = ContactTraits {
            technique_balance: -100,
            endure: enabled,
            hard_hit: enabled,
            air_brake: enabled,
        };
        replacement.equipment.normal_combo_limit = if enabled { 4 } else { 3 };
        let mut expected = before;
        expected.equipment.contact = replacement.equipment.contact;
        expected.equipment.normal_combo_limit = replacement.equipment.normal_combo_limit;
        crate::tests::equip(&mut battle, ActorId(0), replacement).unwrap();
        assert_eq!(battle.actors[0], expected);
        assert_eq!(battle.random_state(), random);
    }
}

/// Build the equipment-only transaction used by component fixtures.
pub(crate) fn equip(battle: &mut Battle, actor: ActorId, replacement: Actor) -> anyhow::Result<()> {
    battle.replace_equipment_batch(vec![crate::EquipmentReplacement {
        actor,
        attributes: replacement.equipment.clone(),
        conditions: replacement.conditions.clone(),
        equipment: None,
    }])
}

/// Count-only fixtures use an explicit catalogue with no automatic learning recipes.
pub(crate) fn counted_techniques(
    actor: ActorId,
    current: &[u16],
    counts: &[(u16, u16)],
) -> crate::learning::TechniqueLearningMember {
    let ids: std::collections::BTreeSet<_> = current
        .iter()
        .copied()
        .chain(counts.iter().map(|&(id, _)| id))
        .collect();
    let catalogue = resonance_content::arte::Catalogue {
        definitions: vec![Default::default(); usize::from(ids.last().copied().unwrap_or(0)) + 1],
        learning: vec![
            ids.into_iter()
                .map(|id| u8::try_from(id).unwrap())
                .collect(),
        ],
    };
    crate::learning::TechniqueLearningMember {
        actor,
        member: crate::learning::LearningCatalogue::new(Arc::new(catalogue))
            .prepare_member(crate::learning::LearningEntry {
                character: 1,
                level: 1,
                balance: 0,
                story_unlocked: true,
                current: current.iter().copied().collect(),
                counts: counts.iter().copied().collect(),
            })
            .unwrap(),
    }
}

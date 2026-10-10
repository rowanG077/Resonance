use super::*;
#[path = "all_divide_tests.rs"]
mod all_divide_tests;
use crate::{
    Activity, BattleInput, Control, MotionBinding,
    conditions::{Condition, ConditionSet},
};
use resonance_content::diagnostics::Diagnostics;
use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

fn actor() -> crate::Actor {
    let mut actor = crate::tests::actor(Side::Party);
    actor.control = Control::SemiAuto;
    actor.equipment.max_hp = 328;
    actor.hp = 300;
    actor.equipment.max_tp = 36;
    actor.tp = 32;
    actor
}

fn prepared(actors: Vec<crate::Actor>) -> PreparedBattle {
    let users: Vec<_> = actors
        .iter()
        .map(|actor| {
            (actor.side == Side::Party).then_some(ActorDefinition {
                motion: Some(MotionBinding { model: 7, clip: 20 }),
                quick: false,
            })
        })
        .collect();
    let count = actors.len();
    PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        0x1234_5678,
    )
    .unwrap()
    .with_items(Definition {
        policies: Arc::new(policies()),
        actors: users,
        targets: vec![TargetDefinition::default(); count],
    })
    .unwrap()
}

// Explicit fixture descriptors exercise the runtime without an inventory-ID interpreter.
fn policies() -> BTreeMap<u16, Policy> {
    use crate::{
        Element,
        conditions::{Buff, Cure},
    };
    use Effect::*;
    [
        (1, Recover { hp: 30, tp: 0 }, true),
        (2, Recover { hp: 60, tp: 0 }, true),
        (3, Recover { hp: 0, tp: 30 }, true),
        (4, Recover { hp: 0, tp: 60 }, true),
        (5, Recover { hp: 30, tp: 30 }, true),
        (6, Recover { hp: 60, tp: 60 }, true),
        (7, FullRecovery, false),
        (8, PartyRecover { hp: 30, tp: 0 }, false),
        (9, PartyRecover { hp: 0, tp: 30 }, false),
        (10, Effect::Cure(Cure::Physical), false),
        (11, Revive, false),
        (12, Effect::Cure(Cure::All), false),
        (13, Effect::Cure(Cure::AntiMagic), false),
        (14, Effect::Buff(Buff::Flare), false),
        (15, Effect::Buff(Buff::Flare), false),
        (16, Effect::Buff(Buff::Guard), false),
        (17, Effect::Buff(Buff::Acuity), false),
        (
            18,
            Effect::Buff(Buff::PhysicalAilmentGuard { persistent: true }),
            false,
        ),
        (
            19,
            Effect::Buff(Buff::PhysicalAilmentGuard { persistent: false }),
            false,
        ),
        (
            20,
            Effect::Buff(Buff::MagicalAilmentGuard { persistent: true }),
            false,
        ),
        (
            21,
            Effect::Buff(Buff::MagicalAilmentGuard { persistent: false }),
            false,
        ),
        (37, Scan, false),
        (38, AllDivide, false),
        (39, Hourglass, false),
        (44, Effect::Buff(Buff::Quartz(Element::Water)), false),
        (45, Effect::Buff(Buff::Quartz(Element::Wind)), false),
        (46, Effect::Buff(Buff::Quartz(Element::Fire)), false),
        (47, Effect::Buff(Buff::Quartz(Element::Earth)), false),
        (48, Effect::Buff(Buff::Quartz(Element::Ice)), false),
        (49, Effect::Buff(Buff::Quartz(Element::Lightning)), false),
        (50, Effect::Buff(Buff::Quartz(Element::Darkness)), false),
        (51, Effect::Buff(Buff::Quartz(Element::Light)), false),
        (600, Recover { hp: 25, tp: 0 }, true),
    ]
    .into_iter()
    .map(|(id, effect, records_gel_use)| {
        (
            id,
            Policy {
                effect,
                records_gel_use,
            },
        )
    })
    .collect()
}

struct Storage {
    inventory: BTreeMap<u16, u8>,
    gel: bool,
    scanned: bool,
    location: bool,
    learns_location: bool,
    acquisitions: usize,
}
impl Storage {
    fn new(item: u16, quantity: u8) -> Self {
        Self {
            inventory: BTreeMap::from([(item, quantity)]),
            gel: false,
            scanned: false,
            location: false,
            learns_location: false,
            acquisitions: 0,
        }
    }
}
impl Provider for Storage {
    fn acquire_item(&mut self, request: Release) -> Result<ItemLoan<'_>> {
        self.acquisitions += 1;
        let Entry::Occupied(stack) = self.inventory.entry(request.item) else {
            anyhow::bail!("missing test item stock");
        };
        ItemLoan::new(stack, Some(&mut self.gel))
    }
    fn acquire_scan(&mut self, request: Release) -> Result<ScanLoan<'_>> {
        self.acquisitions += 1;
        let Entry::Occupied(stack) = self.inventory.entry(request.item) else {
            anyhow::bail!("missing test lens stock");
        };
        let item = ItemLoan::new(stack, None)?;
        Ok(ScanLoan::new(
            item,
            &mut self.scanned,
            self.learns_location.then_some(&mut self.location),
        ))
    }
}

fn battle() -> Battle {
    (prepared(vec![actor(), actor(), crate::tests::actor(Side::Enemy)]))
        .finish()
        .unwrap()
}
fn request(item: u16, target: u8) -> Release {
    Release {
        user: ActorId(0),
        target: ActorId(target),
        item,
    }
}
fn step(battle: &mut Battle, storage: &mut Storage) -> crate::BattleFrame {
    let cues = battle.update(BattleInput::default(), storage).unwrap();
    battle.publish(cues)
}
fn before_release(battle: &mut Battle, storage: &mut Storage, request: Release) {
    let acquisitions = storage.acquisitions;
    battle.queue_item(request).unwrap();
    for _ in 1..RELEASE_TICKS {
        step(battle, storage);
    }
    assert_eq!(storage.acquisitions, acquisitions);
}

#[test]
fn all_ordinary_recoveries_consume_at_full_vitals_and_keep_hp_tp_operands_distinct() {
    for (item, hp, tp) in [
        (1, 98, 0),
        (2, 196, 0),
        (3, 0, 10),
        (4, 0, 21),
        (5, 98, 10),
        (6, 196, 21),
        (7, 328, 36),
        (8, 98, 0),
        (9, 0, 10),
    ] {
        let mut battle = battle();
        battle.actors[0].hp = 1;
        battle.actors[0].tp = 0;
        let mut storage = Storage::new(item, 1);
        before_release(&mut battle, &mut storage, request(item, 0));
        let random = battle.random_state();
        step(&mut battle, &mut storage);
        assert_eq!(battle.actors[0].hp, (1 + hp).min(328), "{item}");
        assert_eq!(battle.actors[0].tp, tp, "{item}");
        assert_eq!(battle.random_state(), random);
        assert!(storage.inventory.is_empty());
        assert_eq!(storage.gel, matches!(item, 1..=6));
        assert_eq!(battle.item_cooldown(), 120);
        assert_eq!(battle.ledger.items[0], 1);
        assert_eq!(battle.ledger.grade(), -5);
    }
    for item in [1, 3, 5, 7, 8, 9] {
        let mut battle = battle();
        battle.actors[0].hp = 328;
        battle.actors[0].tp = 36;
        let mut storage = Storage::new(item, 1);
        before_release(&mut battle, &mut storage, request(item, 0));
        step(&mut battle, &mut storage);
        assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (328, 36));
        assert!(storage.inventory.is_empty());
    }
}

#[test]
fn recovery_modifiers_bind_single_target_or_group_user_and_tp_ignores_ex21() {
    assert_eq!(release::percentage(30, true, true), 18);
    assert_eq!(release::percentage(15, true, true), 9);
    for (item, group) in [(5, false), (8, true), (9, true)] {
        let mut battle = battle();
        battle.actors[1].conditions =
            battle.actors[1]
                .conditions
                .clone()
                .with_traits(crate::conditions::Traits {
                    extended_duration: true,
                    ..Default::default()
                });
        for actor in &mut battle.actors[..2] {
            actor.hp = 1;
            actor.tp = 0;
            actor.equipment.recovery.boost = true;
        }
        let mut storage = Storage::new(item, 1);
        before_release(&mut battle, &mut storage, request(item, 1));
        step(&mut battle, &mut storage);
        // Single5 uses target118:37%, EX21 HP44%, TP37%.
        // Group8/9 uses user without118:30%, individual EX21 HP36%, TP30%.
        if item != 9 {
            assert_eq!(battle.actors[1].hp, if group { 119 } else { 145 });
        }
        if item != 8 {
            assert_eq!(battle.actors[1].tp, if group { 10 } else { 13 });
        }
    }
    let mut weak = actor();
    weak.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: ConditionSet::of(&[Condition::Weak]),
        ..Default::default()
    });
    weak.hp = 200;
    assert_eq!(weak.recovered_hp(30), (200, 98));
    weak.hp = 150;
    assert_eq!(weak.recovered_hp(30), (164, 98));
}

#[test]
fn seal_reduces_single_target_and_group_user_recovery_but_elixir_bypasses() {
    use crate::conditions::{Conditions, Layers};

    let mut single = battle();
    single.actors[1].conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::ReduceItemEffect]),
        ..Default::default()
    });
    single.actors[1].hp = 1;
    let mut storage = Storage::new(5, 1);
    before_release(&mut single, &mut storage, request(5, 1));
    step(&mut single, &mut storage);
    assert_eq!(single.actors[1].hp, 50); // 1 + floor(328 * 15 / 100)
    assert!(storage.inventory.is_empty());

    // Group recovery uses the item user's Seal once for every recipient.
    let mut group = battle();
    group.actors[0].conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::ReduceItemEffect]),
        ..Default::default()
    });
    group.actors[0].hp = 1;
    group.actors[1].hp = 1;
    let mut storage = Storage::new(8, 1);
    before_release(&mut group, &mut storage, request(8, 1));
    step(&mut group, &mut storage);
    assert_eq!(group.actors[0].hp, 50);
    assert_eq!(group.actors[1].hp, 50);

    let mut elixir = battle();
    elixir.actors[0].conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::ReduceItemEffect]),
        ..Default::default()
    });
    elixir.actors[0].hp = 1;
    elixir.actors[0].tp = 0;
    let mut storage = Storage::new(7, 1);
    before_release(&mut elixir, &mut storage, request(7, 0));
    step(&mut elixir, &mut storage);
    assert_eq!((elixir.actors[0].hp, elixir.actors[0].tp), (328, 36));
}

#[test]
fn life_bottle_commits_once_and_recovers_without_waiting_for_a_pose() {
    let mut dead = actor();
    dead.hp = 0;
    dead.availability = crate::ActorAvailability::Dead;
    let mut battle = prepared(vec![actor(), dead, crate::tests::actor(Side::Enemy)])
        .finish()
        .unwrap();
    let diagnostics = Diagnostics::new(true);
    battle.set_diagnostics(diagnostics.clone());
    let mut storage = Storage::new(11, 1);
    before_release(&mut battle, &mut storage, request(11, 1));
    let result = battle.update(BattleInput::default(), &mut storage);
    assert!(result.is_ok());
    let target = &battle.actors[1];
    assert_eq!((target.hp, target.tp), (98, 36));
    assert!(target.available());
    assert!(storage.inventory.is_empty());
    assert_eq!(storage.acquisitions, 1);
    assert_eq!(battle.ledger.items[0], 1);
    assert!(battle.pending_item().is_none());
    assert!(!diagnostics.has_errors());
    if let Ok(cues) = result {
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Recovered {
                kind: crate::RecoveryKind::Hp,
                actor: ActorId(1),
                applied: 98,
                ..
            }
        )));
        for _ in 0..15 {
            step(&mut battle, &mut storage);
        }
        assert!(battle.actor_command_ready(ActorId(1)));
        assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
        assert!(!battle.is_diagnostic());
        assert_eq!(storage.acquisitions, 1);
    }
}

#[test]
fn repeated_lens_use_preserves_knowledge_and_reports_discovery_once() {
    for learns_location in [false, true] {
        let mut battle = battle();
        battle.actors[2].equipment.affinities[0] = crate::Affinity::Weak;
        battle.actors[2].equipment.affinities[1] = crate::Affinity::Resistant;
        battle.actors[2].equipment.affinities[2] = crate::Affinity::Immune;
        let mut storage = Storage::new(37, 2);
        storage.learns_location = learns_location;
        let request = request(37, 2);
        for use_index in 0..2 {
            if use_index != 0 {
                for _ in 0..180 {
                    if battle.can_queue_item(request.user).unwrap()
                        && battle.actor_command_ready(request.user)
                    {
                        break;
                    }
                    step(&mut battle, &mut storage);
                }
                assert!(battle.actor_command_ready(request.user));
                assert_eq!(battle.item_cooldown(), 0);
            }
            before_release(&mut battle, &mut storage, request);
            battle.actors[request.user.index()].hit_stop = 20;
            let frame = step(&mut battle, &mut storage);
            assert!(storage.scanned);
            assert_eq!(storage.location, learns_location);
            assert!(battle.enemy_scanned(request.target).unwrap());
            assert!(battle.ledger.enemy_was_scanned);
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue,
                        Cue::EnemyScanned { actor } if *actor == request.target
                    ))
                    .count(),
                1
            );
            assert!(frame.cues.contains(&Cue::ItemReleased {
                user: request.user,
                target: request.target,
                effect: Effect::Scan,
                discovered: use_index == 0,
            }));
        }
        assert!(storage.inventory.is_empty());
        assert_eq!(storage.acquisitions, 2);
        assert_eq!(battle.ledger.items[request.user.index()], 2);
        assert_eq!(battle.ledger.grade(), -10);
    }
}

#[test]
fn unsupported_items_and_ineligible_targets_never_reserve_or_spend() {
    for item in [0, 22, 36, 40, 43, 52, 65535] {
        let mut battle = battle();
        assert!(battle.item_policy(item).is_none());
        assert!(battle.queue_item(request(item, 0)).is_err());
        assert!(battle.pending_item().is_none());
    }
    let mut battle = battle();
    assert!(battle.item_policy(13).is_some());
    assert!(battle.queue_item(request(11, 1)).is_err());
    let mut storage = Storage::new(1, 1);
    before_release(&mut battle, &mut storage, request(1, 1));
    battle.actors[1].availability = crate::ActorAvailability::Petrified;
    step(&mut battle, &mut storage);
    assert_eq!(storage.acquisitions, 0);
    assert!(battle.pending_item().is_none());
    assert_eq!(battle.ledger.items[0], 0);
    assert_eq!(storage.inventory[&1], 1);
    // Runtime behavior comes from the admitted descriptor, not the inventory key.
    let mut remapped = self::battle();
    remapped.actors[0].hp = 1;
    let mut storage = Storage::new(600, 1);
    before_release(&mut remapped, &mut storage, request(600, 0));
    step(&mut remapped, &mut storage);
    assert_eq!(remapped.actors[0].hp, 83);
    assert!(!storage.inventory.contains_key(&600));
    assert!(storage.gel);
}

#[test]
fn hourglass_freezes_opponents_while_party_keeps_running() {
    let mut battle = prepared(vec![actor(), actor(), crate::tests::actor(Side::Enemy)])
        .finish()
        .unwrap();
    let mut storage = Storage::new(39, 1);
    before_release(&mut battle, &mut storage, request(39, 0));
    battle.enter_stun(ActorId(2), &mut vec![]);
    let released = step(&mut battle, &mut storage);
    assert_eq!(released.hourglass_remaining, 300);
    assert_eq!(battle.actors[0].time_stop, 0);
    assert_eq!(battle.actors[1].time_stop, 0);
    assert_eq!(battle.actors[2].time_stop, 300);
    assert!(!battle.is_paused());
    assert!(storage.inventory.is_empty());
    battle
        .update(
            BattleInput {
                paused: true,
                ..Default::default()
            },
            &mut storage,
        )
        .unwrap();
    assert_eq!(battle.hourglass_remaining(), 300);

    battle.actors[1].hit_stop = 2;
    battle.actors[2].hit_stop = 2;
    for _ in 0..60 {
        step(&mut battle, &mut storage);
    }
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(battle.actors[1].hit_stop, 0);
    assert_eq!(battle.actors[2].hit_stop, 2);
    assert_eq!(battle.activity(ActorId(2)), Activity::Stunned);
    assert!(battle.transition().is_none());
    assert_eq!(battle.hourglass_remaining(), 240);
    for _ in 0..240 {
        step(&mut battle, &mut storage);
    }
    assert_eq!(battle.hourglass_remaining(), 0);
    assert_eq!(battle.actors[2].hit_stop, 1);
    assert_eq!(battle.actors[2].time_stop, 0);
}

#[test]
fn death_clears_its_freeze_and_inactive_actors_still_expire() {
    let mut battle = prepared(vec![
        actor(),
        crate::tests::actor(Side::Enemy),
        crate::tests::actor(Side::Enemy),
        crate::tests::actor(Side::Enemy),
    ])
    .finish()
    .unwrap();
    battle.actors[2].time_stop = 2;
    battle.actors[2].availability = crate::ActorAvailability::Absent;
    battle.actors[3].time_stop = 30;
    battle.actors[3].hp = 0;
    battle.enter_death(ActorId(3), &mut vec![]);
    assert_eq!(battle.actors[3].time_stop, 0);
    assert_eq!(battle.hourglass_remaining(), 2);
    for _ in 0..2 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.hourglass_remaining(), 0);
    battle.actors[2].availability = crate::ActorAvailability::Active;
    battle.actors[2].hit_stop = 2;
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors[2].hit_stop, 1);
}

#[test]
fn preflight_faults_cancel_once_without_effect_or_inventory_mutation() {
    for paranoid in [false, true] {
        let mut battle = battle();
        let diagnostics = Diagnostics::new(paranoid);
        battle.set_diagnostics(diagnostics.clone());
        let mut storage = Storage::new(1, 1);
        before_release(&mut battle, &mut storage, request(1, 0));
        storage.inventory.clear();
        battle.begin_hurt(ActorId(2), 1, &mut vec![]);
        let hp_tp: Vec<_> = battle
            .actors
            .iter()
            .map(|actor| (actor.hp, actor.tp))
            .collect();
        let inventory = storage.inventory.clone();
        let grade = battle.ledger.grade();
        let result = battle.update(BattleInput::default(), &mut storage);
        assert_eq!(result.is_err(), paranoid);
        assert!(battle.pending_item().is_none());
        assert_eq!(
            battle
                .actors
                .iter()
                .map(|actor| (actor.hp, actor.tp))
                .collect::<Vec<_>>(),
            hp_tp
        );
        assert_eq!(storage.inventory, inventory);
        assert!(!storage.gel);
        assert!(!storage.scanned && !storage.location);
        assert_eq!(battle.ledger.grade(), grade);
        assert_eq!(battle.ledger.items[0], 0);
        assert!(!battle.ledger.party_item_effect_used);
        assert_eq!(battle.items.cooldown, 0);
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert!(battle.is_diagnostic());
        if paranoid {
            assert!(battle.update(BattleInput::default(), &mut storage).is_err());
        } else {
            assert_eq!(battle.activity(ActorId(2)), Activity::Idle);
            for _ in 0..3 {
                step(&mut battle, &mut storage);
            }
        }
        assert_eq!(diagnostics.entries().len(), 1);
        assert_eq!(storage.acquisitions, 1);
    }
}

#[test]
fn queued_item_survives_hurt_recovery_and_petrification_but_death_clears_it() {
    let mut battle = battle();
    let mut storage = Storage::new(1, 1);
    battle.queue_item(request(1, 0)).unwrap();
    battle.begin_hurt(ActorId(0), 10, &mut vec![]);
    step(&mut battle, &mut storage);
    assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
    assert_eq!(battle.pending_item(), Some(request(1, 0)));
    assert_eq!(storage.acquisitions, 0);
    battle.interrupt_actor(ActorId(0), &mut vec![]);
    battle.enter_idle(ActorId(0));
    assert_eq!(battle.pending_item(), Some(request(1, 0)));
    step(&mut battle, &mut storage);
    assert_eq!(battle.activity(ActorId(0)), Activity::Item);
    assert_eq!(storage.acquisitions, 0);
    let interrupted = battle.runtime[0].task().item().unwrap().action;
    let mut cues = vec![];
    battle.begin_hurt(ActorId(0), 10, &mut cues);
    assert!(cues.contains(&Cue::Interrupted {
        action: interrupted
    }));
    let frame = step(&mut battle, &mut storage);
    assert_eq!(battle.action_age(interrupted), None);
    assert!(
        !frame
            .actions
            .iter()
            .any(|(action, _, _)| *action == interrupted)
    );
    assert_eq!(battle.pending_item(), Some(request(1, 0)));
    battle.enter_idle(ActorId(0));
    step(&mut battle, &mut storage);
    let restarted = battle.runtime[0].task().item().unwrap();
    assert_ne!(restarted.action, interrupted);
    assert_eq!(storage.acquisitions, 0);
    battle.actors[0].hp = 0;
    battle.enter_death(ActorId(0), &mut vec![]);
    assert!(battle.pending_item().is_none());
    assert_eq!(storage.inventory[&1], 1);

    let mut battle = self::battle();
    battle.queue_item(request(1, 0)).unwrap();
    battle.actors[0].availability = crate::ActorAvailability::Petrified;
    for _ in 0..2 {
        step(&mut battle, &mut storage);
        assert_eq!(battle.pending_item(), Some(request(1, 0)));
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(storage.acquisitions, 0);
        assert_eq!(storage.inventory[&1], 1);
    }
}

#[test]
fn queued_item_takes_priority_over_new_player_actions() {
    let mut candidate = prepared(vec![actor(), actor(), crate::tests::actor(Side::Enemy)]);
    candidate.actors[2].position = [0., 0., 80.];
    let normals = crate::tests::normal_controls(
        &mut candidate.resources.actions,
        crate::PreparedAttack {
            recovery: 4,
            ..crate::tests::attack(90)
        },
        [0., 120.],
    );
    candidate.resources.actor_setup[0].control = Some(Arc::new(crate::ControlDefinition {
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 8,
        motions: None,
        shortcuts: [0; 4],
        normals,
    }));
    let mut battle = candidate.finish().unwrap();
    let random = battle.random_state();
    let mut storage = Storage::new(1, 1);
    battle.queue_item(request(1, 0)).unwrap();
    let cues = battle
        .update(
            BattleInput {
                controllers: vec![crate::ControlInput {
                    attack: crate::ButtonInput {
                        held: true,
                        pressed: true,
                        released: false,
                    },
                    ..crate::ControlInput::neutral(ActorId(0))
                }],
                ..Default::default()
            },
            &mut storage,
        )
        .unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Item);
    assert_eq!(battle.random_state(), random);
    assert_eq!(
        cues.iter()
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
    assert!(
        !cues
            .iter()
            .any(|cue| matches!(cue, Cue::Interrupted { .. }))
    );
    assert_eq!(storage.acquisitions, 0);
}

#[test]
fn lens_eligibility_is_shared_by_selection_confirmation_and_release() {
    for inactive_selectable in [false, true] {
        let mut actors = vec![
            actor(),
            crate::tests::actor(Side::Enemy),
            crate::tests::actor(Side::Enemy),
            crate::tests::actor(Side::Enemy),
        ];
        actors[2].hp = 0;
        let mut prepared = prepared(actors);
        let targets = &mut prepared.resources.items.as_mut().unwrap().targets;
        targets[1].excluded = true;
        targets[2].inactive_selectable = inactive_selectable;
        let mut battle = prepared.finish().unwrap();
        assert!(!battle.item_target_eligible(37, ActorId(0)).unwrap());
        assert!(!battle.item_target_eligible(37, ActorId(1)).unwrap());
        assert_eq!(
            battle.item_target_eligible(37, ActorId(2)).unwrap(),
            inactive_selectable
        );
        assert!(battle.item_target_eligible(37, ActorId(3)).unwrap());
        for control in [Control::Manual, Control::SemiAuto, Control::Auto] {
            battle.actors[0].control = control;
            let expected = Some(ActorId(if inactive_selectable { 2 } else { 3 }));
            for (current, direction) in [(0, 1), (3, 1), (3, -1)] {
                assert_eq!(
                    battle
                        .cycle_item_target(37, ActorId(0), ActorId(current), direction)
                        .unwrap(),
                    expected
                );
            }
        }
        battle.actors[0].control = Control::SemiAuto;
        assert!(battle.queue_item(request(37, 1)).is_err());
        assert!(battle.pending_item().is_none());
        let mut storage = Storage::new(37, 1);
        if inactive_selectable {
            before_release(&mut battle, &mut storage, request(37, 2));
            step(&mut battle, &mut storage);
            assert!(storage.scanned);
            assert_eq!(storage.acquisitions, 1);
        } else {
            assert!(battle.queue_item(request(37, 2)).is_err());
            before_release(&mut battle, &mut storage, request(37, 3));
            battle.actors[3].availability = crate::ActorAvailability::Petrified;
            for direction in [-1, 1] {
                assert_eq!(
                    battle
                        .cycle_item_target(37, ActorId(0), ActorId(3), direction)
                        .unwrap(),
                    None
                );
            }
            step(&mut battle, &mut storage);
            assert_eq!(storage.acquisitions, 0);
            assert!(!storage.scanned);
            assert_eq!(storage.inventory[&37], 1);
            assert!(battle.pending_item().is_none());
        }
    }
}

#[test]
fn item_release_commits_once_and_finishes_without_a_pose() {
    for (pose, quick) in [(Some(20), false), (Some(99), true), (None, false)] {
        let mut candidate = prepared(vec![actor(), actor(), crate::tests::actor(Side::Enemy)]);
        if let Some(clip) = pose {
            candidate.resources.items.as_mut().unwrap().actors[0]
                .as_mut()
                .unwrap()
                .motion
                .as_mut()
                .unwrap()
                .clip = clip;
        } else {
            candidate.resources.items.as_mut().unwrap().actors[0]
                .as_mut()
                .unwrap()
                .motion = None;
        }
        candidate.resources.items.as_mut().unwrap().actors[0]
            .as_mut()
            .unwrap()
            .quick = quick;
        let mut battle = candidate.finish().unwrap();
        let diagnostics = Diagnostics::default();
        battle.set_diagnostics(diagnostics.clone());
        let mut storage = Storage::new(1, 1);
        before_release(&mut battle, &mut storage, request(1, 0));
        assert_eq!(battle.actors[0].hp, 300);
        let frame = step(&mut battle, &mut storage);
        assert_eq!(battle.actors[0].hp, 328);
        assert!(storage.inventory.is_empty());
        assert_eq!(storage.acquisitions, 1);
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Recovered {
                actor: ActorId(0),
                ..
            }
        )));
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::ItemReleased { .. }))
                .count(),
            1
        );
        let mut completed = 0;
        let duration = if quick { QUICK_USE_TICKS } else { USE_TICKS };
        for _ in RELEASE_TICKS..duration {
            let frame = step(&mut battle, &mut storage);
            completed += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Completed { .. }))
                .count();
        }
        assert_eq!(completed, 1);
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(storage.acquisitions, 1);
        assert!(!battle.is_diagnostic());
        assert!(!diagnostics.has_errors());
    }
}

#[test]
fn buff_items_refresh_the_recipient_and_spend_once_per_release() {
    for (user, target) in [(1, 0), (0, 0), (0, 1)] {
        let mut battle = (prepared(vec![actor(), actor(), crate::tests::actor(Side::Enemy)]))
            .finish()
            .unwrap();
        let request = Release {
            user: ActorId(user),
            target: ActorId(target),
            item: 14,
        };
        let mut storage = Storage::new(request.item, 2);
        for used in 1..=2 {
            let cooldown = battle.item_cooldown();
            battle
                .update(
                    BattleInput {
                        paused: true,
                        ..Default::default()
                    },
                    &mut storage,
                )
                .unwrap();
            assert_eq!(battle.item_cooldown(), cooldown);
            while battle.item_cooldown() != 0 {
                step(&mut battle, &mut storage);
            }
            before_release(&mut battle, &mut storage, request);
            let frame = step(&mut battle, &mut storage);
            let condition = &battle.actors[request.target.index()].conditions;
            assert_eq!(condition.base(), ConditionSet::of(&[Condition::Flare]));
            assert_eq!(storage.acquisitions, usize::from(used));
            assert_eq!(storage.inventory.get(&14).copied().unwrap_or(0), 2 - used);
            assert_eq!(battle.ledger.items[request.user.index()], used);
            assert!(battle.pending_item().is_none());
            assert!(frame.cues.iter().any(|cue| matches!(cue,
                Cue::ConditionLabel { actor, kind: crate::conditions::ConditionLabel::Applied, .. }
                if *actor == request.target
            )));
        }
    }
}

#[test]
fn bottles_and_quartz_apply_named_effects_and_record_the_recipient() {
    use crate::Element;
    for (item, condition, persistent, element) in [
        (14, Condition::Flare, false, None),
        (18, Condition::PhysicalProtection, true, None),
        (48, Condition::Quartz, false, Some(Element::Ice)),
    ] {
        let mut battle = battle();
        let mut storage = Storage::new(item, 1);

        assert!(battle.item_policy(item).is_some());
        assert!(!battle.item_target_eligible(item, ActorId(2)).unwrap());
        before_release(&mut battle, &mut storage, request(item, 1));
        let random = battle.random_state();
        let frame = step(&mut battle, &mut storage);
        let target = &battle.actors[1];
        assert_eq!(
            target.conditions.base(),
            if element.is_some() {
                ConditionSet::of(&[condition, Condition::Enchanted])
            } else {
                ConditionSet::of(&[condition])
            },
            "{item}"
        );
        assert_eq!(target.conditions.remaining(condition).is_none(), persistent);
        assert_eq!(target.elements.enchantment, element, "{item}");
        assert_eq!((target.hp, target.tp), (300, 32));
        assert_eq!(battle.random_state(), random);
        assert!(storage.inventory.is_empty());
        assert!(!storage.gel);
        assert!(battle.item_target_eligible(item, ActorId(1)).unwrap());
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Recovered { .. }))
        );
    }
}

#[test]
fn duration_extension_belongs_to_the_buff_recipient() {
    // The actual recipient carries the shared prepared EX118 trait.
    for item in [14, 19, 48] {
        for (user_compound, target_compound) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut battle = battle();
            for (index, extended_duration) in
                [user_compound, target_compound].into_iter().enumerate()
            {
                battle.actors[index].conditions = battle.actors[index]
                    .conditions
                    .clone()
                    .with_traits(crate::conditions::Traits {
                        extended_duration,
                        ..Default::default()
                    });
            }
            let mut storage = Storage::new(item, 1);
            before_release(&mut battle, &mut storage, request(item, 1));
            step(&mut battle, &mut storage);
            let condition = match item {
                14 => Condition::Flare,
                19 => Condition::PhysicalProtection,
                _ => Condition::Quartz,
            };
            let duration = if item == 19 { 900 } else { 1200 };
            assert_eq!(
                battle.actors[1].conditions.remaining(condition),
                Some(duration + if target_compound { duration / 4 } else { 0 } - 1)
            );
        }
    }
}

#[test]
fn immune_buff_is_consumed_without_changing_conditions() {
    for (item, condition) in [
        (14, Condition::Flare),
        (19, Condition::PhysicalProtection),
        (48, Condition::Quartz),
    ] {
        let mut battle = battle();
        battle.actors[0].conditions =
            crate::conditions::Conditions::new(crate::conditions::Layers {
                immunity: ConditionSet::of(&[condition]),
                ..Default::default()
            });
        let mut storage = Storage::new(item, 1);

        let request = Release {
            user: ActorId(1),
            target: ActorId(0),
            item,
        };
        before_release(&mut battle, &mut storage, request);
        let random = battle.random_state();
        let frame = step(&mut battle, &mut storage);
        let target = &battle.actors[0];
        assert!(target.conditions.base().is_empty());
        assert!(target.conditions.active_effects().is_empty());
        assert_eq!(target.elements.enchantment, None);
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::ConditionLabel { .. }))
        );
        assert_eq!(battle.ledger.items, [0, 1, 0]);
        assert_eq!(battle.ledger.grade(), -5);
        assert_eq!(battle.item_cooldown(), 120);
        assert_eq!(battle.random_state(), random);
        assert!(storage.inventory.is_empty());
        assert!(!storage.gel);
    }
}

#[test]
fn buff_faults_preserve_conditions_inventory_and_usage() {
    for item in [14, 48] {
        for paranoid in [false, true] {
            let mut battle = battle();
            let diagnostics = Diagnostics::new(paranoid);
            battle.set_diagnostics(diagnostics.clone());
            let mut storage = Storage::new(item, 1);

            before_release(&mut battle, &mut storage, request(item, 1));
            storage.inventory.clear();
            let inventory = storage.inventory.clone();
            let result = battle.update(BattleInput::default(), &mut storage);
            assert_eq!(result.is_err(), paranoid, "{item}");
            assert!(battle.pending_item().is_none());
            assert_eq!(storage.acquisitions, 1);
            assert_eq!(storage.inventory, inventory);
            assert!(battle.actors[1].conditions.base().is_empty());
            assert!(battle.actors[1].conditions.active_effects().is_empty());
            assert_eq!(battle.actors[1].elements.enchantment, None);
            assert_eq!(battle.ledger.grade(), 0);
            assert_eq!(battle.ledger.items[0], 0);
            assert_eq!(battle.item_cooldown(), 0);
            if !paranoid {
                assert_eq!(diagnostics.entries().len(), 1);
                for _ in 0..3 {
                    step(&mut battle, &mut storage);
                }
                assert_eq!(diagnostics.entries().len(), 1);
                assert_eq!(storage.acquisitions, 1);
            }
        }
    }
}

#[path = "cure_tests.rs"]
mod cures;

use super::*;
use crate::conditions::{Condition, ConditionSet};
use resonance_content::battle_action::Condition as HitAilment;
#[path = "all_divide_tests.rs"]
mod all_divide_tests;
#[path = "damage_modifier_tests.rs"]
mod damage_modifier_tests;
#[path = "kill_recovery_tests.rs"]
mod kill_recovery_tests;
#[path = "unison_tests.rs"]
mod unison_tests;
use crate::{
    Activity, Affinity, Battle, Control, DamageKind, GuardResult, GuardRule, HitElement, Power,
    ProjectileDefinition,
};
use std::sync::Arc;

fn rule(power: u16) -> HitRule {
    HitRule {
        overlimit_pause: true,
        kind: DamageKind::Slash,
        arte: true,
        power: Power::Fixed(power),
        element: HitElement::Neutral,
        prevents_defeat: false,
        reaction: Default::default(),
        guard: GuardRule {
            pressure: 0,
            ..Default::default()
        },
        condition: None,
    }
}

fn strike(battle: &mut Battle, rule: HitRule) -> Result<Vec<Cue>> {
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Melee {
            actor: ActorId(0),
            action: ActionId(1),
        },
        owner: ActorId(0),
        origin: Origin::World([0.; 3]),
        radius: 20.,
        height: 20.,
        shape: HitShape::Box,
        rule,
        clashes: false,
        struck: Vec::new(),
    })?;
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues)?;
    Ok(cues)
}

fn combo_grade_contact(
    victim: Side,
    prior_hits: i32,
    hp: i32,
    guarded: bool,
) -> Result<(Battle, Vec<Cue>)> {
    let owner_side = if victim == Side::Enemy {
        Side::Party
    } else {
        Side::Enemy
    };
    let owner = crate::tests::actor(owner_side);
    let mut target = crate::tests::actor(victim);
    target.hp = hp;
    target.equipment.max_hp = 1000;
    target.heading = 180.;
    target.facing_direction = crate::control::direction_from_heading(target.heading);
    target.guard.active = guarded;
    target.guard.break_pressure = 31;
    target.reaction.combo_hits = prior_hits;
    target.reaction.combo_damage = 210;
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = crate::PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        Default::default(),
        1,
    )?
    .finish()?;
    let cues = strike(&mut battle, rule(54))?;
    Ok((battle, cues))
}

#[test]
fn native_melee_volume_turns_scales_and_follows_the_owner_without_a_model() -> Result<()> {
    let mut owner = crate::tests::actor(Side::Party);
    owner.position = [10., 0., 20.];
    owner.heading = 90.;
    owner.body.scale = 2.;
    let mut front = crate::tests::actor(Side::Enemy);
    front.position = [90., 60., 20.];
    front.body.collider = Some(crate::Collider::sphere(2.));
    let mut behind = front.clone();
    behind.position[0] = -70.;
    let definition = MeleeDefinition {
        hit: HitRule {
            overlimit_pause: false,
            ..rule(5)
        },
        trail: None,
        volume: crate::MeleeVolume {
            offset: [0., 30., 40.],
            radius: 10.,
            half_height: 10.,
        },
    };
    definition.validate()?;
    let mut battle =
        crate::tests::prepared(vec![owner.clone(), front, behind, owner], 1).finish()?;
    let mut contacts = Contacts::default();
    contacts.melee(ActorId(0), ActionId(1), &definition, &[])?;
    contacts.melee(ActorId(3), ActionId(2), &definition, &[])?;
    for actor in &mut battle.actors {
        actor.position[0] += 100.;
    }
    battle.push_bodies();
    for (actual, expected) in contacts.0[0]
        .position(&battle.actors)
        .into_iter()
        .zip([190., 60., 20.])
    {
        assert!((actual - expected).abs() < 0.001);
    }
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues)?;
    assert_eq!(battle.actors[1].hp, 40);
    assert_eq!(battle.actors[2].hp, 50);
    let mut invalid = definition.clone();
    invalid.volume.offset[0] = f32::NAN;
    assert!(invalid.validate().is_err());
    Ok(())
}

#[test]
fn hit_condition_label_matches_the_condition_and_application_pose() -> Result<()> {
    for (condition, expected_label, duration) in [
        (
            HitAilment::Weak,
            crate::conditions::ConditionLabel::StatusDown,
            None,
        ),
        (
            HitAilment::Curse,
            crate::conditions::ConditionLabel::Applied,
            None,
        ),
        (
            HitAilment::Paralysis,
            crate::conditions::ConditionLabel::Applied,
            Some(600),
        ),
    ] {
        let owner = crate::tests::actor(Side::Party);
        let mut target = crate::tests::actor(Side::Enemy);
        target.hp = 1000;
        target.equipment.max_hp = 1000;
        target.position = [10., 20., 30.];
        target.body.center_offset = [1., 2., 3.];
        target.body.collider = Some(crate::Collider::sphere(2.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1)
            .finish()
            .unwrap();
        let mut contacts = Contacts::default();
        contacts.push(Contact {
            source: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1),
            },
            owner: ActorId(0),
            origin: Origin::World(battle.actors[1].position),
            radius: 20.,
            height: 20.,
            shape: HitShape::Box,
            rule: HitRule {
                overlimit_pause: true,
                kind: DamageKind::Slash,
                arte: false,
                power: Power::Fixed(1),
                element: HitElement::Neutral,
                prevents_defeat: false,
                reaction: Default::default(),
                guard: GuardRule::default(),
                condition: Some(crate::HitCondition {
                    condition,
                    chance: 100,
                    value: 0,
                }),
            },
            clashes: false,
            struck: Vec::new(),
        })?;
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues)?;
        assert_eq!(
            battle.actors[1].conditions.effective(),
            Condition::from(condition).into()
        );
        assert_eq!(
            battle.actors[1].conditions.remaining(condition.into()),
            duration
        );
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::ConditionLabel {
                actor: ActorId(1),
                kind,
                ..
            } if *kind == expected_label
        )));
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::ConditionLabel {
                position: [12., 24., 36.],
                ..
            }
        )));
    }
    Ok(())
}

#[test]
fn petrified_contact_is_skipped_without_condition_or_rng() -> Result<()> {
    let owner = crate::tests::actor(Side::Party);
    let mut target = crate::tests::actor(Side::Enemy);
    target.availability = crate::ActorAvailability::Petrified;
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = crate::tests::prepared(vec![owner, target], 1)
        .finish()
        .unwrap();
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Melee {
            actor: ActorId(0),
            action: ActionId(1),
        },
        owner: ActorId(0),
        origin: Origin::World([0.; 3]),
        radius: 20.,
        height: 20.,
        shape: HitShape::Box,
        rule: HitRule {
            overlimit_pause: true,
            kind: DamageKind::Slash,
            arte: false,
            power: Power::Fixed(1),
            element: HitElement::Neutral,
            prevents_defeat: false,
            reaction: Default::default(),
            guard: GuardRule::default(),
            condition: Some(crate::HitCondition {
                condition: resonance_content::battle_action::Condition::Paralysis,
                chance: 100,
                value: 0,
            }),
        },
        clashes: false,
        struck: Vec::new(),
    })?;
    let random = battle.random_state();
    let hp = battle.actors[1].hp;
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues)?;
    assert_eq!(battle.random_state(), random);
    assert_eq!(battle.actors[1].hp, hp);
    assert_eq!(battle.actors[1].conditions.effective(), ConditionSet::EMPTY);
    assert!(!cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    assert!(
        !cues
            .iter()
            .any(|cue| matches!(cue, Cue::ConditionLabel { .. }))
    );
    Ok(())
}

#[test]
fn long_combos_keep_the_recorded_maximum_without_narrowing() -> Result<()> {
    for prior in [i32::from(u16::MAX), i32::MAX] {
        let (battle, cues) = combo_grade_contact(Side::Enemy, prior, 1000, false)?;
        assert_eq!(battle.ledger().maximum_combo, u16::MAX);
        assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    }
    Ok(())
}

#[test]
fn accepted_combo_contacts_adjust_grade_for_each_victim_side() -> Result<()> {
    for (prior, enemy_grade, party_grade) in
        [(0, 0, -1), (3, 0, -1), (4, 2, -10), (5, 0, -1), (9, 2, -10)]
    {
        for (victim, grade) in [(Side::Enemy, enemy_grade), (Side::Party, party_grade)] {
            let (battle, cues) = combo_grade_contact(victim, prior, 1000, false)?;
            assert_eq!(battle.actors[1].reaction.combo_hits, prior + 1);
            assert_eq!(battle.ledger().grade(), grade, "{victim:?}, prior={prior}");
            assert_eq!(
                cues.iter().any(|cue| matches!(cue, Cue::Combo { .. })),
                prior > 0
            );
        }
    }
    // A blocked contact leaves the enemy's existing fifth-hit count unchanged.
    // It must not replay the combo-grade adjustment on that same count.
    let (battle, cues) = combo_grade_contact(Side::Enemy, 5, 1000, true)?;
    assert_eq!(battle.actors[1].reaction.combo_hits, 5);
    assert_eq!(battle.ledger().grade(), 0);
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Hit {
            result: crate::HitResult {
                guard: GuardResult::Blocked { .. },
                ..
            },
            ..
        }
    )));
    Ok(())
}

#[test]
fn lethal_fifth_contact_keeps_combo_grade_through_death_and_result_doubling() -> Result<()> {
    for (prior_hits, contact_grade, result_grade) in [(3, 0, 150), (4, 2, 154)] {
        let (mut battle, cues) = combo_grade_contact(Side::Enemy, prior_hits, 31, false)?;
        assert_eq!(battle.actors[1].reaction.combo_hits, 0);
        assert_eq!(battle.actors[1].reaction.combo_damage, 0);
        assert_eq!(battle.ledger().maximum_combo_damage, 264);
        assert_eq!(battle.ledger().kills, [1, 0]);
        assert_eq!(battle.ledger().last_party_killer, Some(ActorId(0)));
        assert_eq!(battle.ledger().maximum_combo, (prior_hits + 1) as u16);
        assert_eq!(battle.ledger().grade(), contact_grade);
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Combo { hits, damage: 264, .. } if *hits == prior_hits + 1
        )));
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        battle.retire_combat()?;
        battle.ledger.elapsed_ticks = 301;
        assert_eq!(
            battle.finalize_grade(&[ConditionSet::EMPTY], &[0], 0)?,
            result_grade
        );
    }
    Ok(())
}

#[test]
fn combo_feedback_includes_unselected_targets_and_automatic_allies() -> Result<()> {
    for side in [Side::Party, Side::Enemy] {
        let mut owner = crate::tests::actor(side);
        owner.control = Control::Auto;
        let mut target = crate::tests::actor(if side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        });
        target.control = Control::Auto;
        target.hp = 975;
        target.equipment.max_hp = 1000;
        target.reaction.combo_hits = 1;
        target.reaction.combo_damage = 25;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let mut other = target.clone();
        other.body.collider = None;
        if side == Side::Enemy {
            other.control = Control::Manual;
        }
        let mut battle = crate::PreparedBattle::new(
            vec![
                (owner, Default::default()),
                (target, Default::default()),
                (other, Default::default()),
            ],
            Default::default(),
            1,
        )?
        .finish()?;
        battle.runtime[0].target = ActorId(2);
        let cues = strike(&mut battle, rule(25))?;
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Combo {
                actor: ActorId(1),
                hits: 2,
                damage: 50
            }
        )));
        assert_eq!(battle.actors[1].hp, 950);
    }
    Ok(())
}

#[test]
fn defeat_commits_credit_once() -> Result<()> {
    for lethal in [false, true] {
        let owner = crate::tests::actor(Side::Party);
        let mut target = crate::tests::actor(Side::Enemy);
        target.hp = if lethal { 1 } else { 1000 };
        target.equipment.max_hp = 1000;
        target.overlimit = crate::OverLimit::new(73)?;
        target.body.collider = Some(crate::Collider::sphere(1.));

        let prepared = crate::PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            Default::default(),
            1,
        )?;
        let mut battle = prepared.finish()?;
        battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(false));
        let hit = rule(100);
        strike(&mut battle, hit)?;
        let victim = &battle.actors[1];
        assert_eq!(victim.available(), !lethal);
        assert_eq!(battle.activity(ActorId(1)) == Activity::Defeated, lethal);
        assert_eq!(victim.overlimit.charge(), if lethal { 0 } else { 73 });
        assert_eq!(battle.ledger.kills[0], u16::from(lethal));

        assert!(!battle.is_diagnostic());
        if lethal {
            assert!(strike(&mut battle, hit)?.is_empty());
            assert_eq!(battle.ledger.kills[0], 1);
        }
    }
    Ok(())
}

#[test]
fn ledger_counts_party_defeats_and_guarded_contacts() -> Result<()> {
    for guarded in [false, true] {
        let (battle, cues) =
            combo_grade_contact(Side::Party, 9, if guarded { 1000 } else { 1 }, guarded)?;
        assert!(battle.ledger().party_was_hit);
        assert_eq!(battle.ledger().kills, [0, 0]);
        assert_eq!(battle.ledger().deaths, [0, u8::from(!guarded)]);
        assert_eq!(battle.ledger().last_party_killer, None);
        assert_eq!(battle.ledger().grade(), if guarded { 0 } else { -110 });
        assert_eq!(
            cues.iter()
                .any(|cue| matches!(cue, Cue::Combo { hits: 10, .. })),
            !guarded
        );
    }
    Ok(())
}

#[test]
fn ordinary_projectile_hits_once_restores_tp_and_retires() -> Result<()> {
    let mut owner = crate::tests::actor(Side::Party);
    owner.control = Control::Manual;
    owner.tp = 1;
    let mut target = crate::tests::actor(Side::Enemy);
    target.control = Control::Manual;
    target.hp = 100;
    target.position = [10., 0., 0.];
    target.body.collider = Some(crate::Collider::sphere(1.));
    let mut battle = crate::tests::prepared(vec![owner, target], 1)
        .finish()
        .unwrap();
    battle.emit(
        Arc::new(ProjectileDefinition {
            lifetime: Some(20),
            velocity: [2., 0., 0.],
            acceleration: [0.; 3],
            offset: [0.; 3],
            clamp_ground: true,
            active: None,
            birth: None,
            motion: Default::default(),
            effects: Default::default(),
            contact: Some(crate::ProjectileContact {
                hit: HitRule {
                    overlimit_pause: false,
                    kind: DamageKind::Slash,
                    arte: false,
                    power: Power::Fixed(8),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    guard: GuardRule::default(),
                    reaction: Default::default(),
                    condition: None,
                },
                cooldown: 20,
                repeat_limit: 0,
                radius: 1.,
                height: 2.,
                shape: HitShape::Sphere,
                offset: [0.; 3],
                radius_growth: 0.,
                height_growth: 0.,
                survives_contact: false,
                clashes: false,
            }),
        }),
        ActionId(1),
        ActorId(0),
        ActorId(1),
        [0.; 3],
    )?;
    let (mut hits, mut expired) = (0, 0);
    for _ in 0..25 {
        let frame = battle.step(crate::BattleInput::default())?;
        for cue in &frame.cues {
            match cue {
                Cue::Hit {
                    actor: ActorId(1),
                    result,
                    ..
                } => {
                    hits += 1;
                    assert_eq!(result.hp_change, -8);
                }
                Cue::ProjectileExpired { .. } => expired += 1,
                _ => {}
            }
        }
    }
    assert_eq!((hits, expired), (1, 1));
    assert_eq!(battle.actors[1].hp, 92);
    assert_eq!(battle.actors[0].tp, 2);
    assert!(battle.projectiles.is_empty());
    assert!(!battle.is_diagnostic());
    Ok(())
}

#[test]
fn companion_hit_suppresses_selected_enemy_recoil_until_the_leaders_own_hit() -> Result<()> {
    let mut actors = [Side::Party, Side::Party, Side::Enemy].map(crate::tests::actor);
    actors[0].control = Control::SemiAuto;
    actors[1].control = Control::Auto;
    actors[0].position = [0., 0., 0.];
    actors[1].position = [0., 0., -100.];
    actors[2].position = [150., 0., 0.];
    actors[2].hp = 1000;
    actors[2].equipment.max_hp = 1000;
    let mut battle = crate::tests::prepared(actors.into(), 1).finish().unwrap();
    battle.runtime[0].target = ActorId(2);
    battle.actors[2].body.collider = Some(crate::Collider::sphere(1.));
    let previous_direction = [0., 0., -1.];
    battle.actors[2].reaction.direction = previous_direction;
    for (owner, expected_speed) in [(ActorId(1), 0.), (ActorId(0), 4.)] {
        let mut contacts = Contacts::default();
        contacts.push(Contact {
            source: ContactSource::Melee {
                actor: owner,
                action: ActionId(1),
            },
            owner,
            origin: Origin::World(battle.actors[2].position),
            radius: 2.,
            height: 2.,
            shape: HitShape::Box,
            rule: HitRule {
                overlimit_pause: true,
                kind: DamageKind::Slash,
                arte: true,
                power: Power::Fixed(25),
                element: HitElement::Neutral,
                prevents_defeat: false,
                reaction: crate::ReactionRule {
                    recoil: crate::RecoilRule {
                        impulse: [4., 0.],
                        suppression_distance: 400.,
                        ..Default::default()
                    },
                    hitstun: 40,
                    ..Default::default()
                },
                guard: Default::default(),
                condition: None,
            },
            clashes: false,
            struck: Vec::new(),
        })?;
        contacts.resolve(&mut battle, &mut vec![])?;
        let target = &battle.actors[2];
        assert_eq!(battle.activity(ActorId(2)), Activity::Hurt);
        assert_eq!(target.movement.forward, expected_speed);
        assert_eq!(target.reaction.recoil.pending, [expected_speed, 0.]);
        crate::tests::assert_vector_close(
            target.reaction.direction,
            if owner == ActorId(1) {
                previous_direction
            } else {
                [1., 0., 0.]
            },
            0.00001,
        );
    }
    assert_eq!(battle.actors[2].hp, 950);
    assert_eq!(battle.actors[2].reaction.combo_hits, 2);
    Ok(())
}

#[test]
fn defeat_retargets_a_surviving_opponent() -> Result<()> {
    let mut actors: Vec<_> = [Side::Party, Side::Enemy, Side::Enemy]
        .into_iter()
        .map(crate::tests::actor)
        .collect();
    actors[0].control = Control::SemiAuto;
    actors[1].hp = 1;
    actors[1].body.collider = Some(crate::Collider::sphere(1.));
    actors[2].position = [300., 0., 0.];
    let mut battle = crate::PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )?
    .finish()?;
    battle.set_decision_target(ActorId(0), ActorId(1))?;
    strike(&mut battle, rule(25))?;
    assert_eq!(battle.activity(ActorId(1)), Activity::Defeated);
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(2)));
    Ok(())
}

#[test]
fn contacts_update_gauge_and_defeat_state() -> Result<()> {
    for eligible in [false, true] {
        for (active, lethal, affinity, armored, guarded, initial, expected) in [
            (false, false, Affinity::Normal, false, false, 73, 88),
            (false, false, Affinity::Absorb, false, false, 73, 73),
            (false, false, Affinity::Immune, false, false, 73, 73),
            (false, false, Affinity::Normal, true, false, 73, 88),
            (false, false, Affinity::Normal, false, true, 73, 73),
            (false, true, Affinity::Normal, false, false, 73, 15),
            (true, true, Affinity::Normal, false, false, 1000, 15),
            (true, false, Affinity::Normal, false, false, 1000, 995),
            (true, false, Affinity::Immune, false, false, 3, 3),
        ] {
            let mut owner = crate::tests::actor(Side::Party);
            owner.tp = 17;
            let mut target = crate::tests::actor(Side::Enemy);
            target.hp = if lethal { 1 } else { 500 };
            target.equipment.max_hp = 1000;
            target.overlimit = if active {
                crate::OverLimit::active(initial).unwrap()
            } else {
                crate::OverLimit::new(initial).unwrap()
            };
            target.equipment.affinities[0] = affinity;
            target.guard.active = guarded;
            target.facing_direction = [0., 0., -1.];
            target.heading = 180.;
            target.guard.break_pressure = 20;
            target.guard.reduction = 75;
            if armored {
                target.reaction.protection.mode = crate::ProtectionMode::Armor;
            }
            target.body.collider = Some(crate::Collider::sphere(1.));
            let mut prepared = crate::PreparedBattle::new(
                vec![(owner, Default::default()), (target, Default::default())],
                Default::default(),
                1,
            )?;
            prepared.resources.actor_setup[1].overlimit_gain = 15;
            let mut battle = prepared.finish()?;
            let mut contacts = Contacts::default();
            contacts.push(Contact {
                source: ContactSource::Melee {
                    actor: ActorId(0),
                    action: ActionId(1),
                },
                owner: ActorId(0),
                origin: Origin::World([0.; 3]),
                radius: 2.,
                height: 2.,
                shape: HitShape::Sphere,
                rule: HitRule {
                    kind: DamageKind::Slash,
                    arte: true,
                    overlimit_pause: eligible,
                    power: Power::Fixed(100),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    reaction: Default::default(),
                    guard: Default::default(),
                    condition: None,
                },
                clashes: false,
                struck: Vec::new(),
            })?;
            let mut cues = vec![];
            contacts.resolve(&mut battle, &mut cues)?;
            if active && !lethal {
                assert_eq!(battle.actors[1].overlimit.remaining(), expected);
            } else {
                assert_eq!(battle.actors[1].overlimit.charge(), expected);
            }
            assert_eq!(
                battle.actors[1].overlimit.is_active(),
                active && !lethal && expected > 0
            );
            assert_eq!(battle.actors[1].available(), !lethal);
            assert_eq!(battle.ledger.kills[0], u16::from(lethal));
            assert_eq!(battle.actors[0].tp, 17); // Arte excludes TP but not tension.
            assert_eq!(battle.is_paused(), active && eligible);
            if lethal {
                assert_eq!(battle.activity(ActorId(1)), Activity::Defeated);
            }
        }
    }
    Ok(())
}

#[test]
fn ailment_ward_reports_feedback_without_erasing_critical_damage() -> Result<()> {
    let mut owner = crate::tests::actor(Side::Enemy);
    owner.equipment.damage.critical_chance_bonus = 100;
    owner.equipment.stats.slash = 100;
    let mut target = crate::tests::actor(Side::Party);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.conditions =
        crate::conditions::Conditions::default().with_traits(crate::conditions::Traits {
            magical_ailment_guard: true,
            ..Default::default()
        });
    target.position = [10., 20., 30.];
    target.body.center_offset = [1., 2., 3.];
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = crate::tests::prepared(vec![owner, target], 0)
        .finish()
        .unwrap();
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Melee {
            actor: ActorId(0),
            action: ActionId(1),
        },
        owner: ActorId(0),
        origin: Origin::World(battle.actors[1].position),
        radius: 20.,
        height: 20.,
        shape: HitShape::Box,
        rule: HitRule {
            overlimit_pause: true,
            kind: DamageKind::Slash,
            arte: true,
            power: Power::Normal,
            element: HitElement::Neutral,
            prevents_defeat: false,
            reaction: Default::default(),
            guard: GuardRule::default(),
            condition: Some(crate::HitCondition {
                condition: resonance_content::battle_action::Condition::Weak,
                chance: 100,
                value: 0,
            }),
        },
        clashes: false,
        struck: Vec::new(),
    })?;
    let mut cues = Vec::new();
    contacts.resolve(&mut battle, &mut cues)?;
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::ConditionLabel {
            actor: ActorId(1),
            kind: crate::conditions::ConditionLabel::ExSkillEffect,
            position: [12., 24., 36.],
        }
    )));
    assert!(
        cues.iter().any(
            |cue| matches!(cue, Cue::Hit { actor: ActorId(1), result, .. } if result.critical)
        )
    );
    assert_eq!(battle.actors[1].conditions.base(), ConditionSet::EMPTY);
    Ok(())
}

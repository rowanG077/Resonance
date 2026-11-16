use super::*;
use crate::{Affinity, BattleInput, GuardResult, HitProtection, tests::actor};

fn battle() -> Battle {
    crate::tests::prepare_actors(vec![actor(Side::Party), actor(Side::Enemy)], 17)
        .finish()
        .unwrap()
}

fn damage() -> HitResult {
    HitResult {
        amount: 12,
        hp_change: -12,
        critical: false,
        boosted: false,
        affinity: Affinity::Normal,
        guard: GuardResult::None,
        protection: HitProtection::None,
        reaction_suppressed: false,
        reaction_forced: false,
    }
}

fn contact(battle: &mut Battle, vector: [u16; 3], hit: HitResult) -> Vec<Cue> {
    let rule = HitRule {
        fatal_damage: Some(vector),
        ..crate::tests::hit()
    };
    let mut cues = Vec::new();
    battle.record_fatal_contact(ActorId(0), ActorId(1), rule, hit, &mut cues);
    cues
}

#[test]
fn optional_system_keeps_default_combat_and_random_state_unchanged() -> Result<()> {
    let mut battle = battle();
    let seed = battle.random_state();
    assert!(contact(&mut battle, [u16::MAX; 3], damage()).is_empty());
    assert!(battle.snapshot().fatal_strikes.is_none());
    assert_eq!(battle.random_state(), seed);
    battle.set_fatal_strikes(true);
    assert!(battle.snapshot().fatal_strikes.is_some());
    battle.set_fatal_strikes(false);
    assert!(battle.snapshot().fatal_strikes.is_none());
    Ok(())
}

#[test]
fn hit_vectors_break_independent_gauges_with_deterministic_color_priority() -> Result<()> {
    let mut battle = battle();
    battle.set_fatal_strikes(true);
    battle.configure_fatal_enemy(
        ActorId(1),
        FatalEnemyRules {
            capacity: [10, 20, 30],
            ..Default::default()
        },
    )?;
    assert!(contact(&mut battle, [0, 15, 25], damage()).is_empty());
    let frame = battle.snapshot().fatal_strikes.unwrap();
    assert_eq!(frame.targets[0].remaining, [10, 5, 5]);
    let cues = contact(&mut battle, [10, 5, 5], damage());
    assert!(
        matches!(cues.as_slice(), [Cue::FatalOpening { actor: ActorId(1), opening }]
        if opening.color == FatalColor::Blue && opening.chain == 1)
    );
    assert!(contact(&mut battle, [u16::MAX; 3], damage()).is_empty());
    Ok(())
}

#[test]
fn non_damage_and_lethal_contacts_cannot_open_a_finisher() -> Result<()> {
    let mut battle = battle();
    battle.set_fatal_strikes(true);
    for hit in [
        HitResult {
            amount: 0,
            hp_change: 0,
            ..damage()
        },
        HitResult {
            affinity: Affinity::Absorb,
            hp_change: 12,
            ..damage()
        },
        HitResult {
            affinity: Affinity::Immune,
            hp_change: 0,
            ..damage()
        },
        HitResult {
            protection: HitProtection::Avoided,
            hp_change: 0,
            ..damage()
        },
    ] {
        assert!(contact(&mut battle, [u16::MAX; 3], hit).is_empty());
    }
    assert_eq!(
        battle.snapshot().fatal_strikes.unwrap().targets[0].remaining,
        [90; 3]
    );
    battle.actors[1].hp = 0;
    assert!(contact(&mut battle, [u16::MAX; 3], damage()).is_empty());
    Ok(())
}

#[test]
fn guard_and_armor_mitigate_gauge_damage_without_random_draws() -> Result<()> {
    let mut battle = battle();
    battle.set_fatal_strikes(true);
    contact(
        &mut battle,
        [20; 3],
        HitResult {
            guard: GuardResult::Blocked {
                first: true,
                special: false,
            },
            ..damage()
        },
    );
    assert_eq!(
        battle.snapshot().fatal_strikes.unwrap().targets[0].remaining,
        [85; 3]
    );
    contact(
        &mut battle,
        [20; 3],
        HitResult {
            protection: HitProtection::Armored,
            ..damage()
        },
    );
    assert_eq!(
        battle.snapshot().fatal_strikes.unwrap().targets[0].remaining,
        [75; 3]
    );
    Ok(())
}

#[test]
fn expired_openings_allow_chains_and_menus_freeze_the_window() -> Result<()> {
    let mut battle = battle();
    battle.set_fatal_strikes(true);
    contact(&mut battle, [90, 0, 0], damage());
    let opening = battle.snapshot().fatal_strikes.unwrap().targets[0]
        .opening
        .unwrap();
    for _ in 0..120 {
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
    }
    assert_eq!(
        battle.snapshot().fatal_strikes.unwrap().targets[0].opening,
        Some(opening)
    );
    for _ in 0..OPENING_TICKS {
        battle.advance_fatal_gauges();
    }
    assert!(
        battle.snapshot().fatal_strikes.unwrap().targets[0]
            .opening
            .is_none()
    );
    let cues = contact(&mut battle, [0, 90, 0], damage());
    assert!(
        matches!(cues.as_slice(), [Cue::FatalOpening { opening, .. }]
        if opening.color == FatalColor::Red && opening.chain == 2 && opening.generation > 1)
    );
    for _ in 0..OPENING_TICKS + CHAIN_TICKS {
        battle.advance_fatal_gauges();
    }
    assert_eq!(battle.snapshot().fatal_strikes.unwrap().targets[0].chain, 0);
    Ok(())
}

#[test]
fn authored_enemy_rules_are_validated_and_disabled_enemies_never_open() -> Result<()> {
    let mut battle = battle();
    battle.set_fatal_strikes(true);
    assert!(
        battle
            .configure_fatal_enemy(ActorId(0), Default::default())
            .is_err()
    );
    assert!(
        battle
            .configure_fatal_enemy(
                ActorId(1),
                FatalEnemyRules {
                    capacity: [0; 3],
                    ..Default::default()
                }
            )
            .is_err()
    );
    battle.configure_fatal_enemy(
        ActorId(1),
        FatalEnemyRules {
            outcome: FatalOutcome::Disabled,
            ..Default::default()
        },
    )?;
    assert!(contact(&mut battle, [u16::MAX; 3], damage()).is_empty());
    assert!(battle.snapshot().fatal_strikes.unwrap().targets.is_empty());
    Ok(())
}

fn performance() -> FatalPerformance {
    FatalPerformance {
        style: FatalStyle::TwinBlades,
        emitter: None,
        poses: Vec::new(),
        rates: Vec::new(),
        impact_at: 56,
        duration: 112,
        reach: 60.,
        contact: None,
        visual_height: None,
        voice: None,
        charge_sound: None,
        impact_kind: crate::DamageKind::Slash,
        impact_sound: None,
        impact_effect: None,
    }
}

fn cinematic(outcome: FatalOutcome, color: FatalColor) -> Result<Battle> {
    let mut battle = battle();
    battle.actors[1].hp = 1000;
    battle.actors[1].equipment.max_hp = 1000;
    battle.actors[0].position = [-200., 0., 0.];
    battle.actors[1].position = [200., 0., 0.];
    battle.set_fatal_strikes(true);
    battle.configure_fatal_enemy(
        ActorId(1),
        FatalEnemyRules {
            outcome,
            ..Default::default()
        },
    )?;
    battle.configure_fatal_performances(ActorId(0), std::array::from_fn(|_| performance()))?;
    let mut vector = [0; 3];
    vector[color.index()] = 90;
    contact(&mut battle, vector, damage());
    Ok(battle)
}

#[test]
fn finisher_claim_is_exclusive_and_impacts_only_at_the_authored_frame() -> Result<()> {
    let mut battle = cinematic(FatalOutcome::Defeat, FatalColor::Green)?;
    assert!(battle.request_fatal_strike(ActorId(0))?);
    assert!(!battle.request_fatal_strike(ActorId(0))?);
    assert!(!battle.command_admission_allowed());
    for _ in 0..56 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.actors[1].hp, 1000);
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[1].hp, 0);
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::FatalImpact(success) if success.defeated))
    );
    assert_eq!(battle.ledger.kills[0], 1);
    assert_eq!(battle.ledger.fatal_strikes.len(), 1);
    assert_eq!(
        battle.recognize_update(),
        None,
        "victory waits for the recovery shot"
    );
    for _ in 0..56 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.ledger.kills[0], 1);
    assert_eq!(battle.ledger.fatal_strikes.len(), 1);
    assert_eq!(
        battle.recognize_update(),
        Some(crate::BattleResult::Victory)
    );
    Ok(())
}

#[test]
fn authored_contact_placement_respects_performer_scale_and_heading() -> Result<()> {
    for direction in [[1., 0.], [-1., 0.], [0., 1.], [0., -1.], [0.6, 0.8]] {
        let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Green)?;
        let contact = [12., -95., 42.];
        battle.actors[0].position = [-direction[0] * 200., 0., -direction[1] * 200.];
        battle.actors[0].body.scale = 1.5;
        battle.actors[1].position = [direction[0] * 200., 0., direction[1] * 200.];
        battle.actors[1].body.collider = Some(crate::Collider::standing(35., 160.));
        battle.configure_fatal_performances(
            ActorId(0),
            std::array::from_fn(|_| FatalPerformance {
                contact: Some(contact),
                ..performance()
            }),
        )?;
        assert!(battle.request_fatal_strike(ActorId(0))?);
        for _ in 0..=56 {
            battle.step(BattleInput::default())?;
        }
        let actor = &battle.actors[0];
        let target = battle.actors[1].target_center();
        let [x, y, z] = contact.map(|v| v * actor.body.scale);
        let [dx, _, dz] = actor.facing_direction;
        let point = [
            actor.position[0] + dz * x - dx * y,
            actor.position[1] + z,
            actor.position[2] - dx * x - dz * y,
        ];
        assert_eq!(
            actor.position[1], 0.,
            "a grounded contact must not levitate its performer"
        );
        assert!((0. ..=160.).contains(&point[1]));
        for axis in [0, 2] {
            assert!(
                (point[axis] - target[axis]).abs() < 0.01,
                "{direction:?}: {point:?} vs {target:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn boss_finisher_deals_damage_and_preserves_scripted_health_protection() -> Result<()> {
    for hold in [false, true] {
        let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Red)?;
        if hold {
            battle.actors[1].hp = 1;
            battle.hold_event_boss_hp(Some(ActorId(1)))?;
        }
        let before = battle.actors[1].hp;
        assert!(battle.request_fatal_strike(ActorId(0))?);
        for _ in 0..57 {
            battle.step(BattleInput::default())?;
        }
        assert!(battle.actors[1].hp > 0);
        if hold {
            assert_eq!(battle.actors[1].hp, before);
        } else {
            assert!(battle.actors[1].hp < before);
        }
        assert_eq!(battle.ledger.kills[0], 0);
        assert!(!battle.ledger.fatal_strikes[0].defeated);
    }
    Ok(())
}

#[test]
fn cinematic_camera_is_finite_and_menus_pause_choreography_and_impact() -> Result<()> {
    for color in FatalColor::ALL {
        let mut battle = cinematic(FatalOutcome::Damage, color)?;
        assert!(battle.request_fatal_strike(ActorId(0))?);
        for age in 0..112 {
            let frame = battle.step(BattleInput::default())?;
            let camera = frame.camera.unwrap();
            assert!(
                camera
                    .eye
                    .into_iter()
                    .chain(camera.focus)
                    .all(f32::is_finite)
            );
            assert!(camera.radius > 0.);
            assert!((13. ..=35.).contains(&frame.camera_fov_degrees.unwrap()));
            if age == 40 {
                for _ in 0..120 {
                    battle.step(BattleInput {
                        paused: true,
                        ..Default::default()
                    })?;
                }
                assert_eq!(
                    battle.snapshot().fatal_strikes.unwrap().scene,
                    frame.fatal_strikes.unwrap().scene
                );
                assert_eq!(battle.ledger.fatal_strikes.len(), 0);
            }
        }
        assert_eq!(battle.ledger.fatal_strikes.len(), 1);
    }
    Ok(())
}

#[test]
fn unavailable_performer_and_missing_opening_reject_without_consuming_the_window() -> Result<()> {
    let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Blue)?;
    let opening = battle.snapshot().fatal_strikes.unwrap().targets[0].opening;
    battle.actors[0].time_stop = 30;
    assert!(!battle.request_fatal_strike(ActorId(0))?);
    assert_eq!(
        battle.snapshot().fatal_strikes.unwrap().targets[0].opening,
        opening
    );
    battle.actors[0].time_stop = 0;
    assert!(battle.request_fatal_strike(ActorId(0))?);
    battle.set_fatal_strikes(false);
    assert!(!battle.fatal_strikes.active());
    assert!(!battle.request_fatal_strike(ActorId(0))?);
    assert!(battle.ledger.fatal_strikes.is_empty());
    Ok(())
}

#[test]
fn cancellation_lands_the_performer_and_releases_all_held_actors() -> Result<()> {
    let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Red)?;
    battle.configure_fatal_performances(
        ActorId(0),
        std::array::from_fn(|_| FatalPerformance {
            style: FatalStyle::Martial,
            ..performance()
        }),
    )?;
    battle.actors[0].position[1] = 160.;
    assert!(battle.request_fatal_strike(ActorId(0))?);
    for _ in 0..42 {
        battle.step(BattleInput::default())?;
    }
    assert!(battle.actors[0].position[1] > 0.);
    assert!(battle.snapshot().paused_actors.contains(&ActorId(1)));
    battle.set_fatal_strikes(false);
    assert_eq!(battle.actors[0].position[1], 0.);
    assert!(battle.snapshot().paused_actors.is_empty());
    assert_eq!(battle.actors[1].hp, 1000);
    assert!(battle.ledger.fatal_strikes.is_empty());
    Ok(())
}

#[test]
fn aerial_target_finisher_lands_before_the_idle_handoff() -> Result<()> {
    let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Blue)?;
    battle.actors[1].position[1] = 80.;
    assert!(battle.request_fatal_strike(ActorId(0))?);
    for age in 0..=110 {
        battle.step(BattleInput::default())?;
        if age == 56 {
            assert!((battle.actors[0].position[1] - 80.).abs() < 0.01);
        }
    }
    assert!(battle.fatal_strikes.active());
    assert_eq!(battle.actors[0].position[1], 0.);
    Ok(())
}

#[test]
fn cinematic_holds_the_impact_then_releases_the_victim_and_gameplay_random_waits() -> Result<()> {
    let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Blue)?;
    assert!(battle.request_fatal_strike(ActorId(0))?);
    let random = battle.random_state();
    for age in 0..=60 {
        let frame = battle.step(BattleInput::default())?;
        assert!(!frame.paused_actors.contains(&ActorId(0)));
        assert_eq!(frame.paused_actors.contains(&ActorId(1)), age < 60);
        if age < 56 {
            assert_eq!(battle.random_state(), random);
        }
    }
    let after_contact = battle.random_state();
    for _ in 0..51 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.random_state(), after_contact);
    Ok(())
}

#[test]
fn camera_expands_for_large_targets_and_moves_continuously_between_shots() -> Result<()> {
    let mut battle = cinematic(FatalOutcome::Damage, FatalColor::Green)?;
    battle.actors[1].body.collider = Some(crate::Collider::standing(140., 720.));
    assert!(battle.request_fatal_strike(ActorId(0))?);
    let mut previous = None;
    for age in 0..112 {
        let frame = battle.step(BattleInput::default())?;
        let camera = frame.camera.unwrap();
        if let Some(previous) = previous {
            let delta = glam::Vec3::from_array(camera.eye) - glam::Vec3::from_array(previous);
            assert!(delta.length() < 250., "camera jumped at frame {age}");
        }
        if age == 56 {
            assert!(camera.radius > 1500.);
        }
        previous = Some(camera.eye);
    }
    Ok(())
}

#[test]
fn companion_waits_for_humans_and_only_claims_an_unselected_opening() -> Result<()> {
    let mut human = actor(Side::Party);
    human.control = crate::Control::Manual;
    let mut companion = actor(Side::Party);
    companion.control = crate::Control::Auto;
    let mut battle =
        crate::tests::prepare_actors(vec![human, companion, actor(Side::Enemy)], 17).finish()?;
    battle.set_fatal_strikes(true);
    battle.set_decision_target(ActorId(0), ActorId(2))?;
    battle.set_decision_target(ActorId(1), ActorId(2))?;
    battle.configure_fatal_performances(ActorId(1), std::array::from_fn(|_| performance()))?;
    let rule = HitRule {
        fatal_damage: Some([90, 0, 0]),
        ..crate::tests::hit()
    };
    battle.record_fatal_contact(ActorId(0), ActorId(2), rule, damage(), &mut Vec::new());
    for _ in 0..46 {
        battle.advance_fatal_gauges();
    }
    battle.admit_companion_fatal_strike()?;
    assert!(
        !battle.fatal_strikes.active(),
        "companion stole the human's selected opening"
    );
    battle.actors[0].hp = 0;
    battle.actors[0].availability = crate::ActorAvailability::Dead;
    battle.admit_companion_fatal_strike()?;
    assert!(battle.fatal_strikes.active());
    Ok(())
}

#[test]
fn caster_finishers_keep_their_distance_and_never_emit_melee_trails() -> Result<()> {
    for style in [FatalStyle::Arcane, FatalStyle::Sacred] {
        for color in FatalColor::ALL {
            let mut battle = cinematic(FatalOutcome::Damage, color)?;
            battle.actors[0].position = [-1600., 0., 0.];
            let origin = battle.actors[0].position;
            battle.configure_fatal_performances(
                ActorId(0),
                std::array::from_fn(|_| FatalPerformance {
                    style,
                    impact_kind: crate::DamageKind::Magic,
                    ..performance()
                }),
            )?;
            assert!(battle.request_fatal_strike(ActorId(0))?);
            for age in 0..113 {
                let frame = battle.step(BattleInput::default())?;
                assert_eq!(
                    battle.actors[0].position, origin,
                    "caster slid toward the victim at {age}"
                );
                assert!(
                    !frame
                        .cues
                        .iter()
                        .any(|cue| matches!(cue, Cue::WeaponTrail { .. }))
                );
                assert_eq!(battle.ledger.fatal_strikes.len(), usize::from(age >= 56));
            }
        }
    }
    Ok(())
}

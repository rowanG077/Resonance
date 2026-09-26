use super::*;
use crate::{
    ActionDefinition, ActionPhase, Battle, BattleInput, ContactSource, Cue, DamageKind, HitElement,
    ModelDefinition, Playback, Power, PreparedBattle, ResourceBinding, Side, WeaponDefinition,
    WeaponPlayback,
};
use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
use std::collections::BTreeMap;

fn definition(slot: u8, outbound_ticks: u16, direction_y: f32) -> Arc<WeaponFlightDefinition> {
    Arc::new(WeaponFlightDefinition {
        slot,
        outbound_ticks,
        speed: 25.,
        return_speed: 25.,
        direction_y,
        hit: HitRule {
            kind: DamageKind::Slash,
            arte: false,
            power: Power::Fixed(10),
            element: HitElement::Neutral,
            prevents_defeat: false,
            reaction: Default::default(),
            guard: Default::default(),
            impact: None,
        },
        cooldown: 3,
        radius: 40.,
        height: 40.,
        shape: HitShape::Sphere,
    })
}

fn model() -> Arc<ModelDefinition> {
    let skeleton = Skeleton {
        bones: vec![Bone {
            name: "hand".into(),
            parent: None,
            bind_channels: TransformChannels(8),
            bind: Transform {
                translation: [0., 0., 50.],
                ..Default::default()
            },
        }],
    };
    Arc::new(ModelDefinition {
        resource: 1,
        skeleton: skeleton.clone(),
        motions: BTreeMap::from([(
            0,
            Motion {
                duration_frames: 20.,
                tracks: vec![],
            },
        )]),
        secondary_motion: vec![],
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        anchors: vec![crate::Anchor {
            bone: 0,
            offset: [0.; 3],
        }],
        weapons: (0..2)
            .map(|slot| {
                Arc::new(WeaponDefinition {
                    slot,
                    resource: 10 + u32::from(slot),
                    attachment: 0,
                    skeleton: skeleton.clone(),
                    motions: BTreeMap::new(),
                    playback: WeaponPlayback::Rigid,
                    anchors: vec![crate::Anchor {
                        bone: 0,
                        offset: [0.; 3],
                    }],
                    links: vec![],
                })
            })
            .collect(),
        hurt_bones: vec![],
        target_bones: vec![0],
        approach_bones: vec![],
        shadow: None,
        target_marker: None,
        suppress_root_translation: [false; 3],
    })
}

fn battle(script: &str, definition: Arc<WeaponFlightDefinition>) -> Battle {
    battle_with_leader(script, definition, false)
}

fn battle_with_leader(
    script: &str,
    definition: Arc<WeaponFlightDefinition>,
    with_leader: bool,
) -> Battle {
    let compiled = symphonia_script_compiler::compile(
        "flight", &BTreeMap::from([("flight".into(), format!("script battle; use battle; asset flight: battle::WeaponFlight = \"flight\"; {script}"))]),
        &crate::native_declarations(),
    ).unwrap();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == "flight::run")
        .unwrap()
        .entry;
    let mut owner = crate::tests::actor(Side::Party);
    owner.movement.direction = [0., 0., 1.];
    owner.facing_direction = owner.movement.direction;
    let mut target = crate::tests::actor(Side::Enemy);
    target.position = [0., 0., 500.];
    target.body.target_center = target.position;
    let mut actors = vec![owner, target];
    if with_leader {
        actors[0].control = crate::Control::Auto;
        let mut leader = crate::tests::actor(Side::Party);
        leader.control = crate::Control::SemiAuto;
        leader.position = [1000., 0., 0.];
        actors.push(leader);
    }
    let mut models = vec![Some(model()), None];
    if with_leader {
        models.push(None);
    }
    let prepared = PreparedBattle::new(
        actors,
        vec![ActionDefinition {
            id: 1,
            phase: ActionPhase::Actor,
            program: Arc::new(compiled.program),
            entry,
            duration: 200,
            tp_cost: 0,
            resources: vec![ResourceBinding::WeaponFlight(definition)],
        }],
        17,
        models,
        vec![],
    )
    .unwrap();
    let prepared = prepared
        .with_controls(vec![crate::ControlDefinition {
            actor: ActorId(0),
            target: ActorId(1),
            normals: [crate::NormalControl {
                action: 1,
                allowed_directions: 0,
                fallback: None,
                reach: 600.,
                minimum_reach: 0.,
                combo_at: [0; 2],
                buffer_until: 200,
            }; 7],
            shortcuts: [None; 4],
            combo_limit: 1,
            walk_speed: 5.,
            run_speed: 10.,
            turn_ticks: 8,
            motions: None,
        }])
        .unwrap();
    Battle::new(Arc::new(prepared))
}

fn start() -> BattleInput {
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
    }
}

#[test]
fn original_profiles_hold_the_outbound_direction_then_seek_the_live_hand() -> Result<()> {
    for (ticks, y) in [(14, 0.), (10, -0.7), (8, 0.4)] {
        let mut flight = Flight::new(
            definition(0, ticks, y),
            ActionId(1),
            [0., 300., 0.],
            90.,
            [1., 0., 0.],
        );
        let initial = flight.direction;
        for remaining in (0..ticks).rev() {
            flight.step([900., 500., 900.], Some([0.; 3]), None)?;
            assert_eq!(flight.direction, initial);
            assert_eq!(flight.remaining, remaining);
            assert!(!flight.caught);
        }
        flight.step([-300., 500., -100.], Some([0.; 3]), None)?;
        assert_ne!(flight.direction, initial);
        let mut caught = false;
        for _ in 0..300 {
            flight.step([-300., 500., -100.], Some([0.; 3]), None)?;
            if flight.caught {
                caught = true;
                break;
            }
        }
        assert!(caught, "source profile {ticks}/{y}");
    }
    Ok(())
}

#[test]
fn unproved_steering_rejects_before_any_flight_mutation() {
    for remaining in [0, 1] {
        let mut flight = Flight::new(
            definition(0, remaining, 0.),
            ActionId(7),
            [0., 50., 0.],
            90.,
            [1., 0., 0.],
        );
        flight.cooldowns[2] = 3;
        let resource = Arc::clone(&flight.definition);
        let before = (
            flight.position,
            flight.direction,
            flight.angles,
            flight.remaining,
            flight.speed,
            flight.caught,
            flight.cooldowns,
        );
        // The return point coincides with the flight. The outbound case has
        // no proved callback operand. Neither rejection may spend cooldowns.
        let error = flight.step([-0.1, 50., -0.1], None, None).unwrap_err();
        assert!(error.to_string().contains(if remaining == 0 {
            "return steering below 0.1 is unproved"
        } else {
            "catch operand is unproved"
        }));
        assert_eq!(
            (
                flight.position,
                flight.direction,
                flight.angles,
                flight.remaining,
                flight.speed,
                flight.caught,
                flight.cooldowns,
            ),
            before
        );
        assert_eq!(flight.action, ActionId(7));
        assert!(Arc::ptr_eq(&flight.definition, &resource));
    }
}

#[test]
fn tolerant_unproved_return_retires_one_slot_and_advances_the_other() -> Result<()> {
    let mut battle = battle("pub task run() {}", definition(0, 0, 0.));
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
    battle.set_diagnostics(diagnostics.clone());
    put_return_at_sampled_hand(&mut battle)?;
    battle.throw_weapon(ActorId(0), ActionId(2), definition(1, 0, 0.))?;
    let world = battle.weapon_flights[&(ActorId(0), 0)].world();
    battle.models[0]
        .as_mut()
        .unwrap()
        .detach_weapon(0, Some(world), &mut battle.actors[0])?;
    assert!(!battle.models[0].as_ref().unwrap().anchor_attached(1));
    let good = battle.weapon_flights.get_mut(&(ActorId(0), 1)).unwrap();
    good.position[0] += 500.;
    let before = good.position;
    let mut contacts = crate::contact::Contacts::default();
    battle.advance_weapon_flights(ActorId(0), None, None, &mut contacts)?;
    assert!(!battle.weapon_flights.contains_key(&(ActorId(0), 0)));
    assert_ne!(battle.weapon_flights[&(ActorId(0), 1)].position, before);
    let model = battle.models[0].as_ref().unwrap();
    assert!(model.anchor_attached(1));
    assert!(!model.anchor_attached(2));
    assert_eq!(battle.trail_timers[0][0], 0);
    assert_eq!(battle.trail_timers[0][1], 30);
    assert_eq!(diagnostics.entries().len(), 1);
    assert!(
        diagnostics.entries()[0]
            .message
            .contains("return steering below 0.1 is unproved")
    );
    assert!(battle.is_diagnostic());
    assert_eq!(contacts.0[0].len(), 1);
    assert!(matches!(
        contacts.0[0][0].source,
        ContactSource::Weapon { slot: 1, .. }
    ));
    Ok(())
}

#[test]
fn return_admission_uses_the_inclusive_sdk_distance_threshold() {
    // The SDK estimate is not monotonic at adjacent input floats: exactly
    // 0.1 measures one ULP below the threshold; the preceding input measures
    // exactly at it. Preserve the original length/compare order.
    for (position_bits, distance_bits, admitted) in [
        (0x3dcc_cccc, 0x3dcc_cccd, true),
        (0x3dcc_cccd, 0x3dcc_cccc, false),
        (0x3dcc_ccce, 0x3dcc_ccce, true),
    ] {
        let position = [f32::from_bits(position_bits), 50., 0.];
        assert_eq!(
            crate::distance::length([-position[0], 0., 0.]).to_bits(),
            distance_bits
        );
        let mut flight = Flight::new(
            definition(0, 0, 0.),
            ActionId(1),
            position,
            0.,
            [0., 0., 1.],
        );
        flight.cooldowns[1] = 2;
        assert_eq!(flight.step([-0.1, 50., -0.1], None, None).is_ok(), admitted);
        assert_eq!(flight.cooldowns[1], if admitted { 1 } else { 2 });
        assert_eq!(flight.position == position, !admitted);
    }
}

#[test]
fn launch_snapshots_cached_facing_independently_of_heading_and_movement() -> Result<()> {
    let mut battle = battle(
        "pub task run() { await battle::wait_ticks(ticks(100)); }",
        definition(0, 14, 0.),
    );
    battle.actors[0].heading = 90.;
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].facing_direction = [0., 7., -1.];
    battle.throw_weapon(ActorId(0), ActionId(1), definition(0, 14, 0.))?;
    let flight = &battle.weapon_flights[&(ActorId(0), 0)];
    assert_eq!(flight.angles, [-60., 105., 0.]);
    assert_eq!(flight.direction[0], 0.);
    assert_eq!(flight.direction[1], 0.);
    assert!((flight.direction[2] + 1.).abs() < 0.0000002);
    let direction = flight.direction;
    battle.actors[0].facing_direction = [1., 0., 0.];
    battle.advance_weapon_flights(ActorId(0), Some([0.; 3]), None, &mut Default::default())?;
    assert_eq!(battle.weapon_flights[&(ActorId(0), 0)].direction, direction);
    Ok(())
}

#[test]
fn launch_direction_matches_observed_cached_facing_and_source_rounding() -> Result<()> {
    let row: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/weapon-flight-launch.json"
    ))?;
    let vector = |key: &str| -> [f32; 3] {
        std::array::from_fn(|i| f32::from_bits(row[key][i].as_u64().unwrap() as u32))
    };
    let heading = f32::from_bits(row["heading_bits"].as_u64().unwrap() as u32);
    let forward = vector("forward_bits");
    // Independent source observation also checks the cached-facing writer's
    // source degree factor and f32 -> f64 -> f32 trigonometry on this heading.
    assert_eq!(
        crate::control::direction_from_heading(heading).map(f32::to_bits),
        forward.map(f32::to_bits),
    );
    let flight = Flight::new(
        definition(1, 10, -0.7),
        ActionId(1),
        vector("position_bits"),
        heading,
        forward,
    );
    assert_eq!(
        flight.direction.map(f32::to_bits),
        vector("direction_bits").map(f32::to_bits),
    );
    Ok(())
}

#[test]
fn launch_visits_move_immediately_and_continue_through_interruption_and_owner_hit_stop()
-> Result<()> {
    let mut battle = battle(
        "pub task run() { await battle::throw_weapon(flight,ticks(2)); await battle::wait_ticks(ticks(100)); }",
        definition(0, 14, 0.),
    );
    battle.step(start())?;
    // Player selection installs the approach; its initializer is next visit.
    let seed = battle.random.0;
    battle.step(Default::default())?;
    battle.step(Default::default())?;
    assert!(battle.weapon_flights.is_empty());
    let carried = battle.models[0].as_ref().unwrap().weapon_frames()[0].world;
    let frame = battle.step(Default::default())?;
    let flight = &battle.weapon_flights[&(ActorId(0), 0)];
    assert_eq!(flight.remaining, 13);
    assert!((flight.position[2] - 25.).abs() < 0.001);
    assert_eq!(frame.weapons[0].world, carried);
    assert_ne!(frame.weapons[0].world, flight.world());
    let flight_world = flight.world();
    let action = flight.action;
    battle.actors[0].hit_stop = 2;
    let position = flight.position;
    let age = battle.sequences[&action].age;
    let frame = battle.step(Default::default())?;
    assert_eq!(frame.weapons[0].world, flight_world);
    assert_eq!(battle.sequences[&action].age, age);
    assert!(battle.weapon_flights[&(ActorId(0), 0)].position[2] > position[2]);
    let position = battle.weapon_flights[&(ActorId(0), 0)].position;
    battle.actors[0].activity = crate::Activity::Hurt;
    battle.actors[0].reaction.remaining = 20;
    battle.step(Default::default())?;
    assert!(battle.weapon_flights[&(ActorId(0), 0)].position[2] > position[2]);
    let position = battle.weapon_flights[&(ActorId(0), 0)].position;
    battle.step(Default::default())?;
    assert!(battle.weapon_flights[&(ActorId(0), 0)].position[2] > position[2]);
    assert_eq!(battle.random.0, seed);
    Ok(())
}

#[test]
fn catch_restores_hand_on_its_final_contact_and_retires_next_dispatch() -> Result<()> {
    let mut battle = battle(
        "pub task run() { await battle::throw_weapon(flight,ticks(0)); }",
        definition(0, 0, 0.),
    );
    battle.step(start())?;
    battle.step(Default::default())?;
    let hand = battle.models[0].as_ref().unwrap().weapon_attachment(0)?;
    // A moving hand comes within one integration step on the return visit.
    let flight = battle.weapon_flights.get_mut(&(ActorId(0), 0)).unwrap();
    flight.position = [hand[0], hand[1], hand[2] - 25.];
    flight.direction = [0., 0., 1.];
    flight.caught = false;
    let drawn = flight.world();
    battle.models[0]
        .as_mut()
        .unwrap()
        .detach_weapon(0, Some(drawn), &mut battle.actors[0])?;
    battle.actors[1].body.points = vec![crate::HurtPoint {
        center: hand,
        radius: 1.,
    }];
    let frame = battle.step(Default::default())?;
    assert!(battle.weapon_flights[&(ActorId(0), 0)].caught);
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Hit {
            source: ContactSource::Weapon { slot: 0, .. },
            ..
        }
    )));
    assert_eq!(frame.weapons[0].world, drawn);
    assert!(battle.models[0].as_ref().unwrap().anchor_attached(1));
    let frame = battle.step(Default::default())?;
    assert!(battle.weapon_flights.is_empty());
    assert_eq!(frame.weapons[0].world[3][..3], hand);
    Ok(())
}

#[test]
fn slots_keep_independent_cooldowns_and_an_occupied_slot_keeps_its_launch() -> Result<()> {
    let mut battle = battle(
        "pub task run() { await battle::throw_weapon(flight,ticks(0)); }",
        definition(0, 14, 0.),
    );
    battle.step(start())?;
    battle.step(Default::default())?;
    let original = battle.weapon_flights[&(ActorId(0), 0)].position;
    battle.throw_weapon(ActorId(0), ActionId(99), definition(0, 8, 0.4))?;
    assert_eq!(battle.weapon_flights[&(ActorId(0), 0)].position, original);
    assert_ne!(battle.weapon_flights[&(ActorId(0), 0)].action, ActionId(99));
    battle.throw_weapon(ActorId(0), ActionId(2), definition(1, 10, -0.7))?;
    battle
        .weapon_flights
        .get_mut(&(ActorId(0), 0))
        .unwrap()
        .hit(ActorId(1));
    assert!(!battle.weapon_flights[&(ActorId(0), 0)].can_hit(ActorId(1)));
    assert!(battle.weapon_flights[&(ActorId(0), 1)].can_hit(ActorId(1)));
    assert!(battle.weapon_flights[&(ActorId(0), 0)].can_hit(ActorId(2)));
    for _ in 0..3 {
        battle.advance_weapon_flights(ActorId(0), Some([0.; 3]), None, &mut Default::default())?;
    }
    assert!(battle.weapon_flights[&(ActorId(0), 0)].can_hit(ActorId(1)));
    Ok(())
}

#[test]
fn detached_world_uses_native_euler_axes_and_unit_model_scale() {
    let mut flight = Flight::new(
        definition(0, 14, 0.),
        ActionId(1),
        [2., 3., 4.],
        0.,
        [0., 0., 1.],
    );
    flight.angles = [90., 0., 0.];
    let world = flight.world();
    let point = resonance_content::animation::transform_point(world, [0., 1., 0.]);
    for (actual, expected) in point.into_iter().zip([2., 3., 5.]) {
        assert!((actual - expected).abs() < 0.00001);
    }
}

#[test]
fn detached_anchor_does_not_submit_an_ordinary_hand_contact() -> Result<()> {
    let mut battle = battle(
        "pub task run() { await battle::throw_weapon(flight,ticks(0)); }",
        definition(0, 14, 0.),
    );
    battle.step(start())?;
    battle.step(Default::default())?;
    let contact = crate::MeleeDefinition {
        hit: definition(0, 14, 0.).hit,
        cooldown: 0,
        radius: 40.,
        height: 40.,
        shape: HitShape::Sphere,
        // Body anchor0 remains attached; weapon slots0/1 append anchors1/2.
        anchors: vec![1],
        trail: None,
    };
    let model = battle.models[0].as_ref().unwrap();
    assert!(model.anchor_attached(0));
    assert!(!model.anchor_attached(1));
    assert!(model.anchor_attached(2));
    let mut contacts = crate::contact::Contacts::default();
    contacts.melee(
        ActorId(0),
        ActionId(1),
        &battle.actors[0],
        battle.models[0].as_ref(),
        &contact,
    )?;
    assert!(contacts.0[0].is_empty());
    battle.retire_weapon_flights();
    contacts.melee(
        ActorId(0),
        ActionId(1),
        &battle.actors[0],
        battle.models[0].as_ref(),
        &contact,
    )?;
    assert_eq!(contacts.0[0].len(), 1);
    Ok(())
}

#[test]
fn flight_motion_matches_pinned_original_ground_and_aerial_profiles() -> Result<()> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/weapon-flight.json"))?;
    let vector = |row: &serde_json::Value, key: &str| -> [f32; 3] {
        std::array::from_fn(|i| f32::from_bits(row[key][i].as_u64().unwrap() as u32))
    };
    for row in fixture["observations"].as_array().unwrap() {
        let before = &row["before"];
        let expected = &row["after"];
        let point: [f32; 3] = std::array::from_fn(|i| {
            f32::from_bits(row["catch_point_bits"][i].as_u64().unwrap() as u32)
        });
        let mut flight = Flight::new(
            definition(0, 0, 0.),
            ActionId(1),
            vector(before, "position_bits"),
            0.,
            [0., 0., 1.],
        );
        flight.direction = vector(before, "direction_bits");
        flight.angles = vector(before, "angles_bits");
        flight.remaining = before["outbound"].as_u64().unwrap() as u16;
        flight.speed = f32::from_bits(before["speed_bits"].as_u64().unwrap() as u32);
        flight.step(
            [point[0] - 0.1, point[1], point[2] - 0.1],
            Some(point),
            None,
        )?;
        assert_eq!(
            flight.position.map(f32::to_bits),
            vector(expected, "position_bits").map(f32::to_bits),
            "tick{} position",
            row["combat_tick"]
        );
        assert_eq!(
            flight.direction.map(f32::to_bits),
            vector(expected, "direction_bits").map(f32::to_bits),
            "tick{} direction",
            row["combat_tick"]
        );
        assert_eq!(
            flight.angles.map(f32::to_bits),
            vector(expected, "angles_bits").map(f32::to_bits),
            "tick{} angles",
            row["combat_tick"]
        );
        assert_eq!(
            flight.remaining,
            expected["outbound"].as_u64().unwrap() as u16
        );
        assert_eq!(flight.caught, expected["detached"] == 0);
    }
    Ok(())
}

#[test]
fn pointer_sized_catch_coordinates_have_the_origin_distance_above_the_floor() {
    // Cached MEM1 pointers interpreted as f32 have magnitude <= 2^-124.
    // For |coordinate| >= 2^-98, subtraction rounds identically to subtracting
    // zero. Below that cutoff, the changed squared term is < 2^-194 and the
    // original NI-mode SDK arithmetic discards it. Y >= 5.1 keeps the total
    // normal. This proves equivalence only after the writer is proved to have
    // stored a pointer; it does not justify replacing arbitrary stack residue.
    let pointer = f32::from_bits(0x8180_0000);
    let points = [[pointer; 3], [pointer, 0., 0.], [0., pointer, pointer]];
    let mut coordinates = vec![0., -0., 5.1, -5.1, 25., -25., 2000., -2000.];
    for exponent in 1..=138 {
        for mantissa in [0, 1, 0x7f_ffff] {
            let value = f32::from_bits((exponent << 23) | mantissa);
            coordinates.extend([value, -value]);
        }
    }
    // Exercise adjacent representable positions at the actual speed25 catch
    // boundary, including the SDK estimate's own rounding behavior.
    let boundary = (25_f32 * 25. - 5.1_f32 * 5.1).sqrt().to_bits();
    coordinates.extend((boundary - 32..=boundary + 32).map(f32::from_bits));
    for coordinate in coordinates {
        for position in [
            [coordinate, 5.1, 0.],
            [0., 5.1, coordinate],
            [coordinate, 5.1, coordinate],
            [coordinate, 25., -coordinate],
        ] {
            let origin = crate::distance::length(position.map(|value| -value));
            for point in points {
                let observed =
                    crate::distance::length(std::array::from_fn(|i| point[i] - position[i]));
                assert_eq!(observed.to_bits(), origin.to_bits(), "{position:?}");
                assert_eq!(observed <= 25., origin <= 25., "{position:?}");
            }
        }
    }
}

#[test]
fn outbound_catch_uses_the_callback_point_before_its_timer_expires() -> Result<()> {
    let mut flight = Flight::new(
        definition(0, 14, 0.),
        ActionId(1),
        [0., 5.1, -25.],
        0.,
        [0., 0., 1.],
    );
    flight.step([900.; 3], Some([0.; 3]), None)?;
    assert_eq!(flight.remaining, 13);
    assert!(flight.caught);
    Ok(())
}

#[test]
fn observed_hurt_and_death_catch_operands_preserve_native_distances() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/weapon-flight-catch.json"
    ))?;
    for case in fixture["cases"].as_array().unwrap() {
        for row in case["samples"].as_array().unwrap() {
            let vector = |key: &str| -> [f32; 3] {
                std::array::from_fn(|i| f32::from_bits(row[key][i].as_u64().unwrap() as u32))
            };
            let point = if case["kind"] == "death" {
                sampled_heading_point(f32::from_bits(row["body_yaw_bits"].as_u64().unwrap() as u32))
            } else {
                [0.; 3]
            };
            assert_eq!(
                point.map(f32::to_bits),
                vector("semantic_point_bits").map(f32::to_bits)
            );
            let position = vector("position_bits");
            let distance = crate::distance::length(std::array::from_fn(|i| point[i] - position[i]));
            assert_eq!(
                distance.to_bits(),
                row["catch_distance_bits"].as_u64().unwrap() as u32
            );
        }
    }
    Ok(())
}

#[test]
fn martial_hit_stop_retains_the_preceding_facing_operand() -> Result<()> {
    let definition = definition(0, 14, 0.);
    let mut battle = battle(
        "pub task run() { await battle::wait_ticks(ticks(100)); }",
        Arc::clone(&definition),
    );
    // Direct actor admission has no registered normal callback. Its hit-stop
    // chain is the martial path, whose catch operand differs from normal zero.
    battle.start(
        crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: 1,
        },
        &mut vec![],
    )?;
    battle.actors[0].heading = 90.;
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].facing_direction = [1., 0., 0.];
    battle.actors[0].hit_stop = 2;
    battle.throw_weapon(ActorId(0), ActionId(1), definition)?;
    let flight = battle.weapon_flights.get_mut(&(ActorId(0), 0)).unwrap();
    flight.position = [0., 5.1, 0.];
    flight.direction = [0., 0., 1.];
    battle.step(Default::default())?;
    let flight = &battle.weapon_flights[&(ActorId(0), 0)];
    assert_eq!(flight.remaining, 13);
    assert!(flight.caught);
    assert!(crate::distance::length(flight.position) > flight.speed);
    Ok(())
}

#[test]
fn sampled_model_heading_survives_later_actor_heading_changes() -> Result<()> {
    let mut battle = battle("pub task run() {}", definition(0, 14, 0.));
    battle.actors[0].heading = -90.000_02;
    battle.step(Default::default())?;
    let sampled = battle.models[0].as_ref().unwrap().sampled_heading();
    assert_eq!(sampled, -90.000_02);
    battle.actors[0].heading = 12.;
    assert_eq!(
        battle.models[0].as_ref().unwrap().sampled_heading(),
        sampled
    );
    assert_ne!(
        sampled_heading_point(sampled),
        sampled_heading_point(battle.actors[0].heading)
    );
    Ok(())
}

#[test]
fn movement_retains_the_rounded_translation_before_world_position_addition() {
    let mut movement = crate::Movement {
        direction: [1., 0., 0.],
        forward: 1.,
        ..Default::default()
    };
    let mut position = [33_554_432., 0., 0.];
    movement.integrate(&mut position, [0.; 2]);
    assert_eq!(position[0], 33_554_432.);
    assert_eq!(movement.translation, [1., 0., 0.]);
}

#[test]
fn tiny_return_uses_one_normalization_and_the_actual_hand_gap() -> Result<()> {
    let old = [1., 0., 0.];
    let desired = [2., f32::from_bits(0x42b4_0002), 1.];
    let mut flight = Flight::new(definition(0, 0, 0.), ActionId(1), [0., 50., 0.], 0., old);
    flight.direction = old;
    flight.speed = 5.;
    flight.cooldowns[1] = 2;
    let hand = [-0.05, 50., -0.1];
    let distance = crate::distance::length([hand[0] + 0.1, 0., hand[2] + 0.1]);
    assert!(distance > 0. && distance < 0.1);
    let blend = 0.001_f32.mul_add(200. - distance, 0.35);
    let combine = |desired: [f32; 3], amount: f32| {
        crate::distance::normalize(std::array::from_fn(|i| {
            old[i] * (1. - amount) + desired[i] * amount
        }))
    };
    let once = crate::distance::normalize(desired);
    let twice = crate::distance::normalize(once);
    let expected = combine(once, blend);
    assert_ne!(
        expected,
        combine(twice, blend),
        "fixture distinguishes normalization count"
    );
    assert_ne!(
        expected,
        combine(once, 0.55),
        "fixture distinguishes the real gap"
    );
    flight.step(hand, None, Some(ReturnSteering::Desired(desired)))?;
    assert_eq!(flight.direction, expected);
    assert_eq!(flight.speed, 7.5);
    assert_eq!(flight.cooldowns[1], 1);
    assert_eq!(flight.remaining, 0);
    Ok(())
}

#[test]
fn tiny_return_strict_length_and_unordered_cases_keep_direction_but_advance() -> Result<()> {
    for steering in [
        ReturnSteering::KeepDirection,
        ReturnSteering::Desired([0.; 3]),
        // Adjacent source SDK lengths straddle the strict 0.1 gate.
        ReturnSteering::Desired([f32::from_bits(0x3dcc_cccc), 0., 0.]),
        ReturnSteering::Desired([f32::from_bits(0x6000_0000), 0., 0.]),
    ] {
        let mut flight = Flight::new(
            definition(0, 0, 0.),
            ActionId(1),
            [0., 50., 0.],
            0.,
            [1., 0., 0.],
        );
        flight.direction = [1., 0., 0.];
        flight.speed = 5.;
        flight.cooldowns[1] = 2;
        flight.step([-0.1, 50., -0.1], None, Some(steering))?;
        assert_eq!(flight.direction, [1., 0., 0.]);
        assert_eq!(flight.position, [7.5, 50., 0.]);
        assert_eq!(flight.speed, 7.5);
        assert_eq!(flight.cooldowns[1], 1);
        // The SDK estimate of exactly7.5 is one ULP above7.5. The existing
        // post-integration catch comparison must retain that strict boundary.
        assert!(!flight.caught);
    }
    let mut flight = Flight::new(
        definition(0, 0, 0.),
        ActionId(1),
        [0., 50., 0.],
        0.,
        [0., 0., 1.],
    );
    flight.step(
        [-0.1, 50., -0.1],
        None,
        Some(ReturnSteering::Desired([
            f32::from_bits(0x3dcc_ccce),
            0.,
            0.,
        ])),
    )?;
    assert!(flight.direction[0] > 0.);
    Ok(())
}

#[test]
fn ordinary_return_ignores_callback_operand_and_retains_double_normalization() -> Result<()> {
    let hand = [1.9, 140., 0.9];
    let mut expected = None;
    for steering in [
        None,
        Some(ReturnSteering::KeepDirection),
        Some(ReturnSteering::Desired([0.; 3])),
    ] {
        let mut flight = Flight::new(
            definition(0, 0, 0.),
            ActionId(1),
            [0., 50., 0.],
            0.,
            [1., 0., 0.],
        );
        let old = flight.direction;
        let delta = [hand[0] + 0.1, hand[1] - 50., hand[2] + 0.1];
        let distance = crate::distance::length(delta);
        let amount = 0.001_f32.mul_add(200. - distance, 0.35);
        let desired = crate::distance::normalize(crate::distance::normalize(delta));
        let direction = crate::distance::normalize(std::array::from_fn(|i| {
            old[i] * (1. - amount) + desired[i] * amount
        }));
        flight.step(hand, None, steering)?;
        assert_eq!(flight.direction, direction);
        let actual = (
            flight.position,
            flight.direction,
            flight.angles,
            flight.caught,
        );
        if let Some(expected) = expected {
            assert_eq!(actual, expected);
        } else {
            expected = Some(actual);
        }
    }
    Ok(())
}

fn facing_return_battle(normal: bool, active: bool) -> Result<Battle> {
    let mut battle = battle_with_leader(
        "pub task run() { await battle::wait_ticks(ticks(100)); }",
        definition(0, 0, 0.),
        true,
    );
    if normal {
        assert!(battle.start_automatic_normal(ActorId(0), 1, ActorId(1), &mut vec![])?);
        if active {
            battle.face_normal_entry(ActionId(1), ActorId(0))?;
        }
    } else {
        battle.start(
            crate::ActionRequest {
                actor: ActorId(0),
                target: ActorId(1),
                action: 1,
            },
            &mut vec![],
        )?;
    }
    if active {
        battle.sequences.get_mut(&ActionId(1)).unwrap().age = 1;
    }
    battle.actors[0].heading = 0.;
    battle.actors[0].movement.direction = [0., 0., 1.];
    battle.actors[0].movement.forward = 0.;
    battle.actors[0].movement.vertical = 0.;
    Ok(battle)
}

fn put_return_at_sampled_hand(battle: &mut Battle) -> Result<()> {
    let model = battle.models[0].as_mut().unwrap();
    model.step(
        &mut battle.actors[0],
        false,
        crate::model::PlacementUpdate::Actor,
    )?;
    let hand = model.weapon_attachment(0)?;
    battle.throw_weapon(ActorId(0), ActionId(1), definition(0, 0, 0.))?;
    let flight = battle.weapon_flights.get_mut(&(ActorId(0), 0)).unwrap();
    flight.position = [hand[0] + 0.1, hand[1], hand[2] + 0.1];
    flight.direction = [1., 0., 0.];
    flight.cooldowns[1] = 2;
    Ok(())
}

#[test]
fn failed_facing_and_active_normal_or_martial_hit_stop_admit_tiny_returns() -> Result<()> {
    for (normal, active, hit_stop, heading) in [
        (true, false, 0, -90.), // Failed facing skips even the normal initializer.
        (true, true, 2, 0.),
        (false, true, 2, 0.),
    ] {
        let mut battle = facing_return_battle(normal, active)?;
        battle.actors[0].heading = heading;
        battle.actors[0].hit_stop = hit_stop;
        put_return_at_sampled_hand(&mut battle)?;
        let age = battle.sequences[&ActionId(1)].age;
        battle.step(Default::default())?;
        let flight = &battle.weapon_flights[&(ActorId(0), 0)];
        assert!(flight.direction[0] > 0. && flight.direction[2] > 0.);
        assert_eq!(flight.cooldowns[1], 1);
        assert_eq!(battle.sequences[&ActionId(1)].age, age);
        assert_eq!(
            battle.actors[0].heading,
            if heading == -90. { -67.5 } else { 0. }
        );
    }
    Ok(())
}

#[test]
fn advancing_martial_tiny_return_keeps_direction_and_advances_flight() -> Result<()> {
    for command_age in [0, i16::MAX] {
        let mut battle = facing_return_battle(false, true)?;
        battle.sequences.get_mut(&ActionId(1)).unwrap().command_age = command_age;
        put_return_at_sampled_hand(&mut battle)?;
        let flight = battle.weapon_flights.get_mut(&(ActorId(0), 0)).unwrap();
        flight.speed = 10.;
        let position = flight.position;

        battle.step(Default::default())?;

        let sequence = &battle.sequences[&ActionId(1)];
        assert_eq!(sequence.age, 2);
        assert_eq!(sequence.command_age, command_age.wrapping_add(1));
        let flight = &battle.weapon_flights[&(ActorId(0), 0)];
        assert_eq!(flight.direction, [1., 0., 0.]);
        assert_eq!(flight.speed, 12.5);
        assert_eq!(
            flight.position,
            [position[0] + 12.5, position[1], position[2]]
        );
        assert_eq!(flight.cooldowns[1], 1);
    }
    Ok(())
}

#[test]
fn martial_tiny_return_excludes_entry_model_holds_and_completion() -> Result<()> {
    for case in ["entry", "model_hold", "completion", "recovery"] {
        let mut battle = facing_return_battle(false, case != "entry")?;
        match case {
            "entry" => {}
            "model_hold" => {
                let model = battle.models[0].as_mut().unwrap();
                model.play(crate::MotionBinding { model: 1, clip: 0 }, 0., 0.5, true, 8)?;
                assert!(model.blending());
            }
            "completion" => {
                let sequence = battle.sequences.get_mut(&ActionId(1)).unwrap();
                sequence.age = u32::from(sequence.definition.duration);
            }
            "recovery" => {
                battle.sequences.get_mut(&ActionId(1)).unwrap().recovery = Some(10);
            }
            _ => unreachable!(),
        }
        put_return_at_sampled_hand(&mut battle)?;
        let error = battle.step(Default::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("return steering below 0.1 is unproved"),
            "{case}: {error:#}"
        );
    }
    Ok(())
}

#[test]
fn advancing_martial_operand_is_not_retained_during_a_later_model_hold() -> Result<()> {
    let mut battle = facing_return_battle(false, true)?;
    put_return_at_sampled_hand(&mut battle)?;
    battle.step(Default::default())?;
    assert_eq!(
        battle.weapon_flights[&(ActorId(0), 0)].direction,
        [1., 0., 0.]
    );

    battle.models[0].as_mut().unwrap().play(
        crate::MotionBinding { model: 1, clip: 0 },
        0.,
        0.5,
        true,
        8,
    )?;
    battle
        .weapon_flights
        .get_mut(&(ActorId(0), 0))
        .unwrap()
        .caught = false;
    put_return_at_sampled_hand(&mut battle)?;
    let error = battle.step(Default::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("return steering below 0.1 is unproved")
    );
    Ok(())
}

#[test]
fn advancing_normal_tiny_return_requires_an_active_unheld_nonlanding_visit() -> Result<()> {
    for case in ["advancing", "entry", "landing", "model_hold", "completion"] {
        let mut battle = facing_return_battle(true, case != "entry")?;
        match case {
            "advancing" | "entry" => {}
            "landing" => battle.actors[0].movement.airborne_action = true,
            "model_hold" => {
                let model = battle.models[0].as_mut().unwrap();
                model.play(crate::MotionBinding { model: 1, clip: 0 }, 0., 0.5, true, 8)?;
                assert!(model.blending());
            }
            "completion" => {
                let sequence = battle.sequences.get_mut(&ActionId(1)).unwrap();
                sequence.age = u32::from(sequence.definition.duration);
            }
            _ => unreachable!(),
        }
        put_return_at_sampled_hand(&mut battle)?;
        let position = battle.weapon_flights[&(ActorId(0), 0)].position;
        let result = battle.step(Default::default());
        if case == "advancing" {
            result?;
            let sequence = &battle.sequences[&ActionId(1)];
            assert_eq!(sequence.age, 2);
            assert_eq!(sequence.command_age, 1);
            let flight = &battle.weapon_flights[&(ActorId(0), 0)];
            assert_eq!(flight.direction, [1., 0., 0.]);
            assert_eq!(
                flight.position,
                [position[0] + 25., position[1], position[2]]
            );
            assert_eq!(flight.cooldowns[1], 1);
        } else {
            let error = result.unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("return steering below 0.1 is unproved"),
                "{case}: {error:#}"
            );
        }
    }
    Ok(())
}

#[test]
fn unproved_callback_exits_still_reject_tiny_returns() -> Result<()> {
    for case in [
        "manual", "leader", "airborne", "short", "disabled", "entry", "landing", "recovery", "idle",
    ] {
        let mut battle = facing_return_battle(true, case != "entry")?;
        battle.actors[0].hit_stop = 2;
        match case {
            "manual" => battle.actors[0].control = crate::Control::Manual,
            "leader" => battle.actors[2].control = crate::Control::Auto,
            "airborne" => battle.actors[0].position[1] = 1.,
            "short" => battle.actors[0].movement.direction = [0.; 3],
            "disabled" => battle.actors[0].movement.turning_disabled = true,
            "entry" => {}
            "landing" => battle.actors[0].movement.airborne_action = true,
            "recovery" => {
                battle
                    .sequences
                    .get_mut(&ActionId(1))
                    .unwrap()
                    .action_recovery = true
            }
            "idle" => {
                battle.sequences.clear();
                battle.actors[0].activity = crate::Activity::Idle;
            }
            _ => unreachable!(),
        }
        put_return_at_sampled_hand(&mut battle)?;
        let error = battle.step(Default::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("return steering below 0.1 is unproved"),
            "{case}: {error:#}"
        );
    }
    // Martial phase zero also has a later writer, despite the held action.
    let mut battle = facing_return_battle(false, false)?;
    battle.actors[0].hit_stop = 2;
    put_return_at_sampled_hand(&mut battle)?;
    assert!(
        battle
            .step(Default::default())
            .unwrap_err()
            .to_string()
            .contains("return steering below 0.1 is unproved")
    );
    Ok(())
}

#[test]
fn a_proved_facing_operand_is_not_retained_by_the_next_callback() -> Result<()> {
    let mut battle = facing_return_battle(true, true)?;
    battle.actors[0].hit_stop = 2;
    put_return_at_sampled_hand(&mut battle)?;
    battle.step(Default::default())?;
    assert!(battle.weapon_flights[&(ActorId(0), 0)].direction[2] > 0.);

    battle.actors[0].hit_stop = 0;
    battle
        .weapon_flights
        .get_mut(&(ActorId(0), 0))
        .unwrap()
        .caught = false;
    put_return_at_sampled_hand(&mut battle)?;
    battle.step(Default::default())?;
    assert_eq!(battle.sequences[&ActionId(1)].command_age, 1);
    assert_eq!(
        battle.weapon_flights[&(ActorId(0), 0)].direction,
        [1., 0., 0.]
    );
    Ok(())
}

#[test]
fn retained_death_heading_admission_preserves_source_branch_boundaries() {
    for (heading, keep) in [
        (-1000., false),
        (-999., true),
        (-360., true),
        ((-225_f32).next_down(), false),
        (-225., false),
        ((-225_f32).next_up(), false),
        ((-180_f32).next_down(), false),
        (-180., true),
        ((-180_f32).next_up(), false),
        (-135., false),
        ((-135_f32).next_up(), false),
        (f32::from_bits((-135_f32).to_bits() - 4), true),
        (0., true),
        (180., true),
        (540., true),
        (999., true),
        (1000., false),
        (f32::NAN, false),
        (f32::INFINITY, false),
        (f32::NEG_INFINITY, false),
    ] {
        assert_eq!(
            ReturnSteering::retained_death(heading),
            keep.then_some(ReturnSteering::KeepDirection),
            "heading {heading}"
        );
    }
}

fn retained_death_battle() -> Result<Battle> {
    let compiled = symphonia_script_compiler::compile(
        "death",
        &BTreeMap::from([(
            "death".into(),
            include_str!("../../../../scripts/battle/actor_death.sym").into(),
        )]),
        &crate::native_declarations(),
    )?;
    let program = Arc::new(compiled.program);
    let actions = ["fall", "initial"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| ActionDefinition {
            id: index as u16 + 1,
            phase: ActionPhase::Controller,
            entry: program
                .authored()
                .unwrap()
                .functions
                .iter()
                .find(|function| function.name == format!("death::{name}"))
                .unwrap()
                .entry,
            program: program.clone(),
            duration: 0,
            tp_cost: 0,
            resources: [3, 7]
                .map(|clip| {
                    ResourceBinding::OptionalMotion(vec![
                        Some(crate::MotionBinding { model: 1, clip }),
                        None,
                        None,
                    ])
                })
                .to_vec(),
        })
        .collect();
    let mut owner = crate::tests::actor(Side::Party);
    owner.control = crate::Control::Auto;
    owner.hp = 0;
    owner.body.approach_points.push(crate::HurtPoint {
        center: [0.; 3],
        radius: 10.,
    });
    let mut target = crate::tests::actor(Side::Enemy);
    target.position = [0., 0., 500.];
    target.body.approach_points.push(crate::HurtPoint {
        center: target.position,
        radius: 10.,
    });
    let mut leader = crate::tests::actor(Side::Party);
    leader.control = crate::Control::SemiAuto;
    leader.position = [1000., 0., 0.];
    let mut model = (*model()).clone();
    model.approach_bones.push(0);
    for clip in [3, 7] {
        model.motions.insert(clip, model.motions[&0].clone());
    }
    let prepared = PreparedBattle::new(
        vec![owner, target, leader],
        actions,
        17,
        vec![Some(Arc::new(model)), None, None],
        vec![],
    )?
    .with_initial_targets(vec![1, 0, 1])?
    .with_deaths(vec![
        Some(crate::DeathBinding {
            fall: 1,
            initial: 2,
            wait_for_motion: false,
            integrate: true,
            darken_immediately: false,
            revival_motion: None,
        }),
        None,
        None,
    ])?;
    Ok(Battle::new(Arc::new(prepared)))
}

#[test]
fn retained_death_tiny_return_uses_controller_visits_without_action_clock_gates() -> Result<()> {
    for case in [
        "initial",
        "existing",
        "hit_stop",
        "blend",
        "airborne",
        "landing",
        "dead_leader",
        "dead_target",
    ] {
        let mut battle = retained_death_battle()?;
        match case {
            "initial" => {}
            "existing" => {
                battle.step(Default::default())?;
            }
            "hit_stop" => battle.actors[0].hit_stop = 2,
            "blend" => battle.models[0].as_mut().unwrap().play(
                crate::MotionBinding { model: 1, clip: 3 },
                0.,
                0.5,
                false,
                8,
            )?,
            "airborne" => battle.actors[0].position[1] = 10.,
            "landing" => {
                battle.actors[0].position[1] = 0.05;
                battle.actors[0].movement.vertical = -1.;
            }
            "dead_leader" => {
                battle.actors[2].availability = crate::ActorAvailability::Dead;
                battle.actors[2].activity = crate::Activity::Defeated;
            }
            "dead_target" => {
                battle.actors[1].availability = crate::ActorAvailability::Dead;
                battle.actors[1].activity = crate::Activity::Defeated;
            }
            _ => unreachable!(),
        }
        put_return_at_sampled_hand(&mut battle)?;
        battle.step(Default::default())?;
        let flight = &battle.weapon_flights[&(ActorId(0), 0)];
        assert_eq!(flight.direction, [1., 0., 0.], "{case}");
        assert_eq!(flight.cooldowns[1], 1, "{case}");
        assert!(battle.sequences.values().any(|sequence| {
            sequence.actor == ActorId(0) && sequence.definition.phase == ActionPhase::Controller
        }));
    }
    Ok(())
}

#[test]
fn retained_death_tiny_return_rejects_unproved_contexts() -> Result<()> {
    for case in [
        "no_binding",
        "no_integration",
        "no_owner_body",
        "no_target_body",
        "manual",
        "leader",
        "active",
        "idle",
        "direct_non_tiny",
    ] {
        let mut battle = retained_death_battle()?;
        match case {
            "no_binding" => Arc::get_mut(&mut battle.prepared).unwrap().deaths[0] = None,
            "no_integration" => {
                Arc::get_mut(&mut battle.prepared).unwrap().deaths[0]
                    .as_mut()
                    .unwrap()
                    .integrate = false;
            }
            "no_owner_body" => battle.actors[0].body.approach_points.clear(),
            "no_target_body" => battle.actors[1].body.approach_points.clear(),
            "manual" => battle.actors[0].control = crate::Control::Manual,
            "leader" => battle.actors[2].control = crate::Control::Auto,
            "active" => battle.actors[0].availability = crate::ActorAvailability::Active,
            "idle" => battle.actors[0].activity = crate::Activity::Idle,
            "direct_non_tiny" => battle.actors[0].heading = -135.,
            _ => unreachable!(),
        }
        put_return_at_sampled_hand(&mut battle)?;
        let error = battle.step(Default::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("return steering below 0.1 is unproved"),
            "{case}: {error:#}"
        );
    }
    Ok(())
}

#[test]
fn retained_death_tiny_return_is_reclassified_each_visit() -> Result<()> {
    for remove_pair in [false, true] {
        let mut battle = retained_death_battle()?;
        put_return_at_sampled_hand(&mut battle)?;
        battle.step(Default::default())?;
        assert_eq!(
            battle.weapon_flights[&(ActorId(0), 0)].direction,
            [1., 0., 0.]
        );

        if remove_pair {
            battle.actors[1].body.approach_points.clear();
        } else {
            battle.actors[0].heading = -135.;
        }
        put_return_at_sampled_hand(&mut battle)?;
        let error = battle.step(Default::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("return steering below 0.1 is unproved"),
            "{error:#}"
        );
    }
    Ok(())
}

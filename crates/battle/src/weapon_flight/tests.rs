use super::*;
use crate::{
    ActionDefinition, Battle, BattleInput, DamageKind, HitElement, Power, PreparedBattle, Side,
};
use anyhow::Context;

fn definition(slot: u8, outbound_ticks: u32, direction_y: f32) -> Arc<WeaponFlightDefinition> {
    Arc::new(WeaponFlightDefinition {
        slot,
        origin: [0., 70., 25.],
        outbound_ticks,
        speed: 25.,
        return_speed: 25.,
        direction_y,
        hit: HitRule {
            kind: DamageKind::Slash,
            arte: false,
            overlimit_pause: false,
            power: Power::Fixed(10),
            element: HitElement::Neutral,
            prevents_defeat: false,
            reaction: Default::default(),
            guard: Default::default(),
            condition: None,
        },
        radius: 40.,
    })
}

fn battle(launch_at: u16, definition: Arc<WeaponFlightDefinition>) -> Battle {
    let mut owner = crate::tests::actor(Side::Party);
    owner.movement.direction = [0., 0., 1.];
    owner.facing_direction = owner.movement.direction;
    let mut target = crate::tests::actor(Side::Enemy);
    target.position = [0., 0., 500.];

    let action = ActionDefinition {
        normal: None,
        execution: crate::ActionExecution::Attack(crate::PreparedAttack {
            chain_at: None,
            end_at: 200,
            opening: None,
            events: vec![(launch_at, crate::AttackEvent::Throw(definition))],
            recovery: 1,
        }),
        tp_cost: 0,
    };
    PreparedBattle::new(
        vec![
            (
                owner,
                crate::ActorSetup {
                    techniques: vec![crate::tests::technique(crate::ActionKey(0), 2)],
                    ..Default::default()
                },
            ),
            (target, crate::ActorSetup::default()),
        ],
        (vec![action]).into(),
        17,
    )
    .unwrap()
    .finish()
    .unwrap()
}

fn start() -> BattleInput {
    BattleInput {
        actions: vec![crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }],
        ..Default::default()
    }
}

#[test]
fn native_launch_point_scales_and_turns_then_returns_to_the_moving_owner() -> Result<()> {
    let mut actor = crate::tests::actor(Side::Party);
    actor.position = [10., 200., 30.];
    actor.heading = 90.;
    actor.facing_direction = [1., 0., 0.];
    actor.body.scale = 2.;
    for (ticks, y) in [(14, 0.), (10, -0.7), (8, 0.4), (0, 0.)] {
        let definition = definition(0, ticks, y);
        let mut flight = Flight::new(definition.clone(), ActionId(1), &actor);
        for (actual, expected) in flight.position.into_iter().zip([60., 340., 30.]) {
            assert!((actual - expected).abs() < 0.001);
        }
        let launch = actor.local_point(definition.origin);
        for _ in 0..ticks {
            flight.step(launch)?;
            assert!(!flight.caught, "outbound flight cannot catch at launch");
        }
        // Also covers a coincident catch for the zero-length flight.
        flight.step(launch)?;
        for update in 0..100 {
            let target = [launch[0] + update as f32, launch[1], launch[2]];
            if flight.caught {
                break;
            }
            flight.step(target)?;
            if flight.caught {
                assert_eq!(flight.position, target);
            }
        }
        assert!(flight.caught, "flight must catch without overshooting");
    }
    Ok(())
}

#[test]
fn launched_weapon_keeps_direction_and_power_after_owner_changes() -> Result<()> {
    let mut damage = None;
    for next_power in [10, 200] {
        let mut definition = definition(0, 14, 0.);
        Arc::make_mut(&mut definition).hit.power = Power::Normal;
        let mut battle = battle(0, definition.clone());
        battle.actors[0].attack_power = 60;
        battle.actors[0].equipment.stats.slash = 200;
        battle.actors[0].facing_direction = [0., 0., -1.];
        battle.actors[1].body.collider = Some(crate::Collider::sphere(2.));
        battle.throw_weapon(ActorId(0), ActionId(1), definition);
        let launch = battle.weapon_flights[&(ActorId(0), 0)].position;
        battle.actors[1].position = [launch[0], launch[1], launch[2] - 25.];
        battle.actors[0].facing_direction = [1., 0., 0.];
        battle.actors[0].attack_power = next_power;
        let mut contacts = crate::contact::Contacts::default();
        let mut cues = vec![];
        battle.advance_weapon_flights(ActorId(0), &mut contacts, &mut cues)?;
        assert!(cues.iter().any(|cue| matches!(
            cue,
            crate::Cue::WeaponTrail {
                actor: ActorId(0),
                slot: 0,
                duration: 1,
            }
        )));
        contacts.resolve(&mut battle, &mut cues)?;
        let hit = cues
            .iter()
            .find_map(|cue| match cue {
                crate::Cue::Hit {
                    actor: ActorId(1),
                    result,
                    ..
                } => Some(result.amount),
                _ => None,
            })
            .expect("thrown weapon hits along its launch direction");
        assert!(hit > 0);
        assert_eq!(hit, *damage.get_or_insert(hit));
    }
    Ok(())
}

#[test]
fn released_weapon_survives_interruption_stops_and_owner_death_without_artwork() -> Result<()> {
    let mut battle = battle(2, definition(0, 14, 0.));
    battle.step(start())?;
    for _ in 0..4 {
        battle.step(Default::default())?;
        if !battle.weapon_flights.is_empty() {
            break;
        }
    }
    let flight = battle
        .weapon_flights
        .get(&(ActorId(0), 0))
        .context("scheduled weapon launch did not occur")?;
    let action = flight.action;
    let position = flight.position;
    let age = battle.sequence(&action).unwrap().age;
    battle.actors[0].hit_stop = 2;
    battle.actors[0].time_stop = 3;
    battle.step(Default::default())?;
    assert_eq!(battle.sequence(&action).unwrap().age, age);
    assert!(battle.weapon_flights[&(ActorId(0), 0)].position[2] > position[2]);
    let position = battle.weapon_flights[&(ActorId(0), 0)].position;
    battle.begin_hurt(ActorId(0), 20, &mut vec![]);
    battle.step(Default::default())?;
    assert!(battle.weapon_flights[&(ActorId(0), 0)].position[2] > position[2]);
    battle.actors[0].availability = crate::ActorAvailability::Dead;
    battle.actors[0].position[0] += 100.;
    let mut cues = vec![];
    for _ in 0..100 {
        cues.clear();
        battle.advance_weapon_flights(ActorId(0), &mut Default::default(), &mut cues)?;
        if battle.weapon_flights.is_empty() {
            break;
        }
    }
    assert!(battle.weapon_flights.is_empty());
    assert!(
        cues.is_empty(),
        "a caught weapon stops requesting trail samples"
    );
    assert!(!battle.is_diagnostic());
    Ok(())
}

#[test]
fn each_slot_hits_each_target_once_and_keeps_an_occupied_launch() -> Result<()> {
    let mut battle = battle(0, definition(0, 14, 0.));
    battle.step(start())?;
    battle.step(Default::default())?;
    let original = battle.weapon_flights[&(ActorId(0), 0)].position;
    battle.throw_weapon(ActorId(0), ActionId(99), definition(0, 8, 0.4));
    assert_eq!(battle.weapon_flights[&(ActorId(0), 0)].position, original);
    assert_ne!(battle.weapon_flights[&(ActorId(0), 0)].action, ActionId(99));
    battle.throw_weapon(ActorId(0), ActionId(2), definition(1, 10, -0.7));
    battle
        .weapon_flights
        .get_mut(&(ActorId(0), 0))
        .unwrap()
        .hit(ActorId(1));
    assert!(!battle.weapon_flights[&(ActorId(0), 0)].can_hit(ActorId(1)));
    assert!(battle.weapon_flights[&(ActorId(0), 1)].can_hit(ActorId(1)));
    assert!(battle.weapon_flights[&(ActorId(0), 0)].can_hit(ActorId(2)));
    for _ in 0..3 {
        battle.advance_weapon_flights(ActorId(0), &mut Default::default(), &mut vec![])?;
    }
    assert!(!battle.weapon_flights[&(ActorId(0), 0)].can_hit(ActorId(1)));
    Ok(())
}

#[test]
fn finishing_a_battle_with_a_thrown_weapon_seals_the_outcome() -> Result<()> {
    let mut battle = battle(0, definition(0, 14, 0.));
    battle.step(start())?;
    battle.step(Default::default())?;
    battle.recognize_escape(true)?;
    assert_eq!(
        battle.recognize_result(),
        Some(crate::BattleResult::Escaped)
    );
    let outcome = battle.finish_result()?.outcome.unwrap();
    assert!(battle.owns_outcome(&outcome));
    assert!(battle.weapon_flights.is_empty());
    let mut wrong = outcome.clone();
    wrong.result = crate::BattleResult::Victory;
    assert!(!battle.owns_outcome(&wrong));
    let mut other =
        crate::tests::prepared(vec![crate::tests::actor(crate::Side::Party)], 1).finish()?;
    other.recognize_escape(true)?;
    other.recognize_result();
    let foreign = other.finish_result()?.outcome.unwrap();
    assert!(!battle.owns_outcome(&foreign));
    let finished = battle.snapshot();
    assert!(battle.finish_result().is_err());
    assert!(battle.set_actor_vitals(ActorId(0), 1, 1, 0, 0).is_err());
    assert!(battle.end_overlimit(ActorId(0)).is_err());
    assert!(battle.record_technique_acquisition(ActorId(0), 1).is_err());
    assert_eq!(battle.snapshot(), finished);
    assert!(battle.owns_outcome(&outcome));
    Ok(())
}

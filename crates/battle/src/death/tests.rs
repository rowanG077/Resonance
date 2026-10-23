use super::*;
use crate::Activity;
use crate::BattleInput;
use crate::PreparedBattle;
use crate::conditions::{Condition, ConditionSet};
use crate::tests::{actor, prepare_actors, step_idle};

fn prepared(actors: Vec<Actor>) -> PreparedBattle {
    let mut prepared = prepare_actors(actors, 77);
    for setup in &mut prepared.resources.actor_setup {
        setup.overlimit_gain = 11;
    }
    prepared
}

#[test]
fn unavailable_members_settle_without_admitting_commands() {
    let mut actors = vec![actor(Side::Party); 4];
    actors.extend(vec![actor(Side::Enemy); 3]);
    actors[4].hp = 0;
    actors[6].hp = 0;
    actors[0].hp = 0;
    actors[1].availability = ActorAvailability::Petrified;
    actors[1].conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: Condition::Petrified.into(),
        ..Default::default()
    });
    actors[2].availability = ActorAvailability::Absent;
    for (slot, actor) in actors.iter_mut().enumerate() {
        actor.position = [slot as f32 * 100., 10., 0.];
        actor.movement.vertical = -1.;
        actor.movement.gravity = -0.5;
    }
    actors[4].movement.flying = true;
    actors[4].movement.vertical = 0.;
    actors[4].movement.gravity = 0.;
    let absent_position = actors[2].position;
    actors[6].movement.forward = 4.;
    actors[6].reaction.direction = [1., 0., 0.];
    let fading_position = actors[6].position;
    let mut prepared = prepared(actors);
    prepared.resources.actor_setup[6].stationary_defeat = true;
    prepared.resources.actions.insert(crate::ActionDefinition {
        tp_cost: 5,
        ..crate::tests::action(1)
    });
    for index in 0..prepared.actors.len() {
        crate::tests::assign_action(&mut prepared, index, crate::ActionKey(0));
    }
    let mut battle = prepared.finish().unwrap();

    let tp = battle.actors[0].tp;
    let frame = battle
        .step(BattleInput {
            actions: (0..3)
                .map(|index| crate::ActionRequest {
                    actor: ActorId(index),
                    target: ActorId(4),
                    action: crate::ActionKey(0),
                })
                .collect(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Rejected { .. }))
            .count(),
        3
    );
    step_idle(&mut battle, 20).unwrap();
    assert_eq!(battle.actors[0].availability, ActorAvailability::Dead);
    assert!(battle.actors[1].is_petrified());
    assert_eq!(battle.actors[0].position[1], 0.);
    // A pre-existing dead hover has no recoil gravity to pull it down.
    assert_eq!(battle.actors[4].position[1], 10.);
    assert_eq!(battle.actors[1].position[1], 0.);
    assert_eq!(battle.actors[2].position, absent_position);
    assert_eq!(battle.actors[6].position, fading_position);
    assert!(battle.actors[..3].iter().all(|actor| actor.tp == tp));
}

#[test]
fn defeated_airborne_motion_preserves_scripted_and_recoil_gravity() {
    for gravity in [0., crate::movement::GRAVITY] {
        let mut owner = actor(Side::Enemy);
        owner.position = [0., 20., 0.];
        owner.movement.flying = true;
        let mut battle = prepared(vec![actor(Side::Party), owner, actor(Side::Enemy)])
            .finish()
            .unwrap();
        let id = ActorId(1);
        battle.actors[id.index()].movement.vertical = 0.;
        battle.actors[id.index()].movement.gravity = gravity;
        let hp = battle.actors[id.index()].hp;
        assert!(battle.defeat_actor(id, &mut Vec::new()));
        for visit in 1..=4 {
            battle.step(BattleInput::default()).unwrap();
            let actor = &battle.actors[id.index()];
            assert_eq!(
                actor.position[1],
                20. + gravity * ((visit - 1) * visit / 2) as f32
            );
            assert_eq!(actor.movement.gravity, gravity);
            assert_eq!(actor.hp, hp);
            assert_eq!(actor.availability, ActorAvailability::Dead);
        }
    }
}

#[test]
fn revival_opens_availability_before_get_up() {
    let mut dead = actor(Side::Party);
    dead.hp = 0;
    let mut battle = prepared(vec![dead, actor(Side::Party), actor(Side::Enemy)])
        .finish()
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    battle.actors[0].hp = 1; // a vitals edit alone cannot reactivate an actor
    battle.actors[0].equipment.recovery.lucky = true;
    battle.actors[0].equipment.luck = 255;
    let before_random = battle.random_state();
    assert!(!battle.actors[0].available());
    let mut cues = vec![];
    crate::tests::revive(&mut battle, ActorId(0), 30, &mut cues).unwrap();
    assert!(battle.actors[0].available());
    assert_eq!(battle.actors[0].hp, 31);
    assert_eq!(battle.random_state(), before_random);
    assert_eq!(battle.activity(ActorId(0)), Activity::GettingUp);
    assert_eq!(battle.actors[0].reaction.protection.remaining, 120);
    step_idle(&mut battle, 20).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
}

#[test]
fn ally_death_rewards_survivors_and_publishes_defeat_once() {
    let mut actors = vec![actor(Side::Party); 4];
    actors.push(actor(Side::Enemy));
    actors[0].overlimit = crate::OverLimit::new(700).unwrap();
    actors[1].overlimit = crate::OverLimit::new(950).unwrap();
    actors[3].hp = 0;
    actors[3].overlimit = crate::OverLimit::new(400).unwrap();
    let mut battle = prepared(actors).finish().unwrap();
    assert_eq!(battle.actors[2].overlimit.charge(), 0);
    let random = battle.random_state();
    let mut cues = vec![];
    battle.actors[0].hp = 0;
    battle.enter_death(ActorId(0), &mut cues);
    battle.enter_death(ActorId(0), &mut cues);
    assert_eq!(
        battle
            .actors
            .iter()
            .map(|a| a.overlimit.charge())
            .collect::<Vec<_>>(),
        [0, 1000, 110, 400, 0]
    );
    assert_eq!(
        cues.iter()
            .filter(|cue| matches!(cue, Cue::Defeated { actor: ActorId(0) }))
            .count(),
        1
    );
    battle.actors[2].overlimit = crate::OverLimit::active(600).unwrap();
    battle.actors[1].hp = 0;
    battle.enter_death(ActorId(1), &mut vec![]);
    assert_eq!(
        battle.actors[2].overlimit,
        crate::OverLimit::active(600).unwrap()
    );
    assert_eq!(battle.random_state(), random);
}

#[test]
fn result_overlimit_reset_distinguishes_active_state_from_full_gauge() {
    let mut actors = vec![actor(Side::Party); 2];
    actors[0].overlimit = crate::OverLimit::new(1000).unwrap();
    actors[1].overlimit = crate::OverLimit::active(400).unwrap();
    let mut battle = prepared(actors).finish().unwrap();
    battle.end_overlimit(ActorId(0)).unwrap();
    battle.end_overlimit(ActorId(1)).unwrap();
    assert_eq!(battle.actors[0].overlimit.charge(), 1000);
    assert_eq!(battle.actors[1].overlimit.remaining(), 0);
    assert!(!battle.actors[1].overlimit.is_active());
}

#[test]
fn death_clears_mutable_effects_and_enchantment_without_removing_equipment_grants() {
    use crate::conditions::{Buff, Conditions, Layers};
    let mut owner = actor(Side::Party);
    owner.conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::CastingSpeed]),
        ..Default::default()
    });
    for buff in [
        Buff::PhysicalAilmentGuard,
        Buff::Quartz(crate::Element::Ice),
    ] {
        owner
            .conditions
            .prepare_buff(buff, false)
            .commit(&mut owner);
    }
    assert!(!owner.conditions.active_effects().is_empty());
    let mut battle = prepared(vec![owner, actor(Side::Enemy)]).finish().unwrap();
    battle.actors[0].hp = 0;
    battle.enter_death(ActorId(0), &mut Vec::new());
    assert!(battle.actors[0].conditions.base().is_empty());
    assert!(battle.actors[0].conditions.active_effects().is_empty());
    assert!(battle.actors[0].conditions.periodic_effects().is_empty());
    assert_eq!(
        battle.actors[0].conditions.effective(),
        ConditionSet::of(&[Condition::CastingSpeed])
    );
    assert_eq!(battle.actors[0].elements.enchantment, None);
    let random = battle.random_state();
    crate::tests::revive(&mut battle, ActorId(0), 30, &mut Vec::new()).unwrap();
    assert_eq!(battle.actors[0].availability, ActorAvailability::Active);
    assert!(battle.actors[0].conditions.base().is_empty());
    assert_eq!(
        battle.actors[0].conditions.effective(),
        ConditionSet::of(&[Condition::CastingSpeed])
    );
    assert_eq!(battle.actors[0].elements.enchantment, None);
    assert_eq!(battle.random_state(), random);
}

#[test]
fn lethal_contact_can_be_revived_without_retaining_its_recoil() {
    let mut target = actor(Side::Party);
    target.guard.auto_disabled = true;
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = prepared(vec![target, actor(Side::Enemy)]).finish().unwrap();

    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(
            ActorId(1),
            crate::ActionId(99),
            &crate::MeleeDefinition {
                hit: crate::HitRule {
                    arte: true,
                    power: crate::Power::Fixed(999),
                    reaction: crate::ReactionRule {
                        hitstun: 45,
                        recoil: crate::RecoilRule {
                            impulse: [16., 0.],
                            knock_down: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..crate::tests::hit()
                },
                trail: None,
                volume: crate::MeleeVolume::Fixed {
                    offset: [0.; 3],
                    radius: 20.,
                    half_height: 20.,
                },
            },
            &[],
        )
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues, &[]).unwrap();
    assert!(cues.iter().any(|cue| matches!(
        cue,
        crate::Cue::Hit {
            actor: ActorId(0),
            ..
        }
    )));
    assert_eq!(battle.actors[0].hp, 0);
    assert_eq!(battle.actors[0].availability, ActorAvailability::Dead);
    assert_eq!(
        battle.actors[0].reaction.recoil.kind,
        crate::RecoilKind::Down
    );
    crate::tests::revive(&mut battle, ActorId(0), 30, &mut cues).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::GettingUp);
    for _ in 0..35 {
        battle.step(BattleInput::default()).unwrap();
        if battle.activity(ActorId(0)) == Activity::Idle {
            break;
        }
    }
    assert!(battle.actors[0].available());
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(
        battle.actors[0].reaction.recoil.kind,
        crate::RecoilKind::Normal
    );
}

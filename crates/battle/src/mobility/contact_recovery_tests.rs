use super::*;
use crate::{
    ActionId, Cue, HitRule, MeleeDefinition, Power, ReactionRule, RecoilKind, RecoilRule,
    contact::Contacts,
};

fn recovery_battle(mode: Control) -> Battle {
    let mut prepared = prepared();
    let action = prepared.resources.actions.insert(crate::tests::action(90));
    let setup = &mut prepared.resources.actor_setup[0];
    setup.contact_recovery = true;
    setup.techniques = vec![crate::PreparedTechnique {
        player_range: [0., 1200.],
        ai_range: [0., 1200.],
        ..crate::tests::technique(action, 1)
    }];
    let owner = &mut prepared.actors[0];
    owner.control = mode;
    owner.hp = 500;
    owner.equipment.max_hp = 500;
    owner.guard.auto_disabled = true;
    owner.reaction.stagger.duration = 40;
    owner.body.collider = Some(crate::Collider::sphere(2.));
    prepared.finish().unwrap()
}

fn guard(pressed: bool) -> BattleInput {
    let mut value = input([0, 0], true, 0);
    value.controllers[0].guard.pressed = pressed;
    value
}

fn hit(battle: &mut Battle, delay: u8) {
    hit_recoil(battle, delay, false);
}

fn hit_recoil(battle: &mut Battle, delay: u8, launch: bool) {
    battle.actors[1].position = battle.actors[0].position;

    let mut contacts = Contacts::default();
    contacts
        .melee(
            ActorId(1),
            ActionId(99),
            &MeleeDefinition {
                hit: HitRule {
                    arte: true,
                    power: Power::Fixed(9),
                    reaction: ReactionRule {
                        hitstun: 45,
                        stagger: 1,
                        recoil: RecoilRule {
                            impulse: [16., if launch { 6. } else { 0. }],
                            knock_down: !launch,
                            launch,
                            delay,
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
    contacts.resolve(battle, &mut cues, &[]).unwrap();
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Hit {
            actor: ActorId(0),
            ..
        }
    )));
    assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
    assert_eq!(
        battle.actors[0].reaction.recoil.kind,
        if launch {
            RecoilKind::Launched
        } else {
            RecoilKind::Down
        }
    );
}

#[test]
fn fresh_guard_breakfall_preserves_impulse_pause_and_landing_recovery() {
    let mut battle = recovery_battle(Control::Manual);
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::default());
    hit(&mut battle, 0);
    battle.actors[0].guard.pressure = 7;
    battle.actors[0].movement.gravity = -0.75;
    let frame = battle.step(guard(true)).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    assert_eq!(
        battle.runtime[0].task().mobility(),
        Some(Mobility::Breakfall)
    );
    assert!(battle.actors[0].movement.vertical > 0. && battle.actors[0].movement.vertical <= 16.5);
    assert_eq!(battle.actors[0].guard.pressure, 0);
    assert_eq!(battle.actors[0].reaction.combo_hits, 0);

    assert!(frame.cues.contains(&Cue::Breakfall { actor: ActorId(0) }));
    let position = battle.actors[0].position;
    pause(&mut battle);
    assert_eq!(battle.actors[0].position, position);
    let mut fall = false;
    let mut landing = false;
    for _ in 0..130 {
        battle.step(BattleInput::default()).unwrap();
        assert!(!battle.is_diagnostic());
        fall |= battle.actors[0].movement.vertical < 0.;
        landing |= matches!(
            battle.runtime[0].task().mobility(),
            Some(Mobility::Landing {
                breakfall: true,
                ..
            })
        );
        if battle.activity(ActorId(0)) == Activity::Idle {
            break;
        }
    }
    assert!(fall && landing);
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert!(!battle.actors[0].movement.airborne_action);
}

#[test]
fn guard_breakfall_requires_a_fresh_press_after_the_hurt_delay() {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = recovery_battle(mode);
        hit(&mut battle, 2);
        battle.actors[0].hit_stop = 2;
        battle.step(guard(true)).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
        for _ in 0..10 {
            battle.step(guard(false)).unwrap();
            assert_ne!(
                battle.activity(ActorId(0)),
                Activity::Jumping,
                "held Guard must not replay the edge from hit-stop"
            );
            if battle.activity(ActorId(0)) == Activity::KnockedDown {
                break;
            }
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::KnockedDown);
        battle.step(guard(true)).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    }
}

#[test]
fn queued_technique_survives_breakfall_landing() {
    let mut battle = recovery_battle(Control::Manual);
    hit(&mut battle, 0);
    battle.step(guard(true)).unwrap();
    step_until(&mut battle, Activity::Recovering, 100);
    assert!(
        battle
            .queue_technique(ActorId(0), crate::ActionKey(7))
            .unwrap()
    );
    assert_eq!(
        battle.pending_technique(ActorId(0)),
        Some(crate::ActionKey(7))
    );
    step_until(&mut battle, Activity::Idle, 20);
    assert_eq!(
        battle.pending_technique(ActorId(0)),
        Some(crate::ActionKey(7))
    );
}

#[test]
fn roll_reduces_landing_damage() {
    let mut damage = Vec::new();
    for roll in [false, true] {
        let mut battle = recovery_battle(Control::Manual);
        battle.actors[0].equipment.control_ex.roll = roll;
        hit_recoil(&mut battle, 0, true);
        let hp = battle.actors[0].hp;
        step_until(&mut battle, Activity::Idle, 150);
        damage.push(hp - battle.actors[0].hp);
    }
    assert!(damage[0] > 0);
    assert!(damage[1] < damage[0]);
}

#[path = "launch_tests.rs"]
mod launch_tests;
fn enemy_recovery_battle() -> Battle {
    let mut enemy = actor(Side::Enemy);
    enemy.control = Control::Enemy;
    enemy.hp = 500;
    enemy.equipment.max_hp = 500;
    enemy.guard.auto_disabled = true;
    enemy.reaction.stagger.duration = 40;
    enemy.body.collider = Some(crate::Collider::sphere(2.));
    enemy.movement.direction = [1., 0., 0.];
    enemy.facing_direction = [1., 0., 0.];
    let mut attacker = actor(Side::Party);
    attacker.position = [1000., 0., 0.];
    let mut prepared = crate::tests::prepare_actors(vec![enemy, attacker], 1);
    prepared.resources.actor_setup[0].contact_recovery = true;
    prepared.finish().unwrap()
}

#[test]
fn automatic_breakfall_tries_once_on_descent_and_retains_the_decision() {
    use crate::knockdown::sample_contact_recovery;

    for (control, roll, expected) in [
        (Control::Auto, 49, true),
        (Control::Auto, 50, false),
        (Control::Enemy, 49, true),
        (Control::Enemy, 50, false),
    ] {
        let mut owner = actor(Side::Enemy);
        owner.control = control;
        owner.hp = 50;
        owner.equipment.max_hp = 100;
        owner.reaction.recoil.kind = RecoilKind::Launched;
        owner.position = [1000., 100., 0.];
        owner.movement.vertical = 1.;
        sample_contact_recovery(&mut owner, Activity::Hurt, true, false, || {
            panic!("still rising")
        });
        assert!(!owner.reaction.contact_recovery_command);
        owner.movement.vertical = 0.;
        sample_contact_recovery(&mut owner, Activity::Hurt, true, false, || roll);
        assert_eq!(owner.reaction.contact_recovery_command, expected);
        sample_contact_recovery(&mut owner, Activity::Hurt, true, false, || {
            panic!("already tried")
        });
        assert_eq!(owner.reaction.contact_recovery_command, expected);

        crate::reaction::enter_hurt(&mut owner);
        assert!(!owner.reaction.contact_recovery_command);
        sample_contact_recovery(&mut owner, Activity::Hurt, true, false, || 0);
        assert!(owner.reaction.contact_recovery_command);
    }
}

#[test]
fn enemy_breakfall_lands_and_recovers_without_a_model() {
    let mut battle = enemy_recovery_battle();
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::default());
    hit_recoil(&mut battle, 0, true);
    let hp = battle.actors[0].hp;
    battle.actors[0].reaction.contact_recovery_command = true;
    assert!(battle.begin_contact_recovery(ActorId(0), true, &mut vec![]));
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    let mut rose = false;
    let mut landed = false;
    for _ in 0..120 {
        battle.step(BattleInput::default()).unwrap();
        let actor = &battle.actors[0];
        assert_eq!(actor.hp, hp);
        rose |= actor.position[1] > 0.;
        landed |= battle.activity(ActorId(0)) == Activity::Recovering;
        if battle.activity(ActorId(0)) == Activity::Idle {
            break;
        }
    }
    assert!(rose && landed);
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(battle.actors[0].position[1], 0.);
    assert!(!battle.is_diagnostic());
    assert!(!battle.diagnostics.has_errors());
}

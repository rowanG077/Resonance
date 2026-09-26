use super::*;
use crate::{
    ContactSource, DamageKind, GuardRule, HitRule, HitShape, MeleeDefinition, ResourceBinding,
    Side,
    tests::{actor, prepared},
};

fn definition(anchors: Vec<u16>) -> Arc<MeleeDefinition> {
    Arc::new(MeleeDefinition {
        hit: HitRule {
            impact: None,
            arte: false,
            reaction: Default::default(),
            kind: DamageKind::Slash,
            power: crate::Power::Fixed(5),
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            guard: GuardRule::default(),
        },
        cooldown: 2,
        radius: 30.,
        height: 30.,
        shape: HitShape::Box,
        anchors,
        trail: None,
    })
}

fn battle(source: &str, mut actors: Vec<Actor>, duration: u16) -> Battle {
    for actor in &mut actors {
        actor.body.anchors = vec![[0.; 3]; 4];
        actor.body.points = vec![crate::HurtPoint {
            center: [0.; 3],
            radius: 1.,
        }];
    }
    let mut prepared = prepared(source, actors, duration);
    let p = Arc::get_mut(&mut prepared).unwrap();
    p.actions[0].phase = ActionPhase::Actor;
    p.actions[0].id = 1;
    p.actions[0].tp_cost = 0;
    p.actions[0].resources = vec![
        ResourceBinding::Melee(definition(vec![0, 1])),
        ResourceBinding::Melee(definition(vec![2, 3])),
    ];
    Battle::new(prepared)
}

fn request(actor: u8) -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(actor),
            action: 1,
            target: ActorId(actor ^ 1),
        }],
        ..Default::default()
    }
}

const TWO_WINDOWS: &str = r#"
    asset first: battle::Melee = "test/right";
    asset second: battle::Melee = "test/left";
    pub task run() {
        await battle::hit_window(first, ticks(1), ticks(2));
        await battle::hit_window(second, ticks(4), ticks(1));
    }
"#;

#[test]
fn contact_tp_depends_on_arte_guard_affinity_and_protection_and_clamps_per_hit() {
    use crate::{Affinity, GuardKind, GuardResult, HitProtection, ProtectionMode};
    for case in [
        "hit",
        "armor",
        "down",
        "avoid",
        "absorb",
        "immune",
        "guard",
        "special",
        "break",
        "lethal",
        "signed_lethal",
    ] {
        for arte in [false, true] {
            for tp in [0, 39, 40] {
                let mut owner = actor(Side::Party);
                owner.tp = tp;
                let mut target = actor(Side::Enemy);
                let mut hit = definition(vec![0]);
                let rule = &mut Arc::make_mut(&mut hit).hit;
                rule.arte = arte;
                match case {
                    "armor" => target.reaction.armor.threshold = 1,
                    "down" => target.reaction.protection.mode = ProtectionMode::Down,
                    "avoid" => {
                        target.side = Side::Party;
                        owner.side = Side::Enemy;
                        target.reaction.protection.mode = ProtectionMode::Recovery;
                    }
                    "absorb" => target.affinities[0] = Affinity::Absorb,
                    "immune" => target.affinities[0] = Affinity::Immune,
                    "guard" | "special" | "break" | "lethal" => {
                        target.guard.active = true;
                        target.guard.break_pressure = 10;
                        target.heading = 180.;
                        target.facing_direction =
                            crate::control::direction_from_heading(target.heading);
                        target.guard.kind = if case == "special" {
                            GuardKind::Special
                        } else {
                            GuardKind::Normal
                        };
                        rule.guard.breaks = case == "break";
                        if case == "lethal" {
                            target.hp = 1;
                        }
                    }
                    "signed_lethal" => {
                        target.affinities[0] = Affinity::Absorb;
                        rule.power = crate::Power::Fixed(32768);
                    }
                    _ => {}
                }
                let mut battle = battle("pub task run() {}", vec![owner, target], 20);
                let mut contacts = Contacts::default();
                contacts
                    .melee(ActorId(0), ActionId(1), &battle.actors[0], None, &hit)
                    .unwrap();
                let mut cues = vec![];
                contacts.resolve(&mut battle, &mut cues).unwrap();
                let result = cues
                    .iter()
                    .find_map(|cue| match cue {
                        Cue::Hit { result, .. } => Some(result),
                        _ => None,
                    })
                    .expect("one contact");
                match case {
                    "guard" | "special" => {
                        assert!(matches!(result.guard, GuardResult::Blocked { .. }))
                    }
                    "break" => assert_eq!(result.guard, GuardResult::Broken),
                    "avoid" => assert_eq!(result.protection, HitProtection::Avoided),
                    "down" => assert_eq!(result.protection, HitProtection::Reduced),
                    "armor" => assert!(result.armored),
                    "lethal" | "signed_lethal" => assert_eq!(battle.actors[1].hp, 0),
                    _ => {}
                }
                let gain =
                    !arte && matches!(case, "hit" | "armor" | "down" | "lethal" | "signed_lethal");
                assert_eq!(
                    battle.actors[0].tp,
                    (tp + u16::from(gain)).min(40),
                    "{case} arte={arte} tp={tp}"
                );
            }
        }
    }
}

#[test]
fn scripted_armor_keeps_the_action_alive_until_the_contact_after_its_threshold() {
    let source = r#"
        asset first: battle::Melee = "test/right";
        pub task run() {
            if battle::automatic_control() {
                battle::armor(2);
                spawn late();
                await battle::at_age(ticks(20));
            } else {
                await battle::hit_window(first, ticks(1), ticks(0));
                await battle::hit_window(first, ticks(4), ticks(0));
                await battle::hit_window(first, ticks(7), ticks(0));
            }
        }
        task late() {
            await battle::at_age(ticks(20));
            battle::heal_percent(battle::owner(), 50);
        }
    "#;
    let mut target = actor(Side::Enemy);
    target.control = crate::Control::Auto;
    target.reaction.armor.base = 1;
    let mut battle = battle(source, vec![actor(Side::Party), target], 30);
    let p = Arc::get_mut(&mut battle.prepared).unwrap();
    let ResourceBinding::Melee(hit) = &mut p.actions[0].resources[0] else {
        panic!()
    };
    Arc::make_mut(hit).hit.reaction.armor_damage = 1;
    let mut input = request(0);
    input.actions.extend(request(1).actions);
    battle.step(input).unwrap();
    let mut contacts = 0;
    for _ in 0..24 {
        let frame = battle.step(BattleInput::default()).unwrap();
        if let Some(result) = frame.cues.iter().find_map(|c| match c {
            Cue::Hit { result, .. } => Some(result),
            _ => None,
        }) {
            contacts += 1;
            assert_eq!(result.armored, contacts <= 2);
            assert_eq!(result.hp_change, -5);
            assert_eq!(
                frame.cues.contains(&Cue::Interrupted {
                    action: ActionId(2)
                }),
                contacts == 3
            );
            assert_eq!(
                frame.actors[1].reaction.combo_hits,
                i32::from(contacts == 3)
            );
        }
    }
    assert_eq!(contacts, 3);
    assert_eq!(battle.actors[1].hp, 35); // The cancelled heal never runs.
    assert_eq!(
        battle.actors[1].reaction.armor,
        crate::Armor {
            base: 1,
            threshold: 1,
            received: 0
        }
    );
}

#[test]
fn armor_script_calls_require_an_actor_and_a_byte_threshold() {
    for threshold in [-1, 256] {
        let mut battle = battle(
            &format!("pub task run() {{ battle::armor({threshold}); }}"),
            vec![actor(Side::Party), actor(Side::Enemy)],
            3,
        );
        assert!(
            battle
                .step(request(0))
                .unwrap_err()
                .to_string()
                .contains("invalid battle armor threshold")
        );
    }
    let mut battle = Battle::new(prepared(
        "pub task run() { battle::armor(2); }",
        vec![actor(Side::Party), actor(Side::Enemy)],
        3,
    ));
    let input = BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: 99,
            target: ActorId(1),
        }],
        ..Default::default()
    };
    assert!(
        battle
            .step(input)
            .unwrap_err()
            .to_string()
            .contains("armor needs an actor sequence")
    );
}

#[test]
fn enemy_recovery_completion_and_explicit_cancellation_release_action_armor() {
    for ending in [
        "battle::finish();",
        "await battle::recover(ticks(2)); battle::finish();",
        "await battle::at_age(ticks(30));",
    ] {
        let source = format!(
            "pub task run() {{ battle::armor(5); await battle::at_age(ticks(1)); {ending} }}"
        );
        let mut owner = actor(Side::Enemy);
        owner.reaction.armor.base = 2;
        let mut battle = battle(&source, vec![owner, actor(Side::Party)], 30);
        let frame = battle.step(request(0)).unwrap();
        assert_eq!(frame.actors[0].reaction.armor.threshold, 5);
        battle.actors[0].reaction.armor.received = 3;
        let input = if ending.contains("at_age") {
            BattleInput {
                interrupt: vec![ActionId(1)],
                ..Default::default()
            }
        } else {
            BattleInput::default()
        };
        let frame = battle.step(input).unwrap();
        if ending.contains("recover") {
            assert_eq!(frame.actors[0].reaction.armor.threshold, 0);
            assert_eq!(frame.actors[0].reaction.armor.received, 0);
            for _ in 0..3 {
                battle.step(BattleInput::default()).unwrap();
            }
        }
        assert_eq!(
            battle.actors[0].reaction.armor,
            crate::Armor {
                base: 2,
                threshold: 2,
                received: 0
            }
        );
    }
}

#[test]
fn contact_enters_hurt_and_cancels_owned_tasks_before_the_next_actor_visit() {
    let source = r#"
        asset first: battle::Melee = "test/right";
        asset second: battle::Melee = "test/left";
        pub task run() {
            spawn late();
            await battle::hit_window(first, ticks(1), ticks(0));
            await battle::at_age(ticks(20));
        }
        task late() {
            await battle::at_age(ticks(3));
            battle::heal_percent(battle::owner(), 50);
        }
    "#;
    let mut battle = battle(source, vec![actor(Side::Party), actor(Side::Enemy)], 30);
    let mut input = request(0);
    input.actions.extend(request(1).actions);
    battle.step(input).unwrap();
    battle.actors[1].hit_stop = 3;
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(recipients(&frame), [ActorId(1)]);
    assert!(frame.cues.contains(&Cue::Interrupted {
        action: ActionId(2)
    }));
    assert_eq!(frame.actors[1].activity, crate::Activity::Hurt);
    assert_eq!(frame.actors[1].hit_stop, 0);
    assert_eq!(frame.actors[1].reaction.remaining, 2); // Even zero source hitstun flinches.
    assert_eq!(frame.actors[1].reaction.combo_hits, 1);
    assert_eq!(frame.actors[1].reaction.combo_damage, 5);
    assert_eq!(frame.actors[1].movement.braking, 0.275);
    assert!(!battle.sequences.contains_key(&ActionId(2)));
    for _ in 0..6 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[1].hp, 45);
        assert!(!frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Recovered {
                actor: ActorId(1),
                ..
            }
        )));
    }
}

fn recipients(frame: &BattleFrame) -> Vec<ActorId> {
    frame
        .cues
        .iter()
        .filter_map(|c| match c {
            Cue::Hit {
                source: ContactSource::Melee { .. },
                actor,
                ..
            } => Some(*actor),
            _ => None,
        })
        .collect()
}

#[test]
fn lloyd_finisher_script_matches_original_serial_clock_and_contact_origins() {
    let source = include_str!("../../../../scripts/battle/normal_lloyd.sym")
        .replace("script battle;", "")
        .replace("use battle;", "");
    let mut battle = battle(
        &(source + "\npub task run() { await finisher_hits(); }"),
        vec![actor(Side::Party), actor(Side::Enemy)],
        50,
    );
    battle
        .start(request(0).actions[0], &mut Vec::new())
        .unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-melee.json")).unwrap();
    for row in fixture["observations"].as_array().unwrap() {
        assert_eq!(
            battle.sequences[&ActionId(1)].hit_age,
            row["clock"].as_i64().unwrap() as i16
        );
        let expected = row["contacts"].as_array().unwrap();
        for contact in expected {
            let index = contact["anchor"].as_u64().unwrap() as usize;
            battle.actors[0].body.anchors[index] = std::array::from_fn(|i| {
                f32::from_bits(contact["position_bits"][i].as_u64().unwrap() as u32)
            });
        }
        let mut contacts = Contacts::default();
        battle
            .advance_sequences(ActionPhase::Actor, &[], &[], &mut contacts, &mut Vec::new())
            .unwrap();
        assert_eq!(
            battle.sequences[&ActionId(1)].hit_age,
            row["clock_after"].as_i64().unwrap() as i16,
            "{row}"
        );
        if row["advanced"].as_bool().unwrap() {
            assert!(battle.sequences[&ActionId(1)].melee.is_none());
        }
        assert_eq!(contacts.0[0].len(), expected.len(), "{row}");
        assert!(contacts.0[1].is_empty());
        for (contact, expected) in contacts.0[0].iter().zip(expected) {
            assert_eq!(
                contact.source,
                ContactSource::Melee {
                    actor: ActorId(0),
                    action: ActionId(1)
                }
            );
            assert_eq!(
                contact.position.map(f32::to_bits),
                std::array::from_fn(|i| expected["position_bits"][i].as_u64().unwrap() as u32)
            );
        }
    }
}

#[test]
fn separate_origins_share_actor_hit_cache_and_new_windows_rearm_all_targets() {
    let mut battle = battle(
        TWO_WINDOWS,
        vec![actor(Side::Party), actor(Side::Enemy), actor(Side::Enemy)],
        20,
    );
    assert!(recipients(&battle.step(request(0)).unwrap()).is_empty());
    assert_eq!(
        recipients(&battle.step(BattleInput::default()).unwrap()),
        [ActorId(1), ActorId(2)]
    );
    for _ in 0..3 {
        assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
    // First window ended at hit age 3 without advancing; new start 4 is update 5.
    assert_eq!(
        recipients(&battle.step(BattleInput::default()).unwrap()),
        [ActorId(1), ActorId(2)]
    );
    assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    assert_eq!((battle.actors[1].hp, battle.actors[2].hp), (40, 40));
}

#[test]
fn paused_and_cancelled_hit_tasks_neither_advance_nor_submit() {
    let mut battle = battle(
        TWO_WINDOWS,
        vec![actor(Side::Party), actor(Side::Enemy)],
        20,
    );
    battle.step(request(0)).unwrap();
    for _ in 0..3 {
        let frame = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert!(recipients(&frame).is_empty());
        assert_eq!(battle.sequences[&ActionId(1)].hit_age, 1);
    }
    battle
        .step(BattleInput {
            interrupt: vec![ActionId(1)],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..3 {
        assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
    assert_eq!(battle.random_state(), 1);
    assert_eq!(battle.actors[1].hp, 50);
}

#[test]
fn returning_parent_cancels_child_window_before_submission() {
    let source = r#"
        asset first: battle::Melee = "test/right";
        pub task child() { await battle::hit_window(first, ticks(2), ticks(10)); }
        pub task run() { spawn child(); await battle::next_update(); }
    "#;
    let mut battle = battle(source, vec![actor(Side::Party), actor(Side::Enemy)], 20);
    battle.step(request(0)).unwrap();
    for _ in 0..4 {
        assert!(recipients(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
    assert!(battle.sequences[&ActionId(1)].melee.is_none());
    assert_eq!(battle.random_state(), 1);
}

#[test]
fn simultaneous_melee_contacts_survive_the_owners_defeat_in_the_same_contact_pass() {
    let source = r#"asset hit: battle::Melee = "test/hit";
        pub task run() { await battle::hit_window(hit, ticks(0), ticks(0)); }"#;
    let mut party = actor(Side::Party);
    party.hp = 5;
    let mut enemy = actor(Side::Enemy);
    enemy.hp = 5;
    let mut battle = battle(source, vec![party, enemy], 20);
    let mut input = request(1);
    input.actions.extend(request(0).actions);
    let frame = battle.step(input).unwrap();
    assert_eq!(recipients(&frame), [ActorId(1), ActorId(0)]);
    assert_eq!(
        frame.actors.iter().map(|a| a.hp).collect::<Vec<_>>(),
        [0, 0]
    );
    assert!(frame.outcome.is_none());
    assert_eq!(frame.recognized_result, None);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.recognized_result, Some(BattleResult::Defeat));
}

#[test]
fn hit_windows_reject_resident_phase_missing_pose_and_overlapping_tasks() {
    for (source, missing_pose, resident) in [
        (
            "pub task run() { await battle::hit_window(hit, ticks(0), ticks(1)); }",
            false,
            true,
        ),
        (
            "pub task run() { await battle::hit_window(hit, ticks(0), ticks(1)); }",
            true,
            false,
        ),
        (
            "pub task child() { await battle::hit_window(hit, ticks(0), ticks(1)); } pub task run() { spawn child(); await battle::hit_window(hit, ticks(0), ticks(1)); }",
            false,
            false,
        ),
        (
            "pub task run() { await battle::hit_window(hit, ticks(32767), ticks(1)); }",
            false,
            false,
        ),
    ] {
        let mut battle = battle(
            &format!("asset hit: battle::Melee = \"test/hit\"; {source}"),
            vec![actor(Side::Party), actor(Side::Enemy)],
            20,
        );
        if missing_pose {
            battle.actors[0].body.anchors.clear();
        }
        if resident {
            Arc::get_mut(&mut battle.prepared).unwrap().actions[0].phase = ActionPhase::Resident;
        }
        assert!(battle.step(request(0)).is_err());
        assert!(battle.sequences.is_empty());
        assert!(battle.step(BattleInput::default()).is_err());
    }
}

#[test]
fn projectile_can_clash_with_an_already_submitted_melee_origin_without_consuming_it() {
    let source = r#"asset hit: battle::Melee = "test/hit";
        pub task run() { await battle::hit_window(hit, ticks(0), ticks(1)); }"#;
    let mut battle = battle(source, vec![actor(Side::Party), actor(Side::Enemy)], 20);
    let definition = Arc::new(ProjectileDefinition {
        motion: Default::default(),
        effects: Default::default(),
        lifetime: 20,
        velocity: [0.; 3],
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: false,
        active: None,
        birth: None,
        contact: Some(crate::ProjectileContact {
            hit: definition(vec![]).hit,
            cooldown: 1,
            repeat_limit: 0,
            radius: 30.,
            height: 30.,
            shape: HitShape::Box,
            offset: [0.; 3],
            radius_growth: 0.,
            height_growth: 0.,
            survives_contact: true,
            clash_effect: Some(crate::EffectAppearance {
                resource: 19,
                member: 11,
            }),
        }),
    });
    Arc::get_mut(&mut battle.prepared)
        .unwrap()
        .effects
        .insert(19, crate::tests::effect_binding(19, [11]));
    battle
        .emit(definition, ActionId(9), ActorId(1), ActorId(0), [0.; 3])
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(request(0)).unwrap();
    assert_eq!(recipients(&frame), [ActorId(1)]);
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::ProjectileClashed {
            other: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1)
            },
            ..
        }
    )));
    assert!(frame.projectiles[0].disarmed);
    assert_eq!(frame.actors[0].hp, 50);
}

#[test]
fn actor_emissions_initialize_in_the_same_update_but_resident_emissions_wait() {
    let source = r#"asset shot: battle::Projectile = "test/shot";
        pub task run() { battle::emit(shot, battle::ground_point(battle::owner(), 0.0, 0.0)); }"#;
    for phase in [ActionPhase::Actor, ActionPhase::Resident] {
        let projectile = ProjectileDefinition {
            motion: Default::default(),
            effects: Default::default(),
            lifetime: 4,
            velocity: [1., 0., 0.],
            acceleration: [0.; 3],
            offset: [0.; 3],
            clamp_ground: false,
            active: None,
            birth: None,
            contact: None,
        };
        let mut battle = crate::tests::projectile_battle(
            source,
            vec![actor(Side::Party), actor(Side::Enemy)],
            10,
            projectile,
        );
        Arc::get_mut(&mut battle.prepared).unwrap().actions[0].phase = phase;
        let frame = battle
            .step(BattleInput {
                actions: vec![ActionRequest {
                    actor: ActorId(0),
                    action: 99,
                    target: ActorId(1),
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            frame.projectiles.len(),
            usize::from(phase == ActionPhase::Actor)
        );
        if phase == ActionPhase::Actor {
            assert_eq!(frame.projectiles[0].position, [1., 0., 0.]);
        } else {
            assert_eq!(
                battle.step(BattleInput::default()).unwrap().projectiles[0].position,
                [1., 0., 0.]
            );
        }
    }
}

#[test]
fn contact_extends_its_first_blade_timer_while_commands_replace_and_narrow() {
    let mut battle = battle(
        TWO_WINDOWS,
        vec![actor(Side::Party), actor(Side::Enemy)],
        20,
    );
    let prepared = Arc::get_mut(&mut battle.prepared).unwrap();
    for resource in &mut prepared.actions[0].resources {
        if let ResourceBinding::Melee(definition) = resource {
            Arc::make_mut(definition).trail = Some(1);
        }
    }
    battle.trail_timers[0][1] = 20;
    battle.step(request(0)).unwrap();
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.trail_timers[0][1], 18); // duration2 cannot shorten20
    battle.trail_timers[0][1] = 0;
    for _ in 0..4 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.trail_timers[0][1], 3); // duration1 extends to4, then2503C decrements

    let mut battle = self::battle(
        "pub task run() { battle::weapon_trail(1, ticks(346)); await battle::at_age(ticks(5)); }",
        vec![actor(Side::Party), actor(Side::Enemy)],
        10,
    );
    battle.trail_timers[0][1] = 200;
    battle.step(request(0)).unwrap();
    assert_eq!(battle.trail_timers[0][1], 89); // op13 u8 narrowing, then common timer
}

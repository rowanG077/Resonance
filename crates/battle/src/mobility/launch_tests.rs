use super::*;

#[test]
fn launch_waits_for_hold_then_rises_falls_and_lands() {
    let mut battle = recovery_battle(Control::Manual);

    hit_recoil(&mut battle, 2, true);
    battle.actors[0].hit_stop = 2;
    let position = battle.actors[0].position;
    for _ in 0..battle.actors[0].hit_stop {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[0].position, position);
    }
    battle.step(BattleInput::default()).unwrap();
    assert!(battle.actors[0].position[1] > position[1]);
    let mut fell = false;
    for _ in 0..50 {
        battle.step(BattleInput::default()).unwrap();
        fell |= battle.actors[0].movement.vertical < 0.;
        if battle.activity(ActorId(0)) == Activity::KnockedDown {
            break;
        }
    }
    assert!(fell);
    assert_eq!(battle.activity(ActorId(0)), Activity::KnockedDown);
    assert_eq!(
        battle.actors[0].reaction.recoil.kind,
        RecoilKind::LandingLaunch
    );
}

#[test]
fn launch_impact_preserves_landed_bit_for_contact_getup_and_is_once_only() {
    let mut impact_update = None;
    for replay_guard in [false, true] {
        let mut battle = recovery_battle(Control::Manual);
        battle.actors[0].reaction.stagger.duration = 2;
        let mut model = crate::tests::model_definition(11, (0..=6).map(|clip| (clip, 3.)));
        model.idle_motions[0] = Some(0);
        model.reactions = crate::ReactionMotions {
            hurt: [Some(1); 2],
            down: Some(2),
            get_up: Some(3),
            rising: Some(4),
            rising_blend: 4,
            falling: Some(5),
            falling_blend: 16,
            breakfall: Some(6),
            ..Default::default()
        };
        set_model(&mut battle, model);

        hit_recoil(&mut battle, 0, true);
        let hp = battle.actors[0].hp;
        let nominal = battle.runtime[0].recovery.last_hit_damage;
        let impact = (i64::from(nominal) * 25 / 100) as i32;
        let mut getup = false;
        let mut down_finished = false;
        let mut impacts = 0;
        for update in 0..150 {
            let request_guard = replay_guard && impact_update == Some(update);
            let mut frame = battle
                .step(if request_guard {
                    guard(true)
                } else {
                    BattleInput::default()
                })
                .unwrap();
            battle
                .advance_models(&mut frame, crate::BattleClock::Running, true)
                .unwrap();
            let models = battle.models.as_ref().unwrap();
            let pose = models.main_motion_observation(ActorId(0)).unwrap();
            down_finished |= pose.clip == 2 && pose.finished;
            if frame
                .cues
                .contains(&Cue::KnockdownImpact { actor: ActorId(0) })
            {
                impacts += 1;
                assert_eq!(
                    (pose.clip, pose.frame),
                    (2, 0.),
                    "impact follows the down transition, before playback advances"
                );
                assert!(!models.motion_blending(ActorId(0)));
                if replay_guard {
                    assert!(request_guard);
                } else {
                    impact_update = Some(update);
                }
            }
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Breakfall { .. })),
                "impact closes the breakfall window before Guard is consumed"
            );
            let kind = battle.actors[0].reaction.recoil.kind;
            if kind == RecoilKind::SettledLaunch {
                assert_eq!(battle.actors[0].hp, hp - impact);
                assert_eq!(battle.runtime[0].recovery.last_hit_damage, impact);
            }
            if battle.activity(ActorId(0)) == Activity::GettingUp {
                getup = true;
                assert!(down_finished, "get-up waits for the down pose to finish");
                assert_eq!(kind, RecoilKind::SettledLaunch);
            }
            if impacts > 0 && battle.activity(ActorId(0)) == Activity::Idle {
                break;
            }
        }
        assert!(getup);
        assert_eq!(impacts, 1);
        let protection = battle.actors[0].reaction.protection;
        let remaining = protection.remaining;
        assert!(remaining > 0);
        assert_eq!(protection.mode, crate::ProtectionMode::Recovery);
        let budget = (
            battle.runtime[0].recovery.last_hit_damage,
            battle.runtime[0].recovery.last_hit_recovery,
        );
        let assert_settled = |battle: &Battle| {
            let actor = &battle.actors[0];
            let recovery = &battle.runtime[0].recovery;
            assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
            assert_eq!(actor.reaction.recoil.kind, RecoilKind::SettledLaunch);
            assert_eq!(actor.hp, hp - impact);
            assert_eq!(
                (recovery.last_hit_damage, recovery.last_hit_recovery),
                budget
            );
        };
        assert_settled(&battle);
        for expected in (0..remaining).rev() {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.actors[0].reaction.protection.remaining, expected);
            assert_settled(&battle);
        }
        assert_eq!(battle.actors[0].reaction.protection, Default::default());
        hit_recoil(&mut battle, 0, false);
        assert_eq!(battle.actors[0].reaction.recoil.kind, RecoilKind::Down);
    }
}

#[test]
fn automatic_down_recovery_uses_actor_state_without_a_pose() {
    let mut battle = recovery_battle(Control::Auto);

    battle.enter_knockdown(ActorId(0), &mut vec![]);
    battle.actors[0].reaction.recoil.kind = RecoilKind::Down;
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    assert_eq!(battle.actors[0].reaction.recoil.kind, RecoilKind::Normal);
}

#[test]
fn automatic_breakfall_waits_for_descent_and_survives_hit_stop() {
    let mut battle = recovery_battle(Control::Auto);
    hit_recoil(&mut battle, 0, true);
    battle.actors[0].position[1] = 100.;
    battle.actors[0].movement.vertical = 2.;
    battle.actors[0].hp = battle.actors[0].equipment.max_hp;
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
    battle.actors[0].movement.vertical = 0.;
    battle.actors[0].hit_stop = 3;
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
    step_until(&mut battle, Activity::Jumping, 4);
    assert_eq!(battle.actors[0].reaction.recoil.kind, RecoilKind::Normal);
}

#[test]
fn settled_launch_survives_equipment_reload_but_is_rejected_as_initial_state() {
    let mut battle = recovery_battle(Control::Manual);
    battle.actors[0].reaction.recoil.kind = RecoilKind::SettledLaunch;
    battle
        .replace_equipment_batch(vec![crate::EquipmentReplacement {
            actor: ActorId(0),
            attributes: battle.actors[0].equipment.clone(),
            conditions: battle.actors[0].conditions.clone(),
            equipment: None,
        }])
        .unwrap();
    assert_eq!(
        battle.actors[0].reaction.recoil.kind,
        RecoilKind::SettledLaunch
    );
    assert!(
        PreparedBattle::new(
            vec![(battle.actors[0].clone(), Default::default())],
            Default::default(),
            1
        )
        .is_err()
    );
}

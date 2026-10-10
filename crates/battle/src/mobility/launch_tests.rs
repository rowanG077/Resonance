use super::*;

#[test]
fn launch_waits_for_hold_then_rises_falls_and_lands() {
    let mut battle = recovery_battle(Control::Manual);

    hit_recoil(&mut battle, 2, true);
    battle.actors[0].hit_stop = 2;
    let position = battle.actors[0].position;
    for _ in 0..3 {
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
    let mut battle = recovery_battle(Control::Manual);

    hit_recoil(&mut battle, 0, true);
    let hp = battle.actors[0].hp;
    let nominal = battle.runtime[0].recovery.last_hit_damage;
    let impact = (i64::from(nominal) * 25 / 100) as i32;
    let mut impacted = false;
    let mut getup = false;
    for _ in 0..150 {
        battle.step(BattleInput::default()).unwrap();
        let kind = battle.actors[0].reaction.recoil.kind;
        if kind == RecoilKind::SettledLaunch {
            impacted = true;
            assert_eq!(battle.actors[0].hp, hp - impact);
            assert_eq!(battle.runtime[0].recovery.last_hit_damage, impact);
        }
        if battle.activity(ActorId(0)) == Activity::GettingUp {
            getup = true;
            assert_eq!(kind, RecoilKind::SettledLaunch);
        }
        if impacted && battle.activity(ActorId(0)) == Activity::Idle {
            break;
        }
    }
    assert!(impacted && getup);
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(
        battle.actors[0].reaction.recoil.kind,
        RecoilKind::SettledLaunch
    );
    let remaining = battle.actors[0].reaction.protection.remaining;
    assert!(remaining > 0);
    assert_eq!(
        battle.actors[0].reaction.protection.mode,
        crate::ProtectionMode::Recovery
    );
    let budget = (
        battle.runtime[0].recovery.last_hit_damage,
        battle.runtime[0].recovery.last_hit_recovery,
    );
    for expected in (0..remaining).rev() {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[0].reaction.protection.remaining, expected);
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(
            battle.actors[0].reaction.recoil.kind,
            RecoilKind::SettledLaunch
        );
        assert_eq!(battle.actors[0].hp, hp - impact);
        assert_eq!(
            (
                battle.runtime[0].recovery.last_hit_damage,
                battle.runtime[0].recovery.last_hit_recovery,
            ),
            budget
        );
    }
    assert_eq!(battle.actors[0].reaction.protection, Default::default());
    hit_recoil(&mut battle, 0, false);
    assert_eq!(battle.actors[0].reaction.recoil.kind, RecoilKind::Down);
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
    for _ in 0..4 {
        battle.step(BattleInput::default()).unwrap();
        if battle.activity(ActorId(0)) == Activity::Jumping {
            break;
        }
    }
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
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

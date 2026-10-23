use super::*;

fn read(
    bytes: &[u8],
    projectiles: &[crate::battle_projectile::EnemyProjectile],
) -> Result<EnemyActions> {
    super::read(bytes, projectiles, &[])
}

#[test]
fn spell_rows_keep_their_authored_armor_including_zero() -> Result<()> {
    let mut bytes = package();
    bytes[580..582].copy_from_slice(&204_u16.to_be_bytes());
    for threshold in [0, 4, 5, 9] {
        bytes[516 + 49] = threshold;
        let actions = read(&bytes, &[])?;
        let row = &actions.rows[0];
        assert!(matches!(row.execution, Some(EnemyExecution::Spell(204))));
        assert_eq!(row.cast_armor, Some(threshold));
        let decoded: EnemyActions = serde_json::from_slice(&serde_json::to_vec(&actions)?)?;
        assert_eq!(decoded.rows[0].cast_armor, Some(threshold));
    }
    bytes[580..582].fill(0);
    assert_eq!(read(&bytes, &[])?.rows[0].cast_armor, None);
    Ok(())
}

#[test]
fn completed_enemy_pose_keeps_its_final_sample_and_then_blends_to_the_next_pose() -> Result<()> {
    let mut bytes = package();
    bytes[693] = 4;
    bytes[700..702].copy_from_slice(&(-3_i16).to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!(
            "missing continued pose: {:?}",
            parsed.rows[0].unsupported_reason
        )
    };
    assert!(attack.events.iter().any(|(at, event)| matches!(
        event, EnemyEvent::Pose(pose) if *at == 9 && pose.clip == 31
    )));
    assert!(attack.events.iter().any(|(at, event)| matches!(
        event, EnemyEvent::Contact(_) if *at == 28
    )));
    assert_eq!(attack.duration, 77);

    // A chain follows each clip's completion, including its own blend, rather
    // than shifting a completion trigger as though it were an absolute age.
    bytes[705] = 4;
    bytes[712..714].copy_from_slice(&(-3_i16).to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!("missing continued pose chain")
    };
    let poses: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| match event {
            EnemyEvent::Pose(pose) => Some((*at, pose.clip)),
            _ => None,
        })
        .collect();
    assert_eq!(poses, [(0, 30), (9, 31), (16, 32)]);
    Ok(())
}

#[test]
fn direct_releases_keep_attack_timing_while_physical_artes_do_not_chant() -> Result<()> {
    let mut bytes = package();
    bytes[504..506].copy_from_slice(&81_u16.to_be_bytes());
    for (opcode, slot) in [(38_u16, SpellSlot::Primary), (39, SpellSlot::Secondary)] {
        bytes[614..616].copy_from_slice(&opcode.to_be_bytes());
        bytes[616..618].copy_from_slice(&204_u16.to_be_bytes());
        let actions = read(&bytes, &[])?;
        let row = &actions.rows[0];
        assert!(row.unsupported_reason.is_none());
        assert_eq!(row.cast_ticks, None);
        let Some(EnemyExecution::Attack(attack)) = &row.execution else {
            panic!("direct release did not keep its attack")
        };
        assert_eq!(attack.duration, 77);
        assert!(attack.events.iter().any(|(at, event)| {
            *at == 28
                && matches!(event, EnemyEvent::ReleaseSpell { technique: 204, slot: actual } if *actual == slot)
        }));
        assert!(attack.events.iter().any(|(_, event)| {
            matches!(event, EnemyEvent::Contact(contact) if contact.hit.sound == Some(81))
        }));
    }

    bytes[0x20 + 30 * 4..0x24 + 30 * 4].fill(0);
    let absent = read(&bytes, &[])?;
    assert!(matches!(&absent.rows[0].execution,
        Some(EnemyExecution::Attack(attack)) if attack.duration == 74
            && attack.events.iter().any(|(at, event)| matches!(event,
                EnemyEvent::Pose(pose) if *at == 0 && pose.clip == 30 && pose.blend == 0))
            && attack.events.iter().any(|(at, event)| matches!(event,
                EnemyEvent::Contact(_) if *at == 25))));

    bytes[580..582].copy_from_slice(&81_u16.to_be_bytes());
    let member = bytes.len() as u32;
    bytes.push(0);
    for clip in [30, 31, 32] {
        let pointer = 0x20 + clip * 4;
        bytes[pointer..pointer + 4].copy_from_slice(&member.to_be_bytes());
    }
    let header = 4 + 82 * 4;
    let mut common = vec![0; header + 44 + 28];
    common[..4].copy_from_slice(&82_u32.to_be_bytes());
    common[4 + 81 * 4..4 + 82 * 4].copy_from_slice(&(header as u32).to_be_bytes());
    common[header..header + 4].copy_from_slice(&44_u32.to_be_bytes());
    common[header + 4..header + 8].copy_from_slice(&72_u32.to_be_bytes());
    common[header + 18..header + 20].copy_from_slice(&12_u16.to_be_bytes());
    common[header + 44..].copy_from_slice(&bytes[488..516]);
    common[header + 44] |= 8;
    common[header + 44 + 13] = 1;
    common[header + 44 + 14..header + 44 + 16].copy_from_slice(&80_u16.to_be_bytes());
    common[header + 44 + 22] = 96;
    let bank = bytes.len();
    bytes[0x1cc..0x1d0].copy_from_slice(&(bank as u32).to_be_bytes());
    bytes.resize(bank + 226, 0);
    bytes[bank + 4] = 2;
    bytes[bank + 10..bank + 12].copy_from_slice(&20_u16.to_be_bytes());
    bytes[bank + 16..bank + 18].copy_from_slice(&32_u16.to_be_bytes());
    bytes[bank + 20..bank + 26].copy_from_slice(&[0, 0, 254, 0, 0, 0]);
    let actions = super::read(&bytes, &[], &common)?;
    assert_eq!(actions.rows[0].cast_ticks, None);
    let Some(EnemyExecution::Physical { technique, attack }) = &actions.rows[0].execution else {
        panic!("physical arte lost its local timeline")
    };
    assert_eq!(*technique, 81);
    assert_eq!(attack.duration, 76);
    assert_eq!(attack.recovery_ticks, 12);
    assert_eq!(attack.recovery_pose.clip, 0);
    assert_eq!(attack.recovery_pose.rate, 0.5);
    assert_eq!(attack.recovery_pose.blend, 12);
    assert!(attack.events.iter().any(|(_, event)| matches!(event,
        EnemyEvent::Contact(contact) if contact.hit.power == Power::Percent(80)
            && contact.hit.effect == Some(0) && contact.hit.effect_on_guard
            && contact.hit.sound == Some(81))));
    assert!(attack.events.iter().any(|(_, event)| matches!(event,
        EnemyEvent::Pose(pose) if pose.clip == 30)));
    bytes[bank + 225] = 1;
    assert!(
        super::read(&bytes, &[], &common)?.rows[0]
            .unsupported_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("not an empty alias"))
    );
    bytes[bank + 225] = 0;
    // A null opening motion retains the live pose for the admission update,
    // then starts the remaining authored streams without a blend hold.
    let pointer = 0x20 + 30 * 4;
    bytes[pointer..pointer + 4].fill(0);
    let absent = super::read(&bytes, &[], &common)?;
    assert!(matches!(&absent.rows[0].execution,
        Some(EnemyExecution::Physical { attack, .. }) if attack.duration == 73
            && !attack.events.iter().any(|(_, event)| matches!(event,
                EnemyEvent::Pose(pose) if pose.clip == 30))
            && attack.events.iter().any(|(at, event)| matches!(event,
                EnemyEvent::Pose(pose) if *at == 20 && pose.clip == 31))));
    common.splice(header + 44..header + 44, [0; 28]);
    common[header..header + 4].copy_from_slice(&72_u32.to_be_bytes());
    common[header + 4..header + 8].copy_from_slice(&100_u32.to_be_bytes());
    common[header + 46..header + 48].copy_from_slice(&17_u16.to_be_bytes());
    for technique in [1_u16, 2, 4] {
        bytes[580..582].copy_from_slice(&technique.to_be_bytes());
        let entry = 4 + usize::from(technique) * 4;
        common[entry..entry + 4].copy_from_slice(&(header as u32).to_be_bytes());
        let actions = super::read(&bytes, &[], &common)?;
        assert!(matches!(&actions.rows[0].execution,
            Some(EnemyExecution::Physical { attack, .. })
                if attack.recovery_ticks == 17 && attack.recovery_pose.blend == 17));
    }
    bytes[580..582].copy_from_slice(&204_u16.to_be_bytes());
    let actions = read(&bytes, &[])?;
    assert_eq!(actions.rows[0].cast_ticks, Some(71));
    assert!(matches!(
        actions.rows[0].execution,
        Some(EnemyExecution::Spell(204))
    ));
    Ok(())
}

#[test]
fn enemy_items_keep_authored_ids_target_scope_and_blend_timing() -> Result<()> {
    for (opcode, item, time, target) in [
        (43_u16, 14_u16, 20_u16, 9),
        (43, 11, 20, 11),
        (43, 4, 24, 9),
        (44, 39, 24, 0),
    ] {
        let mut bytes = package();
        bytes[517] = target;
        bytes[612..614].copy_from_slice(&time.to_be_bytes());
        bytes[614..616].copy_from_slice(&opcode.to_be_bytes());
        bytes[616..618].copy_from_slice(&item.to_be_bytes());
        bytes[700..702].copy_from_slice(&(-2_i16).to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let row = &parsed.rows[0];
        assert!(row.unsupported_reason.is_none());
        if target == 11 {
            assert_eq!(row.target_policy, Some(TargetPolicy::NearestIncapacitated));
        }
        let Some(EnemyExecution::Attack(attack)) = &row.execution else {
            panic!("missing enemy item action")
        };
        assert!(attack.events.iter().any(|(at, event)| matches!(event,
            EnemyEvent::UseItem { item: actual, party }
                if *at == time + 4 && *actual == item && *party == (opcode == 44))));
        bytes[619] = 1;
        assert!(
            read(&bytes, &[])?.rows[0]
                .unsupported_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("enemy item modifier"))
        );
    }
    Ok(())
}

#[test]
fn position_commands_preserve_signed_units_and_reject_unresolved_operands() -> Result<()> {
    let mut bytes = package();
    let commands: [i16; 28] = [
        23, 45, -20, 80, 40, 0, 23, 46, 0, 300, -100, 0, 23, 47, 0, -80, 20, 0, 23, 41, 1800, 23,
        48, 0, 24, 0, 0, -1,
    ];
    bytes.splice(612..624, commands.into_iter().flat_map(i16::to_be_bytes));
    bytes[16..18].copy_from_slice(&668_u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&732_u16.to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!("missing positions: {:?}", parsed.rows[0].unsupported_reason)
    };
    assert!(matches!(
        &attack
            .events
            .iter()
            .filter(|(at, _)| *at == 28)
            .map(|(_, event)| event)
            .collect::<Vec<_>>()[..5],
        [
            EnemyEvent::SetPosition([-20, 80, 40]),
            EnemyEvent::PositionFromTarget {
                height: 300,
                retreat: -100
            },
            EnemyEvent::OffsetPosition {
                height: -80,
                retreat: 20
            },
            EnemyEvent::TurnMotion(180.),
            EnemyEvent::FaceTarget
        ]
    ));
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (29, EnemyEvent::Forward(0.))))
    );
    bytes[658..660].copy_from_slice(&1_i16.to_be_bytes());
    assert!(
        read(&bytes, &[])?.rows[0]
            .unsupported_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("facing mode"))
    );
    bytes[658..660].copy_from_slice(&0_i16.to_be_bytes());
    bytes[640..642].copy_from_slice(&400_i16.to_be_bytes());
    bytes[646..648].copy_from_slice(&1_i16.to_be_bytes());
    assert_eq!(
        serde_json::to_value(&read(&bytes, &[])?.rows[0].execution)?,
        serde_json::to_value(&parsed.rows[0].execution)?
    );
    bytes[628..630].copy_from_slice(&1_i16.to_be_bytes());
    assert!(
        read(&bytes, &[])?.rows[0]
            .unsupported_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("relative position mode"))
    );
    Ok(())
}

#[test]
fn effective_hit_waits_keep_the_contact_clock_without_requiring_capture() -> Result<()> {
    let mut bytes = package();
    bytes[488..490].copy_from_slice(&0x10_u16.to_be_bytes());
    // The earlier timestamp on the vertical command cannot bypass the preceding wait.
    let commands: [i16; 13] = [24, -5, 25, 20, 50, 20, 21, 150, 25, 18, 0, 30, -1];
    bytes.splice(612..624, commands.into_iter().flat_map(i16::to_be_bytes));
    bytes[16..18].copy_from_slice(&638_u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&702_u16.to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!("missing capture: {:?}", parsed.rows[0].unsupported_reason)
    };
    assert!(attack.events.iter().any(|event| matches!(
        event,
        (28, EnemyEvent::Contact(contact)) if contact.hit.capture
    )));
    assert!(attack.events.iter().any(|event| matches!(
        event,
        (29, EnemyEvent::WaitForEffectiveHit { deadline: 76 })
    )));
    assert!(matches!(
        attack
            .events
            .iter()
            .filter(|(at, _)| *at == 30)
            .map(|(_, event)| event)
            .collect::<Vec<_>>()
            .as_slice(),
        [
            EnemyEvent::CapturedForward(5.),
            EnemyEvent::CapturedVertical(15.),
            EnemyEvent::ReleaseCaptured { hitstun: 30 }
        ]
    ));
    bytes[488..490].fill(0);
    let ordinary = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &ordinary.rows[0].execution else {
        panic!(
            "ordinary hit wait was rejected: {:?}",
            ordinary.rows[0].unsupported_reason
        )
    };
    assert!(attack.events.iter().any(|event| matches!(
        event,
        (29, EnemyEvent::WaitForEffectiveHit { deadline: 76 })
    )));
    Ok(())
}

#[test]
fn secondary_wind_preserves_enable_and_duration_without_changing_action_timing() -> Result<()> {
    for (enabled, duration) in [(1_u16, 40_u16), (0, 40), (1, 80)] {
        let mut bytes = package();
        bytes[614..616].copy_from_slice(&23_u16.to_be_bytes());
        bytes[616..618].copy_from_slice(&enabled.to_be_bytes());
        bytes[618..620].copy_from_slice(&duration.to_be_bytes());
        let actions = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &actions.rows[0].execution else {
            panic!(
                "missing secondary wind: {:?}",
                actions.rows[0].unsupported_reason
            )
        };
        assert_eq!(attack.duration, 77);
        assert!(attack.events.iter().any(|(at, event)| matches!(event,
            EnemyEvent::SecondaryWind { enabled: actual, duration: ticks }
            if *at == 28 && *actual == (enabled != 0) && *ticks == duration)));
        bytes[616..618].copy_from_slice(&2_u16.to_be_bytes());
        assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    }
    Ok(())
}

#[test]
fn event_boss_actions_keep_armor_blends_and_reject_other_condition_commands() -> Result<()> {
    let mut bytes = package();
    let commands: [i16; 13] = [0, 12, 1, 100, 12, 14, 0, 0, 0, 0, 0, 1 << 12, -1];
    bytes.splice(612..624, commands.into_iter().flat_map(i16::to_be_bytes));
    bytes[16..18].copy_from_slice(&638_u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&702_u16.to_be_bytes());
    bytes[592] = 9;
    let parsed = read(&bytes, &[])?;
    assert!(parsed.policy.event_hp_hold);
    assert!(parsed.policy.unsupported_reason.is_none());
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!(
            "missing boss commands: {:?}",
            parsed.rows[0].unsupported_reason
        )
    };
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (4, EnemyEvent::ReactionArmor(100))))
    );
    assert!(attack.events.iter().any(|event| matches!(
        event,
        (
            16,
            EnemyEvent::StatUp {
                condition: Condition::AttackUp,
                amount: 0
            }
        )
    )));
    for (reserved, amount) in [(1000_u16, 50_i16), (500, 50), (0, 25), (0, 0)] {
        bytes[624..626].copy_from_slice(&reserved.to_be_bytes());
        bytes[626..628].copy_from_slice(&amount.to_be_bytes());
        assert!(matches!(
            &read(&bytes, &[])?.rows[0].execution,
            Some(EnemyExecution::Attack(attack)) if attack.events.iter().any(|event| matches!(event,
                (16, EnemyEvent::StatUp { condition: Condition::AttackUp, amount: actual }) if *actual == amount))
        ));
    }
    bytes[626..628].copy_from_slice(&(-1_i16).to_be_bytes());
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    bytes[626..628].copy_from_slice(&0_i16.to_be_bytes());
    bytes[628..630].copy_from_slice(&1_u16.to_be_bytes());
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    bytes[628..630].copy_from_slice(&0_u16.to_be_bytes());
    bytes[634..636].copy_from_slice(&(1_i16 << 13).to_be_bytes());
    assert!(matches!(
        &read(&bytes, &[])?.rows[0].execution,
        Some(EnemyExecution::Attack(attack)) if attack.events.iter().any(|event| matches!(event, (16, EnemyEvent::StatUp { condition: Condition::DefenseUp, amount: 0 })))
    ));
    bytes[634..636].copy_from_slice(&(1_i16 << 2).to_be_bytes());
    assert!(matches!(
        &read(&bytes, &[])?.rows[0].execution,
        Some(EnemyExecution::Attack(attack)) if attack.events.iter().any(|event| matches!(event, (16, EnemyEvent::Stun)))
    ));
    bytes[634..636].copy_from_slice(&(1_i16 << 3).to_be_bytes());
    assert!(
        read(&bytes, &[])?.rows[0]
            .unsupported_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("condition command"))
    );
    Ok(())
}

// A compact package with the zombie's first attack timings and contact rule.
fn package() -> Vec<u8> {
    let mut bytes = vec![0; 726];
    bytes[..4].copy_from_slice(b"em8\0");
    for (field, offset) in [
        (8, 488_u16),
        (10, 516),
        (12, 584),
        (14, 612),
        (16, 624),
        (18, 688),
    ] {
        bytes[field..field + 2].copy_from_slice(&offset.to_be_bytes());
    }
    bytes[491..496].copy_from_slice(&[20, 30, 1, 1, 1]);
    bytes[516..520].copy_from_slice(&[35, 2, 10, 0]);
    bytes[520..524].copy_from_slice(&0.5_f32.to_be_bytes());
    bytes[530..532].copy_from_slice(&71_u16.to_be_bytes());
    bytes[551] = 15;
    bytes[607] = 1;
    bytes[612..620].copy_from_slice(&[0, 23, 0, 28, 0, 60, 0, 0]);
    bytes[620..622].copy_from_slice(&(-1_i16).to_be_bytes());
    bytes[624..629].copy_from_slice(&[0, 23, 25, 1, 1]);
    bytes[632..636].copy_from_slice(&30_f32.to_be_bytes());
    bytes[636..640].copy_from_slice(&30_f32.to_be_bytes());
    bytes[643..645].copy_from_slice(&[1, 1]);
    bytes[656..658].copy_from_slice(&(-1_i16).to_be_bytes());
    for (index, (time, clip, blend)) in [(0_u16, 30, 4), (19, 31, 2), (39, 32, 2)]
        .into_iter()
        .enumerate()
    {
        let row = &mut bytes[688 + index * 12..700 + index * 12];
        row[..2].copy_from_slice(&time.to_be_bytes());
        row[2..8].copy_from_slice(&[clip, blend, 0, 0, 8, 255]);
        row[8..].copy_from_slice(&0.5_f32.to_be_bytes());
    }
    bytes[724..726].copy_from_slice(&(-2_i16).to_be_bytes());
    for clip in [30, 31, 32] {
        let pointer = 0x20 + clip * 4;
        bytes[pointer..pointer + 4].copy_from_slice(&488_u32.to_be_bytes());
    }
    bytes
}

#[test]
fn hit_wait_precedes_same_time_pose_and_excludes_unstarted_blends_from_timeout() -> Result<()> {
    let mut bytes = package();
    bytes[612..620].copy_from_slice(&[0, 19, 255, 253, 255, 255, 0, 0]);
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!("missing hit wait: {:?}", parsed.rows[0].unsupported_reason)
    };
    let events: Vec<_> = attack.events.iter().filter(|(at, _)| *at == 23).collect();
    assert!(matches!(
        events[0],
        (23, EnemyEvent::WaitForHit { deadline: 75 })
    ));
    assert!(matches!(events[1], (23, EnemyEvent::Pose(_))));
    assert_eq!(attack.duration, 77);

    bytes[614..616].copy_from_slice(&17_i16.to_be_bytes());
    bytes[616..620].copy_from_slice(&[0, 20, 0, 21]);
    assert!(
        read(&bytes, &[])?.rows[0]
            .unsupported_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("deadline extension pair"))
    );
    Ok(())
}

#[test]
fn deadline_extensions_preserve_earlier_hit_waits_and_event_timing() -> Result<()> {
    let mut bytes = package();
    let commands: [i16; 14] = [10, 17, 5, 5, 19, -3, 19, 17, 20, 20, 20, 0, 10, -1];
    bytes.splice(612..624, commands.into_iter().flat_map(i16::to_be_bytes));
    bytes[16..18].copy_from_slice(&640_u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&704_u16.to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!(
            "missing deadline extension: {:?}",
            parsed.rows[0].unsupported_reason
        )
    };
    assert_eq!(attack.duration, 102);
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (23, EnemyEvent::WaitForHit { deadline: 80 })))
    );
    assert!(attack.events.iter().any(|event| matches!(
        event, (25, EnemyEvent::Forward(speed)) if *speed == 1.
    )));
    Ok(())
}

#[test]
fn overlapping_melee_rows_keep_expiry_in_the_contact_clock() -> Result<()> {
    for (first, second, emission, blends, expected) in [
        // An unblended opening still consumes the admission update.
        (4_u16, 10_u16, 20, vec![(0_u16, 0)], [(5, 21), (26, 7)]),
        // An opening contact begins after the blend and keeps its authored window.
        (0, 10, 20, vec![(0, 4)], [(4, 21), (25, 11)]),
        // Recovery truncates the second contact on the action deadline update.
        (74, 80, 30, vec![(0, 0), (40, 4)], [(78, 31), (109, 6)]),
        // The contact crosses two later blends after the opening hold.
        (
            11,
            20,
            15,
            vec![(0, 4), (21, 4), (25, 4)],
            [(15, 22), (37, 10)],
        ),
        (4, 10, 20, vec![(0, 0), (27, 4)], [(5, 21), (26, 10)]),
    ] {
        let mut bytes = package();
        bytes.splice(688..688, [0; 32]);
        bytes[18..20].copy_from_slice(&720_u16.to_be_bytes());
        bytes[530..532].copy_from_slice(&110_u16.to_be_bytes());
        bytes[624..626].copy_from_slice(&first.to_be_bytes());
        bytes[626] = emission;
        bytes.copy_within(624..656, 656);
        bytes[656..658].copy_from_slice(&second.to_be_bytes());
        bytes[688..690].copy_from_slice(&(-1_i16).to_be_bytes());
        bytes.truncate(720);
        for (blend_at, blend) in blends {
            bytes.extend(blend_at.to_be_bytes());
            bytes.extend([30, blend, 0, 0, 8, 255]);
            bytes.extend(0.5_f32.to_be_bytes());
        }
        bytes.extend((-2_i16).to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!(
                "missing melee attack: {:?}",
                parsed.rows[0].unsupported_reason
            )
        };
        let contacts: Vec<_> = attack
            .events
            .iter()
            .filter_map(|(at, event)| match event {
                EnemyEvent::Contact(contact) | EnemyEvent::ContinueContact(contact) => {
                    Some((*at, contact.duration))
                }
                _ => None,
            })
            .collect();
        assert_eq!(contacts, expected);
        assert_eq!(
            attack
                .events
                .iter()
                .filter(|(_, event)| matches!(event, EnemyEvent::ContinueContact(_)))
                .count(),
            1
        );

        bytes[658] = 1;
        assert!(
            read(&bytes, &[])?.rows[0]
                .unsupported_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("fully elapsed enemy contact row"))
        );
    }
    Ok(())
}

#[test]
fn root_and_partial_blends_advance_different_clocks() -> Result<()> {
    for (flags, binding, trigger, blend, rate, duration, expected_sound) in [
        (8, 0, 10_i16, 4, 2_f32, 75, 27),
        (0x42, 5, 10, 4, 2., 72, 24),
        (0, 0, -2, 0, 0., 72, 24),
    ] {
        let mut bytes = package();
        bytes.truncate(688);
        bytes.extend(trigger.to_be_bytes());
        bytes.extend([30, blend, 0, 0, flags, binding]);
        bytes.extend(rate.to_be_bytes());
        bytes.extend((-2_i16).to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing pose: {:?}", parsed.rows[0].unsupported_reason)
        };
        assert_eq!(attack.duration, duration);
        assert!(
            attack.events.iter().any(|(at, event)| {
                *at == expected_sound && matches!(event, EnemyEvent::Sound(_))
            })
        );
        assert!(attack.events.iter().any(|(at, event)| {
            *at == 0
                && match event {
                    EnemyEvent::Pose(pose) => flags == 8 && pose.blend == blend,
                    EnemyEvent::PartialPose { first_bone, pose } => {
                        *first_bone == u16::from(binding)
                            && pose.blend == blend
                            && pose.rate == rate
                            && pose.repeat == (flags & 0x40 != 0)
                    }
                    _ => false,
                }
        }));
    }
    Ok(())
}

#[test]
fn partial_pose_groups_keep_their_own_timing_and_only_hold_gameplay_for_body_blends() -> Result<()>
{
    let mut bytes = package();
    bytes[530..532].copy_from_slice(&110_u16.to_be_bytes());
    bytes[612..614].copy_from_slice(&40_u16.to_be_bytes());
    bytes[624..626].copy_from_slice(&46_u16.to_be_bytes());
    bytes.truncate(688);
    for (time, body_clip, body_blend, body_start) in
        [(0_u16, 31, 1, 0), (40, 31, 4, 18), (70, 30, 1, 0)]
    {
        for (clip, blend, start, flags, binding) in [
            (0, 1, 0, 0, 5),
            (0, 1, 0, 1, 7),
            (body_clip, body_blend, body_start, 6, 255),
        ] {
            bytes.extend(time.to_be_bytes());
            bytes.extend([clip, blend, start, 0, flags, binding]);
            bytes.extend(0.5_f32.to_be_bytes());
        }
    }
    bytes.extend((-2_i16).to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!(
            "missing partial-pose attack: {:?}",
            parsed.rows[0].unsupported_reason
        )
    };
    assert_eq!(attack.duration, 114);
    let poses: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| match event {
            EnemyEvent::PartialPose { first_bone, pose } => {
                Some((*at, *first_bone, pose.clip, pose.start, pose.blend))
            }
            EnemyEvent::PoseKeepingParts(pose) => Some((*at, 0, pose.clip, pose.start, pose.blend)),
            _ => None,
        })
        .collect();
    assert_eq!(
        poses,
        [
            (0, 5, 0, 0., 0),
            (2, 0, 31, 0., 0),
            (43, 5, 0, 0., 0),
            (45, 0, 31, 9., 4),
            (79, 5, 0, 0., 0),
            (81, 0, 30, 0., 0)
        ]
    );
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (41, EnemyEvent::Sound(_))))
    );
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (50, EnemyEvent::Contact(_))))
    );
    bytes[695] = 6;
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    Ok(())
}

#[test]
fn projectile_rows_bake_independent_modifiers_and_keep_emission_order() -> Result<()> {
    let mut bytes = package();
    bytes.splice(688..688, [0; 32]);
    bytes[18..20].copy_from_slice(&720_u16.to_be_bytes());
    bytes[624..632].copy_from_slice(&[0, 23, 254, 1, 0, 0, 0, 0]);
    bytes.copy_within(624..656, 656);
    bytes[678..680].copy_from_slice(&4_u16.to_be_bytes());
    bytes[688..690].copy_from_slice(&(-1_i16).to_be_bytes());
    let modifiers = bytes.len();
    bytes[0x1dc..0x1e0].copy_from_slice(&(modifiers as u32).to_be_bytes());
    bytes.extend([0; 4]);
    for (operation, target, values) in [
        (4_u16, 0x24_u16, vec![(-8_f32).to_bits()]),
        (1, 0x6c, vec![2_f32.to_bits(); 3]),
        (5, 0x5d, vec![1]),
        (2, 0x13, vec![1]),
    ] {
        bytes.extend(operation.to_be_bytes());
        bytes.extend(target.to_be_bytes());
        bytes.extend(values.into_iter().flat_map(u32::to_be_bytes));
    }
    bytes.extend([0; 4]);

    let mut template = [0; 400];
    template[0x24..0x28].copy_from_slice(&4_f32.to_be_bytes());
    template[0x5d] = 2;
    template[0x13] = 6;
    template[0x11] = 2;
    let template_start = bytes.len();
    bytes[0x1c8..0x1cc].copy_from_slice(&(template_start as u32).to_be_bytes());
    bytes.extend(template);
    let members = crate::model_preview::PointerMembers::new(&bytes, 24..488)?;
    let projectiles = crate::battle_projectile::read_enemy_member(&template, 0, &members)?;
    let parsed = read(&bytes, &projectiles)?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!(
            "missing projectile attack: {:?}",
            parsed.rows[0].unsupported_reason
        )
    };
    let shots: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| match event {
            EnemyEvent::Projectile {
                definition, hit, ..
            } => Some((*at, definition, hit.recoil, hit.kind)),
            _ => None,
        })
        .collect();
    assert_eq!(
        shots.iter().map(|(at, ..)| *at).collect::<Vec<_>>(),
        [28, 29]
    );
    assert_eq!(**shots[0].1, projectiles[0].definition);
    assert_eq!(shots[1].1.velocity, [-8., 0., 0.]);
    assert_eq!(shots[1].1.motion.velocity_jitter, [2.; 3]);
    assert_eq!(shots[1].1.birth_effect.member, 1);
    assert_eq!(projectiles[0].definition.velocity, [4., 0., 0.]);
    assert_eq!(shots.iter().map(|shot| shot.2).collect::<Vec<_>>(), [6, 1]);
    assert!(
        shots
            .iter()
            .all(|shot| shot.3 == resonance_content::battle_projectile::DamageKind::Magic)
    );
    assert_eq!(projectiles[0].recoil, 6);

    for offset in [624, 656] {
        bytes[offset..offset + 2].copy_from_slice(&0_u16.to_be_bytes());
    }
    let parsed = read(&bytes, &projectiles)?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!("missing opening projectile attack")
    };
    let shots: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| matches!(event, EnemyEvent::Projectile { .. }).then_some(*at))
        .collect();
    assert_eq!(shots, [4, 5]);

    bytes[modifiers + 4..modifiers + 6].copy_from_slice(&9_u16.to_be_bytes());
    assert!(
        read(&bytes, &projectiles)?.rows[0]
            .unsupported_reason
            .is_some()
    );
    Ok(())
}

#[test]
fn projectile_rows_bind_independent_emission_bones() -> Result<()> {
    use resonance_content::battle_projectile::ProjectileOrigin;

    let mut bytes = package();
    bytes.splice(688..688, [0; 32]);
    bytes[18..20].copy_from_slice(&720_u16.to_be_bytes());
    bytes[624..632].copy_from_slice(&[0, 23, 254, 1, 0, 31, 0, 0]);
    bytes[632..640].fill(0);
    bytes.copy_within(624..656, 656);
    bytes[661] = 60;
    bytes[688..690].copy_from_slice(&(-1_i16).to_be_bytes());
    let mut template = [0; 400];
    template[8..12].copy_from_slice(&0x800011_u32.to_be_bytes());
    template[0x14..0x18].copy_from_slice(&[1, 4, 1, 0]);
    template[0x9c] = 4;
    template[0x2c..0x30].copy_from_slice(&8_f32.to_be_bytes());
    let template_start = bytes.len();
    bytes[0x1c8..0x1cc].copy_from_slice(&(template_start as u32).to_be_bytes());
    bytes.extend(template);
    let members = crate::model_preview::PointerMembers::new(&bytes, 24..488)?;
    let projectiles = crate::battle_projectile::read_enemy_member(&template, 0, &members)?;
    let actions = read(&bytes, &projectiles)?;
    let Some(EnemyExecution::Attack(attack)) = &actions.rows[0].execution else {
        panic!(
            "missing bone emissions: {:?}",
            actions.rows[0].unsupported_reason
        )
    };
    let shots: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| match event {
            EnemyEvent::Projectile { definition, .. } => Some((*at, definition)),
            _ => None,
        })
        .collect();
    assert_eq!(
        shots
            .iter()
            .map(|(at, shot)| (*at, shot.motion.origin))
            .collect::<Vec<_>>(),
        [
            (28, ProjectileOrigin::Bone(31)),
            (29, ProjectileOrigin::Bone(60))
        ]
    );
    assert!(
        shots
            .iter()
            .all(|(_, shot)| shot.motion.bone_direction.is_none() && shot.velocity == [0., 0., 8.])
    );
    assert_eq!(
        projectiles[0].definition.motion.origin,
        ProjectileOrigin::Actor
    );
    let mut directed = projectiles.clone();
    directed[0].definition.motion.bone_direction =
        Some(resonance_content::battle_projectile::ProjectileBoneDirection { from: 68, to: 69 });
    bytes[629] = 0;
    bytes[661] = 0;
    let directed_actions = read(&bytes, &directed)?;
    let Some(EnemyExecution::Attack(attack)) = &directed_actions.rows[0].execution else {
        panic!("missing actor-origin bone direction");
    };
    for (_, event) in &attack.events {
        if let EnemyEvent::Projectile { definition, .. } = event {
            assert_eq!(definition.motion.origin, ProjectileOrigin::Actor);
            assert_eq!(
                definition.motion.bone_direction,
                directed[0].definition.motion.bone_direction
            );
        }
    }
    bytes[627] = 2;
    assert!(
        read(&bytes, &projectiles)?.rows[0]
            .unsupported_reason
            .is_some()
    );
    Ok(())
}

#[test]
fn enemy_resources_keep_signed_values_and_action_choices() -> Result<()> {
    let mut bytes = package();
    bytes[488..490].copy_from_slice(&0x6220_u16.to_be_bytes());
    bytes[551] = 255;
    bytes[546..548].copy_from_slice(&40_u16.to_be_bytes());
    bytes[548] = 1;
    bytes[566] = 25;
    bytes[532..536].copy_from_slice(&[0x80, 0, 0x7f, 0xff]);
    bytes[568] = 8;
    bytes[550] = 3;
    bytes[694] = 0x48;
    bytes[692..694].copy_from_slice(&[6, 12]);
    bytes[510] = 7;
    bytes[567] = 2;

    bytes[664..668].copy_from_slice(&f32::NAN.to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let row = &parsed.rows[0];
    assert_eq!(row.weight, 35);
    assert_eq!((row.chain_group, row.followup_chance), (1, 25));
    assert_eq!(row.guard_chance, -1);
    assert_eq!(row.range, [i16::MIN, i16::MAX]);
    assert_eq!(row.tp, 8);
    assert!(parsed.policy.unsupported_reason.is_none());
    assert!(row.unsupported_reason.is_none());
    let Some(EnemyExecution::Attack(attack)) = &row.execution else {
        panic!("missing attack")
    };
    assert_eq!((attack.duration, attack.recovery_ticks), (77, 10));
    assert_eq!(attack.chain_at, Some(45));
    let poses: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| match event {
            EnemyEvent::Pose(pose) => Some((*at, pose.clip, pose.blend, pose.rate)),
            _ => None,
        })
        .collect();
    assert_eq!(poses, [(0, 30, 4, 0.5), (23, 31, 2, 0.5), (45, 32, 2, 0.5)]);
    assert!(matches!(attack.events[0], (0, EnemyEvent::Effect(3))));
    assert!(matches!(&attack.events[1], (0, EnemyEvent::Pose(pose))
        if pose.repeat && pose.start == 3. && pose.end == Some(6.)));
    let (
        at,
        EnemyEvent::Contact(MeleeContact {
            duration,
            attachments: ContactAttachments::Groups(groups),
            hit,
            ..
        }),
    ) = attack
        .events
        .iter()
        .find(|(_, event)| matches!(event, EnemyEvent::Contact(_)))
        .context("missing contact")?
    else {
        panic!("missing contact")
    };
    assert_eq!(
        (*at, *duration, groups.as_slice(), hit.power, hit.hitstun),
        (28, 27, &[1][..], Power::Normal, 20)
    );
    assert_eq!(hit.effect, Some(7));
    assert!(hit.arte && hit.suppress_reaction && hit.force_reaction && hit.finisher_grade);
    let json = serde_json::to_vec(&parsed)?;
    serde_json::from_slice::<EnemyActions>(&json)?;
    for rate in [0.5_f32, 1., -1.] {
        bytes[696..700].copy_from_slice(&rate.to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing scaled pose")
        };
        let EnemyEvent::Pose(pose) = attack.events[1].1 else {
            panic!("missing opening pose")
        };
        assert_eq!(
            (pose.start, pose.end, pose.rate),
            (6. * rate.abs(), Some(12. * rate.abs()), rate)
        );
    }
    // A closing gesture plays immediately once; only idle blends over recovery.
    for (clip, rate, blend, repeat) in [(35, 1.25, 0, false), (0, 0.5, 10, true)] {
        bytes[519] = clip;
        bytes[520..524].copy_from_slice(&1.25_f32.to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing recovery pose")
        };
        assert_eq!(attack.recovery_ticks, 10);
        let pose = attack.recovery_pose;
        assert_eq!(
            (
                pose.clip,
                pose.start,
                pose.end,
                pose.rate,
                pose.blend,
                pose.repeat
            ),
            (u16::from(clip), 0., None, rate, blend, repeat)
        );
    }
    Ok(())
}

#[test]
fn animation_rate_changes_preserve_pose_and_do_not_add_a_blend_hold() -> Result<()> {
    let mut bytes = package();
    bytes[702..708].copy_from_slice(&[254, 0, 0, 0, 0, 0]);
    bytes[708..712].copy_from_slice(&0_f32.to_be_bytes());
    let parsed = read(&bytes, &[])?;
    let decoded: EnemyActions = serde_json::from_slice(&serde_json::to_vec(&parsed)?)?;
    let Some(EnemyExecution::Attack(attack)) = &decoded.rows[0].execution else {
        panic!("missing attack")
    };
    assert_eq!(attack.duration, 76);
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (24, EnemyEvent::AnimationRate(0.))))
    );
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (43, EnemyEvent::Pose(pose)) if pose.clip == 32))
    );
    Ok(())
}

#[test]
fn common_action_effects_keep_their_member_and_blended_clock() -> Result<()> {
    let mut bytes = package();
    bytes[614..620].copy_from_slice(&[0, 26, 0, 18, 0, 1]);
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!("missing attack")
    };
    assert!(
        attack
            .events
            .iter()
            .any(|event| matches!(event, (28, EnemyEvent::CommonEffect(18))))
    );
    bytes[619] = 2;
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    Ok(())
}

#[test]
fn body_pass_through_starts_after_the_opening_blend() -> Result<()> {
    let mut bytes = package();
    bytes[612..620].copy_from_slice(&[0, 0, 0, 5, 0, 1, 0, 0]);
    for duration in [0, 4, 255] {
        bytes[619] = duration;
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing attack")
        };
        assert!(attack.events.iter().any(|event| matches!(
            event,
            (4, EnemyEvent::PassThrough { enabled: true, duration: actual }) if *actual == duration
        )));
    }
    Ok(())
}

#[test]
fn carried_poses_keep_the_action_clock_and_repeat_choice() -> Result<()> {
    let mut bytes = package();
    let mut command = Vec::new();
    for value in [23_u16, 25, 0, 33] {
        command.extend(value.to_be_bytes());
    }
    command.extend(0.75_f32.to_be_bytes());
    command.extend(1_u32.to_be_bytes());
    command.extend((-1_i16).to_be_bytes());
    bytes.splice(612..624, command);
    bytes[16..18].copy_from_slice(&630_u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&694_u16.to_be_bytes());
    for (at, expected, repeat) in [
        (0_u16, 4, false),
        (0, 4, true),
        (23, 28, false),
        (23, 28, true),
    ] {
        bytes[612..614].copy_from_slice(&at.to_be_bytes());
        bytes[624..628].copy_from_slice(&u32::from(repeat).to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing carried pose")
        };
        assert_eq!(attack.duration, 77);
        assert!(attack.events.iter().any(|event| matches!(event,
            (at, EnemyEvent::CarriedPose { slot: 0, clip: 33, rate, repeat: actual })
                if *at == expected && *rate == 0.75 && *actual == repeat)));
    }
    bytes[624..628].copy_from_slice(&2_u32.to_be_bytes());
    assert!(read(&bytes, &[])?.rows[0].execution.is_none());
    for (declared, present) in [(false, false), (true, false), (false, true)] {
        let mut bytes = package();
        bytes[614..616].copy_from_slice(&7_u16.to_be_bytes());
        bytes[616..618].copy_from_slice(&1_u16.to_be_bytes());
        bytes[618..620].copy_from_slice(&(-1_i16).to_be_bytes());
        let profile = bytes.len();
        bytes[4..6].copy_from_slice(&(profile as u16).to_be_bytes());
        bytes.resize(profile + 0x1e5, 0);
        bytes[profile + 0x1e4] = u8::from(declared);
        if present {
            let pointer = bytes.len() as u32;
            bytes.push(0);
            bytes[0x160..0x164].copy_from_slice(&pointer.to_be_bytes());
        }
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing weapon visibility action")
        };
        assert_eq!(attack.duration, 77);
        assert_eq!(
            attack.events.iter().any(|(_, event)| matches!(
                event,
                EnemyEvent::WeaponVisibility {
                    slot: 0,
                    visible: true
                }
            )),
            declared || present
        );
    }
    Ok(())
}

#[test]
fn action_tints_decode_rgb_without_changing_opacity() -> Result<()> {
    let mut bytes = package();
    bytes.splice(624..624, [255, 255]);
    bytes[16..18].copy_from_slice(&626_u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&690_u16.to_be_bytes());
    bytes[612..624].copy_from_slice(&[0, 23, 0, 29, 0, 64, 0, 32, 0, 0, 0, 0]);
    for fourth in [0, 64, 255] {
        bytes[623] = fourth;
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing attack")
        };
        assert!(
            attack
                .events
                .iter()
                .any(|event| matches!(event, (28, EnemyEvent::Tint([64, 32, 0]))))
        );
    }
    bytes[623] = 1;
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    Ok(())
}

#[test]
fn body_bone_contacts_keep_the_operand_after_the_marker() -> Result<()> {
    let mut bytes = package();
    bytes[628..630].copy_from_slice(&[254, 52]);
    bytes[614..618].copy_from_slice(&[0, 9, 0, 15]);
    bytes[618..620].copy_from_slice(&(-1_i16).to_be_bytes());
    let parsed = read(&bytes, &[])?;
    assert!(parsed.rows[0].unsupported_reason.is_none());
    let json = serde_json::to_vec(&parsed)?;
    let decoded: EnemyActions = serde_json::from_slice(&json)?;
    let Some(EnemyExecution::Attack(attack)) = &decoded.rows[0].execution else {
        panic!("missing attack")
    };
    assert!(attack.events.iter().any(|(_, event)| matches!(
        event,
        EnemyEvent::Contact(MeleeContact {
            attachments: ContactAttachments::BodyBone(52),
            ..
        })
    )));
    assert!(attack.events.iter().any(|(_, event)| matches!(event,
        EnemyEvent::Braking(value) if *value == 1.5
    )));
    bytes[627] = 2;
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    bytes[627] = 1;
    bytes[628..630].copy_from_slice(&[255, 0]);
    let parsed = read(&bytes, &[])?;
    let decoded: EnemyActions = serde_json::from_slice(&serde_json::to_vec(&parsed)?)?;
    let Some(EnemyExecution::Attack(attack)) = &decoded.rows[0].execution else {
        panic!("missing body-centered attack")
    };
    assert!(attack.events.iter().any(|(_, event)| matches!(
        event,
        EnemyEvent::Contact(MeleeContact {
            attachments: ContactAttachments::BodyCenter {},
            ..
        })
    )));
    bytes[627] = 2;
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    bytes[627] = 1;
    bytes[616..618].copy_from_slice(&(-1_i16).to_be_bytes());
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    Ok(())
}

#[test]
fn opening_movement_waits_for_the_pose_blend() -> Result<()> {
    for blend in [0_u8, 4] {
        for command in 0..=3 {
            let mut bytes = package();
            bytes[550] = 3;
            bytes[691] = blend;
            bytes[612..620].copy_from_slice(&[0, 0, 0, command, 0, 40, 255, 255]);
            let parsed = read(&bytes, &[])?;
            let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
                panic!("missing attack")
            };
            assert!(
                attack
                    .events
                    .iter()
                    .any(|event| matches!(event, (0, EnemyEvent::Effect(3))))
            );
            assert!(
                attack
                    .events
                    .iter()
                    .any(|event| matches!(event, (0, EnemyEvent::Pose(_))))
            );
            assert!(
                attack
                    .events
                    .iter()
                    .any(|(at, event)| *at == u16::from(blend)
                        && matches!(
                            event,
                            EnemyEvent::Forward(_)
                                | EnemyEvent::Vertical(_)
                                | EnemyEvent::Acceleration(_)
                                | EnemyEvent::Gravity(_)
                        ))
            );
            let delay = u16::from(blend.max(1));
            assert!(attack.events.iter().any(|(at, event)| matches!(
                event, EnemyEvent::Pose(pose) if pose.clip == 31 && *at == 19 + delay
            )));
            assert_eq!(attack.duration, 73 + delay);
        }
    }
    Ok(())
}

#[test]
fn enemy_blends_compile_recorded_golem_action_durations() -> Result<()> {
    // Recorded attack-to-recovery intervals are 86 and 70 updates. The second
    // pose begins after 24 and 26 updates respectively.
    for (duration, opening_blend, next_at, next_blend, elapsed, next_elapsed) in
        [(70_u16, 16, 8_u16, 0, 86, 24), (55, 12, 14, 4, 70, 26)]
    {
        let mut bytes = package();
        bytes[530..532].copy_from_slice(&duration.to_be_bytes());
        bytes[691] = opening_blend;
        bytes[700..702].copy_from_slice(&next_at.to_be_bytes());
        bytes[703] = next_blend;
        bytes[712..714].copy_from_slice(&(-2_i16).to_be_bytes());
        let parsed = read(&bytes, &[])?;
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing attack")
        };
        assert_eq!(attack.duration, elapsed);
        assert!(
            attack
                .events
                .iter()
                .any(|(at, event)| *at == next_elapsed && matches!(event, EnemyEvent::Pose(_)))
        );
    }
    // The station controller uses the same authored clock as its action.
    // Its later pose at65 must not delay the change at60.
    for (opening_blend, change_at) in [(4, 64), (12, 72)] {
        let mut bytes = package();
        bytes[530..532].copy_from_slice(&120_u16.to_be_bytes());
        bytes[691] = opening_blend;
        bytes[700..702].copy_from_slice(&65_u16.to_be_bytes());
        bytes[703] = 4;
        bytes[712..714].copy_from_slice(&(-2_i16).to_be_bytes());
        let row = bytes[516..584].to_vec();
        bytes.splice(584..584, row.iter().chain(&row).copied());
        for field in [12, 14, 16, 18] {
            let offset = u16::read(&bytes, field)? + 136;
            bytes[field..field + 2].copy_from_slice(&offset.to_be_bytes());
        }
        let policy = usize::from(u16::read(&bytes, 12)?);
        bytes[policy + 8] = 11;
        bytes[policy + 23] = 3;
        let parsed = read(&bytes, &[])?;
        assert!(parsed.rows[2].unsupported_reason.is_none());
        assert!(matches!(parsed.policy.behavior,
            Some(EnemyBehavior::ThreeStations { change_at: actual, .. }) if actual == change_at));
    }
    Ok(())
}

#[test]
fn broken_weapon_controller_uses_the_actions_blend_clock() -> Result<()> {
    let mut bytes = package();
    bytes[530..532].copy_from_slice(&160_u16.to_be_bytes());
    bytes[700..702].copy_from_slice(&90_u16.to_be_bytes());
    bytes[703] = 10;
    bytes[712..714].copy_from_slice(&100_u16.to_be_bytes());
    bytes[715] = 0;
    let row = bytes[516..584].to_vec();
    bytes.splice(584..584, row.iter().chain(&row).copied());
    for field in [12, 14, 16, 18] {
        let offset = u16::read(&bytes, field)? + 136;
        bytes[field..field + 2].copy_from_slice(&offset.to_be_bytes());
    }
    let policy = usize::from(u16::read(&bytes, 12)?);
    bytes[policy + 8] = 9;
    bytes[policy + 23] = 3;
    let parsed = read(&bytes, &[])?;
    assert!(
        parsed
            .rows
            .iter()
            .all(|row| row.unsupported_reason.is_none())
    );
    assert!(parsed.policy.event_hp_hold);
    assert!(matches!(
        parsed.policy.behavior,
        Some(EnemyBehavior::ReplaceWeapon {
            action: 2,
            hide_at: 57,
            replace_at: 158,
            source: 1,
            destination: 0,
            disabled_action: 0,
        })
    ));
    assert!(matches!(&parsed.rows[2].execution,
        Some(EnemyExecution::Attack(attack)) if attack.duration == 173));
    Ok(())
}

#[test]
fn hammer_rain_hit_keeps_authored_feedback_without_the_default_guard_sound() -> Result<()> {
    let mut row = [
        0x88, 0x20, 10, 30, 30, 10, 1, 2, 0, 0, 0, 0, 0, 1, 0, 40, 0, 107, 0, 0, 0, 0, 122, 0, 0,
        0, 0, 0,
    ];
    let hit = hit_rule(
        &row,
        0,
        resonance_content::battle_projectile::DamageKind::Slash,
        1,
    )?;
    assert!(hit.suppress_guard_sound && hit.effect_on_guard && hit.arte);
    assert_eq!(hit.sound, Some(107));
    assert_eq!(hit.effect, Some(122));
    assert_eq!(hit.power, Power::Percent(40));
    assert!(!hit.hp_drain);
    row[0] |= 4;
    for unused_condition_value in [50, 100] {
        row[23] = unused_condition_value;
        let hit = hit_rule(
            &row,
            0,
            resonance_content::battle_projectile::DamageKind::Slash,
            1,
        )?;
        assert!(hit.hp_drain);
        assert!(hit.condition.is_none());
    }
    Ok(())
}

#[test]
fn post_contact_power_adjustments_preserve_the_authored_sign() -> Result<()> {
    let mut rule = [0; 28];
    rule[13] = 4;
    for amount in [-15_i16, -5, 0, 60] {
        rule[14..16].copy_from_slice(&amount.to_be_bytes());
        assert_eq!(
            hit_rule(
                &rule,
                0,
                resonance_content::battle_projectile::DamageKind::Slash,
                0
            )?
            .power,
            Power::AdjustAfterContact(amount)
        );
    }
    rule[13] = 5;
    assert!(
        hit_rule(
            &rule,
            0,
            resonance_content::battle_projectile::DamageKind::Slash,
            0
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn enemy_hit_conditions_and_voices_use_shared_bindings() -> Result<()> {
    use resonance_content::battle_voice::Sound;
    for (encoded, expected) in [
        (0_u16, None),
        (1035, Some(Sound::Cue(1536))),
        (0x8002, Some(Sound::Stream(2))),
    ] {
        let mut bytes = package();
        bytes[496..500].copy_from_slice(&1_u32.to_be_bytes());
        bytes[500] = 5;
        bytes[614..616].copy_from_slice(&27_u16.to_be_bytes());
        bytes[616..618].copy_from_slice(&encoded.to_be_bytes());
        bytes[618..620].copy_from_slice(&2_u16.to_be_bytes());
        bytes[576..578].copy_from_slice(&encoded.to_be_bytes());
        bytes[578..580].copy_from_slice(&encoded.to_be_bytes());
        let parsed = read(&bytes, &[])?;
        assert_eq!(parsed.rows[0].voices.begin, expected);
        assert_eq!(parsed.rows[0].voices.release, expected);
        let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
            panic!("missing attack")
        };
        assert!(attack.events.iter().any(|(_, event)| matches!(event,
        EnemyEvent::Contact(MeleeContact { hit, .. }) if hit.condition == Some(HitCondition {
            condition: Condition::PoisonMild, chance: 5, value: 0
        }))));
        assert!(attack.events.iter().any(|(_, event)| matches!(event,
            EnemyEvent::Voice { sound, announcement: false } if *sound == expected)));
        bytes[619] = 3;
        let announced = read(&bytes, &[])?;
        assert!(matches!(&announced.rows[0].execution,
            Some(EnemyExecution::Attack(attack)) if attack.events.iter().any(|(_, event)| matches!(event,
                EnemyEvent::Voice { sound, announcement: true } if *sound == expected))));
        bytes[619] = 1;
        assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
        bytes[619] = 2;
        for unsupported in [3_u32, 1 << 6] {
            bytes[496..500].copy_from_slice(&unsupported.to_be_bytes());
            assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
        }
    }
    let mut bytes = package();
    bytes[496..500].copy_from_slice(&(1_u32 << 9).to_be_bytes());
    bytes[500] = 2;
    let parsed = read(&bytes, &[])?;
    let Some(EnemyExecution::Attack(attack)) = &parsed.rows[0].execution else {
        panic!(
            "missing Heavy contact: {:?}",
            parsed.rows[0].unsupported_reason
        )
    };
    assert!(attack.events.iter().any(|(_, event)| matches!(event,
    EnemyEvent::Contact(MeleeContact { hit, .. }) if hit.condition == Some(HitCondition {
        condition: Condition::Heavy, chance: 2, value: 0
    }))));
    Ok(())
}

#[test]
fn enemy_decisions_decode_eligibility_targets_and_weighted_rows() -> Result<()> {
    let mut bytes = package();
    bytes[575] = 73;
    bytes[517] = 5;
    bytes[524..528].copy_from_slice(
        &(0x8_0000_u32 | 0x2_0000 | 8 | 2 | 0x20 | 0x10 | 0x4000 | TARGET_ALLIES).to_be_bytes(),
    );
    bytes[567] = 2;
    bytes[608] = 1;
    bytes[602] = 75;
    let parsed = read(&bytes, &[])?;
    let row = &parsed.rows[0];
    assert!(row.unsupported_reason.is_none());
    assert_eq!(row.target_policy, Some(TargetPolicy::Flying));
    assert_eq!(row.required_flag, Some(73));
    assert!(row.target_allies);
    assert!(!row.return_to_formation);
    assert!(!row.run_return);
    // A custom approach does not replace the return's walk/run selection.
    for (flags, run_return) in [(0, false), (0x40, true), (0x60, false)] {
        let mut returning = package();
        returning[524..528].copy_from_slice(&u32::to_be_bytes(flags));
        returning[556..560].copy_from_slice(&8_f32.to_be_bytes());
        returning[560..564].copy_from_slice(&0.5_f32.to_be_bytes());
        returning[564] = 1;
        let parsed = read(&returning, &[])?;
        assert!(matches!(
            parsed.rows[0].approach,
            EnemyApproach::Custom { .. }
        ));
        assert_eq!(parsed.rows[0].run_return, run_return);
    }
    assert!(row.requirements.priority);
    assert!(row.requirements.once_per_battle);
    assert_eq!(row.requirements.max_enemies, Some(2));
    assert_eq!(row.requirements.difficulty, 2..=2);
    assert_eq!(row.requirements.hp_percent, Some(50));
    assert_eq!(parsed.policy.back_row.len(), 1);
    assert_eq!(
        (
            parsed.policy.back_row[0].action,
            parsed.policy.back_row[0].weight
        ),
        (0, 75)
    );
    assert!(requirements(0x2000_0000 | 8, 0, 0)?.difficulty.is_empty());
    assert_eq!(requirements(0x60, 0, 0)?.difficulty, 0..=2);
    assert_eq!(requirements(0x2_0000, 0, 0)?, requirements(0, 0, 0)?);
    assert!(requirements(0x4000, 255, 0).is_err());
    assert!(requirements(0x90044, 0, 5)?.target_guarding);
    assert!(requirements(0x90044, 0, 28).is_err());
    assert_eq!(
        requirements(0x800, 10, 0)?.predicates,
        [EnemyPredicate::AllyAbsent(10)]
    );
    assert_eq!(
        requirements(0x0200_0800, 30, 0)?.predicates,
        [EnemyPredicate::AllyAbsent(30)]
    );
    for flags in [0x80_0000, 0x100_0000] {
        assert!(requirements(flags, 0, 2)?.predicates.is_empty());
        assert!(requirements(flags, 1, 2).is_err());
    }
    let chained = requirements(0x600044, 0, 0)?;
    assert!(chained.followup_only && chained.followup_requires_hit);
    bytes[517] = 0;
    assert!(!read(&bytes, &[])?.rows[0].target_allies);

    let mut departure = package();
    departure[516] = 100;
    departure[524..528].copy_from_slice(&0x0200_0800_u32.to_be_bytes());
    departure[567] = 30;
    departure[540..546].fill(255);
    let decoded = read(&departure, &[])?;
    let row = &decoded.rows[0];
    assert!(row.unsupported_reason.is_none());
    assert!(matches!(row.execution, Some(EnemyExecution::LeaveArena)));
    assert_eq!(row.weight, 100);
    assert_eq!(
        row.requirements.predicates,
        [EnemyPredicate::AllyAbsent(30)]
    );
    assert!(row.cast_ticks.is_none());
    Ok(())
}

#[test]
fn unsupported_enemy_decisions_remain_explicit_after_import() -> Result<()> {
    for (at, value) in [(516, 255), (517, 255), (564, 1)] {
        let mut bytes = package();
        bytes[at] = value;
        assert!(
            read(&bytes, &[])?.rows[0].unsupported_reason.is_some(),
            "byte {at}"
        );
    }
    for value in [-1., 0., f32::INFINITY, f32::NAN] {
        let mut bytes = package();
        bytes[564] = 1;
        bytes[560..564].copy_from_slice(&1_f32.to_be_bytes());
        bytes[556..560].copy_from_slice(&value.to_be_bytes());
        assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    }
    let mut bytes = package();
    bytes[524..528].copy_from_slice(&0x400_0000_u32.to_be_bytes());
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_some());
    for (at, value) in [(592, 14), (595, 1), (606, 1), (607, 0), (609, 1)] {
        let mut bytes = package();
        bytes[at] = value;
        assert!(
            read(&bytes, &[])?.policy.unsupported_reason.is_some(),
            "byte {at}"
        );
    }
    let mut bytes = package();
    bytes[566] = 255;
    bytes[556..560].copy_from_slice(&1_f32.to_be_bytes());
    assert!(read(&bytes, &[])?.rows[0].unsupported_reason.is_none());
    Ok(())
}

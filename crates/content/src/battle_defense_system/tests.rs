use super::*;

fn fixture() -> Definition {
    Definition {
        version: VERSION,
        source_sha256: "a".repeat(64),
        recipe_sha256: "b".repeat(64),
        core: FixedMember {
            enemy: 11,
            position: [850., 0., 0.],
            heading: -90.,
        },
        repair: FixedMember {
            enemy: 14,
            position: [200., 0., 0.],
            heading: -90.,
        },
        orbit: 12,
        orbit_count: 2,
        arm: 13,
        arm_slots: [3, 2, 1, 0],
        deployment: Deployment {
            clip: 32,
            ticks: 120,
            hp_percent: 66,
            rate: 0.175,
            blend: 4,
        },
        repair_cycle: RepairCycle {
            self_ticks: 1500,
            repeat_ticks: 120,
            orbit_ticks: 900,
            self_percent: 100,
            orbit_percent: 75,
            defense_divisor: 4,
            defense_multiplier: 3,
        },
        repair_sound: 104,
        repair_sound_priority: 2,
        messages: Messages {
            automatic: "Auto Repair".into(),
            orbit: "Repair".into(),
            deployment: "Alert".into(),
            duration: 90,
        },
        camera_yaw_limits: [102., 256.],
    }
}

#[test]
fn composition_rejects_ambiguous_members_and_invalid_bindings() -> Result<()> {
    let valid = fixture();
    valid.validate()?;
    assert!(valid.contains(11) && !valid.contains(211));
    assert_eq!(valid.fixed_member(14), Some(valid.repair));
    assert_eq!(valid.fixed_member(13), None);
    let changes: &[fn(&mut Definition)] = &[
        |v| v.version = 0,
        |v| v.arm = v.core.enemy,
        |v| v.repair.enemy = 255,
        |v| v.orbit_count = 3,
        |v| v.arm_slots[0] = v.arm_slots[1],
        |v| v.arm_slots[0] = 10,
        |v| v.core.position[1] = f32::NAN,
        |v| v.repair.heading = f32::INFINITY,
        |v| v.deployment.ticks = 0,
        |v| v.deployment.hp_percent = 101,
        |v| v.deployment.rate = f32::NAN,
        |v| v.deployment.rate = 0.,
        |v| v.repair_cycle.self_ticks = 0,
        |v| v.repair_cycle.orbit_ticks = 0,
        |v| v.repair_cycle.self_percent = 0,
        |v| v.repair_cycle.orbit_percent = 101,
        |v| v.repair_cycle.defense_divisor = 0,
        |v| v.repair_cycle.defense_multiplier = 5,
        |v| v.repair_sound = 0,
        |v| v.messages.duration = 0,
        |v| v.messages.automatic.clear(),
        |v| v.messages.orbit = "bad\0message".into(),
        |v| v.camera_yaw_limits = [256., 102.],
        |v| v.camera_yaw_limits = [0., f32::INFINITY],
    ];
    for change in changes {
        let mut invalid = valid.clone();
        change(&mut invalid);
        assert!(invalid.validate().is_err(), "accepted {invalid:?}");
    }
    let mut old = serde_json::to_value(&valid)?;
    old.as_object_mut().unwrap().remove("deployment");
    assert!(serde_json::from_value::<Definition>(old).is_err());
    Ok(())
}

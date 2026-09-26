use super::*;

fn rule() -> RecoilRule {
    RecoilRule {
        impulse: [4., 10.],
        delay: 0,
        knock_down: false,
        launch: false,
        lift_guard: false,
        guard_speed: 3.,
    }
}

fn profile() -> RecoilProfile {
    RecoilProfile {
        vertical: VerticalRecoil::Unchanged,
        can_knock_down: true,
        can_launch: true,
        clear_pending: false,
    }
}

#[test]
fn natural_opening_hits_and_guards_match_original_pending_and_live_bits() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-recoil.json")).unwrap();
    let parameters = &fixture["source_parameters"];
    let speed = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
    for row in fixture["observations"].as_array().unwrap() {
        let flags = row["flags"].as_u64().unwrap();
        let impulse = &parameters["impulses"][row["selector"].as_u64().unwrap() as usize];
        let rule = RecoilRule {
            impulse: [speed(&impulse[0]), speed(&impulse[1])],
            delay: row["delay"].as_u64().unwrap() as u8,
            knock_down: flags & 4 != 0,
            launch: flags & 8 != 0,
            lift_guard: flags & 0x100 != 0,
            guard_speed: speed(&parameters["guard_speed"]),
        };
        let flags = row["profile_flags"].as_u64().unwrap();
        let weight = row["weight"].as_u64().unwrap();
        let profile = RecoilProfile {
            vertical: match weight {
                1 => VerticalRecoil::Scale(speed(&parameters["light_vertical_scale"])),
                2 => VerticalRecoil::Scale(speed(&parameters["heavy_vertical_scale"])),
                3 => VerticalRecoil::Grounded,
                _ => VerticalRecoil::Unchanged,
            },
            can_knock_down: flags & 0x200 == 0,
            can_launch: flags & 0x200 == 0 && flags & 0x02000000 == 0 && weight != 3,
            clear_pending: flags & 0x400 != 0,
        };
        let result = row["result"].as_u64().unwrap();
        assert_eq!(result & 6, 0);
        let guarded = result & 0xf == 0 || result & 0x406 != 0;
        let mut movement = Movement::default();
        let mut recoil = Recoil::default();
        assert!(recoil.start(&mut movement, rule, profile, guarded, false));
        for (values, key) in [
            ([movement.forward, movement.vertical], "velocity_bits"),
            (recoil.pending, "pending_bits"),
        ] {
            let expected: [u32; 2] = serde_json::from_value(row["after"][key].clone()).unwrap();
            assert_eq!(values.map(f32::to_bits), expected, "{row}");
        }
        assert_eq!(recoil.delay, row["after"]["delay"].as_u64().unwrap() as u8);
        assert_eq!(recoil.kind, RecoilKind::Normal);
    }
}

#[test]
fn delayed_launch_releases_scaled_impulse_before_any_integration() {
    let mut movement = Movement::default();
    let mut recoil = Recoil::default();
    let rule = RecoilRule {
        delay: 2,
        launch: true,
        knock_down: true,
        ..rule()
    };
    let profile = RecoilProfile {
        vertical: VerticalRecoil::Scale(0.75),
        ..profile()
    };
    assert!(recoil.start(&mut movement, rule, profile, false, true));
    assert_eq!(recoil.kind, RecoilKind::Launched);
    assert_eq!(recoil.pending, [4., 7.5]);
    assert_eq!([movement.forward, movement.vertical], [0., 0.]);
    recoil.advance_delay(&mut movement);
    assert_eq!(recoil.delay, 1);
    assert_eq!([movement.forward, movement.vertical], [0., 0.]);
    recoil.advance_delay(&mut movement);
    assert_eq!([movement.forward, movement.vertical], [4., 7.5]);
    movement.forward = 1.;
    recoil.advance_delay(&mut movement);
    assert_eq!(movement.forward, 1., "a released impulse is not reapplied");
}

#[test]
fn suppressed_motion_still_keeps_vertical_recoil_and_can_be_overridden() {
    let mut movement = Movement::default();
    let mut recoil = Recoil::default();
    assert!(!recoil.start(&mut movement, rule(), profile(), false, true));
    assert_eq!(recoil.pending, [0., 10.]);
    assert_eq!([movement.forward, movement.vertical], [0., 10.]);
    let rule = RecoilRule {
        knock_down: true,
        ..rule()
    };
    assert!(recoil.start(&mut movement, rule, profile(), false, true));
    assert_eq!(recoil.kind, RecoilKind::Down);
    let profile = RecoilProfile {
        can_knock_down: false,
        can_launch: false,
        ..profile()
    };
    assert!(!recoil.start(&mut movement, rule, profile, false, true));
    assert_eq!(recoil.kind, RecoilKind::Normal);
}

#[test]
fn guard_adjustment_and_pending_immunity_preserve_original_write_order() {
    let mut movement = Movement::default();
    let mut recoil = Recoil::default();
    let guarded = RecoilRule {
        delay: 2,
        launch: true,
        ..rule()
    };
    recoil.start(&mut movement, guarded, profile(), true, false);
    assert_eq!(recoil.kind, RecoilKind::Normal);
    assert_eq!(recoil.pending, [4., 10.]);
    assert_eq!([movement.forward, movement.vertical], [3., 0.]);
    let immune = RecoilProfile {
        clear_pending: true,
        ..profile()
    };
    recoil.start(&mut movement, rule(), immune, false, false);
    assert_eq!(recoil.pending, [0., 0.]);
    assert_eq!([movement.forward, movement.vertical], [4., 10.]);
    recoil.start(&mut movement, guarded, immune, false, false);
    recoil.advance_delay(&mut movement);
    recoil.advance_delay(&mut movement);
    assert_eq!([movement.forward, movement.vertical], [0., 0.]);
}

#[test]
fn grounded_vertical_speed_is_positive_zero_even_for_downward_impulses() {
    let mut movement = Movement::default();
    let mut recoil = Recoil::default();
    let grounded = RecoilProfile {
        vertical: VerticalRecoil::Grounded,
        can_launch: false,
        ..profile()
    };
    recoil.start(
        &mut movement,
        RecoilRule {
            impulse: [6., -10.],
            launch: true,
            ..rule()
        },
        grounded,
        false,
        false,
    );
    assert_eq!(recoil.kind, RecoilKind::Normal);
    assert_eq!(movement.vertical.to_bits(), 0);
    assert_eq!(recoil.pending[1].to_bits(), 0);
    assert!(
        RecoilRule {
            impulse: [f32::INFINITY, 0.],
            ..rule()
        }
        .validate()
        .is_err()
    );
    assert!(
        RecoilProfile {
            vertical: VerticalRecoil::Scale(f32::NAN),
            ..profile()
        }
        .validate()
        .is_err()
    );
}

use super::*;

fn rule() -> RecoilRule {
    RecoilRule {
        impulse: [4., 10.],
        delay: 0,
        knock_down: false,
        launch: false,
        lift_guard: false,
        guard_speed: 3.,
        suppression_distance: 400.,
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
fn guard_prevents_launch_and_pending_immunity_cancels_delayed_impulses() {
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
fn grounded_profiles_prevent_vertical_recoil_and_launch() {
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
    assert_eq!(movement.vertical, 0.);
    assert_eq!(recoil.pending[1], 0.);
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

#[test]
fn suppression_distance_requires_a_finite_nonnegative_prepared_operand() {
    assert!(RecoilRule::default().validate().is_ok());
    for suppression_distance in [f32::NAN, f32::INFINITY, -1.] {
        assert!(
            RecoilRule {
                suppression_distance,
                ..rule()
            }
            .validate()
            .is_err()
        );
    }
}

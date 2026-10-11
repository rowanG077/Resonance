use super::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

#[derive(Resource, Default)]
struct Ticks(u32);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Controls>()
        .init_resource::<Ticks>()
        .insert_resource(Time::<Fixed>::from_duration(
            resonance_game::clock::UPDATE_STEP,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(4)))
        .add_systems(FixedUpdate, |mut ticks: ResMut<Ticks>| ticks.0 += 1);
    app.update();
    app
}

#[test]
fn slow_motion_preserves_fixed_step_and_interpolates_on_every_render_frame() {
    let mut app = app();
    let step = resonance_game::clock::UPDATE_STEP;
    for speed in [Speed::Half, Speed::Fifth, Speed::Tenth, Speed::Normal] {
        press(app.world_mut(), 3); // Works at the title, without a field session.
        assert_eq!(app.world().resource::<Controls>().speed, speed);
        let real_start = app.world().resource::<Time<Real>>().elapsed();
        let virtual_start = app.world().resource::<Time<Virtual>>().elapsed();
        let mut previous = app.world().resource::<Time<Virtual>>().elapsed_secs_f64();
        let mut between_ticks = 0;
        for _ in 0..500 {
            let ticks = app.world().resource::<Ticks>().0;
            app.update();
            let fixed = app.world().resource::<Time<Fixed>>();
            let current = app.world().resource::<Ticks>().0;
            let presented =
                (f64::from(current) + fixed.overstep_fraction_f64()) * step.as_secs_f64();
            assert_eq!(fixed.timestep(), step);
            assert!((presented - previous - 0.004 * speed.factor()).abs() < 1e-7);
            previous = presented;
            between_ticks += usize::from(ticks == current);
        }
        assert_eq!(
            app.world().resource::<Time<Real>>().elapsed() - real_start,
            Duration::from_secs(2)
        );
        assert!(
            ((app.world().resource::<Time<Virtual>>().elapsed() - virtual_start).as_secs_f64()
                - 2. * speed.factor())
            .abs()
                < 1e-7
        );
        assert!(
            between_ticks > 350,
            "slow motion reduced render cadence instead of simulation cadence"
        );
    }
}

#[test]
fn changing_speed_preserves_pause_and_fast_forward_can_restore_normal_speed() {
    let mut app = app();
    press(app.world_mut(), 1);
    press(app.world_mut(), 3);
    let before = app.world().resource::<Time<Virtual>>().elapsed();
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(app.world().resource::<Time<Virtual>>().elapsed(), before);
    assert_eq!(app.world().resource::<Controls>().speed, Speed::Half);
    press(app.world_mut(), 1);
    app.update();
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    assert_eq!(
        app.world().resource::<Time<Virtual>>().relative_speed_f64(),
        0.5
    );
    press(app.world_mut(), 0);
    assert_eq!(app.world().resource::<Controls>().speed, Speed::Double);
    press(app.world_mut(), 0);
    assert_eq!(
        app.world().resource::<Time<Virtual>>().relative_speed_f64(),
        1.
    );
}

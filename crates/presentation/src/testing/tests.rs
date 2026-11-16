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

fn press(app: &mut App, action: StandardAction) {
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            toggle(
                &mut controls,
                &mut world.resource_mut::<Time<Virtual>>(),
                action,
                false,
            );
        });
}

fn slow(app: &mut App) {
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            slow_motion(&mut controls, &mut world.resource_mut::<Time<Virtual>>());
        });
}

#[test]
fn slow_motion_preserves_fixed_step_and_interpolates_on_every_render_frame() {
    let mut app = app();
    let step = resonance_game::clock::UPDATE_STEP;
    for speed in [Speed::Half, Speed::Fifth, Speed::Tenth, Speed::Normal] {
        slow(&mut app);
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
    press(&mut app, StandardAction::Pause);
    slow(&mut app);
    let before = app.world().resource::<Time<Virtual>>().elapsed();
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(app.world().resource::<Time<Virtual>>().elapsed(), before);
    assert_eq!(app.world().resource::<Controls>().speed, Speed::Half);
    press(&mut app, StandardAction::Pause);
    app.update();
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    assert_eq!(
        app.world().resource::<Time<Virtual>>().relative_speed_f64(),
        0.5
    );
    press(&mut app, StandardAction::SpeedToggle);
    assert_eq!(app.world().resource::<Controls>().speed, Speed::Double);
    press(&mut app, StandardAction::SpeedToggle);
    assert_eq!(
        app.world().resource::<Time<Virtual>>().relative_speed_f64(),
        1.
    );
}

#[test]
fn pause_resumes_virtual_time_without_losing_the_selected_speed() {
    use StandardAction::*;
    let mut controls = Controls::default();
    let mut time = Time::<Virtual>::default();
    toggle(&mut controls, &mut time, SpeedToggle, false);
    assert_eq!(time.relative_speed(), 2.);
    toggle(&mut controls, &mut time, Pause, false);
    assert!(controls.paused && time.is_paused());
    toggle(&mut controls, &mut time, SpeedToggle, false);
    assert!(time.is_paused());
    assert_eq!(time.relative_speed(), 1.);
    toggle(&mut controls, &mut time, Pause, false);
    assert!(!controls.paused && !time.is_paused());
    toggle(&mut controls, &mut time, SkipEvent, false);
    assert!(!controls.skipping);
    toggle(&mut controls, &mut time, SkipEvent, true);
    assert!(controls.skipping);
}

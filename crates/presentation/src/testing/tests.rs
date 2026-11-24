use super::*;
use crate::test_support::send_key;
use bevy::input::ButtonState::{self, Pressed, Released};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{PrimaryWindow, WindowEvent, WindowFocused};
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

struct StepInput {
    app: App,
    window: Entity,
}

impl StepInput {
    fn new() -> Self {
        let mut app = app();
        app.add_plugins(bevy::input::InputPlugin);
        crate::input::install(&mut app);
        app.add_systems(
            PreUpdate,
            (|| true).pipe(controls).in_set(crate::input::HostActions),
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        Self { app, window }
    }

    fn key(&mut self, state: ButtonState, repeat: bool) {
        send_key(
            self.app.world_mut(),
            self.window,
            KeyCode::F9,
            state,
            repeat,
        );
    }

    fn focus(&mut self, focused: bool) {
        let world = self.app.world_mut();
        world.get_mut::<Window>(self.window).unwrap().focused = focused;
        world.write_message(WindowEvent::WindowFocused(WindowFocused {
            window: self.window,
            focused,
        }));
    }

    fn frame(&mut self, states: &[ButtonState], millis: u64) -> u32 {
        for &state in states {
            self.key(state, false);
        }
        let elapsed = Duration::from_millis(millis);
        self.app
            .insert_resource(TimeUpdateStrategy::ManualDuration(elapsed));
        self.app.update();
        let fixed = self.app.world().resource::<Time<Fixed>>();
        assert_eq!(
            self.app.world().resource::<Time<Virtual>>().elapsed(),
            fixed.elapsed() + fixed.overstep()
        );
        self.app.world().resource::<Ticks>().0
    }

    fn stepping(&self) -> bool {
        self.app.world().resource::<Controls>().stepping()
    }
}

#[test]
fn tick_steps_freeze_immediately_and_resume_after_a_real_time_hold() {
    let mut input = StepInput::new();
    input.app.add_systems(Last, audio);
    slow(&mut input.app);
    let start = input.frame(&[], 20);
    let (control, mut output) = resonance_playback::Offline::new();
    let attach = |app: &mut App| {
        let pcm = std::sync::Arc::<resonance_playback::Pcm>::default();
        pcm.push(0, vec![1.; 4096]).unwrap();
        let handle = control
            .play(false, move || Ok(Box::new(pcm.source(false, 1.))))
            .unwrap();
        app.world_mut()
            .spawn(crate::audio_output::Sink(handle.clone()));
        handle
    };
    let first_sound = attach(&mut input.app);
    assert_eq!(input.frame(&[Pressed], 160), start);
    assert_eq!(input.frame(&[Released], 20), start);
    // Two complete taps in one input batch still present two distinct ticks.
    assert_eq!(
        input.frame(&[Pressed, Released, Pressed, Released], 1),
        start + 1
    );
    let next_sound = attach(&mut input.app);
    assert_eq!(input.frame(&[], 1), start + 2);
    assert_eq!(input.frame(&[], 100), start + 2);
    output.by_ref().take(4096).for_each(drop);
    assert_eq!(first_sound.rendered_frames(), 1068);
    assert_eq!(next_sound.rendered_frames(), 534);
    assert_eq!(input.frame(&[Pressed], 1), start + 3);
    input.key(Pressed, true);
    assert_eq!(input.frame(&[], 2999), start + 3);
    assert!(input.stepping());
    output.take(4096).for_each(drop);
    assert_eq!(first_sound.rendered_frames(), 1603);
    assert_eq!(input.frame(&[], 1), start + 3);
    assert!(!input.stepping());
    assert_eq!(input.frame(&[], 100), start + 6);
    assert_eq!(
        input
            .app
            .world()
            .resource::<Time<Virtual>>()
            .relative_speed(),
        0.5
    );

    input.frame(&[Released], 1);
    let before_hold = input.frame(&[Pressed, Released, Pressed], 1);
    // The old hold ends at its deadline. Its queued tick is still presented,
    // and a new complete tap in the release batch freezes again.
    let frozen = input.frame(&[Released, Pressed, Released], 3000);
    assert_eq!(frozen, before_hold + 1);
    assert!(input.stepping());
    assert_eq!(input.frame(&[], 4000), frozen);
    assert_eq!(input.frame(&[Pressed], 1), frozen + 1);
}

#[test]
fn focus_changes_preserve_delivered_taps_and_cancel_interrupted_holds() {
    let mut input = StepInput::new();
    let frozen = input.frame(&[Pressed], 1);
    // Winit sets window focus before PreUpdate and synthesizes the release in Last.
    input.focus(false);
    let window = input.window;
    input.app.add_systems(
        Last,
        (move |world: &mut World| {
            world.write_message(bevy::input::keyboard::KeyboardFocusLost);
            send_key(world, window, KeyCode::F9, Released, false);
        })
        .run_if(bevy::ecs::schedule::common_conditions::run_once),
    );
    assert_eq!(input.frame(&[], 3000), frozen);
    assert!(input.stepping());
    input.focus(true);
    assert_eq!(input.frame(&[Pressed], 1), frozen + 1);
    // If focus leaves and returns in one batch, Winit sends no synthetic release.
    input.focus(false);
    input.focus(true);
    assert_eq!(input.frame(&[], 3000), frozen + 1);
    assert!(input.stepping());
    assert_eq!(input.frame(&[Pressed], 1), frozen + 2);
    input.focus(false);
    input.focus(true);
    assert_eq!(input.frame(&[Pressed], 3000), frozen + 3);
    assert!(input.stepping());
    // A release before focus loss still completes the hold.
    input.key(Released, false);
    input.focus(false);
    assert_eq!(input.frame(&[], 3000), frozen + 3);
    assert!(!input.stepping());

    // Only the tap after focus returns can freeze the simulation.
    input.key(Pressed, false);
    input.key(Released, false);
    input.focus(true);
    let frozen = input.frame(&[Pressed, Released], 1);
    assert!(input.stepping());
    assert_eq!(input.frame(&[], 40), frozen);
    // A valid tap before focus loss also survives a batch ending unfocused.
    input.key(Pressed, false);
    input.key(Released, false);
    input.focus(false);
    assert_eq!(input.frame(&[Pressed, Released], 1), frozen + 1);
    assert_eq!(input.frame(&[], 40), frozen + 1);
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS for a live field session"]
fn stepping_yields_to_bindings_explicit_pause_media_and_replays() -> anyhow::Result<()> {
    use crate::{movie, new_game::Session, saves};
    use KeyCode::{F7, F9};
    use resonance_game::menu::{Menu, Page, input_capture::Capture};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").unwrap();
    let mut app = app();
    install(&mut app);
    app.add_plugins(bevy::input::InputPlugin);
    crate::input::install(&mut app);
    app.init_resource::<crate::timing::Ready>()
        .init_resource::<crate::boot::Playback>()
        .init_resource::<movie::Playback>()
        .init_resource::<crate::field_view::Controls>()
        .add_systems(PreUpdate, crate::field_view::gather_controls);
    let world = app.world_mut();
    let window = world.spawn((Window::default(), PrimaryWindow)).id();
    let tap = |world: &mut World, keys: &[KeyCode]| {
        for &key in keys {
            send_key(world, window, key, Pressed, false);
            send_key(world, window, key, Released, false);
        }
        world.run_schedule(PreUpdate);
    };
    tap(world, &[F9]); // Title, before a session exists.
    assert!(!world.resource::<Controls>().stepping());
    world.insert_resource(Session::load(root.as_ref())?);
    world.init_resource::<crate::loading::Resident>();
    tap(world, &[F9]); // The caller has loaded but its scene is still preparing.
    assert!(!world.resource::<Controls>().stepping());
    world
        .resource::<crate::loading::Resident>()
        .active
        .store(true, std::sync::atomic::Ordering::Release);
    tap(world, &[F9]); // Gameplay still waits for its destination audio bank.
    assert!(!world.resource::<Controls>().stepping());
    world.init_resource::<Assets<crate::field_audio::FieldSource>>();
    assert!(crate::field_audio::prepare_handoff(world)?);
    // Record F9 in the field menu and re-press it before a fixed tick can
    // publish the edited party. The new binding already owns the key.
    let session = world.resource::<Session>();
    let mut checkpoint = crate::test_support::field_checkpoint(&session.files())?;
    let arte = session.data().characters[0].allowed_techniques[0];
    checkpoint.progress.party.members[0].techniques.insert(arte);
    let mut menu = Menu::new(
        Page::Tech,
        Some(resonance_game::Checkpoint::Field(checkpoint)),
        false,
    );
    menu.tech.binding_technique = arte;
    menu.tech.capture = Some(Capture::chord([]));
    world.resource_mut::<Session>().field_mut().menu = Some(menu);
    for state in [Pressed, Released] {
        send_key(world, window, F9, state, false);
        world.run_schedule(PreUpdate);
    }
    let menu = world.resource::<Session>().menu().unwrap();
    assert_eq!(
        menu.tech.binding_message.as_deref(),
        Some("Combo assigned.")
    );
    tap(world, &[F9]);
    assert!(
        !world.resource::<Controls>().stepping(),
        "newly recorded F9 froze gameplay"
    );
    world.resource_mut::<Session>().field_mut().menu = None;
    tap(world, &[F9, F7]);
    assert!(world.resource::<Controls>().paused);
    assert!(!world.resource::<Controls>().stepping());
    tap(world, &[F7]);
    tap(world, &[F9]);
    assert!(world.resource::<Controls>().stepping());
    tap(world, &[F9, F7]);
    assert!(!world.resource::<Controls>().paused);
    assert!(world.resource::<Time<Virtual>>().delta().is_zero());
    saves::install_scenario_input(
        &mut app,
        serde_json::from_str(include_str!("../../examples/scenarios/field-route.json"))?,
    )?;
    let world = app.world_mut();
    tap(world, &[F9]);
    assert!(!world.resource::<Controls>().stepping());
    world.remove_resource::<saves::ScenarioInput>();
    for credits in [false, true] {
        tap(world, &[F9]);
        assert!(world.resource::<Controls>().stepping());
        if credits {
            crate::test_support::request_credits(&mut world.resource_mut::<Session>())?;
        } else {
            world.resource_mut::<movie::Playback>().active = true;
        }
        // The last gameplay tick has just started media. Release before audio attaches.
        world.run_schedule(PostUpdate);
        assert!(!world.resource::<Time<Virtual>>().is_paused());
        tap(world, &[F9]);
        assert!(!world.resource::<Controls>().stepping());
        world.resource_mut::<movie::Playback>().active = false;
    }
    Ok(())
}

#[test]
fn device_recovery_defers_manual_ticks_and_honors_the_players_resume() {
    use crate::input::{HostInput, InspectionKey::*};
    let mut app = app();
    app.init_resource::<HostInput>()
        .add_systems(PreUpdate, (|| true).pipe(controls));
    let update = |app: &mut App, key, millis| {
        app.world_mut().resource_mut::<HostInput>().tick_step = key;
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            millis,
        )));
        app.update();
        app.world().resource::<Ticks>().0
    };

    let frozen = update(&mut app, Pressed(1), 20);
    DevicePause::begin(app.world_mut());
    assert_eq!(update(&mut app, Pressed(2), 20), frozen);
    assert_eq!(update(&mut app, Released(0), 3000), frozen);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    assert_eq!(update(&mut app, Up, 100), frozen);
    DevicePause::finish(app.world_mut());
    assert_eq!(update(&mut app, Up, 100), frozen + 1);
    assert_eq!(update(&mut app, Up, 100), frozen + 2);
    assert_eq!(update(&mut app, Up, 0), frozen + 2);
    assert!(!app.world().resource::<Controls>().stepping());
    let running = update(&mut app, Up, 100);
    assert!(running > frozen + 2);

    // A freeze requested during a later outage also survives reconnection.
    DevicePause::begin(app.world_mut());
    assert_eq!(update(&mut app, Pressed(1), 20), running);
    assert_eq!(update(&mut app, Pressed(1), 20), running);
    DevicePause::finish(app.world_mut());
    assert_eq!(update(&mut app, Up, 100), running + 1);
    assert_eq!(update(&mut app, Up, 100), running + 1);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
}

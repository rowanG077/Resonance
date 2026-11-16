//! Bevy preview of the embeddable yggdrasil plugin.
use bevy::{
    camera::ScalingMode,
    core_pipeline::tonemapping::Tonemapping,
    ecs::schedule::{ScheduleLabel, SingleThreadedExecutor},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::{PrimaryWindow, WindowMode},
    winit::{UpdateMode, WinitSettings},
};
use resonance_yggdrasil::{
    FPS, HEIGHT, Lighting, Playback, Renderer, WIDTH, YggdrasilOutput, YggdrasilPlayback,
    YggdrasilPlugin, YggdrasilSystems,
};
use std::time::Duration;
fn main() {
    let seconds = std::env::args()
        .nth(1)
        .map(|v| v.parse::<f64>().expect("starting time in seconds"))
        .unwrap_or(0.);
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Resonance · Yggdrasil".into(),
                    resolution: (WIDTH, HEIGHT).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(WinitSettings {
        focused_mode: UpdateMode::reactive(Duration::from_secs_f64(1. / FPS as f64)),
        unfocused_mode: UpdateMode::reactive_low_power(Duration::from_secs_f64(1. / FPS as f64)),
    })
    .insert_resource(YggdrasilPlayback {
        seconds,
        ..default()
    })
    .add_plugins(YggdrasilPlugin)
    .add_systems(Startup, setup)
    .add_systems(Update, controls.before(YggdrasilSystems::Advance))
    .add_systems(Update, title.after(YggdrasilSystems::Render));
    for label in [
        First.intern(),
        PreUpdate.intern(),
        Update.intern(),
        PostUpdate.intern(),
        Last.intern(),
    ] {
        app.get_schedule_mut(label)
            .unwrap()
            .set_executor(SingleThreadedExecutor::new());
    }
    app.sub_app_mut(bevy::render::RenderApp)
        .get_schedule_mut(bevy::render::Render)
        .unwrap()
        .set_executor(SingleThreadedExecutor::new());
    app.run();
}
fn setup(mut commands: Commands, output: Res<YggdrasilOutput>) {
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: WIDTH as f32,
                min_height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn(Sprite::from_image(output.image.clone()));
}
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<YggdrasilPlayback>,
    renderer: Res<Renderer>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::F11) {
        window.mode = match window.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
    if keys.just_pressed(KeyCode::Space) {
        state.paused = !state.paused;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        state.seconds = (state.seconds - 5.).max(0.);
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        state.seconds += 5.;
    }
    if keys.just_pressed(KeyCode::Comma) {
        state.seconds = (resonance_yggdrasil::exposure(state.seconds) - 1. / FPS as f64).max(0.);
        state.paused = true;
    }
    if keys.just_pressed(KeyCode::Period) {
        state.seconds = resonance_yggdrasil::exposure(state.seconds) + 1. / FPS as f64;
        state.paused = true;
    }
    if keys.just_pressed(KeyCode::Home) {
        state.seconds = 0.;
    }
    if keys.just_pressed(KeyCode::End) {
        let visits = renderer.events();
        state.seconds = match &state.playback {
            Playback::Event(id) => visits
                .iter()
                .find(|e| &e.id == id)
                .map(|e| e.duration)
                .unwrap_or(0.),
            _ => renderer.duration(),
        };
    }
    if keys.just_pressed(KeyCode::Tab) {
        let visits = renderer.events();
        state.playback = match &state.playback {
            Playback::Event(id) => visits
                .iter()
                .position(|e| &e.id == id)
                .and_then(|i| visits.get(i + 1))
                .map(|e| Playback::Event(e.id.clone()))
                .unwrap_or_default(),
            _ => Playback::Event(visits[0].id.clone()),
        };
        state.seconds = 0.;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        state.options.lighting = match state.options.lighting {
            Lighting::Auto => Lighting::Day,
            Lighting::Day => Lighting::Night,
            Lighting::Night => Lighting::Auto,
        };
    }
    if keys.just_pressed(KeyCode::Equal) {
        state.speed = (state.speed * 2.).min(8.);
    }
    if keys.just_pressed(KeyCode::Minus) {
        state.speed = (state.speed / 2.).max(0.125);
    }
    if keys.just_pressed(KeyCode::KeyS) {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(
                std::env::temp_dir().join("resonance-yggdrasil.png"),
            ));
    }
}
fn title(
    state: Res<YggdrasilPlayback>,
    output: Res<YggdrasilOutput>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let name = match &state.playback {
        Playback::Journey => "Complete journey",
        Playback::Event(id) => id,
        Playback::Growth => "Growth",
    };
    window.title = format!(
        "Resonance · {name} · {:.2}s · {:.0}% · {} · Space: pause · ←/→: seek · Tab: visit · N: sky",
        state.seconds,
        output.progress * 100.,
        if state.paused { "Paused" } else { "Playing" }
    );
}

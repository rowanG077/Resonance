//! Embeddable Bevy animation plugin. The host owns its window and camera;
//! Yggdrasil supplies an image rendered from sprite parts and GPU passes.
use crate::{HEIGHT, Options, Renderer, WIDTH, exposure, gpu};
use bevy::prelude::*;
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Playback {
    #[default]
    Journey,
    Event(String),
    Growth,
}
/// Independent story time and extraction/cooking progress.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct YggdrasilPlayback {
    pub seconds: f64,
    pub speed: f64,
    pub paused: bool,
    pub playback: Playback,
    pub progress: Option<f64>,
    pub options: Options,
}
impl Default for YggdrasilPlayback {
    fn default() -> Self {
        Self {
            seconds: 0.,
            speed: 1.,
            paused: false,
            playback: Playback::Journey,
            progress: None,
            options: Options::default(),
        }
    }
}
/// A normal Bevy sRGB image. Attach it to a Sprite or installer image widget.
/// Render-graph passes run before host cameras and preserve the 24 FPS exposure.
#[derive(Resource)]
pub struct YggdrasilOutput {
    pub image: Handle<Image>,
    pub seconds: f64,
    pub progress: f64,
    pub error: Option<String>,
}
#[derive(Resource, Default)]
pub(crate) struct LastSample(pub Option<YggdrasilPlayback>);
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum YggdrasilSystems {
    Advance,
    Render,
}
pub struct YggdrasilPlugin;
impl Plugin for YggdrasilPlugin {
    fn build(&self, app: &mut App) {
        let renderer = Renderer::new().expect("embedded yggdrasil assets");
        gpu::install(app);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(gpu::target(WIDTH, HEIGHT));
        app.init_resource::<YggdrasilPlayback>()
            .init_resource::<LastSample>()
            .insert_resource(renderer)
            .insert_resource(YggdrasilOutput {
                image,
                seconds: 0.,
                progress: 0.,
                error: None,
            })
            .configure_sets(
                Update,
                (YggdrasilSystems::Advance, YggdrasilSystems::Render).chain(),
            )
            .add_systems(Update, advance.in_set(YggdrasilSystems::Advance))
            .add_systems(Update, render.in_set(YggdrasilSystems::Render));
    }
}
fn advance(time: Res<Time<Real>>, mut state: ResMut<YggdrasilPlayback>) {
    if !state.paused {
        state.seconds = (state.seconds + time.delta_secs_f64() * state.speed).max(0.);
    }
}
fn render(
    state: Res<YggdrasilPlayback>,
    mut renderer: ResMut<Renderer>,
    mut last: ResMut<LastSample>,
    mut input: ResMut<gpu::Input>,
    mut output: ResMut<YggdrasilOutput>,
) {
    let seconds = exposure(state.seconds);
    if last.0.as_ref().is_some_and(|old| {
        old.seconds == seconds
            && old.playback == state.playback
            && old.progress == state.progress
            && old.options == state.options
    }) {
        return;
    }
    let mut sampled = state.clone();
    sampled.seconds = seconds;
    last.0 = Some(sampled);
    match renderer.render(&state.playback, seconds, state.progress, &state.options) {
        Ok(frame) => {
            output.seconds = frame.seconds;
            output.progress = frame.progress;
            output.error = None;
            *input = gpu::Input {
                frame: Some(frame),
                output: output.image.clone(),
            };
        }
        Err(error) => output.error = Some(error.to_string()),
    }
}

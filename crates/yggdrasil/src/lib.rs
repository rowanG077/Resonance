//! Yggdrasil and its companions, drawn natively through Bevy.

pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 640;
pub const FPS: u32 = 24;
pub(crate) const ENVIRONMENT_SCALE: f64 = 2.;
pub(crate) const ENVIRONMENT_WIDTH: i32 = WIDTH as i32 / 2;
pub(crate) const ENVIRONMENT_HEIGHT: i32 = HEIGHT as i32 / 2;
pub(crate) const SCENE_BOUNDS: [f64; 4] = [0., 0., WIDTH as f64, HEIGHT as f64];
pub(crate) const ENVIRONMENT_BOUNDS: [f64; 4] =
    [0., 0., ENVIRONMENT_WIDTH as f64, ENVIRONMENT_HEIGHT as f64];

mod actors;
mod data;
mod gpu;
mod lighting;
mod plugin;
mod props;
mod scene;
pub use plugin::{Playback, YggdrasilOutput, YggdrasilPlayback, YggdrasilPlugin, YggdrasilSystems};
mod drawing;
#[cfg(test)]
mod gpu_tests;
mod renderer;
#[cfg(test)]
mod tests;
mod timeline;
mod tree;

pub use lighting::Lighting;
pub use renderer::{Options, Renderer};
pub use timeline::Event;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Unknown yggdrasil event: {0}")]
    UnknownEvent(String),
    #[error("Invalid authored yggdrasil data in {file}: {source}")]
    Data {
        file: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("Invalid yggdrasil texture: {0}")]
    Texture(#[from] bevy::image::TextureError),
    #[error("Invalid yggdrasil atlas: {0}")]
    Atlas(String),
}

/// Sample an animation exposure on the approved 24-drawings-per-second clock.
pub fn exposure(seconds: f64) -> f64 {
    let seconds = if seconds.is_finite() {
        seconds.max(0.)
    } else {
        0.
    };
    // Four nanoseconds of tolerance also absorbs cancellation when subtracting
    // a visit start from a journey timestamp before sampling it again.
    const FRAME_BOUNDARY_TOLERANCE: f64 = 1e-7;
    ((seconds * FPS as f64 + FRAME_BOUNDARY_TOLERANCE).floor()) / FPS as f64
}

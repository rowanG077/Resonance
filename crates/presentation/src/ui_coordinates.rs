//! Coordinates shared by dialogue and menu drawing.
use resonance_content::{HEIGHT, SCENE_HEIGHT};

/// Round Y in the 448-row scene projection before adding pixel offsets, then
/// convert back to the dialogue overlay’s 480-row coordinates.
pub(super) fn drawing_y(y: f32) -> f32 {
    (y * SCENE_HEIGHT as f32 / HEIGHT as f32).trunc()
}

pub(super) fn overlay_rect([left, top, right, bottom]: [f32; 4]) -> [f32; 4] {
    [
        left,
        top * (HEIGHT as f32 / SCENE_HEIGHT as f32),
        right,
        bottom * (HEIGHT as f32 / SCENE_HEIGHT as f32),
    ]
}

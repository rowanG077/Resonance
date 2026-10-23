//! Numbered scenes combine camera tracks, baked actor motion and UV controllers.
use super::*;
use resonance_game::overworld::cinematic::Playback;

#[cfg(test)]
mod tests;

pub(super) const FAR_CLIP: f32 = 12800.;

pub(super) fn origin(playback: &Playback) -> Position {
    let eye = playback.camera().position;
    Position::from_map([38400. + eye[0], 28800. - eye[1], 0.]).unwrap()
}
/// Place the virtual player 2800 units ahead of the eye.
/// Sky geometry follows that point, including the two scene-specific skies.
pub(super) fn sky_position(playback: &Playback) -> Position {
    let camera = playback.camera();
    let forward = Vec2::new(
        camera.target[0] - camera.position[0],
        camera.target[1] - camera.position[1],
    )
    .normalize_or_zero();
    origin(playback)
        .translated([2800. * forward.x, 2800. * forward.y, -100.])
        .unwrap()
}
pub(super) fn background(definition: &resonance_content::overworld::Cinematic, actor: u8) -> bool {
    definition.presentation.actors[&actor].background
}
pub(super) fn draw_order(
    definition: &resonance_content::overworld::Cinematic,
    actor: u8,
    material: u32,
) -> super::super::draw_order::DrawOrder {
    use super::super::draw_order::{DrawOrder, Layer};
    let layer = if background(definition, actor) {
        Layer::Background
    } else {
        Layer::Effects
    };
    DrawOrder(layer, usize::from(actor), material)
}
pub(super) fn surface(
    definition: &resonance_content::overworld::Cinematic,
    actor: u8,
    surface: &mut TitleSurface,
) {
    use resonance_content::overworld::CinematicBlend;
    let track = &definition.presentation.actors[&actor];
    if let Some(depth_write) = track.depth_write {
        surface.depth_write = depth_write;
    }
    if let Some(depth_test) = track.depth_test {
        surface.depth_test = depth_test;
    }
    if let Some(blend) = track.blend {
        surface.blend = Some(match blend {
            CinematicBlend::Alpha => Blend::Alpha,
            CinematicBlend::Additive => Blend::Additive,
        });
    }
}
pub(super) fn opacity(playback: &Playback, actor: u8) -> f32 {
    f32::from(
        playback.definition.presentation.actors[&actor]
            .opacity
            .map_or(255, |window| window.alpha(playback.ticks())),
    ) / 255.
}
pub(super) fn uv(playback: &Playback, actor: u8, texture: usize, presentation_tick: u32) -> Vec2 {
    playback.definition.presentation.actors[&actor]
        .uv
        .iter()
        .find(|track| track.animation.texture == texture)
        .map_or(Vec2::ZERO, |track| {
            Vec2::from_array(track.offset(playback.ticks(), presentation_tick))
        })
}

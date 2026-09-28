//! Numbered scenes use the original camera, baked actor motion and UV controllers.
use super::*;
use resonance_game::overworld::cinematic::Playback;

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
pub(super) fn background(id: u16, actor: u8) -> bool {
    matches!(id, 521 | 526) && actor == 1
}
pub(super) fn surface(id: u16, actor: u8, surface: &mut TitleSurface) {
    if matches!(
        (id, actor),
        (516..=518, 5..=9) | (520, 2) | (521, 2..=4) | (522..=524, _) | (525..=526, 1..=2)
    ) {
        surface.blend = true;
        surface.depth_write = false;
        surface.depth_test = !matches!((id, actor), (520, 2) | (521, 4) | (525..=526, _));
        surface.additive = !matches!((id, actor), (521, 2..=3) | (525..=526, 2) | (526, 1));
    }
    if id == 516 && actor <= 4 {
        surface.blend = true;
    }
    if background(id, actor) {
        surface.depth_write = false;
    }
}
pub(super) fn opacity(playback: &Playback, actor: u8) -> f32 {
    if playback.id != 516 || actor > 8 {
        return 1.;
    }
    let (begin, end) = [
        (66, 76),
        (86, 96),
        (118, 128),
        (172, 182),
        (60, 176),
        (80, 196),
        (112, 228),
        (166, 240),
    ][actor as usize - 1];
    let tick = playback.ticks();
    if tick >= end {
        0.
    } else if tick > begin {
        (end - 1 - tick) as f32 / (end - begin) as f32
    } else {
        1.
    }
}
pub(super) fn uv(playback: &Playback, actor: u8, texture: usize) -> Vec2 {
    let tick = playback.ticks();
    match (playback.id, actor, texture) {
        (513, 1, 0) => Vec2::new(0., ((tick % 32) >> 3) as f32 / 4.),
        (516..=518, 5..=8, 0) => Vec2::new(0., (tick % 128) as f32 / 128.),
        (516, 9, 1) => {
            let tick = tick.saturating_sub(1).min(240);
            let y = if tick < 60 {
                tick as f32 / 300.
            } else if tick < 210 {
                (tick - 60) as f32 / 600. + 0.4
            } else {
                (tick - 210) as f32 / 300. + 0.6
            };
            Vec2::new(0., y)
        }
        (516, 9, 2) => Vec2::new(0., tick.saturating_sub(1).min(40) as f32 / 40.),
        (521, 2, 0) => Vec2::new((tick % 512) as f32 / 512., 0.),
        (525, 1, 0) => Vec2::new(0., (tick % 32) as f32 / 32.),
        (522..=524, 1..=7, 0) => {
            let frame = if actor <= 3 {
                let begin = [46, 62, 88][actor as usize - 1];
                if (begin..=begin + 46).contains(&tick) {
                    (tick - begin + 1) / 6 + 1
                } else {
                    0
                }
            } else {
                let begin = [
                    [130, 104, 162, 142],
                    [132, 100, 110, 142],
                    [100, 122, 142, 168],
                ][(playback.id - 522) as usize][actor as usize - 4];
                if (begin..=begin + 94).contains(&tick) {
                    match (tick - begin) / 2 {
                        0..=2 => 1,
                        3..=8 => 2,
                        9..=15 => 3,
                        16..=23 => 4,
                        24..=28 => 3,
                        29..=33 => 2,
                        34..=42 => 1,
                        _ => 0,
                    }
                } else {
                    0
                }
            };
            Vec2::new(0., frame as f32 / if actor <= 3 { 8. } else { 4. })
        }
        _ => Vec2::ZERO,
    }
}

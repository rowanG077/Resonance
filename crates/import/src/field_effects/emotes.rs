//! Emote artwork with a short entrance and a smooth repeating motion.
use resonance_content::effect::{EmoteTrack, Sprite, VerticalAnchor};
use std::{collections::BTreeMap, f32::consts::TAU};

const INTRO_TICKS: usize = 12;
const CYCLE_TICKS: usize = 48;
const ATLAS_SIZE: f32 = 256.;

fn glyph(x: f32, z: f32, size: f32, rect: [f32; 4]) -> Sprite {
    Sprite {
        offset: [x, 0., z],
        size: [size; 2],
        uv: rect.map(|v| v / ATLAS_SIZE),
        rotation: 0.,
        vertical_anchor: VerticalAnchor::Center,
        alpha: 255,
    }
}

fn artwork(kind: u16, tick: usize) -> Vec<Sprite> {
    let mut sprites = Vec::new();
    if matches!(kind, 0 | 1 | 4..=8 | 10 | 12) {
        let left = if matches!(kind, 5 | 12) { 96. } else { 0. };
        sprites.push(glyph(24., 82., 80., [left, 0., left + 96., 96.]));
    }
    let mut mark = |x, z, size, rect| sprites.push(glyph(x, z, size, rect));
    match kind {
        0 => {
            for x in [12., 24., 36.] {
                mark(x, 90., 20., [223., 144., 255., 176.]);
            }
        }
        1 => {
            let left = 96. + (tick / 16 % 3) as f32 * 32.;
            mark(28., 90., 52., [left, 112., left + 32., 144.]);
        }
        2 => {
            for (x, z, left) in [
                (-20., 24., 192.),
                (-5., 24., 128.),
                (-1., 44., 96.),
                (5., 24., 160.),
                (20., 29., 128.),
            ] {
                mark(x, z, 32., [left, 176., left + 32., 208.]);
            }
        }
        3 => {
            let left = (tick / 24 % 2) as f32 * 48.;
            mark(32., 66., 80., [left, 176., left + 48., 224.]);
        }
        4 => {
            for x in [10., 22., 34., 46.] {
                mark(x, 64., 28., [192., 112., 224., 144.]);
            }
        }
        5 => mark(24., 60., 54., [96., 144., 128., 176.]),
        6 => mark(28., 64., 52., [64., 144., 96., 176.]),
        7 => mark(28., 64., 52., [32., 144., 64., 176.]),
        8 => mark(24., 90., 52., [32., 112., 64., 144.]),
        9 => mark(24., 92., 52., [0., 112., 32., 144.]),
        10 => mark(27., 94., 52., [0., 144., 32., 176.]),
        11 => {
            for x in [5., 20., 35.] {
                mark(x, 64., 22., [223., 112., 255., 144.]);
            }
        }
        12 => {
            let left = 128. + (tick / 16 % 3) as f32 * 32.;
            mark(24., 86., 52., [left, 144., left + 32., 176.]);
        }
        13 => {
            for x in [-24., -8., 8., 24.] {
                mark(x, 72., 28., [64., 112., 96., 144.]);
            }
        }
        14 => {
            for x in [-24., -8., 8., 24.] {
                mark(x, 50., 30., [192., 176., 224., 208.]);
            }
        }
        15 => mark(10., 32., 30., [223., 176., 255., 208.]),
        _ => {}
    }
    sprites
}

pub(super) fn tracks() -> BTreeMap<u16, EmoteTrack> {
    (0..20)
        .map(|kind| {
            let intro = (0..INTRO_TICKS)
                .map(|tick| {
                    let progress = tick as f32 / INTRO_TICKS as f32;
                    let scale = 1. - (1. - progress).powi(2);
                    let mut sprites = artwork(kind, 0);
                    for sprite in &mut sprites {
                        sprite.size = sprite.size.map(|size| size * scale);
                        sprite.alpha = (255. * scale) as u8;
                        sprite.offset[2] -= (1. - scale) * 12.;
                    }
                    sprites
                })
                .collect();
            let cycle = (0..CYCLE_TICKS)
                .map(|tick| {
                    let phase = tick as f32 / CYCLE_TICKS as f32;
                    let mut sprites = artwork(kind, tick);
                    for (index, sprite) in sprites.iter_mut().enumerate() {
                        let wave = (TAU * phase).sin();
                        match kind {
                            3 | 11 => sprite.offset[2] -= wave * 8.,
                            9 => {
                                sprite.rotation = phase * 360.;
                                sprite.offset[2] += wave * 6.;
                            }
                            13 => {
                                let rise = (phase + index as f32 / 4.) % 1.;
                                sprite.offset[2] += rise * 24.;
                                sprite.size = sprite.size.map(|v| v * (0.5 + rise));
                                sprite.alpha = (255. * (TAU * rise / 2.).sin()) as u8;
                            }
                            14 => sprite.rotation = sprite.offset[0],
                            _ => sprite.offset[2] += wave * 3.,
                        }
                    }
                    sprites
                })
                .collect();
            (
                kind,
                EmoteTrack {
                    anchor: "Bone_atama".into(),
                    missing_anchor_offset: [0., 0., 128.],
                    intro,
                    cycle,
                },
            )
        })
        .collect()
}

#[test]
fn emotes_enter_visibly_and_loop_with_valid_artwork() {
    for (kind, track) in tracks() {
        if kind >= 16 {
            assert!(track.intro.iter().chain(&track.cycle).all(Vec::is_empty));
            continue;
        }
        assert!(track.frame(0).iter().all(|s| s.alpha == 0));
        assert!(track.frame(INTRO_TICKS).iter().any(|s| s.alpha > 0));
        for frame in track.intro.iter().chain(&track.cycle) {
            for sprite in frame {
                assert!(
                    sprite
                        .offset
                        .iter()
                        .chain(&sprite.size)
                        .all(|v| v.is_finite())
                );
                assert!(sprite.uv.iter().all(|v| (0. ..=1.).contains(v)));
                assert!(sprite.uv[0] < sprite.uv[2] && sprite.uv[1] < sprite.uv[3]);
            }
        }
        assert_eq!(
            track.frame(INTRO_TICKS)[0].offset,
            track.frame(INTRO_TICKS + CYCLE_TICKS)[0].offset
        );
    }
}

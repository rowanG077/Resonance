//! Head symbols with individual entrances and repeating gestures.
use resonance_content::effect::{EmoteTrack, Sprite, VerticalAnchor};
use std::{collections::BTreeMap, f32::consts::TAU};

const INTRO_TICKS: usize = 60;
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
        let left = if kind == 5 { 96. } else { 0. };
        sprites.push(glyph(24., 82., 80., [left, 0., left + 96., 96.]));
    }
    let mut mark = |x, z, size, rect| sprites.push(glyph(x, z, size, rect));
    match kind {
        0 => {
            for &x in [12., 24., 36.].iter().take((tick / 18).min(3)) {
                mark(x, 90., 20., [223., 144., 255., 176.]);
            }
        }
        1 => {
            let left = 96. + (tick / 4 % 3) as f32 * 32.;
            mark(28., 90., 52., [left, 112., left + 32., 144.]);
        }
        2 => {
            for (x, z, left, rotation) in [
                (-38., 32., 192., 55.),
                (-22., 56., 160., 25.),
                (0., 64., 128., 0.),
                (24., 55., 96., -25.),
                (40., 33., 192., -55.),
            ] {
                let pulse = 1. + 0.2 * (TAU * tick as f32 / 16.).sin();
                let mut sprite = glyph(x * pulse, z * pulse, 36., [left, 176., left + 32., 208.]);
                sprite.rotation = rotation;
                sprites.push(sprite);
            }
        }
        3 => {
            let large = (tick / 12).is_multiple_of(2);
            let left = if large { 48. } else { 0. };
            mark(
                if large { 42. } else { 24. },
                if large { 64. } else { 32. },
                64.,
                [left, 176., left + 48., 224.],
            );
        }
        4 => {
            for &x in [10., 22., 34., 46.].iter().take((tick / 12 + 1).min(4)) {
                mark(x, 88., 28., [192., 112., 224., 144.]);
            }
        }
        5 => mark(24., 88., 54., [96., 144., 128., 176.]),
        6 => mark(28., 88., 52., [64., 144., 96., 176.]),
        7 => mark(28., 88., 52., [32., 144., 64., 176.]),
        8 => mark(24., 90., 52., [32., 112., 64., 144.]),
        10 => mark(
            27.,
            94. + (16. - tick as f32).max(0.) * 2.,
            52.,
            [0., 144., 32., 176.],
        ),
        11 => {
            for (index, x) in [10., 28., 44.].into_iter().enumerate() {
                let phase = ((tick + index * 8) % 24) as f32 / 24.;
                let mut sprite = glyph(
                    x + phase * 10.,
                    50. - phase * 24.,
                    16.,
                    [224., 112., 256., 144.],
                );
                sprite.rotation = -45.;
                sprite.alpha = (255. * (1. - phase)) as u8;
                sprites.push(sprite);
            }
        }
        12 => {
            let left = 128. + (tick / 8 % 3) as f32 * 32.;
            mark(24., 86., 52., [left, 144., left + 32., 176.]);
        }
        13 => {
            for (index, x) in [-20., 8., 28.].into_iter().enumerate() {
                let phase = ((tick + index * 16) % 48) as f32 / 48.;
                let mut sprite = glyph(
                    x,
                    32. + phase * 65.,
                    24. * (0.5 + phase),
                    [64., 112., 96., 144.],
                );
                sprite.alpha = (255. * (TAU * phase / 2.).sin()) as u8;
                sprites.push(sprite);
            }
        }
        14 => {
            for (x, z, angle) in [
                (8., 54., -10.),
                (23., 49., -30.),
                (35., 37., -50.),
                (42., 22., -70.),
            ] {
                let mut sprite = glyph(x, z, 23., [192., 176., 224., 208.]);
                sprite.rotation = angle;
                sprites.push(sprite);
            }
        }
        15 => mark(10., 32., 30., [223., 176., 255., 208.]),
        _ => {}
    }
    for (index, sprite) in sprites.iter_mut().enumerate() {
        let bubble = index == 0 && matches!(kind, 0 | 1 | 4..=8 | 10 | 12);
        let entrance = if bubble {
            tick as f32 / 8.
        } else {
            tick.saturating_sub(4) as f32 / 12.
        }
        .min(1.);
        sprite.size = sprite.size.map(|v| v * entrance);
        sprite.alpha = (f32::from(sprite.alpha) * entrance) as u8;
    }
    sprites
}

pub(super) fn tracks() -> BTreeMap<u16, EmoteTrack> {
    (0..20)
        .map(|kind| {
            let intro = (0..INTRO_TICKS).map(|tick| artwork(kind, tick)).collect();
            let cycle = (0..CYCLE_TICKS)
                .map(|tick| artwork(kind, INTRO_TICKS + tick))
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
        if kind == 9 || kind >= 16 {
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

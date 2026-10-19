//! Head symbols with individual entrances and repeating gestures.
use resonance_content::effect::{Sprite, VerticalAnchor};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Ellipsis,
    Distress,
    ColorBurst,
    Steam,
    Blush,
    Exclamation,
    Question,
    Music,
    Heart,
    Hidden,
    Sweat,
    SweatDrops,
    Sleep,
    Hearts,
    Surprise,
    Anger,
}
impl TryFrom<i32> for Kind {
    type Error = &'static str;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        use Kind::*;
        const KINDS: [Kind; 20] = [
            Ellipsis,
            Distress,
            ColorBurst,
            Steam,
            Blush,
            Exclamation,
            Question,
            Music,
            Heart,
            Hidden,
            Sweat,
            SweatDrops,
            Sleep,
            Hearts,
            Surprise,
            Anger,
            Hidden,
            Hidden,
            Hidden,
            Hidden,
        ];
        usize::try_from(value)
            .ok()
            .and_then(|i| KINDS.get(i))
            .copied()
            .ok_or("unknown emote recipe")
    }
}

const FRAME_UPDATES: u32 = 8;

impl Kind {
    pub(crate) fn phase(self, effect_tick: u32, random: u32) -> u32 {
        match self {
            Self::ColorBurst => random % FRAME_UPDATES,
            Self::Distress | Self::Steam | Self::Sleep => {
                effect_tick.wrapping_add(1) % FRAME_UPDATES
            }
            _ => random,
        }
    }
}

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

pub fn paralysis(frame: u8) -> Sprite {
    let top = if frame == 0 { 16. } else { 0. };
    Sprite {
        size: [72., 24.],
        ..glyph(0., 64., 72., [137., top, 184., top + 15.])
    }
}

pub fn sprites(kind: Kind, tick: u32, phase: u32) -> Vec<Sprite> {
    use Kind::*;
    if tick == 0 {
        return Vec::new();
    }
    let clock = phase.wrapping_add(tick - 1);
    let frame = clock / FRAME_UPDATES;
    // A mark becomes visible on the update after its interval ends.
    let marks = (tick - 1 + phase.wrapping_add(1) % 16) / 16;
    let entrance = (tick as f32 * 6. - 15.).clamp(0., 52.);
    let mut sprites = Vec::new();
    if matches!(
        kind,
        Ellipsis | Distress | Blush | Exclamation | Question | Music | Heart | Sweat | Sleep
    ) {
        let left = if matches!(kind, Exclamation) { 96. } else { 0. };
        sprites.push(glyph(
            24.,
            82.,
            (tick as f32 * 10. + 2.).min(80.),
            [left, 0., left + 96., 96.],
        ));
    }
    let mut mark = |x, z, size, rect| sprites.push(glyph(x, z, size, rect));
    match kind {
        Ellipsis => {
            for &x in [12., 24., 36.].iter().take(marks as usize) {
                mark(x, 90., 52., [223., 144., 255., 176.]);
            }
        }
        Distress => {
            let left = 96. + (frame % 3) as f32 * 32.;
            mark(28., 90., entrance, [left, 112., left + 32., 144.]);
        }
        ColorBurst => {
            let poses = if (clock / 8).is_multiple_of(2) {
                [
                    (-20., 24., 50., 192., 50.),
                    (-4., 24., 90., 128., 30.),
                    (-1., 44., 70., 96., 10.),
                    (5., 24., 80., 160., -32.),
                    (20., 29., 50., 128., -55.),
                ]
            } else {
                [
                    (-10., 26., 50., 96., 50.),
                    (-4., 24., 1., 128., 30.),
                    (-1., 9., 80., 128., 10.),
                    (5., 32., 50., 96., -32.),
                    (10., 29., 50., 192., -55.),
                ]
            };
            for ((x, z, size, left, rotation), width) in
                poses.into_iter().zip(if (clock / 8).is_multiple_of(2) {
                    [50., 65., 45., 90., 50.]
                } else {
                    [55., 1., 30., 30., 30.]
                })
            {
                let mut sprite = glyph(x, z, size, [left, 176., left + 32., 208.]);
                sprite.vertical_anchor = VerticalAnchor::Bottom;
                sprite.rotation = rotation;
                sprite.size[0] = width;

                sprites.push(sprite);
            }
        }
        Steam => {
            let left = (frame % 2) as f32 * 48.;
            mark(32., 66., 80., [left, 176., left + 48., 224.]);
        }
        Blush => {
            for &x in [10., 22., 34., 46.].iter().take(marks as usize) {
                let mut sprite = glyph(x, 64., 52., [192., 112., 224., 144.]);
                sprite.vertical_anchor = VerticalAnchor::Bottom;
                sprites.push(sprite);
            }
        }
        Exclamation | Question | Music => {
            const BANG_ENTRANCE: [f32; 12] =
                [35., 40., 50., 55., 65., 70., 80., 90., 80., 74., 62., 54.];
            let (x, z, size, left) = match kind {
                Exclamation => (24., 60., BANG_ENTRANCE[(tick as usize - 1).min(11)], 96.),
                Question => (28., 64., entrance, 64.),
                Music => (28., 64., entrance, 32.),
                _ => unreachable!(),
            };
            let mut sprite = glyph(x, z, size, [left, 144., left + 32., 176.]);
            sprite.vertical_anchor = VerticalAnchor::Bottom;
            sprites.push(sprite);
        }
        Heart => mark(24., 90., entrance, [32., 112., 64., 144.]),
        Sweat => {
            let mut sprite = glyph(
                27.,
                133. - tick.saturating_sub(12).min(5) as f32 * 4.,
                entrance,
                [0., 144., 32., 176.],
            );
            sprite.vertical_anchor = VerticalAnchor::Top;
            sprites.push(sprite);
        }
        SweatDrops => {
            let elapsed = (tick - 1) % 20;
            let t = elapsed as f32;
            for (x, z, vx, vz, rotation) in [
                (5., 34., 1.5, 3.22, 135.),
                (20., 28., 1.25, 2.6, 135.),
                (25., 18., 1., 1.9, 105.),
            ] {
                let mut sprite = glyph(
                    x + vx * t,
                    z + vz * t - 0.1 * t * (t - 1.),
                    22.,
                    [223., 112., 255., 144.],
                );
                sprite.rotation = rotation;
                sprite.alpha = (255. - (t - 10.).max(0.) * 25.) as u8;
                sprites.push(sprite);
            }
        }
        Sleep => {
            let left = 128. + (frame % 3) as f32 * 32.;
            mark(24., 86., entrance, [left, 144., left + 32., 176.]);
        }
        Hearts => {
            const HEART_BEAT: u32 = 32;
            const HEARTS: [[(f32, f32, f32); 4]; 2] = [
                [
                    (-30., 68., 35.),
                    (20., 56., 20.),
                    (-12., 42., 25.),
                    (-4., 55., 28.),
                ],
                [
                    (0., 83., 1.),
                    (-10., 55., 25.),
                    (24., 52., 35.),
                    (16., 70., 25.),
                ],
            ];
            let first_switch = (30 + HEART_BEAT - phase % HEART_BEAT) % HEART_BEAT + 1;
            let (pose, age) = if tick < first_switch {
                (0, tick)
            } else {
                let elapsed = tick - first_switch;
                (
                    ((elapsed / HEART_BEAT + 1) % 2) as usize,
                    elapsed % HEART_BEAT + 1,
                )
            };
            let scale = (0..age.min(13)).fold(0_f32, |s, _| ((f64::from(s) + 0.08) as f32).min(1.));
            let rise = (0..age).fold(0_f32, |h, _| (f64::from(h) + 0.22) as f32);
            for (x, z, size) in HEARTS[pose] {
                let mut sprite = glyph(x, z + rise, (size * scale).trunc(), [64., 112., 96., 144.]);
                sprite.alpha = (255. * scale) as u8;
                sprites.push(sprite);
            }
        }
        Surprise => {
            const FAN_ENTRANCE: [[(f32, f32); 4]; 3] = [
                [(6., 41.), (13., 37.), (16., 32.), (20., 25.)],
                [(8., 46.), (17., 42.), (22., 35.), (26., 27.)],
                [(10., 50.), (19., 45.), (26., 37.), (29.5, 27.)],
            ];
            for ((x, z), angle) in FAN_ENTRANCE[(tick as usize - 1).min(2)]
                .into_iter()
                .zip([-19., -39., -59., -79.])
            {
                let mut sprite = glyph(x, z, 30., [192., 176., 224., 208.]);
                sprite.rotation = angle;
                sprite.size[0] = 13.;
                sprites.push(sprite);
            }
        }
        Anger => mark(10., 32., 30., [223., 176., 255., 208.]),
        Hidden => {}
    }
    sprites
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progressive_symbols_start_empty_for_every_random_phase() {
        for kind in [Kind::Ellipsis, Kind::Blush] {
            for phase in 0..16 {
                assert_eq!(sprites(kind, 1, phase).len(), 1);
                assert_eq!(sprites(kind, 17, phase).len(), 2);
            }
        }
    }
}

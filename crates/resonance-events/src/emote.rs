//! Head symbols with individual entrances and repeating gestures.
use resonance_content::effect::{Sprite, VerticalAnchor};

pub(crate) const SCRIPT_IDS: std::ops::RangeInclusive<i32> = -299..=-100;

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

    /// A new gesture replaces the previous arrangement, even if it reuses the same atlas cells.
    pub fn next_sprites(self, from: u32, to: u32, phase: u32) -> Option<Vec<Sprite>> {
        (self.gesture(from, phase).0 == self.gesture(to, phase).0).then(|| sprites(self, to, phase))
    }

    /// Gesture number and zero-based age within that gesture.
    fn gesture(self, tick: u32, phase: u32) -> (u32, u32) {
        let elapsed = tick.saturating_sub(1);
        match self {
            Self::ColorBurst => {
                let clock = phase.wrapping_add(elapsed);
                (clock / FRAME_UPDATES, clock % FRAME_UPDATES)
            }
            Self::SweatDrops => (elapsed / 20, elapsed % 20),
            Self::Hearts => {
                const BEAT: u32 = 32;
                let initial = (30 + BEAT - phase % BEAT) % BEAT;
                if elapsed < initial {
                    (0, elapsed)
                } else {
                    let elapsed = elapsed - initial;
                    (1 + elapsed / BEAT, elapsed % BEAT)
                }
            }
            _ => (0, elapsed),
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
            let first = kind.gesture(tick, phase).0.is_multiple_of(2);
            let poses = if first {
                [
                    (-20., 24., 50., 50., 192., 50.),
                    (-4., 24., 65., 90., 128., 30.),
                    (-1., 44., 45., 70., 96., 10.),
                    (5., 24., 90., 80., 160., -32.),
                    (20., 29., 50., 50., 128., -55.),
                ]
            } else {
                [
                    (-10., 26., 55., 50., 96., 50.),
                    (-4., 24., 1., 1., 128., 30.),
                    (-1., 9., 30., 80., 128., 10.),
                    (5., 32., 30., 50., 96., -32.),
                    (10., 29., 30., 50., 192., -55.),
                ]
            };
            for (x, z, width, height, left, rotation) in poses {
                let mut sprite = glyph(x, z, height, [left, 176., left + 32., 208.]);
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
            let t = kind.gesture(tick, phase).1 as f32;
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
            let (gesture, elapsed) = kind.gesture(tick, phase);
            let pose = (gesture % 2) as usize;
            let age = elapsed + 1;
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
    fn gesture_sprites_follow_their_native_reveal_and_restart_clocks() {
        for kind in [Kind::Ellipsis, Kind::Blush] {
            for phase in 0..16 {
                assert_eq!(sprites(kind, 1, phase).len(), 1);
                assert_eq!(sprites(kind, 17, phase).len(), 2);
            }
        }
        for (kind, phase, last) in [
            (Kind::ColorBurst, 2, 6),
            (Kind::SweatDrops, 0, 20),
            (Kind::Hearts, 5, 25),
            (Kind::Hearts, 5, 57),
        ] {
            assert!(kind.next_sprites(last - 1, last, phase).is_some());
            assert!(kind.next_sprites(last, last + 1, phase).is_none());
        }
        for kind in [Kind::Distress, Kind::Sleep] {
            let current = sprites(kind, 6, 2);
            let next = kind.next_sprites(6, 7, 2).unwrap();
            assert_ne!(current[1].uv, next[1].uv);
            assert_eq!([current[1].size[0], next[1].size[0]], [21., 27.]);
        }
    }
}

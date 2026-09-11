//! Choice cursor trail, advanced once per game update.
use resonance_content::{HEIGHT, SCENE_HEIGHT};

// Rounded authoring coefficient: retain its precision before integer truncation.
const SCENE_Y_SCALE: f64 = 0.93333333;

pub(super) fn bob(style: &resonance_content::font::SelectionArt, tick: u32) -> f32 {
    const PERIOD: u32 = 24;
    // Scale the phase before division: rounding the angle per tick changes
    // which integer pixel the cursor occupies at the descending half-amplitude.
    let angle = style.bob_step * PERIOD as f32 * (tick % PERIOD) as f32 / PERIOD as f32;
    (style.bob_amplitude * angle.sin()).trunc()
}

/// Round Y in the 448-row scene projection before adding pixel offsets, then
/// convert back to the dialogue overlay’s 480-row coordinates.
pub(super) fn drawing_y(y: f32) -> f32 {
    ((f64::from(y) * SCENE_Y_SCALE) as f32).trunc()
}

pub(super) fn overlay_rect([left, top, right, bottom]: [f32; 4]) -> [f32; 4] {
    [
        left,
        top * (HEIGHT as f32 / SCENE_HEIGHT as f32),
        right,
        bottom * (HEIGHT as f32 / SCENE_HEIGHT as f32),
    ]
}

#[derive(Default)]
pub(super) struct Trail {
    anchor: Option<[i32; 2]>,
    remaining: u8,
    tick: Option<u32>,
}

impl Trail {
    pub(super) fn sample(&mut self, tick: u32, position: [i32; 2]) -> Vec<([i32; 2], u8)> {
        let anchor = *self.anchor.get_or_insert(position);
        if self.tick != Some(tick) {
            if anchor != position {
                if self.remaining == 0 {
                    self.remaining = 8;
                } else {
                    self.remaining -= 1;
                    if self.remaining == 0 {
                        self.anchor = Some(position);
                    }
                }
            }
            self.tick = Some(tick);
        }
        (1..=self.remaining)
            .filter_map(|i| {
                let alpha = 128 - i * 16;
                (alpha != 0).then(|| {
                    (
                        std::array::from_fn(|axis| {
                            position[axis] + i32::from(i) * (anchor[axis] - position[axis]) / 8
                        }),
                        alpha,
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_bob_preserves_integer_pixel_boundaries() {
        let style = resonance_content::font::SelectionArt {
            mode: 1,
            color: [255; 4],
            row_offsets: [0; 9],
            bob_amplitude: 4.,
            bob_step: std::f32::consts::PI / 24.,
        };
        assert_eq!(
            [0, 12, 20, 24].map(|tick| bob(&style, tick)),
            [0., 4., 1., 0.]
        );
    }

    #[test]
    fn dialogue_coordinates_preserve_the_original_cursor_size_and_rounding() {
        assert_eq!(drawing_y(375.), 350.);
        assert_eq!(drawing_y(68.), 63.);
        let rect = overlay_rect([203., 358., 235., 390.]);
        assert_eq!(rect[2] - rect[0], 32.);
        assert!((rect[3] - rect[1] - 32. * 480. / 448.).abs() < 0.0001);
    }

    #[test]
    fn cursor_motion_has_a_trail_but_paused_readback_does_not_advance_it() {
        let mut trail = Trail::default();
        assert!(trail.sample(10, [0, 0]).is_empty());
        let first = trail.sample(11, [16, 8]);
        assert_eq!(first.len(), 7);
        assert_eq!(first[0], ([14, 7], 112));
        assert_eq!(first[6], ([2, 1], 16));
        for _ in 0..20 {
            assert_eq!(trail.sample(11, [16, 8]), first);
        }
        for tick in 12..19 {
            assert!(!trail.sample(tick, [16, 8]).is_empty());
        }
        assert!(trail.sample(19, [16, 8]).is_empty());
        assert!(trail.sample(20, [16, 8]).is_empty());
    }
}

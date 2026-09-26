//! Source67648's eight display records, updated by67288 and drawn by66BD8.
use super::{Art, Batch, BattleFrame, Side, panel, results};
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Empty,
    Tracking,
    Finishing,
}

#[derive(Clone, Debug, Default)]
struct Record {
    mode: Mode,
    actor: usize,
    hits: i32,
    damage: i32,
    position: [f32; 2],
    step: [f32; 2],
    remaining: i16,
    alpha: i16,
    offset: i16,
    pulse: i8,
    pulse_alpha: u16,
}

impl Record {
    fn advance(&mut self, destination: [f32; 2], side: usize, tracking: Option<(i32, [f32; 2])>) {
        if self.mode == Mode::Empty {
            return;
        }
        self.pulse = self.pulse.wrapping_sub(6);
        if self.pulse < 6 {
            self.pulse = 6;
            self.pulse_alpha = 0;
        } else {
            self.pulse_alpha = (self.pulse_alpha + 24).min(255);
        }
        if self.mode == Mode::Tracking {
            let Some((hits, position)) = tracking else {
                return;
            };
            if hits != 0 {
                self.remaining = self.remaining.wrapping_sub(1);
            } else {
                self.mode = Mode::Finishing;
                self.remaining = 120;
                // Choose the step from the retained display sample BEFORE
                // overwriting it with the current actor-position projection.
                self.step =
                    std::array::from_fn(|i| (self.position[i] - destination[i]).abs() / 20.);
            }
            self.position = [position[0] - 32., position[1]];
            return;
        }
        for (axis, target) in destination.into_iter().enumerate() {
            let current = &mut self.position[axis];
            let step = self.step[axis];
            if (*current - target).abs() <= step {
                *current = target;
                if self.remaining != 0 {
                    self.remaining -= 1;
                }
            } else if *current < target {
                *current += step;
            } else if *current > target {
                *current -= step;
            }
        }
        if self.remaining == 0 {
            self.offset = self.offset.wrapping_add(if side == 0 { -2 } else { 2 });
            if self.alpha != 0 {
                self.alpha -= 8;
            } else {
                self.mode = Mode::Empty;
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Combos {
    rows: [[Record; 4]; 2],
    /// 495E4's retained1A20 body projection, from the preceding draw.
    body_positions: Vec<[f32; 2]>,
}

impl Combos {
    pub fn emitted(&mut self, frame: &BattleFrame) -> Result<()> {
        for cue in &frame.cues {
            if let resonance_battle::Cue::Combo {
                actor,
                hits,
                damage,
            } = *cue
            {
                let actor_index = actor.index();
                let victim = frame
                    .actors
                    .get(actor_index)
                    .context("unknown combo victim")?;
                let position = *self
                    .body_positions
                    .get(actor_index)
                    .context("combo victim has no retained body projection")?;
                self.emit(
                    usize::from(victim.side == Side::Enemy),
                    actor_index,
                    hits,
                    damage,
                    position,
                );
            }
        }
        Ok(())
    }

    fn emit(&mut self, side: usize, actor: usize, hits: i32, damage: i32, position: [f32; 2]) {
        if hits <= 1 {
            return;
        }
        let records = &mut self.rows[side];
        // 6770C..67798 checks existing records before looking for a free one.
        let mut slot = None;
        for (index, record) in records.iter().enumerate() {
            match record.mode {
                Mode::Tracking => {
                    slot = Some(index);
                    break;
                }
                Mode::Finishing => return,
                Mode::Empty => {}
            }
        }
        let Some(index) = slot.or_else(|| records.iter().position(|r| r.mode == Mode::Empty))
        else {
            return;
        };
        records[index] = Record {
            mode: Mode::Tracking,
            actor,
            hits,
            damage,
            position: [position[0] - 32., position[1]],
            remaining: 210,
            alpha: 240,
            pulse: 48,
            ..Default::default()
        };
    }

    pub fn advance(&mut self, frame: &BattleFrame, anchors: [i16; 2]) -> Result<()> {
        for (side, row) in self.rows.iter_mut().enumerate() {
            for record in row {
                let destination = [f32::from(anchors[side]), 48.];
                let tracking = if record.mode == Mode::Tracking && !frame.hud_holds.combo_tracking {
                    let sample = frame
                        .actors
                        .get(record.actor)
                        .context("unknown tracked combo victim")?
                        .hud
                        .combo_tracking;
                    let camera = frame.camera.context("combo tracking camera missing")?;
                    let point = resonance_battle::project_screen_point(camera, sample.position);
                    Some((sample.hits, [point[0], point[1]]))
                } else {
                    None
                };
                record.advance(destination, side, tracking);
            }
        }
        Ok(())
    }

    pub fn retain_body_positions(&mut self, frame: &BattleFrame) -> Result<()> {
        let camera = frame
            .camera
            .context("combo body projection camera missing")?;
        self.body_positions = frame
            .actors
            .iter()
            .map(|actor| {
                let point = resonance_battle::project_screen_point(camera, actor.body.center);
                [point[0], point[1]]
            })
            .collect();
        Ok(())
    }

    pub fn draw(&self, art: &Art, solid: &mut Batch, font: &mut Batch) -> Result<()> {
        for (side, row) in self.rows.iter().enumerate() {
            // 66BD8 shows only the first drawable physical record on each side.
            let Some(record) = row
                .iter()
                .find(|record| record.mode != Mode::Empty && record.alpha != 0 && record.hits >= 2)
            else {
                continue;
            };
            ensure!(
                record.hits > 0 && record.damage >= 0,
                "invalid combo display amount"
            );
            let anchor = i32::from(art.combo.anchors[side]);
            let offset = i32::from(record.offset);
            let digits = record.hits.ilog10() as i32 + 1;
            let amount_digits = (record.damage as u32).checked_ilog10().unwrap_or(0) as i32 + 1;
            let colors: [[u8; 4]; 4] = std::array::from_fn(|i| {
                let mut color = art.combo.panel_colors[side * 4 + i];
                color[3] = (record.alpha >> 1) as u8;
                color
            });
            panel(
                solid,
                [(anchor + offset - 44) as f32, 76., 120., 10.],
                [3.; 4],
                colors,
                5.,
            );
            if record.mode == Mode::Finishing {
                let width = amount_digits * 12;
                panel(
                    solid,
                    [
                        ((record.position[0] - 32.) + offset as f32 - width as f32).trunc(),
                        (52. + record.position[1]).trunc(),
                        (width + 108) as f32,
                        10.,
                    ],
                    [3.; 4],
                    colors,
                    5.,
                );
            }
            for pass in [1, 0] {
                let (color_index, shadow) = if pass == 0 { (0, 0) } else { (2, 2) };
                // 66BD8 uses the same foreground/shadow colors for both sides;
                // only the solid panel palette varies with the victim's side.
                let mut colors = [
                    art.combo.colors[color_index],
                    art.combo.colors[color_index + 1],
                ];
                for color in &mut colors {
                    color[3] = record.alpha as u8;
                }
                if record.mode == Mode::Finishing {
                    let amount = decimal(&art.combo.damage_format, record.damage)?;
                    results::glyphs(
                        font,
                        art,
                        &amount,
                        [
                            ((record.position[0] - 96.) + shadow as f32 + offset as f32).trunc(),
                            (40. + record.position[1] + shadow as f32).trunc(),
                        ],
                        [16., 20.],
                        10.,
                        14.,
                        colors,
                        None,
                    )?;
                    results::glyphs(
                        font,
                        art,
                        &art.combo.damage_suffix,
                        [
                            (record.position[0] + (shadow >> 1) as f32 + offset as f32).trunc(),
                            (42. + record.position[1] + (shadow >> 1) as f32).trunc(),
                        ],
                        [14., 18.],
                        8.,
                        12.,
                        colors,
                        None,
                    )?;
                }
                let text = decimal(&art.combo.count_format, record.hits)?;
                let x = anchor - (digits * 18 - 18) + shadow + offset;
                results::glyphs(
                    font,
                    art,
                    &text,
                    [(x - 6) as f32, (50 + shadow) as f32],
                    [24., 34.],
                    12.,
                    18.,
                    colors,
                    None,
                )?;
                results::glyphs(
                    font,
                    art,
                    &art.combo.hits,
                    [(anchor + shadow + offset + 20) as f32, (64 + shadow) as f32],
                    [16., 20.],
                    8.,
                    12.,
                    colors,
                    None,
                )?;
                if pass == 0 {
                    for color in &mut colors {
                        color[3] = (record.pulse_alpha >> 1) as u8;
                    }
                    let pulse = i32::from(record.pulse);
                    results::glyphs(
                        font,
                        art,
                        &text,
                        [(x - pulse) as f32, (56 - pulse) as f32],
                        [(pulse + 18) as f32, (pulse + 28) as f32],
                        12.,
                        16.,
                        colors,
                        None,
                    )?;
                }
            }
        }
        Ok(())
    }
}

fn decimal(format: &str, value: i32) -> Result<String> {
    let Some(width) = format.strip_prefix('%').and_then(|s| s.strip_suffix('d')) else {
        anyhow::bail!("unsupported source combo format");
    };
    let width: usize = if width.is_empty() { 0 } else { width.parse()? };
    ensure!(width <= 8, "oversized source combo format");
    Ok(format!("{value:width$}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_combo_allocation_scans_finalizing_records_before_free_slots() {
        let mut combos = Combos::default();
        combos.rows[1][2].mode = Mode::Finishing;
        combos.emit(1, 4, 3, 27, [300., 200.]);
        assert_eq!(combos.rows[1][0].mode, Mode::Empty);
        combos.rows[1][1].mode = Mode::Tracking;
        combos.emit(1, 4, 3, 27, [300., 200.]);
        let record = &combos.rows[1][1];
        assert_eq!((record.actor, record.hits, record.damage), (4, 3, 27));
        assert_eq!(record.position, [268., 200.]);
        assert_eq!(
            (
                record.remaining,
                record.alpha,
                record.pulse,
                record.pulse_alpha
            ),
            (210, 240, 48, 0)
        );
        assert_eq!(combos.rows[1][0].mode, Mode::Empty);
    }

    #[test]
    fn source_combo_formats_keep_the_damage_fields_leading_spaces() {
        assert_eq!(decimal("%d", 3).unwrap(), "3");
        assert_eq!(decimal("%6d", 81).unwrap(), "    81");
        assert!(decimal("fallback", 3).is_err());
    }

    #[test]
    fn selector_holds_tracking_but_not_pulse_or_the_two_axis_finalization_clock() {
        let mut record = Record {
            mode: Mode::Tracking,
            position: [280., 248.],
            remaining: 210,
            alpha: 240,
            pulse: 48,
            ..Default::default()
        };
        record.advance([80., 48.], 0, None);
        assert_eq!(
            (record.pulse, record.pulse_alpha, record.remaining),
            (42, 24, 210)
        );
        assert_eq!(record.position, [280., 248.]);
        record.advance([80., 48.], 0, Some((0, [152., 88.])));
        assert_eq!(record.mode, Mode::Finishing);
        assert_eq!(record.step, [10., 10.]);
        assert_eq!(record.position, [120., 88.]);
        assert_eq!(record.remaining, 120);
        for _ in 0..4 {
            record.advance([80., 48.], 0, None);
        }
        assert_eq!(record.position, [80., 48.]);
        assert_eq!(record.remaining, 118);
        for _ in 0..59 {
            record.advance([80., 48.], 0, None);
        }
        assert_eq!(
            (record.remaining, record.alpha, record.offset),
            (0, 232, -2)
        );
        for _ in 0..29 {
            record.advance([80., 48.], 0, None);
        }
        assert_eq!((record.mode, record.alpha), (Mode::Finishing, 0));
        record.advance([80., 48.], 0, None);
        assert_eq!(record.mode, Mode::Empty);
    }
}

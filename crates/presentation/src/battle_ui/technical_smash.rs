//! A short, independent notice; the accumulated reward belongs to gameplay.
use super::super::{Batch, BitmapFont, text};
use anyhow::Result;
use resonance_battle::{BattleFrame, Cue, TechnicalSmash};

const HOLD_TICKS: u32 = 60;
const FADE_TICKS: u32 = 43;
const FADE_IN_TICKS: u32 = 16;

#[derive(Default)]
pub(super) struct Notice(Option<(TechnicalSmash, u8, u32)>);

impl Notice {
    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) {
        if let Some((_, _, age)) = &mut self.0 {
            *age = age.saturating_add(super::hit_feedback_time(frame.clock, 1, elapsed));
            if *age >= HOLD_TICKS + FADE_TICKS {
                self.0 = None;
            }
        }
        for cue in &frame.cues {
            if let Cue::TechnicalSmash {
                bonuses,
                total_percent,
            } = *cue
            {
                self.0 = Some((bonuses, total_percent, 0));
            }
        }
    }

    pub fn draw(&self, font: &BitmapFont, fraction: f32, batch: &mut Batch) -> Result<()> {
        let Some((bonuses, total, age)) = self.0 else {
            return Ok(());
        };
        let age = age as f32 + fraction;
        let fade = (age - HOLD_TICKS as f32).max(0.);
        let alpha = ((age * 16.).min(255.) - fade * 6.).round() as u8;
        let x = -4. * (FADE_IN_TICKS as f32 - age).max(0.) - fade * (fade - 1.).max(0.) / 32.;
        let heading = [[128, 128, 32, alpha], [64, 64, 128, alpha]];
        let detail = [[128, 128, 128, alpha], [64, 64, 128, alpha]];
        for (offset, shadow) in [([3., 2.], true), ([0., 0.], false)] {
            let colors = if shadow {
                [[64, 64, 0, alpha / 2]; 2]
            } else {
                heading
            };
            text::italic(
                batch,
                font,
                "TECHNICAL SMASH!",
                [x + offset[0], 40. + offset[1]],
                [16., 22.],
                13.,
                10.,
                colors,
            )?;
            text::italic(
                batch,
                font,
                &format!("{total}%"),
                [x + 160. + offset[0], 60. + offset[1]],
                [20., 28.],
                18.,
                10.,
                colors,
            )?;
        }
        let combo = format!("{} HITS", bonuses.combo_hits);
        let mut y = 68.;
        for (earned, label) in [
            (bonuses.combo_hits != 0, combo.as_str()),
            (bonuses.no_damage, "NO DAMAGE"),
            (bonuses.aerial, "AERIAL"),
            (bonuses.summon, "SUMMON"),
            (bonuses.high_speed, "HIGH SPEED"),
            (bonuses.consecutive, "CONTINUE"),
            (bonuses.counter, "COUNTER"),
            (bonuses.skill, "SKILL"),
        ] {
            if !earned {
                continue;
            }
            for (offset, colors) in [(2., [[0, 0, 64, alpha / 2]; 2]), (0., detail)] {
                text::italic(
                    batch,
                    font,
                    label,
                    [x + 16. + offset, y + offset],
                    [14., 18.],
                    13.,
                    8.,
                    colors,
                )?;
            }
            y += 22.;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::BattleClock;

    #[test]
    fn smash_waits_in_menus_and_expires_during_a_defeat_hold() {
        let mut frame = BattleFrame::default();
        frame.cues.push(Cue::TechnicalSmash {
            bonuses: TechnicalSmash::default(),
            total_percent: 5,
        });
        let mut notice = Notice::default();
        notice.advance(&frame, 1);
        frame.cues.clear();
        frame.clock = BattleClock::Held;
        notice.advance(&frame, 0);
        assert_eq!(notice.0.as_ref().unwrap().2, 0);
        frame.clock = BattleClock::DefeatHold;
        for _ in 0..HOLD_TICKS + FADE_TICKS - 1 {
            notice.advance(&frame, 0);
        }
        assert_eq!(notice.0.as_ref().unwrap().2, HOLD_TICKS + FADE_TICKS - 1);
        notice.advance(&frame, 0);
        assert!(notice.0.is_none());
    }
}

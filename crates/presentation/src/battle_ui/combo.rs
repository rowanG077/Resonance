//! Fixed summaries of the latest combo dealt by each side.
use super::{Batch, BattleFrame, BitmapFont, Side, text};
use anyhow::{Context, Result, ensure};

/// Keep the latest contact visible for two seconds of unpaused HUD time.
const LIFETIME: u16 = 120;

struct Summary {
    hits: i32,
    damage: i32,
    remaining: u16,
}

#[derive(Default)]
pub(super) struct Combos {
    displayed: [Option<Summary>; 2],
}

impl Combos {
    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) -> Result<()> {
        if frame.recognized_result.is_some() {
            self.displayed = Default::default();
            return Ok(());
        }
        for displayed in &mut self.displayed {
            if let Some(summary) = displayed {
                summary.remaining = summary.remaining.saturating_sub(elapsed as u16);
                if summary.remaining == 0 {
                    *displayed = None;
                }
            }
        }
        for cue in &frame.cues {
            if let resonance_battle::Cue::Combo {
                actor,
                hits,
                damage,
            } = *cue
                && hits > 1
            {
                ensure!(damage >= 0, "invalid combo damage");
                let victim = frame
                    .actors
                    .get(actor.index())
                    .context("unknown combo victim")?;
                // Party damage is dealt to enemies; enemy damage is dealt to party members.
                let side = usize::from(victim.side == Side::Party);
                self.displayed[side] = Some(Summary {
                    hits,
                    damage,
                    remaining: LIFETIME,
                });
            }
        }
        Ok(())
    }

    pub fn draw(&self, font: &BitmapFont, panels: &mut Batch, text: &mut Batch) -> Result<()> {
        for (index, summary) in self.displayed.iter().enumerate() {
            let Some(summary) = summary else { continue };
            let x = 12. + index as f32 * 152.;
            let (side, color) = if index == 0 {
                ("PARTY", [96, 115, 128, 255])
            } else {
                ("ENEMY", [128, 90, 90, 255])
            };
            panels.quad([x, 154., x + 144., 194.], [0.5; 4], [0.04, 0.05, 0.08, 0.9]);
            for (label, y) in [
                (format!("{side} {} HITS", summary.hits), 158.),
                (format!("{} DAMAGE", summary.damage), 176.),
            ] {
                text::fit(text, font, &label, [x + 4., y, 136., 13.], color)?;
            }
        }
        Ok(())
    }
}

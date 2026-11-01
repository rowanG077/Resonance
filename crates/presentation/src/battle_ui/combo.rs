//! Hit counts stay in the corner; completed damage totals travel there from the victim.
use super::{Batch, BattleFrame, BitmapFont, Side, party, text};
use anyhow::{Context as ContextExt, Result, ensure};
use resonance_battle::ActorId;
use resonance_content::battle_ui::ComboArt;

const TOP: f32 = 48.;
const DAMAGE_ROW: f32 = 40.;
const TRAVEL: u32 = 20;
const HOLD: u32 = 60;
const FADE: u32 = 30;
const OPACITY: u8 = 240;
const POP: u32 = 8;

struct Summary {
    victim: ActorId,
    hits: i32,
    damage: i32,
    origin: [f32; 2],
    pop_age: u32,
    finish_age: Option<u32>,
}

#[derive(Default)]
pub(super) struct Combos {
    displayed: [Option<Summary>; 2],
}

fn victim_position(frame: &BattleFrame, victim: ActorId) -> Option<[f32; 2]> {
    let camera = frame.camera?;
    let [x, y] =
        resonance_battle::project_screen_point(camera, frame.actors[victim.index()].position);
    Some([
        x,
        y * resonance_content::HEIGHT as f32 / resonance_content::SCENE_HEIGHT as f32,
    ])
}

impl Combos {
    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) -> Result<()> {
        if frame.recognized_result.is_some() {
            self.displayed = Default::default();
            return Ok(());
        }
        if elapsed > 0 {
            for displayed in &mut self.displayed {
                let Some(summary) = displayed else { continue };
                summary.pop_age = summary.pop_age.saturating_add(elapsed);
                if let Some(age) = &mut summary.finish_age {
                    *age = age.saturating_add(elapsed);
                    if *age >= TRAVEL + HOLD + FADE {
                        *displayed = None;
                    }
                } else if frame.actors[summary.victim.index()].reaction.combo_hits == 0 {
                    summary.finish_age = Some(0);
                } else if let Some(origin) = victim_position(frame, summary.victim) {
                    summary.origin = origin;
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
                // Damage to the party occupies the left corner; damage to enemies the right.
                let side = usize::from(victim.side == Side::Enemy);
                self.displayed[side] = Some(Summary {
                    victim: actor,
                    hits,
                    damage,
                    origin: victim_position(frame, actor).unwrap_or([320., 240.]),
                    pop_age: 0,
                    finish_age: None,
                });
            }
        }
        Ok(())
    }

    pub fn draw(
        &self,
        font: &BitmapFont,
        art: &ComboArt,
        bars: &mut Batch,
        text: &mut Batch,
    ) -> Result<()> {
        for (side, summary) in self.displayed.iter().enumerate() {
            let Some(summary) = summary else { continue };
            let fade = summary
                .finish_age
                .unwrap_or(0)
                .saturating_sub(TRAVEL + HOLD - 1);
            let opacity =
                OPACITY.saturating_sub((fade.min(FADE) * u32::from(OPACITY) / FADE) as u8);
            let outward = if side == 0 { -2. } else { 2. } * fade as f32;
            let x = f32::from(art.anchors[side]) + outward;
            let hits = summary.hits.to_string();
            let count_x = x - (hits.len() - 1) as f32 * 24.;
            strip(
                bars,
                [count_x - 48., TOP + 28., x + 80., TOP + 40.],
                art.bar_colors[side],
                opacity,
            );
            italic(
                text,
                font,
                &hits,
                [count_x, TOP],
                [24., 36.],
                12.,
                art.number_colors,
                opacity,
            )?;
            italic(
                text,
                font,
                "HITS",
                [x + 24., TOP + 16.],
                [12., 24.],
                8.,
                art.number_colors,
                opacity,
            )?;

            // A new contact briefly leaves a larger, translucent count behind the settled one.
            if summary.pop_age < POP {
                let growth = (POP - summary.pop_age) as f32 * 6.;
                italic(
                    text,
                    font,
                    &hits,
                    [count_x - growth / 2., TOP - growth / 2.],
                    [24. + growth, 36. + growth],
                    12. + growth / 2.,
                    art.number_colors,
                    (summary.pop_age * 24).min(u32::from(OPACITY)) as u8,
                )?;
            }
            if let Some(age) = summary.finish_age {
                let travel = age.min(TRAVEL) as f32 / TRAVEL as f32;
                let point = [
                    summary.origin[0] + (x - summary.origin[0]) * travel,
                    summary.origin[1] + (TOP - summary.origin[1]) * travel + DAMAGE_ROW,
                ];
                let damage = summary.damage.to_string();
                let left = point[0] - 8. - damage.len() as f32 * 16.;
                strip(
                    bars,
                    [left - 16., point[1] + 12., point[0] + 80., point[1] + 24.],
                    art.bar_colors[side],
                    opacity,
                );
                italic(
                    text,
                    font,
                    &damage,
                    [left, point[1]],
                    [16., 24.],
                    12.,
                    art.number_colors,
                    opacity,
                )?;
                italic(
                    text,
                    font,
                    "DAMAGE",
                    point,
                    [12., 24.],
                    8.,
                    art.number_colors,
                    opacity,
                )?;
            }
        }
        Ok(())
    }
}

fn strip(batch: &mut Batch, rect: [f32; 4], mut colors: [[u8; 4]; 4], opacity: u8) {
    for color in &mut colors {
        color[3] = (u16::from(color[3]) * u16::from(opacity) / u16::from(OPACITY)) as u8;
    }
    party::slanted_quad(batch, rect, [0.5; 4], 6., colors);
}

#[expect(
    clippy::too_many_arguments,
    reason = "Glyph dimensions, slant, colors, and opacity are independent drawing attributes."
)]
fn italic(
    batch: &mut Batch,
    font: &BitmapFont,
    label: &str,
    [x, y]: [f32; 2],
    [width, height]: [f32; 2],
    skew: f32,
    mut colors: [[u8; 4]; 2],
    opacity: u8,
) -> Result<()> {
    for color in &mut colors {
        color[3] = opacity;
    }
    text::italic(
        batch,
        font,
        label,
        [x, y],
        [width, height],
        width,
        skew,
        colors,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_ui::battle_ui::fixtures;
    use resonance_battle::Cue;

    #[test]
    fn completed_damage_travels_holds_and_fades_without_advancing_while_paused() -> Result<()> {
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors[0].reaction.combo_hits = 3;
        frame.cues.push(Cue::Combo {
            actor: ActorId::from_index(0)?,
            hits: 3,
            damage: 25,
        });
        let mut combos = Combos::default();
        combos.advance(&frame, 1)?;
        frame.cues.clear();
        combos.advance(&frame, 200)?;
        assert!(combos.displayed[0].as_ref().unwrap().finish_age.is_none());
        assert!(combos.displayed[1].is_none());
        frame.actors[0].reaction.combo_hits = 0;
        combos.advance(&frame, 0)?;
        assert!(combos.displayed[0].as_ref().unwrap().finish_age.is_none());
        combos.advance(&frame, 1)?;
        assert_eq!(combos.displayed[0].as_ref().unwrap().finish_age, Some(0));
        combos.advance(&frame, 109)?;
        assert!(combos.displayed[0].is_some());
        combos.advance(&frame, 0)?;
        assert!(combos.displayed[0].is_some());
        combos.advance(&frame, 1)?;
        assert!(combos.displayed[0].is_none());
        Ok(())
    }
}

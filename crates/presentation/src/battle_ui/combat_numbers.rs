//! Presentation-owned damage numbers, recovery notices, and gauge trails.
use super::{Batch, BitmapFont, party, text};
use anyhow::{Context, Result};
use resonance_battle::{Affinity, BattleFrame, Cue, GuardResult, RecoveryKind, Side};

const MAX_HITS: usize = 3;
const HIT_FADE_START: u32 = 40;
const HIT_LIFETIME: u32 = 60;
const RECOVERY_FADE_START: u32 = 20;
const RECOVERY_LIFETIME: u32 = 52;

#[derive(Default)]
pub(super) struct Numbers {
    pub(super) actors: Vec<ActorNumbers>,
}

#[derive(Default)]
pub(super) struct ActorNumbers {
    pub(super) trails: [i16; 2],
    hits: Vec<FloatingNumber>,
    recovery: [Option<RecoveryNumber>; 2],
}

struct FloatingNumber {
    value: i32,
    style: NumberStyle,
    age: u32,
}

#[derive(Clone, Copy)]
enum NumberStyle {
    Normal,
    Guarded,
    Emphasized,
}

impl NumberStyle {
    fn size(self) -> [f32; 2] {
        match self {
            Self::Normal => [14., 20.],
            Self::Guarded => [12., 18.],
            Self::Emphasized => [16., 24.],
        }
    }
    fn color(self, side: Side, alpha: u8) -> [u8; 4] {
        match self {
            Self::Emphasized => [128, 112, 48, alpha],
            Self::Guarded => [88, 96, 112, alpha],
            Self::Normal if side == Side::Party => [128, 64, 64, alpha],
            Self::Normal => [128, 128, 128, alpha],
        }
    }
}

struct RecoveryNumber {
    value: i32,
    age: u32,
}

impl ActorNumbers {
    fn advance(&mut self, current: [i16; 2], elapsed: u32) {
        for (trail, value) in self.trails.iter_mut().zip(current) {
            *trail = trail.saturating_sub(elapsed.min(100) as i16).max(value);
        }
        self.hits.retain_mut(|number| {
            number.age = number.age.saturating_add(elapsed);
            number.age < HIT_LIFETIME
        });
        for number in &mut self.recovery {
            if let Some(active) = number {
                active.age = active.age.saturating_add(elapsed);
                if active.age >= RECOVERY_LIFETIME {
                    *number = None;
                }
            }
        }
    }

    fn hit(&mut self, value: i32, style: NumberStyle) {
        if self.hits.len() == MAX_HITS {
            self.hits.remove(0);
        }
        self.hits.push(FloatingNumber {
            value: value.max(0),
            style,
            age: 0,
        });
    }

    fn recover(&mut self, kind: RecoveryKind, value: i32) {
        let slot = &mut self.recovery[kind as usize];
        let value = slot
            .as_ref()
            .map_or(value, |old| old.value.saturating_add(value));
        *slot = Some(RecoveryNumber { value, age: 0 });
    }
}

impl Numbers {
    pub(super) fn advance(&mut self, frame: &BattleFrame, elapsed: u32) {
        self.actors
            .resize_with(frame.actors.len(), ActorNumbers::default);
        for (numbers, actor) in self.actors.iter_mut().zip(&frame.actors) {
            numbers.advance([actor.hp_percent(), actor.tp_percent()], elapsed);
        }
        for cue in &frame.cues {
            match *cue {
                Cue::Hit { actor, result, .. }
                    if frame.recognized_result.is_none() && result.is_damage() =>
                {
                    let style = if result.guard != GuardResult::None
                        || result.affinity == Affinity::Resistant
                    {
                        NumberStyle::Guarded
                    } else if result.critical || result.boosted || result.affinity == Affinity::Weak
                    {
                        NumberStyle::Emphasized
                    } else {
                        NumberStyle::Normal
                    };
                    self.actors[actor.index()].hit(result.amount, style);
                }
                Cue::IncidentalDamage { actor, amount } => {
                    self.actors[actor.index()].hit(amount, NumberStyle::Normal);
                }
                Cue::Recovered {
                    actor,
                    kind,
                    nominal,
                    ..
                } if frame.actors[actor.index()].side == Side::Party => {
                    self.actors[actor.index()].recover(kind, nominal);
                }
                _ => {}
            }
        }
    }
}

fn fade(age: u32, start: u32, end: u32) -> u8 {
    (255 * end.saturating_sub(age).min(end - start) / (end - start)) as u8
}

fn draw_hits(
    batch: &mut Batch,
    font: &BitmapFont,
    hits: &[FloatingNumber],
    side: Side,
    projected: [f32; 2],
) -> Result<()> {
    const LINE_HEIGHT: f32 = 28.;
    let rows: Vec<_> = hits
        .iter()
        .map(|number| {
            let text = number.value.to_string();
            let width = text.len() as f32 * number.style.size()[0];
            (number, text, width)
        })
        .collect();
    let width = rows.iter().map(|row| row.2).fold(0., f32::max);
    let [x, y] = super::floating_origin(
        [projected[0], projected[1] - 24.],
        [width + 1., LINE_HEIGHT * rows.len() as f32 + 1.],
    );
    for (row, (number, text, text_width)) in rows.iter().enumerate() {
        let size = number.style.size();
        let alpha = fade(number.age, HIT_FADE_START, HIT_LIFETIME);
        for (offset, color) in [
            (1., [0, 0, 0, alpha]),
            (0., number.style.color(side, alpha)),
        ] {
            text::glyphs(
                batch,
                font,
                text,
                [
                    x + (width - text_width) / 2. + offset,
                    y + row as f32 * LINE_HEIGHT + offset,
                ],
                size,
                color,
            )?;
        }
    }
    Ok(())
}

fn recovery(
    batch: &mut Batch,
    font: &BitmapFont,
    number: &RecoveryNumber,
    slot: usize,
    row: usize,
) -> Result<()> {
    let [x, y] = party::recovery_position(slot, row);
    let text = format!("+{}", number.value);
    let width = (party::PORTRAIT_SIZE / text.len() as f32).min(10.);
    text::glyphs(
        batch,
        font,
        &text,
        [x, y - 10. * number.age as f32 / RECOVERY_LIFETIME as f32],
        [width, 12.],
        [
            64,
            128,
            80,
            fade(number.age, RECOVERY_FADE_START, RECOVERY_LIFETIME),
        ],
    )
}

impl super::Artwork {
    pub(super) fn render_numbers(
        &mut self,
        frame: &BattleFrame,
        commands: &mut super::Commands,
        meshes: &mut super::Assets<super::Mesh>,
    ) -> Result<()> {
        let mut floating = Batch::default();
        let mut recovered = Batch::default();
        let mut party_slot = 0;
        for (actor, numbers) in frame.actors.iter().zip(&self.numbers.actors) {
            if !numbers.hits.is_empty() {
                let camera = frame
                    .camera
                    .context("floating number requires battle camera")?;
                let projected =
                    resonance_battle::project_screen_point(camera, actor.effect_origin());
                draw_hits(
                    &mut floating,
                    &self.art.font,
                    &numbers.hits,
                    actor.side,
                    projected,
                )?;
            }
            if actor.side == Side::Party {
                for (row, number) in numbers.recovery.iter().enumerate() {
                    if let Some(number) = number {
                        recovery(&mut recovered, &self.art.font, number, party_slot, row)?;
                    }
                }
                party_slot += 1;
            }
        }
        self.ui.floating.upload(floating, commands, meshes)?;
        self.ui.recovery.upload(recovered, commands, meshes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_is_bounded_holds_and_expires() {
        let mut numbers = ActorNumbers::default();
        for value in 1..=5 {
            numbers.hit(value, NumberStyle::Normal);
        }
        assert_eq!(
            numbers.hits.iter().map(|n| n.value).collect::<Vec<_>>(),
            [3, 4, 5]
        );
        numbers.recover(RecoveryKind::Hp, 12);
        numbers.recover(RecoveryKind::Hp, 7);
        numbers.advance([50, 25], 0);
        assert_eq!(numbers.hits.len(), MAX_HITS);
        assert_eq!(numbers.recovery[0].as_ref().unwrap().value, 19);
        assert_eq!(numbers.recovery[0].as_ref().unwrap().age, 0);
        numbers.advance([50, 25], HIT_LIFETIME);
        assert!(numbers.hits.is_empty());
        assert!(numbers.recovery[0].is_none());
        numbers.recover(RecoveryKind::Hp, 5);
        assert_eq!(numbers.recovery[0].as_ref().unwrap().value, 5);
        numbers = ActorNumbers::default();
        numbers.advance([100, 80], 0);
        numbers.advance([40, 30], 10);
        assert_eq!(numbers.trails, [90, 70]);
        numbers.advance([40, 30], 100);
        assert_eq!(numbers.trails, [40, 30]);
        numbers.advance([100, 80], 1);
        assert_eq!(numbers.trails, [100, 80]);
        assert_eq!((fade(0, 20, 52), fade(52, 20, 52)), (255, 0));
    }

    #[test]
    #[ignore = "requires current battle font artwork; CPU geometry only"]
    fn floating_feedback_fits_the_play_area_at_all_edges() -> Result<()> {
        use super::super::{
            fixtures,
            overlays::{ActorLabel, LabelKind},
        };
        let font = fixtures::battle_font()?;
        let mut numbers = ActorNumbers::default();
        for (value, style) in [
            (1, NumberStyle::Normal),
            (99999, NumberStyle::Guarded),
            (i32::MAX, NumberStyle::Emphasized),
        ] {
            numbers.hit(value, style);
        }
        let label = ActorLabel::new(LabelKind::EquipmentEffect, [0.; 3]);
        for point in [
            [-1000., -1000.],
            [1640., -1000.],
            [-1000., 1480.],
            [1640., 1480.],
            [320., 240.],
        ] {
            let mut text = Batch::default();
            let mut panel = Batch::default();
            draw_hits(&mut text, &font, &numbers.hits, Side::Enemy, point)?;
            label.draw(&font, point, &mut panel, &mut text)?;
            assert!(!text.indices.is_empty() && !panel.indices.is_empty());
            assert!(
                text.positions
                    .iter()
                    .chain(&panel.positions)
                    .all(|p| (-312. ..=312.).contains(&p[0]) && (-140. ..=232.).contains(&p[1])),
                "floating text exceeded the play area at {point:?}"
            );
        }
        Ok(())
    }
}

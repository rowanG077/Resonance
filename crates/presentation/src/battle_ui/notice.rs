//! Native action notices and battle outcome banners.
use super::*;

const FADE_TICKS: u64 = 20;

struct Message {
    actor: usize,
    text: String,
    age: u64,
    duration: u16,
}

impl Message {
    fn alpha(&self) -> u8 {
        let fading = self.age.saturating_sub(u64::from(self.duration));
        (255 * FADE_TICKS.saturating_sub(fading) / FADE_TICKS) as u8
    }
}

#[derive(Default)]
pub(super) struct Notices {
    messages: [Option<Message>; 2],
    pub(super) banner: Option<&'static str>,
}

impl Notices {
    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) {
        if frame.recognized_result.is_some() {
            self.messages = Default::default();
            return;
        }
        for slot in &mut self.messages {
            if let Some(message) = slot {
                message.age = message.age.saturating_add(u64::from(elapsed));
                if message.alpha() == 0 {
                    *slot = None;
                }
            }
        }
    }
}

fn draw_row(
    font: &BitmapFont,
    text: &str,
    [x, y, width, height]: [f32; 4],
    accent: [f32; 3],
    alpha: u8,
    batches: &mut [Batch; 2],
) -> Result<()> {
    let opacity = f32::from(alpha) / 255.;
    batches[0].quad(
        [x, y, x + width, y + height],
        [0.5; 4],
        [0.04, 0.05, 0.08, 0.9 * opacity],
    );
    let [r, g, b] = accent;
    batches[0].quad([x, y, x + 3., y + height], [0.5; 4], [r, g, b, opacity]);
    text::fit(
        &mut batches[1],
        font,
        text,
        [x + 8., y + 6., width - 16., height - 12.],
        [128, 128, 128, alpha],
    )
}

impl Artwork {
    #[cfg(test)]
    pub(crate) fn party_notice_text(&self) -> Option<&str> {
        self.notices.messages[0].as_ref().map(|m| m.text.as_str())
    }

    pub fn notice_request(
        &mut self,
        actor: usize,
        text: &str,
        duration: u16,
        frame: &BattleFrame,
    ) -> Result<()> {
        ensure!(duration != 0, "empty battle notice lifetime");
        let side = frame
            .actors
            .get(actor)
            .context("unknown notice actor")?
            .side;
        self.font.validate_text(text)?;
        self.notices.messages[side as usize] = Some(Message {
            actor,
            text: text.into(),
            age: 0,
            duration,
        });
        Ok(())
    }

    pub(super) fn draw_notices(
        &self,
        frame: &BattleFrame,
        party: PartyHudInput<'_>,
    ) -> Result<[Batch; 2]> {
        let mut batches = Default::default();
        if let Some(banner) = self.notices.banner {
            draw_row(
                &self.font,
                banner,
                [160., 172., 320., 44.],
                [0.95, 0.75, 0.3],
                255,
                &mut batches,
            )?;
        } else if frame.recognized_result.is_none() && self.scan.is_none() {
            for (side, message) in self.notices.messages.iter().enumerate() {
                let Some(message) = message else { continue };
                let name = if side == 0 {
                    let slot = frame.actors[..message.actor]
                        .iter()
                        .filter(|a| a.side == Side::Party)
                        .count();
                    self.party_name(party, slot)?
                } else {
                    &self
                        .combat
                        .enemies
                        .iter()
                        .find(|e| e.actor == message.actor)
                        .context("notice enemy missing")?
                        .name
                };
                let caption = format!("{name}: {}", message.text);
                let accent = if side == 0 {
                    [0.4, 0.75, 1.]
                } else {
                    [1., 0.45, 0.4]
                };
                draw_row(
                    &self.font,
                    &caption,
                    [304., 16. + side as f32 * 34., 324., 28.],
                    accent,
                    message.alpha(),
                    &mut batches,
                )?;
            }
        }
        Ok(batches)
    }

    pub(super) fn render_notices(
        &mut self,
        frame: &BattleFrame,
        party: PartyHudInput<'_>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let batches = self.draw_notices(frame, party)?;
        for (layer, batch) in self.ui.notice.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

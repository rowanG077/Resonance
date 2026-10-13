//! Action notices using the prepared frame, icon, portrait, and font artwork.
use super::*;

const FADE_IN_TICKS: u64 = 8;
const FADE_OUT_TICKS: u64 = 16;
const TEXT: usize = 1;
const FRAME: usize = 2;
const FILL: usize = 3;
const ICONS: usize = 4;
const FACES: usize = ICONS + 3;
const ENEMIES: usize = FACES + 9;

pub(super) fn face_layers(layers: &[HudLayer]) -> &[HudLayer] {
    &layers[FACES..ENEMIES]
}

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Arte,
    Spell,
    Item,
}

pub(super) fn layers(
    art: &Art,
    enemies: &[EnemyHud],
    palette: &Palette,
    materials: &mut Assets<Surface>,
    mut load: impl FnMut(&resonance_content::font::UiTexture, bool) -> Result<Option<Handle<Image>>>,
) -> Result<Vec<HudLayer>> {
    let mut definitions = vec![
        palette.solid.clone().at(Depth::Notice),
        palette
            .main_font
            .clone()
            .at(Depth::Notice)
            .foreground()
            .foreground(),
    ];
    for (index, texture) in [&art.notices.frame, &art.notices.fill]
        .into_iter()
        .chain(&art.notices.icons)
        .chain(&art.notices.faces)
        .enumerate()
    {
        let image = load(texture, false)?.context("battle notice atlas is missing")?;
        let definition =
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Notice);
        definitions.push(if index == FILL - FRAME {
            definition
        } else {
            definition.foreground()
        });
    }
    for group in 0..4 {
        let definition = if let Some(texture) = enemies
            .iter()
            .find(|enemy| usize::from(enemy.group) == group)
            .and_then(|enemy| enemy.icon.as_ref())
        {
            let image = load(texture, false)?.context("battle notice enemy icon is missing")?;
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Notice).foreground()
        } else {
            palette.solid.clone().at(Depth::Notice)
        };
        definitions.push(definition);
    }
    Ok(palette.layers(definitions, materials))
}

fn draw_face(character: u8, destination: [f32; 4], alpha: u8, batches: &mut [Batch]) -> Result<()> {
    let index = usize::from(
        character
            .checked_sub(1)
            .context("unknown notice character")?,
    );
    ensure!(index < 9, "unknown notice character");
    let x = 64. + index as f32 * 32.;
    batches[FACES + index].quad(
        destination,
        [x, 104., x + 32., 136.],
        hud_color([128, 128, 128, alpha]),
    );
    Ok(())
}

fn advance(font: &BitmapFont, character: char) -> f32 {
    if character == ' ' {
        8.
    } else {
        font.glyphs[&character].advance.saturating_sub(1) as f32
    }
}

fn draw_text(
    font: &BitmapFont,
    text: &str,
    [mut x, y]: [f32; 2],
    scale: f32,
    alpha: u8,
    batch: &mut Batch,
) {
    for character in text.chars() {
        if character != ' ' {
            let [u, v, w, h] = font.glyphs[&character].rect.map(|n| n as f32);
            let uv = [u, v, u + w, v + h];
            batch.quad(
                [x + 1., y + 1., x + 1. + w * scale, y + 29.],
                uv,
                hud_color([0, 0, 0, alpha]),
            );
            batch.quad(
                [x, y, x + w * scale, y + 28.],
                uv,
                hud_color([128, 128, 128, alpha]),
            );
        }
        x += advance(font, character) * scale;
    }
}

fn draw_frame(left: f32, top: f32, width: f32, side: usize, alpha: u8, batches: &mut [Batch]) {
    let color = hud_color([128, 128, 128, alpha]);
    let tint = if side == Side::Party as usize {
        [32, 64, 96, alpha]
    } else {
        [80, 48, 48, alpha]
    };
    let right = left + 56. + width;
    batches[FILL].quad(
        [left + 16., top + 12., left + 56., top + 44.],
        [400., 464., 440., 496.],
        hud_color(tint),
    );
    let mut x = left + 56.;
    while x < right + 20. {
        let span = (right + 20. - x).min(24.);
        batches[FILL].quad(
            [x, top + 23., x + span, top + 43.],
            [440., 468., 440. + span, 484.],
            hud_color(tint),
        );
        x += span;
    }
    batches[FILL].quad(
        [right + 4., top + 15., right + 36., top + 47.],
        [472., 456., 504., 488.],
        hud_color(tint),
    );
    batches[FRAME].quad(
        [left, top, left + 56., top + 48.],
        [456., 408., 512., 456.],
        color,
    );
    batches[FRAME].quad(
        [left + 56., top, right + 20., top + 48.],
        [511., 408., 512., 456.],
        color,
    );
    batches[FRAME].quad(
        [right, top + 12., right + 40., top + 52.],
        [416., 408., 456., 448.],
        color,
    );
}

struct Message {
    actor: usize,
    icon: Icon,
    text: String,
    age: u64,
    duration: u16,
}

impl Message {
    fn alpha(&self) -> u8 {
        let fading = self.age.saturating_sub(u64::from(self.duration));
        let entering = self.age.min(FADE_IN_TICKS) * 256 / FADE_IN_TICKS;
        let leaving = 255_u64.saturating_sub(fading.min(FADE_OUT_TICKS) * 256 / FADE_OUT_TICKS);
        entering.min(leaving) as u8
    }
}

#[derive(Default)]
pub(super) struct Notices {
    messages: [Option<Message>; 2],
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
                if message.age >= u64::from(message.duration) + FADE_OUT_TICKS {
                    *slot = None;
                }
            }
        }
    }
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
        let icon = if frame.cues.iter().any(|cue| matches!(cue,
            resonance_battle::Cue::Casting { actor: owner, phase: resonance_battle::CastPhase::Released, .. }
                if owner.index() == actor)) {
            Icon::Spell
        } else {
            Icon::Arte
        };
        self.notice_with_icon(actor, text, duration, frame, icon)
    }

    pub(super) fn notice_with_icon(
        &mut self,
        actor: usize,
        text: &str,
        duration: u16,
        frame: &BattleFrame,
        icon: Icon,
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
            icon,
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
    ) -> Result<Vec<Batch>> {
        let mut batches = (0..self.ui.notice.len())
            .map(|_| Batch::default())
            .collect::<Vec<_>>();
        if let Some(character) = self.results.as_ref().and_then(|r| r.model.cook_prompt) {
            draw_face(character, [480., 416., 512., 448.], 255, &mut batches)?;
        }
        if let Some(character) = self
            .results
            .as_ref()
            .and_then(results::Results::page_character)
        {
            draw_face(character, [104., 357., 136., 389.], 255, &mut batches)?;
        }
        if frame.recognized_result.is_none() && self.scan.is_none() {
            let paired = self.notices.messages.iter().all(Option::is_some);
            for (side, message) in self.notices.messages.iter().enumerate() {
                let Some(message) = message else { continue };
                let alpha = message.alpha();
                let width = message
                    .text
                    .chars()
                    .map(|ch| advance(&self.font, ch))
                    .sum::<f32>();
                let scale = (512. / width.max(1.)).min(1.);
                let width = width * scale;
                let fading = message.age.saturating_sub(u64::from(message.duration)) as f32;
                let left = 320. - width / 2. - 56. + fading;
                let top = 15.
                    + if paired {
                        if side == Side::Party as usize {
                            12.
                        } else {
                            -12.
                        }
                    } else {
                        0.
                    };
                draw_frame(left, top, width, side, alpha, &mut batches);
                if side == Side::Party as usize {
                    let slot = frame.actors[..message.actor]
                        .iter()
                        .filter(|a| a.side == Side::Party)
                        .count();
                    let character = *party
                        .characters
                        .get(slot)
                        .context("notice party character missing")?;
                    draw_face(
                        character,
                        [left + 20., top + 11., left + 52., top + 43.],
                        alpha,
                        &mut batches,
                    )?;
                } else if let Some(enemy) = self
                    .combat
                    .enemies
                    .iter()
                    .find(|enemy| enemy.actor == message.actor && enemy.icon.is_some())
                {
                    let image = enemy.icon.as_ref().unwrap();
                    batches[ENEMIES + usize::from(enemy.group)].quad(
                        [left + 20., top + 11., left + 52., top + 43.],
                        [0., 0., image.width as f32, image.height as f32],
                        hud_color([128, 128, 128, alpha]),
                    );
                }
                draw_text(
                    &self.font,
                    &message.text,
                    [left + 56., top + 11.],
                    scale,
                    alpha,
                    &mut batches[TEXT],
                );
                let icon = message.icon as usize;
                let [u, v] = self.art.notices.icon_origins[icon].map(f32::from);
                batches[ICONS + icon].quad(
                    [
                        left + 56. + width + 8.,
                        top + 21.,
                        left + 56. + width + 32.,
                        top + 45.,
                    ],
                    [u, v, u + 24., v + 24.],
                    hud_color([128, 128, 128, alpha]),
                );
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
        for (index, (layer, batch)) in self.ui.notice.iter_mut().zip(batches).enumerate() {
            if (FACES..ENEMIES).contains(&index) {
                let depth = if self.results.is_some() {
                    Depth::Results
                } else {
                    Depth::Notice
                }
                .value()
                    + 1.;
                if layer.definition.depth != depth {
                    layer.definition.depth = depth;
                    if let Some(rendered) = &layer.rendered {
                        commands
                            .entity(rendered.entity)
                            .insert(Transform::from_xyz(0., 0., depth));
                    }
                }
            }
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

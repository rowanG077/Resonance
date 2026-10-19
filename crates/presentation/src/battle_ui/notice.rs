//! Action notices using the prepared frame, icon, portrait, and font artwork.
use super::*;

const FADE_IN_TICKS: u64 = 8;
const FADE_OUT_TICKS: u64 = 16;
const TEXT: usize = 1;
const FRAME: usize = 2;
const FILL: usize = 3;
const ICONS: usize = 4;
const FACES: usize = ICONS + 4;
const ENEMIES: usize = FACES + 9;
const ENEMY_NOTICE: usize = ENEMIES;
const ENEMY_ICONS: usize = ENEMY_NOTICE + FACES;
const TILE_WIDTH: u32 = 24;
const GLYPH_WIDTH: u32 = 22;

pub(super) fn face_layers(layers: &[HudLayer]) -> &[HudLayer] {
    &layers[FACES..ENEMIES]
}

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Arte,
    Spell,
    Item,
    Outcome,
}

pub(super) fn layers(
    art: &Art,
    enemies: &[EnemyHud],
    palette: &Palette,
    materials: &mut Assets<Surface>,
    mut load: impl FnMut(&resonance_content::font::UiTexture, bool, bool) -> Handle<Image>,
) -> Vec<HudLayer> {
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
        let image = load(texture, false, true);
        let definition =
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Notice).nearest();
        definitions.push(if index == FILL - FRAME {
            definition
        } else {
            definition.foreground()
        });
    }
    // Each side has its own frame batches; enemy icons follow both frames.
    definitions.extend_from_within(..FACES);
    for group in 0..enemy_group_count(enemies) {
        let definition = if let Some(texture) = enemies
            .iter()
            .find(|enemy| usize::from(enemy.group) == group)
            .and_then(|enemy| enemy.icon.as_ref())
        {
            let image = load(texture, false, true);
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Notice)
                .nearest()
                .foreground()
        } else {
            palette.solid.clone().at(Depth::Notice)
        };
        definitions.push(definition);
    }
    palette.layers(definitions, materials)
}

fn draw_face(character: u8, destination: [f32; 4], alpha: u8, batches: &mut [Batch]) -> Result<()> {
    let index = usize::from(
        character
            .checked_sub(1)
            .context("unknown notice character")?,
    );
    ensure!(index < 9, "unknown notice character");
    let (x, width) = match character {
        7 => (256., 40.),
        8 => (296., 32.),
        9 => (328., 32.),
        _ => (64. + index as f32 * 32., 32.),
    };
    batches[FACES + index].quad(
        destination,
        [x, 104., x + width, 136.],
        hud_color([128, 128, 128, alpha]),
    );
    Ok(())
}

fn draw_text(font: &BitmapFont, text: &str, [mut x, y]: [f32; 2], alpha: u8, batch: &mut Batch) {
    for character in text.chars() {
        if character != ' ' {
            let [u, v, w, h] = font.glyphs[&character].rect.map(|n| n as f32);
            let uv = [u, v, u + w, v + h];
            batch.quad(
                [x, y, x + GLYPH_WIDTH as f32, y + 28.],
                uv,
                hud_color([128, 128, 128, alpha]),
            );
        }
        x += (font.glyphs[&character].advance * GLYPH_WIDTH / TILE_WIDTH) as f32;
    }
}

fn subdued(alpha: u8) -> u8 {
    alpha / 2 + alpha / 4
}

fn draw_frame(left: f32, top: f32, tiles: u32, tint: [u8; 4], alpha: u8, batches: &mut [Batch]) {
    let color = hud_color([128, 128, 128, alpha]);
    let fill = hud_color([tint[0] / 2, tint[1] / 2, tint[2] / 2, subdued(alpha)]);
    let right = left + 56. + (tiles * TILE_WIDTH) as f32;
    // Whole patterned tiles and one half tile join the curl to the diamond.
    for tile in 0..=tiles {
        let x = left + 56. + (tile * TILE_WIDTH) as f32;
        let span = if tile == tiles {
            12.
        } else {
            TILE_WIDTH as f32
        };
        let rect = [x, top + 19., x + span, top + 51.];
        batches[FILL].quad(rect, [448., 464., 448. + span, 496.], fill);
        let u = 416. + ((tile % 4) * TILE_WIDTH) as f32;
        batches[FRAME].quad(rect, [u, 376., u + span, 408.], color);
    }
    batches[FILL].quad(
        [left + 16., top + 8., left + 56., top + 56.],
        [408., 464., 448., 512.],
        fill,
    );
    batches[FILL].quad(
        [right + 8., top + 13., right + 40., top + 53.],
        [480., 456., 512., 496.],
        fill,
    );
    batches[FRAME].quad(
        [left, top, left + 56., top + 48.],
        [456., 408., 512., 456.],
        color,
    );
    batches[FRAME].quad(
        [right - 1., top + 13., right + 39., top + 53.],
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
    newest: usize,
    pub defeat_age: Option<u64>,
}

fn layer_depth(index: usize, newest: usize, results: bool) -> f32 {
    if results && (FACES..ENEMIES).contains(&index) {
        return Depth::Results.value() + 1.;
    }
    let side = usize::from(index >= ENEMIES);
    let part = if index >= ENEMY_ICONS {
        FACES
    } else if index >= ENEMY_NOTICE {
        index - ENEMY_NOTICE
    } else {
        index
    };
    let order = match part {
        TEXT => 3.,
        FRAME => 2.,
        ICONS..FACES => 4.,
        FACES..ENEMY_NOTICE => 1.,
        _ => 0.,
    };
    Depth::Notice.value() + if side == newest { 5. } else { 0. } + order
}

impl Notices {
    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) {
        if let Some(age) = &mut self.defeat_age {
            *age = age.saturating_add(u64::from(elapsed));
        }
        if frame.recognized_result.is_some() {
            self.messages = Default::default();
            return;
        }
        for slot in &mut self.messages {
            if let Some(message) = slot {
                let rescue = matches!(frame.clock, resonance_battle::BattleClock::Rescue(actor) if actor.index() == message.actor);
                message.age =
                    message
                        .age
                        .saturating_add(u64::from(if rescue { 1 } else { elapsed }));
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
            resonance_battle::Cue::Rescued { actor: owner, kind: resonance_battle::RescueKind::Revive }
                if owner.index() == actor)) {
            Icon::Item
        } else if frame.cues.iter().any(|cue| matches!(cue,
            resonance_battle::Cue::Casting { actor: owner, phase: resonance_battle::CastPhase::SceneOpening | resonance_battle::CastPhase::Released, .. }
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
        self.notices.newest = side as usize;
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
        for (character, offset) in self
            .results
            .as_ref()
            .into_iter()
            .flat_map(results::Results::page_characters)
        {
            draw_face(
                character,
                [104. + offset, 357., 136. + offset, 389.],
                255,
                &mut batches,
            )?;
        }
        if frame.recognized_result.is_none() && self.scan.is_none() {
            for (side, message) in self.notices.messages.iter().enumerate() {
                let Some(message) = message else { continue };
                let newest = side == self.notices.newest;
                let alpha = if newest {
                    message.alpha()
                } else {
                    subdued(message.alpha())
                };
                let width = message
                    .text
                    .chars()
                    .map(|ch| self.font.glyphs[&ch].advance)
                    .sum::<u32>();
                let text_width = width * GLYPH_WIDTH / TILE_WIDTH;
                let tiles = text_width / TILE_WIDTH + 1;
                let fading = message.age.saturating_sub(u64::from(message.duration)) as f32;
                let left = 320. - 48. - (((width / TILE_WIDTH + 1) * GLYPH_WIDTH + 4) / 2) as f32
                    + if side == Side::Party as usize {
                        -fading
                    } else {
                        fading
                    };
                let top = if newest { 15. } else { 3. };
                let start = if side == Side::Party as usize {
                    0
                } else {
                    ENEMY_NOTICE
                };
                let tint = if side == Side::Party as usize {
                    self.settings.colors.dialogue
                } else {
                    self.settings.colors.choice
                };
                draw_frame(
                    left,
                    top,
                    tiles,
                    tint,
                    alpha,
                    &mut batches[start..start + FACES],
                );
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
                        [
                            left + 20.,
                            top + 11.,
                            left + 20. + if character == 7 { 40. } else { 32. },
                            top + 43.,
                        ],
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
                    batches[ENEMY_ICONS + usize::from(enemy.group)].quad(
                        [left + 20., top + 11., left + 52., top + 43.],
                        [0., 0., image.width as f32, image.height as f32],
                        hud_color([128, 128, 128, alpha]),
                    );
                }
                draw_text(
                    &self.font,
                    &message.text,
                    [
                        left + 66. - ((text_width % TILE_WIDTH) / 2) as f32,
                        top + 11.,
                    ],
                    alpha,
                    &mut batches[start + TEXT],
                );
                let icon = message.icon as usize;
                let [u, v] = self.art.notices.icon_origins[icon].map(f32::from);
                let right = left + 56. + (tiles * TILE_WIDTH) as f32;
                batches[start + ICONS + icon].quad(
                    [right + 5., top + 21., right + 29., top + 45.],
                    [u, v, u + 24., v + 24.],
                    hud_color([128, 128, 128, alpha]),
                );
            }
        }
        if let Some(age) = self.notices.defeat_age {
            let text = &self.art.notices.defeated;
            let width = text
                .chars()
                .map(|ch| self.font.glyphs[&ch].advance)
                .sum::<u32>();
            let text_width = width * GLYPH_WIDTH / TILE_WIDTH;
            let tiles = text_width / TILE_WIDTH + 1;
            let left = 320. - 48. - (((width / TILE_WIDTH + 1) * GLYPH_WIDTH + 4) / 2) as f32;
            let top = 27.;
            let alpha = (age.min(FADE_IN_TICKS) * 256 / FADE_IN_TICKS).min(255) as u8;
            let color = hud_color([128, 128, 128, alpha]);
            draw_frame(
                left,
                top,
                tiles,
                self.settings.colors.popup,
                alpha,
                &mut batches,
            );
            batches[FRAME].quad(
                [left + 20., top + 11., left + 52., top + 43.],
                [384., 384., 416., 416.],
                color,
            );
            draw_text(
                &self.font,
                text,
                [
                    left + 66. - ((text_width % TILE_WIDTH) / 2) as f32,
                    top + 11.,
                ],
                alpha,
                &mut batches[TEXT],
            );
            let icon = Icon::Outcome as usize;
            let [u, v] = self.art.notices.icon_origins[icon].map(f32::from);
            let right = left + 56. + (tiles * TILE_WIDTH) as f32;
            batches[ICONS + icon].quad(
                [right + 5., top + 21., right + 29., top + 45.],
                [u, v, u + 24., v + 24.],
                color,
            );
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
            let depth = layer_depth(index, self.notices.newest, self.results.is_some());
            if layer.definition.depth != depth {
                layer.definition.depth = depth;
                if let Some(rendered) = &layer.rendered {
                    commands
                        .entity(rendered.entity)
                        .insert(Transform::from_xyz(0., 0., depth));
                }
            }
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires cooked HUD artwork; CPU composition only"]
    fn notices_keep_the_full_emblem_and_composite_in_recency_order() -> Result<()> {
        let (mut hud, _, _app) = test_artwork()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        let mut enemy = frame.actors[0].clone();
        enemy.side = Side::Enemy;
        frame.actors.push(enemy);
        let party = PartyHudInput {
            characters: &[3],
            queued_techniques: &[false],
            names: &[None],
        };
        hud.settings.colors.dialogue = [60, 140, 180, 232];
        for (name, emblem_left, text_left) in [("Fire Ball", 379., 260.), ("Aqua Edge", 416., 255.)]
        {
            hud.notice_request(0, name, 90, &frame)?;
            hud.notices.advance(&frame, 8);
            let batches = hud.draw_notices(&frame, party)?;
            let border = &batches[FRAME];
            let emblem = &border.positions[border.positions.len() - 4..];
            assert_eq!(emblem[0], [emblem_left - 320., 212., 0.]);
            assert_eq!(emblem[2], [emblem_left + 40. - 320., 172., 0.]);
            // The horizontal border stops before the emblem's center.
            assert!(
                border.positions[..border.positions.len() - 8]
                    .iter()
                    .all(|p| p[0] + 320. < emblem_left + 20.)
            );
            assert_eq!(batches[TEXT].positions[0], [text_left - 320., 214., 0.]);
            assert_eq!(batches[FILL].colors[0], hud_color([30, 70, 90, 190]));
            assert!(hud.ui.notice[FRAME].definition.nearest);
            assert!(!hud.ui.notice[TEXT].definition.nearest);
        }
        hud.notice_request(1, "Poisonous Pollen", 12, &frame)?;
        hud.notices.advance(&frame, 8);
        let paired = hud.draw_notices(&frame, party)?;
        assert_eq!(paired[FRAME].colors[0][3], 190. / 255.);
        assert_eq!(paired[ENEMY_NOTICE + FRAME].colors[0][3], 1.);
        assert!(layer_depth(ICONS, 1, false) < layer_depth(ENEMY_NOTICE + FILL, 1, false));
        assert!(layer_depth(ENEMY_NOTICE + ICONS, 0, false) < layer_depth(FILL, 0, false));
        hud.notices.advance(&frame, 20);
        let remaining = hud.draw_notices(&frame, party)?;
        assert!(remaining[ENEMY_NOTICE + FRAME].indices.is_empty());
        assert_eq!(remaining[FRAME].positions, paired[FRAME].positions);
        assert_eq!(remaining[FRAME].colors, paired[FRAME].colors);
        Ok(())
    }
}

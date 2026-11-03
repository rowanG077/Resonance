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
const OUTCOME_NOTICE: usize = ENEMY_NOTICE + FACES;
const ENEMY_ICONS: usize = OUTCOME_NOTICE + ENEMIES;
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
        palette.main_font.clone().at(Depth::Notice).offset_depth(2.),
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
            definition.offset_depth(1.)
        });
    }
    // The outcome has its own complete group so it composites after both sides.
    definitions.extend_from_within(..FACES);
    definitions.extend_from_within(..ENEMIES);
    for group in 0..enemy_group_count(enemies) {
        let definition = if let Some(texture) = enemies
            .iter()
            .find(|enemy| usize::from(enemy.group) == group)
            .and_then(|enemy| enemy.icon.as_ref())
        {
            let image = load(texture, false, true);
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Notice)
                .nearest()
                .offset_depth(1.)
        } else {
            palette.solid.clone().at(Depth::Notice)
        };
        definitions.push(definition);
    }
    palette.layers(definitions, materials)
}

fn draw_face(character: Character, destination: [f32; 4], alpha: u8, batches: &mut [Batch]) {
    let index = character.index();
    let (x, width) = match character {
        Character::Presea => (256., 40.),
        Character::Regal => (296., 32.),
        Character::Kratos => (328., 32.),
        _ => (64. + index as f32 * 32., 32.),
    };
    batches[FACES + index].quad(
        destination,
        [x, 104., x + width, 136.],
        hud_color([128, 128, 128, alpha]),
    );
}

fn draw_party_face(
    actor: ActorId,
    frame: &BattleFrame,
    party: PartyHudInput<'_>,
    [left, top]: [f32; 2],
    alpha: u8,
    batches: &mut [Batch],
) -> Result<()> {
    let slot = frame.actors[..actor.index()]
        .iter()
        .filter(|actor| actor.side == Side::Party)
        .count();
    let character = *party
        .characters
        .get(slot)
        .context("notice party character missing")?;
    let width = if character == Character::Presea {
        40.
    } else {
        32.
    };
    draw_face(
        character,
        [left, top, left + width, top + 32.],
        alpha,
        batches,
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
    let emblem_left = left + 56. + (tiles * TILE_WIDTH) as f32;
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
        [emblem_left + 8., top + 13., emblem_left + 40., top + 53.],
        [480., 456., 512., 496.],
        fill,
    );
    batches[FRAME].quad(
        [left, top, left + 56., top + 48.],
        [456., 408., 512., 456.],
        color,
    );
    batches[FRAME].quad(
        [emblem_left, top + 13., emblem_left + 40., top + 53.],
        [416., 408., 456., 448.],
        color,
    );
}

struct Message {
    actor: ActorId,
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
    pub outcome: Option<(resonance_battle::BattleResult, u64)>,
}

fn layer_depth(index: usize, newest: usize, results: bool) -> f32 {
    if results && (FACES..ENEMIES).contains(&index) {
        return Depth::Results.value() + 1.;
    }
    let (group, part) = if (OUTCOME_NOTICE..ENEMY_ICONS).contains(&index) {
        (2, index - OUTCOME_NOTICE)
    } else {
        let side = usize::from(index >= ENEMY_NOTICE);
        (
            usize::from(side == newest),
            if index >= ENEMY_ICONS {
                FACES
            } else if index >= ENEMY_NOTICE {
                index - ENEMY_NOTICE
            } else {
                index
            },
        )
    };
    let order = match part {
        TEXT => 3.,
        FRAME => 2.,
        ICONS..FACES => 4.,
        FACES..ENEMY_NOTICE => 1.,
        _ => 0.,
    };
    // Three complete slots fit below the next HUD band (Recovery at 80).
    Depth::Notice.value() + group as f32 * 3. + order * 0.5
}

impl Notices {
    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) {
        if let Some((_, age)) = &mut self.outcome {
            *age = age.saturating_add(u64::from(elapsed));
        }
        if frame
            .recognized_result
            .is_some_and(|result| result != resonance_battle::BattleResult::Escaped)
        {
            self.messages = Default::default();
            return;
        }
        for slot in &mut self.messages {
            if let Some(message) = slot {
                let rescue = matches!(frame.clock, resonance_battle::BattleClock::Rescue(actor) if actor == message.actor);
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
        actor: ActorId,
        text: &str,
        duration: u16,
        frame: &BattleFrame,
    ) -> Result<()> {
        let icon = if frame.cues.iter().any(|cue| matches!(cue,
            resonance_battle::Cue::Rescued { actor: owner, kind: resonance_battle::RescueKind::Revive }
                if *owner == actor)) {
            Icon::Item
        } else if frame.cues.iter().any(|cue| matches!(cue,
            resonance_battle::Cue::Casting { actor: owner, phase: resonance_battle::CastPhase::SceneOpening | resonance_battle::CastPhase::Released, .. }
                if *owner == actor)) {
            Icon::Spell
        } else {
            Icon::Arte
        };
        self.notice_with_icon(actor, text, duration, frame, icon)
    }

    pub(super) fn notice_with_icon(
        &mut self,
        actor: ActorId,
        text: &str,
        duration: u16,
        frame: &BattleFrame,
        icon: Icon,
    ) -> Result<()> {
        ensure!(duration != 0, "empty battle notice lifetime");
        let side = frame
            .actors
            .get(actor.index())
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

    fn draw_notice(
        &self,
        text: &str,
        icon: Icon,
        [offset, top]: [f32; 2],
        tint: [u8; 4],
        alpha: u8,
        batches: &mut [Batch],
    ) -> f32 {
        let width = text
            .chars()
            .map(|ch| self.font.glyphs[&ch].advance)
            .sum::<u32>();
        let text_width = width * GLYPH_WIDTH / TILE_WIDTH;
        let tiles = text_width / TILE_WIDTH + 1;
        let left = 320. - 48. - (((width / TILE_WIDTH + 1) * GLYPH_WIDTH + 4) / 2) as f32 + offset;
        draw_frame(left, top, tiles, tint, alpha, batches);
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
        let icon = icon as usize;
        let [u, v] = self.art.notices.icon_origins[icon].map(f32::from);
        let right = left + 56. + (tiles * TILE_WIDTH) as f32;
        batches[ICONS + icon].quad(
            [right + 5., top + 21., right + 29., top + 45.],
            [u, v, u + 24., v + 24.],
            hud_color([128, 128, 128, alpha]),
        );
        left
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
            draw_face(character, [480., 416., 512., 448.], 255, &mut batches);
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
            );
        }
        if matches!(
            frame.recognized_result,
            None | Some(resonance_battle::BattleResult::Escaped)
        ) {
            for (side, message) in self.notices.messages.iter().enumerate() {
                let Some(message) = message else { continue };
                let newest = side == self.notices.newest;
                let alpha = if newest {
                    message.alpha()
                } else {
                    subdued(message.alpha())
                };
                let fading = message.age.saturating_sub(u64::from(message.duration)) as f32;
                let offset = if side == Side::Party as usize {
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
                let left = self.draw_notice(
                    &message.text,
                    message.icon,
                    [offset, top],
                    tint,
                    alpha,
                    &mut batches[start..start + FACES],
                );
                if side == Side::Party as usize {
                    draw_party_face(
                        message.actor,
                        frame,
                        party,
                        [left + 20., top + 11.],
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
            }
        }
        if let Some((result, age)) = self.notices.outcome {
            let batches = &mut batches[OUTCOME_NOTICE..ENEMY_ICONS];
            let top = 27.;
            let alpha = (age.min(FADE_IN_TICKS) * 256 / FADE_IN_TICKS).min(255) as u8;
            let text = if result == resonance_battle::BattleResult::Escaped {
                &self.art.notices.escaped
            } else {
                &self.art.notices.defeated
            };
            let left = self.draw_notice(
                text,
                Icon::Outcome,
                [0., top],
                self.settings.colors.popup,
                alpha,
                batches,
            );
            let leader = if result == resonance_battle::BattleResult::Escaped {
                resonance_battle::preferred_party_leader(
                    frame.actors.iter().map(|actor| &actor.state),
                )
            } else {
                None
            };
            if let Some(leader) = leader {
                draw_party_face(
                    leader,
                    frame,
                    party,
                    [left + 20., top + 11.],
                    alpha,
                    batches,
                )?;
            } else {
                batches[FRAME].quad(
                    [left + 20., top + 11., left + 52., top + 43.],
                    [384., 384., 416., 416.],
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
            let depth = layer_depth(index, self.notices.newest, self.results.is_some());
            if layer.definition.depth != depth {
                layer.definition.depth = depth;
                if let Some(rendered) = &layer.rendered {
                    // Battle publication follows transform propagation.
                    let transform = Transform::from_xyz(0., 0., depth);
                    commands
                        .entity(rendered.entity)
                        .insert((transform, GlobalTransform::from(transform)));
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
    use resonance_content::diagnostics::Diagnostics;

    #[test]
    #[ignore = "requires current cooked mystic captions and HUD; CPU notice playback only"]
    fn mystic_captions_survive_typed_reader_and_notice_playback() -> Result<()> {
        let document: resonance_content::arte::document::Document = serde_json::from_slice(
            &std::fs::read(fixtures::asset_root().join(resonance_content::arte::PATH))?,
        )?;
        let names = document.prepare()?.names(&Diagnostics::new(true))?;
        let (mut hud, _, _app) = test_artwork()?;
        let frame = fixtures::frame(fixtures::actor(Default::default()));
        for (id, character) in [
            (144, Character::Zelos),
            (248, Character::Genis),
            (249, Character::Zelos),
            (260, Character::Presea),
            (261, Character::Presea),
        ] {
            let party = PartyHudInput {
                characters: &[character],
                queued_techniques: &[false],
                names: &[None],
            };
            let name = names.get(&id).context("missing prepared mystic caption")?;
            hud.notice_request(ActorId::from_index(0)?, name, 90, &frame)?;
            hud.notices.advance(&frame, 8);
            ensure!(
                hud.party_notice_text() == Some(name),
                "lost mystic caption during playback"
            );
            ensure!(
                !hud.draw_notices(&frame, party)?[TEXT].positions.is_empty(),
                "mystic caption has no glyph geometry"
            );
        }
        Ok(())
    }

    #[test]
    fn outcome_layers_composite_after_both_action_notice_groups() {
        for newest in 0..2 {
            let depth = |index| layer_depth(index, newest, false);
            let highest_action = (0..OUTCOME_NOTICE)
                .chain(ENEMY_ICONS..ENEMY_ICONS + 3)
                .map(depth)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((OUTCOME_NOTICE..ENEMY_ICONS).all(|index| {
                highest_action < depth(index) && depth(index) < Depth::Recovery.value()
            }));
            for base in [0, ENEMY_NOTICE, OUTCOME_NOTICE] {
                let parts = [FILL, FRAME, TEXT, ICONS].map(|part| depth(base + part));
                assert!(parts.windows(2).all(|pair| pair[0] < pair[1]));
            }
            assert!(depth(OUTCOME_NOTICE + FILL) < depth(OUTCOME_NOTICE + FACES));
            assert!(depth(OUTCOME_NOTICE + FACES) < depth(OUTCOME_NOTICE + FRAME));
            assert_eq!(
                layer_depth(FACES, newest, true),
                Depth::Results.value() + 1.
            );
        }
    }

    #[test]
    #[ignore = "requires cooked HUD artwork; CPU composition only"]
    fn escape_outcome_keeps_its_label_and_leader_face_until_the_scene_closes() -> Result<()> {
        use resonance_battle::{BattleResult, Control};
        use resonance_game::battle::lifecycle::Request;

        let (mut hud, _, _app) = test_artwork()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors[0].control = Control::Auto;
        let mut leader = frame.actors[0].clone();
        leader.control = Control::Manual;
        frame.actors.push(leader);
        let mut enemy = frame.actors[0].clone();
        enemy.side = Side::Enemy;
        frame.actors.push(enemy);
        let party = PartyHudInput {
            characters: &[Character::Colette, Character::Lloyd],
            queued_techniques: &[false, false],
            names: &[None, None],
        };
        frame.recognized_result = Some(BattleResult::Escaped);
        assert_eq!(hud.art.notices.escaped, "Escaped");
        for (actor, text) in [(0, "Fire Ball"), (2, "Crush Tortoise")] {
            hud.notice_request(ActorId::from_index(actor)?, text, 90, &frame)?;
        }
        // An escaped result alone does not request a banner.
        assert!(
            hud.draw_notices(&frame, party)?[OUTCOME_NOTICE + TEXT]
                .indices
                .is_empty()
        );
        hud.overlay_request(Request::EscapeNotice)?;
        hud.notices.advance(&frame, 8);
        let shown = hud.draw_notices(&frame, party)?;
        assert_eq!(shown[TEXT].positions.len(), 8 * 4);
        assert_eq!(shown[ENEMY_NOTICE + TEXT].positions.len(), 13 * 4);
        assert_eq!(
            shown[OUTCOME_NOTICE + TEXT].positions.len(),
            "Escaped".len() * 4
        );
        for face in [
            FACES + Character::Colette.index(),
            OUTCOME_NOTICE + FACES + Character::Lloyd.index(),
        ] {
            assert_eq!(shown[face].positions.len(), 4);
        }
        hud.notices.advance(&frame, 200);
        let expired = hud.draw_notices(&frame, party)?;
        assert!(
            expired[..OUTCOME_NOTICE]
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        assert!(
            expired[OUTCOME_NOTICE..ENEMY_ICONS] == shown[OUTCOME_NOTICE..ENEMY_ICONS],
            "the outcome persists after action notices expire"
        );
        hud.overlay_request(Request::DefeatNotice)?;
        hud.notices.advance(&frame, 8);
        let defeated = hud.draw_notices(&frame, party)?;
        assert!(
            defeated[OUTCOME_NOTICE + FACES + Character::Lloyd.index()]
                .positions
                .is_empty()
        );
        assert_eq!(
            defeated[OUTCOME_NOTICE + TEXT].positions.len(),
            "Defeated".len() * 4
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked HUD artwork; checks actual atlas coverage without a GPU"]
    fn notice_emblem_has_no_fill_holes_or_overdraw() -> Result<()> {
        use resonance_asset_writer::texture::{Pixels, ktx2};

        fn covered(batch: &Batch, image: &Pixels, point: Vec2) -> usize {
            batch
                .positions
                .chunks_exact(4)
                .zip(batch.uv.chunks_exact(4))
                .filter(|(positions, uv)| {
                    let a = Vec2::new(positions[0][0], positions[0][1]);
                    let b = Vec2::new(positions[2][0], positions[2][1]);
                    let fraction = (point - a) / (b - a);
                    if !fraction.cmpge(Vec2::ZERO).all() || !fraction.cmplt(Vec2::ONE).all() {
                        return false;
                    }
                    let uv = Vec2::from_array(uv[0])
                        + (Vec2::from_array(uv[2]) - Vec2::from_array(uv[0])) * fraction;
                    let u = (uv.x.floor() as u32).min(image.width - 1);
                    let v = (uv.y.floor() as u32).min(image.height - 1);
                    image.rgba[((v * image.width + u) * 4 + 3) as usize] != 0
                })
                .count()
        }

        let art = fixtures::battle_art()?;
        let frame = ktx2(&std::fs::read(
            fixtures::asset_root().join(&art.notices.frame.path),
        )?)?
        .remove(0);
        let fill = ktx2(&std::fs::read(
            fixtures::asset_root().join(&art.notices.fill.path),
        )?)?
        .remove(0);
        let mut batches = (0..FACES).map(|_| Batch::default()).collect::<Vec<_>>();
        draw_frame(180., 15., 1, [128; 4], 255, &mut batches);
        let border = &batches[FRAME];
        let first = border.positions.len() - 4;
        let origin = Vec2::new(border.positions[first][0], border.positions[first][1]);
        let [u, v] = border.uv[first].map(|value| value as u32);
        for y in 0..40 {
            let alpha = |x: u32| frame.rgba[(((v + y) * frame.width + u + x) * 4 + 3) as usize];
            let Some(left) = (0..40).find(|&x| alpha(x) != 0) else {
                continue;
            };
            let right = (0..40).rfind(|&x| alpha(x) != 0).unwrap();
            for x in left..=right {
                let point = origin + Vec2::new(x as f32 + 0.5, -(y as f32 + 0.5));
                let fills = covered(&batches[FILL], &fill, point);
                ensure!(fills <= 1, "overlapping emblem fill at {x},{y}");
                ensure!(
                    covered(border, &frame, point) != 0 || fills == 1,
                    "empty emblem interior at {x},{y}"
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked HUD artwork; CPU composition only"]
    fn notices_keep_the_full_emblem_and_composite_in_recency_order() -> Result<()> {
        let (mut hud, _, _app) = test_artwork()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        let mut enemy = frame.actors[0].clone();
        enemy.side = Side::Enemy;
        frame.actors.push(enemy);
        let party = PartyHudInput {
            characters: &[Character::Genis],
            queued_techniques: &[false],
            names: &[None],
        };
        hud.settings.colors.dialogue = [60, 140, 180, 232];
        for (name, emblem_left, text_left) in [("Fire Ball", 380., 260.), ("Aqua Edge", 417., 255.)]
        {
            hud.notice_request(ActorId::from_index(0)?, name, 90, &frame)?;
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
        hud.notice_request(ActorId::from_index(1)?, "Poisonous Pollen", 12, &frame)?;
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

        let library = resonance_content::library::Library::open(&fixtures::asset_root(), &[])?;
        let rules = resonance_content::battle_rules::capture(&mut library.snapshot(), &|| false)?;
        hud.configure_unison_names(&rules.unison)?;
        let actor = resonance_battle::ActorId::from_index(0)?;
        frame.unison = Some(resonance_battle::UnisonFrame {
            generation: 1,
            phase: resonance_battle::UnisonPhase::CombinationPose,
            initiator: actor,
            target: None,
            elapsed: 1,
            blocked_slots: 0,
            selected: vec![],
            selected_actions: vec![],
            hidden: vec![],
            stage_color: None,
            completion: None,
            combination: Some(resonance_battle::UnisonCombinationFrame {
                id: resonance_battle::UnisonRecipeId(12),
                native_id: resonance_battle::UnisonArteId(311),
                actors: [actor; 2],
            }),
            feedback: None,
        });
        let names = BTreeMap::new();
        hud.advance_unison(&frame, &names, false)?;
        assert_eq!(hud.party_notice_text(), Some("-Compound Special Attack-"));
        frame.unison.as_mut().unwrap().phase = resonance_battle::UnisonPhase::Combined;
        hud.advance_unison(&frame, &names, false)?;
        assert_eq!(hud.party_notice_text(), Some("Prism Stars"));
        hud.notices.advance(&frame, 12);
        hud.advance_unison(&frame, &names, false)?;
        assert_eq!(
            hud.notices.messages[0].as_ref().unwrap().age,
            12,
            "remaining in the combined phase must not restart its banner"
        );
        Ok(())
    }
}

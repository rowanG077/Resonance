//! Physical prompts and accepted arte rows accompany the shared shortcut editor.
use super::*;
use resonance_battle::{ActionKey, ActorId, UnisonFrame, UnisonPhase, UnisonRecipeId};
use resonance_game::menu::unison::Page;

const BUTTONS: [[f32; 2]; 4] = [[528., 320.], [488., 340.], [568., 312.], [512., 288.]];
const FACE_OFFSETS: [[f32; 2]; 4] = [[16., 16.], [-16., 16.], [16., -16.], [-16., -16.]];
const TEXT: usize = 2;
const BUTTON_ART: usize = 3;
const FACES: usize = BUTTON_ART + 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKey {
    Arte(ActorId, ActionKey),
    Combined(UnisonRecipeId, [ActorId; 2]),
}

struct Row {
    key: RowKey,
    name: String,
    actors: Vec<ActorId>,
    age: u16,
}

#[derive(Default)]
pub(super) struct State {
    generation: Option<u64>,
    phase: Option<UnisonPhase>,
    rows: Vec<Row>,
    combination_names: BTreeMap<UnisonRecipeId, String>,
}

fn rows_visible(frame: &UnisonFrame) -> bool {
    !matches!(frame.phase, UnisonPhase::Windup | UnisonPhase::Opening)
}

pub(super) fn layers(
    art: &Art,
    palette: &Palette,
    notices: &[HudLayer],
    materials: &mut Assets<Surface>,
    mut load: impl FnMut(&resonance_content::font::UiTexture, bool, bool) -> Handle<Image>,
) -> Vec<HudLayer> {
    let mut definitions = vec![
        palette.solid.clone().at(Depth::Unison),
        palette.solid.clone().at(Depth::Unison).offset_depth(1.),
        palette.main_font.clone().at(Depth::Unison).offset_depth(3.),
    ];
    for texture in &art.unison_buttons {
        let image = load(texture, false, true);
        definitions.push(
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Unison)
                .offset_depth(2.),
        );
    }
    definitions.extend(
        notice::face_layers(notices)
            .iter()
            .map(|layer| layer.definition.clone().at(Depth::Unison).offset_depth(3.)),
    );
    palette.layers(definitions, materials)
}

impl Artwork {
    pub fn configure_unison_names(
        &mut self,
        catalogue: &resonance_content::battle_unison::Catalogue,
    ) -> Result<()> {
        for combination in &catalogue.combinations {
            self.font.validate_text(&combination.name)?;
        }
        self.unison.combination_names = catalogue
            .combinations
            .iter()
            .map(|row| (UnisonRecipeId(row.id), row.name.clone()))
            .collect();
        Ok(())
    }

    /// Keep row animation on completed updates, independent of the controller's phase clock.
    pub fn advance_unison(
        &mut self,
        frame: &BattleFrame,
        action_names: &BTreeMap<ActionKey, String>,
        paused: bool,
    ) -> Result<()> {
        let battle = frame;
        let Some(frame) = frame.unison.as_ref() else {
            self.unison.generation = None;
            self.unison.phase = None;
            self.unison.rows.clear();
            return Ok(());
        };
        if self.unison.generation != Some(frame.generation) || !rows_visible(frame) {
            self.unison.rows.clear();
            self.unison.generation = Some(frame.generation);
            self.unison.phase = None;
        }
        if self.unison.phase != Some(frame.phase) {
            self.unison.phase = Some(frame.phase);
            if let Some(combination) = &frame.combination {
                let notice = match frame.phase {
                    UnisonPhase::CombinationPose => Some(("-Compound Special Attack-".into(), 60)),
                    UnisonPhase::Combined => Some((
                        self.unison
                            .combination_names
                            .get(&combination.id)
                            .context("Unison combination name is not prepared")?
                            .clone(),
                        180,
                    )),
                    _ => None,
                };
                if let Some((name, duration)) = notice {
                    self.notice_with_icon(
                        combination.actors[0].index(),
                        &name,
                        duration,
                        battle,
                        notice::Icon::Arte,
                    )?;
                }
            }
        }
        let expected = frame
            .selected_actions
            .iter()
            .map(|&(actor, action)| RowKey::Arte(actor, action))
            .chain(
                frame
                    .combination
                    .as_ref()
                    .map(|row| RowKey::Combined(row.id, row.actors)),
            )
            .collect::<Vec<_>>();
        ensure!(
            self.unison.rows.len() <= expected.len()
                && self
                    .unison
                    .rows
                    .iter()
                    .zip(&expected)
                    .all(|(row, key)| &row.key == key),
            "Unison changed an accepted selection"
        );
        for row in &mut self.unison.rows {
            row.age = row.age.saturating_add(u16::from(!paused));
        }
        for key in expected.into_iter().skip(self.unison.rows.len()) {
            let (name, actors) = match key {
                RowKey::Arte(actor, action) => (
                    action_names
                        .get(&action)
                        .context("Unison arte name is not prepared")?,
                    vec![actor],
                ),
                RowKey::Combined(id, actors) => (
                    self.unison
                        .combination_names
                        .get(&id)
                        .context("Unison combination name is not prepared")?,
                    actors.to_vec(),
                ),
            };
            self.font.validate_text(name)?;
            self.unison.rows.push(Row {
                key,
                name: name.clone(),
                actors,
                age: 0,
            });
        }
        Ok(())
    }

    pub(super) fn render_unison_attack(
        &mut self,
        frame: &BattleFrame,
        party: PartyHudInput<'_>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let mut batches = (0..self.ui.unison.len())
            .map(|_| Batch::default())
            .collect::<Vec<_>>();
        if let Some(unison) = frame.unison.as_ref().filter(|frame| rows_visible(frame)) {
            if unison.phase == UnisonPhase::Selecting {
                batches[0].quad(
                    self.resolution.ui_rect(),
                    [0.5; 4],
                    [0., 0., 0., 128. / 255.],
                );
            }
            if matches!(
                unison.phase,
                UnisonPhase::Selecting | UnisonPhase::Executing
            ) {
                for (slot, [x, y]) in BUTTONS.into_iter().enumerate() {
                    let blocked = unison.blocked_slots & (1 << slot) != 0;
                    let tick = frame.update as u32;
                    let intensity = if blocked {
                        48.
                    } else {
                        176. + (48. * (((tick % 360 * 8) % 360) as f32).to_radians().sin()).trunc()
                    } / 128.;
                    let tint = [intensity, intensity, intensity, 1.];
                    let frame = ((tick >> 4) & 1) ^ 1;
                    let [u, v] = [384. + slot as f32 * 32., 64. + frame as f32 * 32.];
                    batches[BUTTON_ART + slot / 2].quad(
                        [x, y, x + 32., y + 32.],
                        [u, v, u + 32., v + 32.],
                        tint,
                    );
                    if let Some(&character) = party.characters.get(slot) {
                        let [dx, dy] = FACE_OFFSETS[slot];
                        face(&mut batches, character, [x + dx, y + dy], tint);
                    }
                }
            }
            for (index, row) in self.unison.rows.iter().enumerate() {
                let width = row
                    .name
                    .chars()
                    .map(|ch| advance(&self.font, ch))
                    .sum::<f32>();
                let x = (684. - f32::from(row.age) * 24.).max(624. - width);
                let y = 104. + index as f32 * 32.;
                let icon_width = row.actors.len() as f32 * 32.;
                party::backdrop(
                    &mut batches[1],
                    [
                        x - icon_width,
                        y + 16.,
                        x + width + icon_width + 32.,
                        y + 28.,
                    ],
                    self.art.combo.bar_colors[1],
                    3.,
                    4.,
                );
                label(&mut batches[TEXT], &self.font, &row.name, [x, y])?;
                for (index, actor) in row.actors.iter().enumerate() {
                    let character = *party
                        .characters
                        .get(actor.index())
                        .context("Unison row actor is outside party")?;
                    let left = x - (index + 1) as f32 * 32.;
                    face(
                        &mut batches,
                        character,
                        [left - 2., y - 2.],
                        [0., 0., 0., 128. / 255.],
                    );
                    face(&mut batches, character, [left - 4., y - 4.], [1.; 4]);
                }
            }
        }
        for (layer, batch) in self.ui.unison.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }

    pub fn render_unison(
        &mut self,
        page: Page<'_>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.windows.render_battle_unison(
            page,
            &self.font,
            &self.dialogue,
            &self.settings,
            tick,
            commands,
            meshes,
        )
    }
}

fn face(batches: &mut [Batch], character: Character, [x, y]: [f32; 2], tint: [f32; 4]) {
    let index = character.index();
    let u = 64. + index as f32 * 32.;
    batches[FACES + index].quad([x, y, x + 32., y + 32.], [u, 104., u + 32., 136.], tint);
}

fn advance(font: &BitmapFont, character: char) -> f32 {
    if character == ' ' {
        8.
    } else {
        (font.glyphs[&character].advance as f32 * 22. / 24.).trunc()
    }
}

fn label(batch: &mut Batch, font: &BitmapFont, name: &str, [x, y]: [f32; 2]) -> Result<()> {
    for (offset, color) in [(2., [0., 0., 0., 128. / 255.]), (0., [1.; 4])] {
        let mut pen = x;
        for character in name.chars() {
            if character != ' ' {
                let glyph = font
                    .glyphs
                    .get(&character)
                    .context("Unison label glyph is missing")?;
                let [u, v, w, h] = glyph.rect.map(|v| v as f32);
                batch.quad(
                    [
                        pen + offset,
                        y + offset,
                        pen + offset + 22.,
                        y + offset + 28.,
                    ],
                    [u, v, u + w, v + h],
                    color,
                );
            }
            pen += advance(font, character);
        }
    }
    Ok(())
}

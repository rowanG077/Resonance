//! Ordinary party HUD, composed on the existing bitmap UI surfaces (6CDEC).
use super::*;
use anyhow::ensure;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use resonance_battle::{Activity, Actor, BattleFrame, Side};
use resonance_content::battle_ui::{Art, Gauge};

#[path = "battle_ui/combat.rs"]
mod combat;
#[path = "battle_ui/combat_numbers.rs"]
mod combat_numbers;
#[path = "battle_ui/command_strip.rs"]
mod command_strip;
#[path = "battle_ui/notice.rs"]
mod notice;
#[path = "battle_ui/overlays.rs"]
mod overlays;
#[path = "battle_ui/results.rs"]
mod results;
pub(crate) use overlays::FeedbackFrame;
const SOLID: usize = 9;
const FONT: usize = 10;
const RESULT_SOLID: usize = 11;
const RESULT_FONT: usize = 12;
const RESULT_BLOCK: usize = 13;
const CARD_FONT: usize = 14;
const DOL_FONT: usize = 15;
const NEXT: usize = 16;
const ICONS: usize = 17;
const TRANSITION: usize = 26;
const NOTICE_SHADOW: usize = 27;
const NOTICE_OWNER: usize = 28;
const NOTICE_BACKGROUND: usize = 37;
const NOTICE_TEXT: usize = 38;
const NOTICE_GENERIC: usize = 39;
const NOTICE_SYMBOL: usize = 40;
const LABELS: usize = 41;
const COMBAT_NUMBERS: usize = LABELS + overlays::LABEL_CAPACITY * 2;
const RECOVERY_NUMBERS: usize = COMBAT_NUMBERS + 1;

/// Prepared enemy-group artwork; loaded before the battle's GPU warmup.
#[derive(Clone)]
pub(crate) struct EnemyHud {
    pub actor: usize,
    pub group: u8,
    pub name: String,
    pub icon: resonance_content::font::UiTexture,
}

#[derive(Resource)]
pub(crate) struct Artwork {
    art: Art,
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    layers: Vec<Layer>,
    sizes: Vec<[u32; 2]>,
    font: BitmapFont,
    dialogue: DialogueArt,
    menu_data: resonance_content::menu_data::MenuData,
    windows: menu::MenuArtwork,
    settings: resonance_content::menu_data::CustomizeSettings,
    results: Option<results::Results>,
    result_values: Option<resonance_game::battle::results::Results>,
    notice: Option<overlays::Notice>,
    labels: Vec<(usize, overlays::ActorLabel)>,
    feedback: overlays::Feedback,
    combat: combat::Hud,
    command_strip: command_strip::Strip,
}

impl Artwork {
    pub fn load(
        read: impl Fn(&str) -> Result<Vec<u8>>,
        enemies: &[EnemyHud],
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
    ) -> Result<Self> {
        let art: Art = serde_json::from_slice(&read(resonance_content::battle_ui::PATH)?)?;
        art.validate()?;
        let load = |path: String, repeat| {
            server
                .load_builder()
                .with_settings(move |settings: &mut ImageLoaderSettings| {
                    settings.is_srgb = false;
                    let address = if repeat {
                        ImageAddressMode::Repeat
                    } else {
                        ImageAddressMode::ClampToEdge
                    };
                    settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: address,
                        address_mode_v: address,
                        ..ImageSamplerDescriptor::linear()
                    });
                })
                .load(path)
        };
        let mut images: Vec<_> = art
            .portraits
            .iter()
            .map(|image| load(image.path.clone(), true))
            .collect();
        let mut white = Image::new(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![255; 4],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        white.sampler = ImageSampler::linear();
        let mut nearest = white.clone();
        nearest.sampler = ImageSampler::nearest();
        let nearest = image_assets.add(nearest);
        images.push(image_assets.add(white));
        images.push(load(art.font.texture.clone(), false));
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        dialogue.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&dialogue.font)?)?;
        font.validate()?;
        let menu_data = serde_json::from_slice(&read("game/menu-data.json")?)?;
        let white = images[SOLID].clone();
        let atlas = images[FONT].clone();
        images.extend([white.clone(), atlas.clone(), white.clone(), atlas]);
        images.push(load(font.texture.clone(), false));
        images.push(load(art.results.next_button[0].texture.path.clone(), false));
        images.extend(
            art.results
                .character_icons
                .iter()
                .map(|sprite| load(sprite.texture.path.clone(), false)),
        );
        images.push(white.clone());
        images.push(load(art.overlays.notice_shadow.path.clone(), false));
        images.extend(
            art.results
                .character_icons
                .iter()
                .map(|sprite| load(sprite.texture.path.clone(), false)),
        );
        images.push(load(art.overlays.notice_background.path.clone(), false));
        images.push(images[DOL_FONT].clone());
        images.push(load(art.overlays.notice_background.path.clone(), false));
        images.push(load(
            art.overlays.notice_symbols[3].texture.path.clone(),
            false,
        ));
        for _ in 0..overlays::LABEL_CAPACITY {
            images.extend([white.clone(), images[FONT].clone()]);
        }
        images.extend([images[FONT].clone(), images[FONT].clone()]);
        let mut sizes: Vec<_> = (0..images.len())
            .map(|index| match index {
                0..9 => [64, 256],
                SOLID | RESULT_SOLID | RESULT_BLOCK | TRANSITION => [1, 1],
                DOL_FONT | NOTICE_TEXT => [font.width, font.height],
                index
                    if (LABELS..COMBAT_NUMBERS).contains(&index)
                        && (index - LABELS).is_multiple_of(2) =>
                {
                    [1, 1]
                }
                _ => [512, 512],
            })
            .collect();
        let command_strip = command_strip::Strip::load(&art, &font, &mut images, &mut sizes, load);
        let combat = combat::Hud::load(&art, &font, enemies, &mut images, &mut sizes, load)?;
        let surfaces: Vec<_> = images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                materials.add(Surface {
                    source: image.clone(),
                    // 68C90 selects nearest for notice atlases; its font pass
                    // retains the separate DOL font sampler.
                    sampling: if matches!(
                        index,
                        NOTICE_SHADOW..=NOTICE_BACKGROUND | NOTICE_GENERIC | NOTICE_SYMBOL
                    ) || combat.nearest(index)
                    {
                        nearest.clone()
                    } else {
                        image.clone()
                    },
                    frame_mask: image.clone(),
                    color_mask: image.clone(),
                    coverage: Coverage::default(),
                    layered: matches!(index, RESULT_FONT | CARD_FONT),
                    screen_break: false,
                    additive: combat.additive(index),
                    opaque: false,
                })
            })
            .collect();
        let windows = menu::MenuArtwork::load(
            &read,
            server,
            materials,
            &surfaces[DOL_FONT],
            &surfaces[DOL_FONT],
        )?;
        // Sampler-only image belongs to readiness/warmup, not a drawable layer.
        images.push(nearest);
        images.extend(windows.images().iter().cloned());
        Ok(Self {
            art,
            images,
            surfaces,
            layers: Vec::new(),
            sizes,
            font,
            dialogue,
            menu_data,
            windows,
            settings: Default::default(),
            results: None,
            result_values: None,
            notice: None,
            labels: Vec::new(),
            feedback: Default::default(),
            combat,
            command_strip,
        })
    }

    /// Call before activation; the scene loader owns GPU readiness and warmup.
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
        }
        for (index, material) in self.surfaces.iter().enumerate() {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            if matches!(index, RESULT_FONT | CARD_FONT) {
                batch.secondary_uv = vec![[-1.; 2]; 4];
            }
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(
                        0.,
                        0.,
                        if let Some(depth) = self.command_strip.depth(index, 0) {
                            depth
                        } else if let Some(depth) = self.combat.depth(index) {
                            depth
                        } else if index == TRANSITION {
                            900.
                        } else if index == COMBAT_NUMBERS {
                            99.
                        } else if index == RECOVERY_NUMBERS {
                            150.
                        } else if index >= LABELS {
                            // All actor labels precede floating numbers at99.
                            90. + (index - LABELS) as f32 / 3.
                        } else if index >= NOTICE_SHADOW {
                            140. + (index - NOTICE_SHADOW) as f32
                        } else if index > FONT {
                            300. + index as f32
                        } else {
                            100. + index as f32
                        },
                    ),
                    Visibility::Hidden,
                ))
                .id();
            self.layers.push(Layer {
                entity,
                mesh,
                material: material.clone(),
                uploaded: None,
                visible: false,
            });
        }
        self.windows.prepare_windows(commands, meshes);
    }

    pub fn images(&self) -> &[Handle<Image>] {
        &self.images
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.layers
            .iter()
            .chain(&self.windows.layers)
            .map(|layer| layer.entity)
    }

    /// Character IDs are one-based, in the same order as the frame's party actors.
    /// Model visibility intentionally does not hide this separate HUD pass.
    pub fn render(
        &mut self,
        frame: &BattleFrame,
        party_characters: &[u8],
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        ensure!(
            self.layers.len() == self.surfaces.len(),
            "battle HUD was not prepared"
        );
        let actors: Vec<_> = frame
            .actors
            .iter()
            .filter(|actor| actor.side == Side::Party)
            .collect();
        ensure!(
            actors.len() == party_characters.len() && actors.len() <= 4,
            "battle HUD party differs from frame"
        );
        let mut batches: [Batch; FONT + 1] = std::array::from_fn(|_| Batch::default());
        for (slot, (actor, &character)) in actors.into_iter().zip(party_characters).enumerate() {
            ensure!((1..=9).contains(&character), "invalid battle HUD character");
            // 4C0D0 needs the scrolling condition texture pass, which is separate
            // from the ordinary opening HUD. Do not substitute plain text.
            ensure!(
                !actor.petrified,
                "condition-layered battle HUD is not prepared"
            );
            let portrait = usize::from(character - 1);
            let panel = &self.art.party;
            let x = f32::from(panel.origin[0]) + slot as f32 * f32::from(panel.spacing);
            let y = f32::from(panel.origin[1]);
            let expression = expression(actor);
            let bounce = actor.hud.portrait_bounce;
            let left = x + f32::from(panel.portrait_offset[0]);
            let top = y + f32::from(panel.portrait_offset[1]) - f32::from(bounce * (bounce & 1));
            let [width, height] = panel.portrait_size.map(f32::from);
            let inset = f32::from(panel.portrait_inset);
            let source_top = expression as f32 * 64.;
            let shade = portrait_shade(actor, frame.hud_update, &self.art.sine);
            quad(
                &mut batches[portrait],
                [left, top, left + width, top + height],
                [
                    inset,
                    source_top + inset,
                    64. - inset,
                    source_top + 64. - inset,
                ],
                0.,
                [[shade, shade, shade, 255]; 4],
            );
            for (gauge, current, trail) in [
                (&panel.hp, actor.hp_percent(), actor.hud.hp_trail),
                (&panel.tp, actor.tp_percent(), actor.hud.tp_trail),
            ] {
                bar(
                    &mut batches[SOLID],
                    gauge,
                    [x, y],
                    current,
                    trail,
                    panel.bar_shadow_offset,
                    panel.shadow,
                    panel.lost_value_color,
                );
            }
            for (gauge, value, colors) in [
                (&panel.hp, actor.hud.hp, hp_colors(actor)),
                (&panel.tp, actor.hud.tp, panel.tp.number_colors),
            ] {
                let position = [
                    x + f32::from(gauge.number_offset[0]),
                    y + f32::from(gauge.number_offset[1]),
                ];
                let text = format!("{value:4}");
                let shadow = std::array::from_fn(|axis| {
                    position[axis] + f32::from(panel.number_shadow_offset[axis])
                });
                number(
                    &mut batches[FONT],
                    &self.art,
                    &text,
                    shadow,
                    [panel.shadow; 2],
                )?;
                number(&mut batches[FONT], &self.art, &text, position, colors)?;
            }
        }
        for (index, (layer, batch)) in self.layers.iter_mut().zip(batches).enumerate() {
            let visible = !batch.indices.is_empty();
            let size = match index {
                SOLID => [1, 1],
                FONT => [self.art.font.width, self.art.font.height],
                _ => [
                    self.art.portraits[index].width,
                    self.art.portraits[index].height,
                ],
            };
            layer.update_mesh(batch, size, meshes)?;
            layer.show(visible, commands);
        }
        self.render_results(frame.hud_update, commands, meshes)?;
        self.render_overlays(frame, commands, meshes)?;
        self.render_numbers(frame, commands, meshes)?;
        self.render_combat(frame, commands, meshes)?;
        Ok(())
    }

    pub fn settings(&mut self, settings: resonance_content::menu_data::CustomizeSettings) {
        self.settings = settings;
    }

    pub fn render_transition(
        &mut self,
        frame: Option<&resonance_battle::TransitionFrame>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let layer = self
            .layers
            .get_mut(TRANSITION)
            .context("battle transition was not prepared")?;
        let mut batch = Batch::default();
        if let Some(frame) = frame.filter(|frame| frame.alpha != 0) {
            ensure!(
                frame.wipe_rows.is_empty() || frame.wipe_rows.len() == 480,
                "invalid battle wipe rows"
            );
            // 11940 draws opaque black scanlines, then the colored full-screen fade.
            for (row, &offset) in frame.wipe_rows.iter().enumerate() {
                let right = i32::from(frame.wipe_progress) - i32::from(offset);
                if right > 0 {
                    quad(
                        &mut batch,
                        [0., row as f32, right as f32, row as f32 + 1.],
                        [0.5; 4],
                        0.,
                        [[0, 0, 0, 255]; 4],
                    );
                }
            }
            quad(
                &mut batch,
                [0., 0., 640., 480.],
                [0.5; 4],
                0.,
                [[frame.color[0], frame.color[1], frame.color[2], frame.alpha]; 4],
            );
        }
        let visible = !batch.indices.is_empty();
        layer.update_mesh(batch, [1, 1], meshes)?;
        layer.show(visible, commands);
        Ok(())
    }

    pub fn despawn(self, world: &mut World) {
        for layer in self.layers.into_iter().chain(self.windows.layers) {
            world.despawn(layer.entity);
        }
    }
}

fn expression(actor: &Actor) -> u8 {
    if actor.hud.result_performance {
        3
    } else if actor.activity == Activity::Defeated {
        2
    } else if matches!(actor.activity, Activity::Action { .. }) || actor.hud.cast_released {
        1
    } else {
        0
    }
}

fn portrait_shade(actor: &Actor, update: u32, sine: &[f32]) -> u8 {
    if matches!(actor.activity, Activity::Casting { .. }) && !actor.hud.cast_released {
        // 6CDEC uses table[frame*32 %360], then truncates 32*sin(angle)+160.
        let angle = (update.wrapping_mul(32) % 360) as usize;
        (32f32.mul_add(sine[angle], 160.) as i32) as u8
    } else if actor.hp <= 0 || !actor.available() {
        96
    } else {
        128
    }
}

fn hp_colors(actor: &Actor) -> [[u8; 4]; 2] {
    if actor.hp <= 0 || actor.petrified {
        [[128, 80, 80, 255], [128, 48, 48, 255]]
    } else if actor.hp <= actor.max_hp >> 2 {
        [[128, 96, 96, 255], [128, 80, 80, 255]]
    } else {
        [[128, 128, 128, 255], [128, 104, 104, 255]]
    }
}

#[allow(clippy::too_many_arguments)] // Direct gauge values and original shadow/loss styling.
fn bar(
    batch: &mut Batch,
    gauge: &Gauge,
    [x, y]: [f32; 2],
    current: i16,
    trail: i16,
    shadow_offset: [i16; 2],
    shadow: [u8; 4],
    loss: [u8; 4],
) {
    let [x, y] = [
        x + f32::from(gauge.bar_offset[0]),
        y + f32::from(gauge.bar_offset[1]),
    ];
    let [width, height] = gauge.bar_size.map(i32::from);
    let current = i32::from(current) * width / 100;
    let remaining = i32::from(trail) * width / 100 - current;
    let rect = |x: f32, y: f32, width: i32| [x, y, x + width as f32, y + height as f32];
    let skew = f32::from(gauge.skew);
    quad(
        batch,
        rect(
            x + f32::from(shadow_offset[0]),
            y + f32::from(shadow_offset[1]),
            width,
        ),
        [0.5; 4],
        skew,
        [shadow; 4],
    );
    quad(batch, rect(x, y, current), [0.5; 4], skew, gauge.colors);
    if remaining > 0 {
        quad(
            batch,
            rect(x + current as f32, y, remaining),
            [0.5; 4],
            skew,
            [loss; 4],
        );
    }
}

fn number(
    batch: &mut Batch,
    art: &Art,
    text: &str,
    [mut x, y]: [f32; 2],
    colors: [[u8; 4]; 2],
) -> Result<()> {
    let spec = &art.party.number;
    for character in text.chars() {
        if character != ' ' {
            let glyph = art
                .font
                .glyphs
                .get(&character)
                .context("battle number glyph missing")?;
            let [u, v, w, h] = glyph.rect.map(|v| v as f32);
            quad(
                batch,
                [
                    x,
                    y,
                    x + f32::from(spec.glyph_size[0]),
                    y + f32::from(spec.glyph_size[1]),
                ],
                [u, v, u + w, v + h],
                f32::from(spec.skew),
                [colors[0], colors[0], colors[1], colors[1]],
            );
        }
        x += f32::from(spec.advance);
    }
    Ok(())
}

fn quad(batch: &mut Batch, rect: [f32; 4], uv: [f32; 4], skew: f32, colors: [[u8; 4]; 4]) {
    let start = batch.positions.len();
    batch.quad(rect, uv, [1.; 4]);
    for vertex in &mut batch.positions[start..start + 2] {
        vertex[0] += skew;
    }
    // GX's strip is TL,TR,BL,BR; Batch stores TL,TR,BR,BL.
    for (index, color) in [colors[0], colors[1], colors[3], colors[2]]
        .into_iter()
        .enumerate()
    {
        batch.colors[start + index] = std::array::from_fn(|channel| {
            f32::from(color[channel]) / 255. * if channel == 3 { 1. } else { 2. }
        });
    }
    let start = start as u32;
    let offset = batch.indices.len() - 6;
    batch.indices[offset..].copy_from_slice(&[
        start,
        start + 3,
        start + 1,
        start + 1,
        start + 3,
        start + 2,
    ]);
}

impl Artwork {
    /// Called at the shared-world boundary, before result callback requests.
    /// UI effects follow source156FC while combat may remain held by selection.
    pub fn advance(&mut self, frame: &BattleFrame) -> Result<()> {
        self.combat.advance(frame, &self.art)?;
        if let Some(notice) = &mut self.notice {
            notice.advance(frame.hud_update, frame.hud_holds.notices);
        }
        for (_, label) in &mut self.labels {
            label.advance(frame.hud_update);
        }
        // Contacts occur after the shared HUD visit; keep their initial pose
        // visible on the emission frame, independently of effect timelines.
        for cue in &frame.cues {
            if let resonance_battle::Cue::Hit { actor, result, .. } = cue {
                let owner = actor.index();
                let actor = frame
                    .actors
                    .get(owner)
                    .context("unknown actor label victim")?;
                overlays::contact(&mut self.labels, owner, actor, *result, frame.hud_update)?;
            }
        }
        self.feedback.advance(frame.hud_update);
        Ok(())
    }
    pub fn feedback(&self) -> FeedbackFrame {
        self.feedback.frame()
    }

    pub fn overlay_request(
        &mut self,
        kind: resonance_game::battle::lifecycle::RequestKind,
        _selection: Option<&resonance_game::battle::results::Selection>,
        results: Option<&resonance_game::battle::results::Results>,
        battle: &resonance_battle::Battle,
        party_characters: &[u8],
        generation: u64,
    ) -> Result<()> {
        use resonance_game::battle::lifecycle::RequestKind::*;
        let frame = battle.snapshot();
        let party: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .filter(|(_, actor)| actor.side == Side::Party)
            .collect();
        ensure!(
            party.len() == party_characters.len(),
            "result overlay roster differs from party"
        );
        match kind {
            VictoryBanner => self.feedback.start(generation, frame.hud_update),
            DefeatNotice | EscapeNotice => {
                let (notice_kind, owner) = if matches!(kind, DefeatNotice) {
                    (overlays::NoticeKind::Defeat, None)
                } else {
                    // B564 chooses the first non-auto actor, falling back to slot0.
                    let slot = party
                        .iter()
                        .position(|(_, actor)| actor.control != resonance_battle::Control::Auto)
                        .unwrap_or(0);
                    (
                        overlays::NoticeKind::Escape,
                        Some(
                            *party_characters
                                .get(slot)
                                .context("escape notice has no actor")?,
                        ),
                    )
                };
                self.notice = Some(overlays::Notice::new(
                    notice_kind,
                    owner,
                    &self.art,
                    &self.font,
                    frame.hud_update,
                )?);
            }
            LevelNotices | ExNotices => {
                let results = results.context("actor result labels have no reward state")?;
                let kind = if matches!(kind, LevelNotices) {
                    overlays::LabelKind::Level
                } else {
                    overlays::LabelKind::CompoundEx
                };
                for ((index, actor), &character) in party.into_iter().zip(party_characters) {
                    let visible = if matches!(kind, overlays::LabelKind::Level) {
                        results
                            .advancement
                            .iter()
                            .any(|entry| entry.character == character && entry.levels != 0)
                    } else {
                        results.new_ex_skills.iter().any(|&(id, _)| id == character)
                    };
                    if visible {
                        overlays::request(&mut self.labels, index, kind, actor, frame.hud_update)?;
                    }
                }
            }
            _ => anyhow::bail!("request is not a battle overlay"),
        }
        Ok(())
    }
    fn render_overlays(
        &mut self,
        frame: &BattleFrame,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let mut batches: Vec<_> = (NOTICE_SHADOW..COMBAT_NUMBERS)
            .map(|_| Batch::default())
            .collect();
        if let Some(notice) = &self.notice {
            let draw = notice.draw(&self.art, &self.font, self.settings.colors.popup)?;
            batches[0] = draw.shadow;
            if let Some((overlays::NoticeOwner::Party(character), batch)) = draw.owner {
                batches[NOTICE_OWNER - NOTICE_SHADOW + usize::from(character - 1)] = batch;
            }
            for (index, batch) in [
                (NOTICE_BACKGROUND, draw.background),
                (NOTICE_TEXT, draw.text),
                (NOTICE_GENERIC, draw.generic_icon),
            ] {
                batches[index - NOTICE_SHADOW] = batch;
            }
            if let Some((_, batch)) = draw.symbol {
                batches[NOTICE_SYMBOL - NOTICE_SHADOW] = batch;
            }
        }
        for (slot, (_, label)) in self.labels.iter_mut().enumerate() {
            let camera = frame.camera.as_ref().context("actor label has no camera")?;
            let projected = resonance_battle::project_screen_point(*camera, label.position)
                .map(|value| value as i16);
            let start = LABELS - NOTICE_SHADOW + slot * 2;
            let (left, right) = batches.split_at_mut(start + 1);
            label.draw(&self.art, projected, &mut left[start], &mut right[0])?;
        }
        for (offset, batch) in batches.into_iter().enumerate() {
            let index = offset + NOTICE_SHADOW;
            let visible = !batch.indices.is_empty();
            self.layers[index].update_mesh(batch, self.sizes[index], meshes)?;
            self.layers[index].show(visible, commands);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_strip_preserves_skew_and_corner_order() {
        let mut batch = Batch::default();
        quad(
            &mut batch,
            [60., 422., 116., 430.],
            [0.5; 4],
            8.,
            [
                [10, 0, 0, 255],
                [20, 0, 0, 255],
                [30, 0, 0, 255],
                [40, 0, 0, 255],
            ],
        );
        assert_eq!(
            batch.positions,
            [
                [-252., -182., 0.],
                [-196., -182., 0.],
                [-204., -190., 0.],
                [-260., -190., 0.]
            ]
        );
        assert_eq!(batch.indices, [0, 3, 1, 1, 3, 2]);
        assert_eq!(
            batch
                .colors
                .iter()
                .map(|c| (c[0] * 255.).round() as u16)
                .collect::<Vec<_>>(),
            [20, 40, 80, 60]
        );
    }

    #[test]
    fn bar_loss_uses_two_integer_percent_divisions() {
        let gauge = Gauge {
            number_offset: [0; 2],
            bar_offset: [48, 34],
            bar_size: [56, 8],
            skew: 8,
            colors: [[128; 4]; 4],
            number_colors: [[128; 4]; 2],
        };
        let mut batch = Batch::default();
        bar(
            &mut batch,
            &gauge,
            [12., 388.],
            33,
            66,
            [1, 4],
            [0, 0, 0, 128],
            [128; 4],
        );
        // Current 18 pixels, retained 36; trail begins at60+18, ends at60+36.
        assert_eq!(batch.positions[7][0], -260.);
        assert_eq!(batch.positions[6][0], -242.);
        assert_eq!(batch.positions[11][0], -242.);
        assert_eq!(batch.positions[10][0], -224.);
    }
}

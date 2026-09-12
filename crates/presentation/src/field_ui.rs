//! Bitmap dialogue composition from cooked images and high-level text state.
#[path = "field_ui_caption.rs"]
mod caption;
#[path = "field_ui_coverage.rs"]
mod coverage;
#[path = "field_ui_menu.rs"]
mod menu;
pub(super) fn model_preview_depth() -> f32 {
    menu::model_preview_depth()
}
#[path = "field_ui_prompt.rs"]
mod prompt;
#[path = "field_ui_skit.rs"]
mod skit;
use anyhow::{Context, Result};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d},
};
use coverage::Coverage;
use resonance_content::font::{BitmapFont, DialogueArt};
use resonance_events::dialogue::{DIALOGUE_SLOTS, Dialogue, DialogueAnchor, TextToken, flags};
use resonance_game::{dialogue::DialoguePlayer, field::FieldSession};
use std::{collections::BTreeMap, fs, path::Path};

// Explicit compositing order keeps text underneath the cursor and its shadow.
mod layer {
    pub const FILL: usize = 0;
    pub const BEVEL: usize = 1;
    pub const FRAME: usize = 2;
    pub const POINTER_FILL: usize = 3;
    pub const POINTER: usize = 4;
    pub const CORNERS: usize = 5;
    pub const SPEAKER_FILL: usize = 6;
    pub const SPEAKER: usize = 7;
    pub const FONT: usize = 8;
    pub const CURSOR: usize = 9;
    // Cooked atlas indices: fill, frame, color overlay, font and cursor.
    pub const TEXTURES: [usize; 10] = [7, 7, 0, 1, 0, 0, 1, 0, 9, 10];
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct Surface {
    #[texture(0)]
    source: Handle<Image>,
    /// Reuse another prepared image's sampler without duplicating its pixels.
    #[texture(7)]
    #[sampler(1)]
    sampling: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    frame_mask: Handle<Image>,
    #[texture(4)]
    #[sampler(5)]
    color_mask: Handle<Image>,
    #[uniform(6)]
    coverage: Coverage,
}
impl Material2d for Surface {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/field_ui.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        _: bevy::sprite_render::Material2dKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.label = Some("resonance/field-ui".into());
        Ok(())
    }
}

#[derive(Resource)]
pub(super) struct Artwork {
    pub(super) resolution: super::Resolution,
    pub font: BitmapFont,
    spec: DialogueArt,
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    layers: BTreeMap<(u8, usize), Layer>,
    head_heights: BTreeMap<u64, f32>,
    choice_trail: super::choice_cursor::Trail,
    subtitles: resonance_content::font::MovieSubtitles,
    subtitle_layer: Option<Layer>,
    captions: BTreeMap<u32, caption::Caption>,
    menu: menu::MenuArtwork,
    prompt_layers: Vec<Layer>,
    skits: skit::Artwork,
}
struct Layer {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<Surface>,
    uploaded: Option<(Batch, [u32; 2])>,
    visible: bool,
}

/// The same slot renderer can be used before a field session exists.
#[derive(Resource)]
pub(super) struct MenuOverlay {
    font: BitmapFont,
    dialogue: DialogueArt,
    images: Vec<Handle<Image>>,
    artwork: menu::MenuArtwork,
}
impl MenuOverlay {
    pub fn load(
        root: &Path,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
    ) -> Result<Self> {
        let read = |path: &str| -> Result<Vec<u8>> { Ok(fs::read(root.join(path))?) };
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        dialogue.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&dialogue.font)?)?;
        font.validate()?;
        let images: Vec<_> = [&font.texture, &dialogue.cursor.path]
            .into_iter()
            .map(|path| {
                server
                    .load_builder()
                    .with_settings(|s: &mut ImageLoaderSettings| {
                        s.is_srgb = false;
                        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                            address_mode_u: ImageAddressMode::Repeat,
                            address_mode_v: ImageAddressMode::Repeat,
                            ..ImageSamplerDescriptor::linear()
                        });
                    })
                    .load(path.clone())
            })
            .collect();
        let surfaces: Vec<_> = images
            .iter()
            .map(|source| {
                materials.add(Surface {
                    source: source.clone(),
                    sampling: source.clone(),
                    frame_mask: source.clone(),
                    color_mask: source.clone(),
                    coverage: Coverage::default(),
                })
            })
            .collect();
        let artwork = menu::MenuArtwork::load(read, server, materials, &surfaces[0], &surfaces[1])?;
        Ok(Self {
            font,
            dialogue,
            images,
            artwork,
        })
    }
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        self.artwork.prepare(commands, meshes);
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id())) && self.artwork.ready(images)
    }
    pub fn render(
        &mut self,
        menu: Option<&resonance_game::menu::Menu>,
        resolution: super::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.artwork.render(
            menu::Source::Title(menu),
            &self.font,
            &self.dialogue,
            0,
            resolution,
            commands,
            meshes,
        )
    }
    pub fn despawn(self, world: &mut World) {
        for layer in self.artwork.layers {
            world.despawn(layer.entity);
        }
    }
}
impl Layer {
    fn update_mesh(
        &mut self,
        batch: Batch,
        size: [u32; 2],
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        if self
            .uploaded
            .as_ref()
            .is_none_or(|(old, old_size)| *old != batch || *old_size != size)
        {
            *meshes
                .get_mut(&self.mesh)
                .context("retained UI mesh was removed")? = batch.clone().mesh(size);
            self.uploaded = Some((batch, size));
        }
        Ok(())
    }
    fn show(&mut self, visible: bool, commands: &mut Commands) {
        if self.visible != visible {
            self.visible = visible;
            commands.entity(self.entity).insert(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}
impl Artwork {
    pub fn despawn(&mut self, world: &mut World) {
        for layer in std::mem::take(&mut self.layers)
            .into_values()
            .chain(self.captions.values_mut().flat_map(|c| c.layers.drain(..)))
            .chain(self.menu.layers.drain(..))
            .chain(self.prompt_layers.drain(..))
            .chain(self.skits.layers.drain(..))
            .chain(self.skits.warm.drain(..))
        {
            world.despawn(layer.entity);
        }
        if let Some(layer) = self.subtitle_layer.take() {
            world.despawn(layer.entity);
        }
        self.head_heights.clear();
        self.choice_trail = Default::default();
    }
    pub fn load(
        root: &Path,
        field: &resonance_content::field::FieldAssets,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
    ) -> Result<Self> {
        Self::load_with(root, field, server, materials, None)
    }
    pub fn load_with(
        root: &Path,
        field: &resonance_content::field::FieldAssets,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        files: Option<&resonance_content::prepared::Files>,
    ) -> Result<Self> {
        let read = |path: &str| -> Result<Vec<u8>> {
            files.map_or_else(
                || Ok(fs::read(root.join(path))?),
                |files| Ok(files.read(path)?.to_vec()),
            )
        };
        let spec: DialogueArt = serde_json::from_slice(
            &read("ui/dialogue.json")
                .context("classroom dialogue art is missing; run cook-classroom")?,
        )?;
        spec.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&spec.font)?)?;
        font.validate()?;
        let subtitles: resonance_content::font::MovieSubtitles = serde_json::from_slice(
            &read("ui/story-subtitles.json")
                .context("movie subtitles are missing; run cook-classroom")?,
        )?;
        subtitles.validate()?;
        let images: Vec<Handle<Image>> = spec
            .textures
            .iter()
            .map(|t| t.path.clone())
            .chain([font.texture.clone(), spec.cursor.path.clone()])
            .enumerate()
            .map(|(index, path)| {
                server
                    .load_builder()
                    .with_settings(move |s: &mut ImageLoaderSettings| {
                        s.is_srgb = false;
                        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                            address_mode_u: ImageAddressMode::Repeat,
                            address_mode_v: ImageAddressMode::Repeat,
                            ..if index < 9 {
                                // Keep frame slices and patterned backgrounds pixel-sharp.
                                ImageSamplerDescriptor::nearest()
                            } else {
                                ImageSamplerDescriptor::linear()
                            }
                        });
                    })
                    .load(path)
            })
            .collect();
        let surfaces: Vec<_> = images
            .iter()
            .map(|source| {
                materials.add(Surface {
                    source: source.clone(),
                    sampling: source.clone(),
                    frame_mask: images[0].clone(),
                    color_mask: images[1].clone(),
                    coverage: Coverage::default(),
                })
            })
            .collect();
        let menu = menu::MenuArtwork::load(read, server, materials, &surfaces[9], &surfaces[10])?;
        let skits = skit::Artwork::load(read, server, materials, &surfaces[9])?;
        Ok(Self {
            skits,
            resolution: Default::default(),
            font,
            spec,
            images,
            surfaces,
            layers: BTreeMap::new(),
            head_heights: BTreeMap::new(),
            choice_trail: Default::default(),
            subtitles,
            subtitle_layer: None,
            captions: caption::Caption::load(field, read, server, materials)?,
            menu,
            prompt_layers: Vec::new(),
        })
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id()))
            && self.captions.values().all(|caption| caption.ready(images))
            && self.menu.ready(images)
            && self.skits.ready(images)
    }
    /// Allocate every supported dialogue slot/layer before its first request.
    pub(super) fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) {
        self.menu.prepare(commands, meshes);
        self.skits.prepare(commands, meshes);
        self.prepare_prompt(commands, meshes, materials);
        for caption in self.captions.values_mut() {
            caption.prepare(commands, meshes);
        }
        for slot in 0..DIALOGUE_SLOTS {
            for (index, texture) in layer::TEXTURES.into_iter().enumerate() {
                if self.layers.contains_key(&(slot, index)) {
                    continue;
                }
                let mut batch = Batch::default();
                batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
                let mesh = meshes.add(batch.mesh([1, 1]));
                let template = materials.get(&self.surfaces[texture]).unwrap().clone();
                let material = materials.add(template);
                let order = index as f32;
                let entity = commands
                    .spawn((
                        Mesh2d(mesh.clone()),
                        MeshMaterial2d(material.clone()),
                        Transform::from_xyz(0., 0., 10. + f32::from(slot) * 20. + order),
                        Visibility::Hidden,
                    ))
                    .id();
                self.layers.insert(
                    (slot, index),
                    Layer {
                        entity,
                        mesh,
                        material,
                        uploaded: None,
                        visible: false,
                    },
                );
            }
        }
        if self.subtitle_layer.is_none() {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let material = self.surfaces[layer::TEXTURES[layer::FONT]].clone();
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., 1.),
                    Visibility::Hidden,
                    bevy::camera::visibility::RenderLayers::layer(3),
                ))
                .id();
            self.subtitle_layer = Some(Layer {
                entity,
                mesh,
                material,
                uploaded: None,
                visible: false,
            });
        }
    }
    pub(super) fn prepared_layers(
        &self,
    ) -> impl Iterator<Item = (&Handle<Mesh>, &Handle<Surface>)> {
        self.layers
            .values()
            .chain(self.subtitle_layer.iter())
            .chain(self.captions.values().flat_map(|c| &c.layers))
            .chain(&self.menu.layers)
            .chain(&self.prompt_layers)
            .chain(&self.skits.layers)
            .chain(&self.skits.warm)
            .map(|layer| (&layer.mesh, &layer.material))
    }
    pub fn diagnostic_layouts(&self, session: &FieldSession) -> Vec<serde_json::Value> {
        session
            .events
            .world
            .dialogue
            .iter()
            .map(|(slot, request)| {
                let height = self.head_heights.get(&request.operation.id()).copied();
                let player = session
                    .dialogue
                    .get(slot)
                    .filter(|p| p.operation.id() == request.operation.id());
                let rect = player.and_then(|p| {
                    layout(
                        &self.font,
                        request,
                        p,
                        session,
                        height.unwrap_or(170.),
                        self.resolution,
                    )
                    .ok()
                });
                serde_json::json!({
                    "slot":slot, "operation":request.operation.id(),
                    "opening_actor":request.opening_actor, "attachment_height":height,
                    "body_rect":rect.map(|r|r.0), "pointer":rect.and_then(|r|r.1),
                    "opening_fraction":player.and_then(DialoguePlayer::opening_fraction),
                    "window_visible":player.is_some_and(DialoguePlayer::window_visible)
                })
            })
            .collect()
    }
    pub fn render_captions(
        &mut self,
        world: &resonance_events::GameWorld,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<Vec<i32>> {
        let mut handled = Vec::new();
        for (&resource, caption) in &mut self.captions {
            handled.extend(caption.render(resource, world, commands, meshes)?);
        }
        Ok(handled)
    }
    pub fn render(
        &mut self,
        session: &FieldSession,
        presentation_tick: u32,
        heads: &BTreeMap<i32, Vec3>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        self.render_prompt(session, commands, meshes)?;
        self.skits.render(
            session.active_skit.as_ref(),
            &self.font,
            self.resolution,
            commands,
            meshes,
        )?;
        self.menu.render(
            menu::Source::Field(session),
            &self.font,
            &self.spec,
            presentation_tick,
            self.resolution,
            commands,
            meshes,
        )?;
        let mut used = std::collections::BTreeSet::new();
        let (world, dialogue) = session.dialogue_scene();
        self.head_heights.retain(|operation, _| {
            world
                .dialogue
                .values()
                .any(|d| d.operation.id() == *operation)
        });
        for request in world.dialogue.values() {
            if let Some(id) = request.speaker_actor
                && let Some(actor) = world.actors.get(&id)
                && let Some(head) = heads.get(&id)
            {
                // Capture head height once, then follow the actor’s ground position.
                // Following animated head XY makes dialogue drift during turns.
                self.head_heights
                    .entry(request.operation.id())
                    .or_insert((head.z - actor.position[2]).trunc() + 30.);
            }
        }
        for (&slot, player) in dialogue {
            if !player.window_visible() || player.operation.progress().outcome.is_some() {
                continue;
            }
            let Some(request) = world
                .dialogue
                .get(&slot)
                .filter(|r| r.operation.id() == player.operation.id())
            else {
                continue;
            };
            let (rect, pointer) = layout(
                &self.font,
                request,
                player,
                session,
                self.head_heights
                    .get(&request.operation.id())
                    .copied()
                    .unwrap_or(170.),
                self.resolution,
            )?;
            let opening = player.opening_fraction();
            let rect = opening.map_or(rect, |fraction| {
                expanding_rect(
                    rect,
                    pointer.unwrap_or([
                        rect[0] + ((rect[2] - rect[0]) / 2.).trunc(),
                        rect[1] + ((rect[3] - rect[1]) / 2.).trunc(),
                    ]),
                    fraction,
                )
            });
            let mut batches: [Batch; layer::TEXTURES.len()] =
                std::array::from_fn(|_| Batch::default());
            let mut highlight = Batch::default();
            let speaker: String = request
                .speaker
                .tokens
                .iter()
                .filter_map(|token| match token {
                    TextToken::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let preferences = world.party.as_ref().map(|p| &p.settings.preferences);
            let window = preferences.map_or(self.spec.selection.mode, |p| p.window);
            let (cursor_image, cursor_size) = self.menu.choice_cursor(window).unwrap_or((
                &self.images[layer::TEXTURES[layer::CURSOR]],
                [self.spec.cursor.width, self.spec.cursor.height],
            ));
            if request.flags & flags::FRAMELESS == 0 {
                frame(
                    &mut batches,
                    rect,
                    &speaker,
                    &self.font,
                    pointer.filter(|_| opening.is_none()),
                    request.flags,
                    preferences,
                );
            }
            // Mask fills with the frame artwork: even translucent ornaments own
            // their pixels and must not receive a second layer of background color.
            let speaker_coverage =
                Coverage::frame(&batches[layer::SPEAKER], &self.spec, opening.is_some())?;
            let speaker_fill_coverage = speaker_coverage.with_color(
                &batches[layer::SPEAKER_FILL],
                &self.spec,
                opening.is_some(),
            )?;
            let corner_coverage = speaker_fill_coverage.with_frame(
                &batches[layer::CORNERS],
                &self.spec,
                opening.is_some(),
            )?;
            let pointer_coverage = corner_coverage.with_frame(
                &batches[layer::POINTER],
                &self.spec,
                opening.is_some(),
            )?;
            let ornament_coverage = pointer_coverage.with_color(
                &batches[layer::POINTER_FILL],
                &self.spec,
                opening.is_some(),
            )?;
            let frame_coverage = ornament_coverage.with_frame(
                &batches[layer::FRAME],
                &self.spec,
                opening.is_some(),
            )?;
            let fill_coverage =
                frame_coverage.with_solid(&batches[layer::BEVEL], &self.spec, opening.is_some())?;
            let [left, top, _, _] = rect;
            if let Some(choice) = world.choices.get(&slot)
                && player.fully_revealed()
                && (player.accepts_input() || !choice.operation.is_pending())
            {
                let y =
                    super::choice_cursor::drawing_y(top + f32::from(choice.selected_line) * 25.);
                let overlay = super::choice_cursor::overlay_rect;
                let mut style = self.spec.selection.clone();
                style.mode = window;
                [style.bob_amplitude, style.bob_step] = self.menu.cursor_motion(window);
                style.color = preferences.map_or(style.color, |p| p.colors.selection);
                let mut color = style.color.map(|v| f32::from(v) / 255.);
                color[3] = f32::from(u16::from(style.color[3]) * 128 / 255) / 255.;
                if style.mode == 0 {
                    highlight.quad(overlay([left, y + 24., rect[2], y + 26.]), [0.5; 4], color);
                } else {
                    // Draw nine one-pixel highlight rows over the text.
                    for (row, offset) in style.row_offsets.into_iter().enumerate() {
                        let offset = f32::from(offset);
                        let top = y + 10. + row as f32 - 0.5;
                        highlight.quad(
                            overlay([left + 8. - offset, top, rect[2] - 8. + offset, top + 1.]),
                            [0.5; 4],
                            color,
                        );
                    }
                }
                let [w, h] = cursor_size.map(|v| v as f32);
                let bob = super::choice_cursor::bob(&style, presentation_tick);
                let x = left + bob;
                let y = y - bob;
                for ([tx, ty], alpha) in self
                    .choice_trail
                    .sample(presentation_tick, [x as i32, y as i32])
                {
                    let [tx, ty] = [tx as f32, ty as f32];
                    batches[layer::CURSOR].quad(
                        overlay([tx + 4. - w, ty + 8., tx + 4., ty + 8. + h]),
                        [0., 0., w, h],
                        [1., 1., 1., f32::from(alpha) / 255.],
                    );
                }
                batches[layer::CURSOR].quad(
                    overlay([x + 8. - w, y + 12., x + 8., y + 12. + h]),
                    [0., 0., w, h],
                    [0., 0., 0., 127. / 255.],
                );
                batches[layer::CURSOR].quad(
                    overlay([x + 4. - w, y + 8., x + 4., y + 8. + h]),
                    [0., 0., w, h],
                    [1.; 4],
                );
            }
            let mut x = left;
            let mut y = top;
            for (index, glyph) in player
                .current()
                .glyphs
                .iter()
                .take(player.visible)
                .enumerate()
            {
                if glyph.character == '\n' {
                    x = left;
                    y += 25.;
                    continue;
                }
                let spec =
                    self.font.glyphs.get(&glyph.character).with_context(|| {
                        format!("uncooked dialogue glyph {:?}", glyph.character)
                    })?;
                batches[layer::FONT].quad(
                    [x, y, x + 21., y + 25.],
                    glyph_uv(spec.rect),
                    [
                        f32::from(glyph.color[0]) / 255.,
                        f32::from(glyph.color[1]) / 255.,
                        f32::from(glyph.color[2]) / 255.,
                        f32::from(player.glyph_alpha(index)) / 255.,
                    ],
                );
                x += body_advance(spec.advance);
            }
            x = left - 6.;
            let height = rect[3] - top;
            let name_top = frame_top(top, height) - 24.;
            for character in speaker.chars().filter(|_| opening.is_none()) {
                let spec = self
                    .font
                    .glyphs
                    .get(&character)
                    .context("uncooked speaker-name glyph")?;
                batches[layer::FONT].quad(
                    [x, name_top, x + spec.advance as f32, name_top + 18.],
                    glyph_uv(spec.rect),
                    [1.; 4],
                );
                x += spec.advance as f32 - 3.;
            }
            batches[layer::FONT].append(highlight);
            if !player.persistent
                && player.accepts_input()
                && player.fully_revealed()
                && request.flags & flags::FRAMELESS == 0
                && !session
                    .events
                    .world
                    .choices
                    .get(&slot)
                    .is_some_and(|c| c.operation.is_pending())
            {
                // The continue marker’s pulse follows scene age, not window age.
                let bottom = frame_top(top, rect[3] - top) + (rect[3] - top).max(48.);
                let phase = (session.effect_clock.tick() % 90) as f32 * 4.0f32.to_radians();
                batches[layer::FRAME].quad(
                    [rect[2] - 28., bottom, rect[2] - 4., bottom + 24.],
                    if preferences.is_some_and(|s| s.window == 2) {
                        [112., 176., 136., 200.]
                    } else {
                        [224., 120., 248., 144.]
                    },
                    [1., 1., 1., (phase.sin().abs() * 255.).trunc() / 255.],
                );
            }
            for (index, mut batch) in batches.into_iter().enumerate() {
                if opening.is_some() {
                    for color in &mut batch.colors {
                        color[3] = (color[3] * 128.).trunc() / 255.;
                    }
                }
                if batch.positions.is_empty() {
                    continue;
                }
                let key = (slot, index);
                used.insert(key);
                let coverage = match index {
                    layer::FRAME => ornament_coverage.clone(),
                    layer::POINTER => corner_coverage.clone(),
                    layer::CORNERS => speaker_fill_coverage.clone(),
                    layer::SPEAKER_FILL => speaker_coverage.clone(),
                    layer::POINTER_FILL => pointer_coverage.clone(),
                    layer::FILL => fill_coverage.clone(),
                    layer::BEVEL => frame_coverage.clone(),
                    _ => Coverage::default(),
                };
                let texture_index = if matches!(index, layer::FILL | layer::BEVEL) {
                    preferences.map_or(7, |s| {
                        if s.window == 0 && s.background == 5 {
                            8
                        } else {
                            usize::from(s.background) + 2
                        }
                    })
                } else {
                    layer::TEXTURES[index]
                };
                let size = match index {
                    layer::FONT => [self.font.width, self.font.height],
                    layer::CURSOR => cursor_size,
                    _ => [
                        self.spec.textures[texture_index].width,
                        self.spec.textures[texture_index].height,
                    ],
                };
                let layer = self
                    .layers
                    .get_mut(&key)
                    .context("dialogue layer was not prepared")?;
                let material = materials
                    .get(&layer.material)
                    .context("dialogue layer material was removed")?;
                let source = if index == layer::CURSOR {
                    cursor_image
                } else {
                    &self.images[texture_index]
                };
                if material.coverage != coverage || material.source != *source {
                    let mut material = materials.get_mut(&layer.material).unwrap();
                    material.coverage = coverage;
                    material.source = source.clone();
                    material.sampling = source.clone();
                }
                layer.update_mesh(batch, size, meshes)?;
                layer.show(true, commands);
            }
        }
        for (key, layer) in &mut self.layers {
            if !used.contains(key) {
                layer.show(false, commands);
            }
        }
        Ok(())
    }
}

pub(super) fn subtitles(
    mut commands: Commands,
    playback: (
        Res<super::movie::Playback>,
        Option<Res<super::new_game::Session>>,
    ),
    sinks: Query<&super::audio_output::Sink>,
    mut art: Option<ResMut<Artwork>>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut exit: MessageWriter<AppExit>,
) {
    let (movie, session) = playback;
    let Some(art) = &mut art else {
        return;
    };
    let position = movie
        .audio_sink(&sinks)
        .map(super::audio_output::Sink::position);
    let frame = movie.timeline_frame(position);
    let story = movie
        .asset
        .as_ref()
        .is_some_and(|a| a.path == "movies/story-intro.mkv");
    let enabled = session
        .as_ref()
        .and_then(|s| s.field.events.world.party.as_ref())
        .is_none_or(|p| p.settings.preferences.movie_subtitles);
    let cue = frame.filter(|_| story && enabled).and_then(|frame| {
        // Subtitle cue frames are one-based; the decoded movie frame is zero-based.
        art.subtitles
            .cues
            .iter()
            .rev()
            .find(|cue| cue.frame <= frame + 1)
    });
    if cue.is_none() || !art.ready(&images) {
        if let Some(layer) = &mut art.subtitle_layer {
            layer.show(false, &mut commands);
        }
        return;
    }
    let result = (|| -> Result<Batch> {
        let mut batch = Batch::default();
        for line in &cue.unwrap().lines {
            let [mut x, y] = line.position;
            for character in line.text.chars() {
                let glyph = art
                    .font
                    .glyphs
                    .get(&character)
                    .with_context(|| format!("uncooked subtitle glyph {character:?}"))?;
                let (width, scale) = if character.is_ascii() {
                    (18., 18. / 17.)
                } else {
                    (25., 1.)
                };
                batch.quad([x, y, x + width, y + 25.], glyph_uv(glyph.rect), [1.; 4]);
                x += (glyph.advance as f32 * scale).trunc() - 1.;
            }
        }
        Ok(batch)
    })();
    match result {
        Ok(batch) => {
            if batch.positions.is_empty() {
                if let Some(layer) = &mut art.subtitle_layer {
                    layer.show(false, &mut commands);
                }
                return;
            }
            let size = [art.font.width, art.font.height];
            let layer = art
                .subtitle_layer
                .as_mut()
                .expect("subtitle layer was not prepared");
            layer
                .update_mesh(batch, size, &mut meshes)
                .expect("subtitle mesh is retained");
            layer.show(true, &mut commands);
        }
        Err(error) => {
            error!("Movie subtitle rendering failed: {error:#}");
            exit.write(AppExit::error());
        }
    }
}
fn glyph_uv([x, y, width, height]: [u32; 4]) -> [f32; 4] {
    [x as f32, y as f32, (x + width) as f32, (y + height) as f32]
}
fn frame_top(top: f32, height: f32) -> f32 {
    if height < 48. {
        top + (height / 2.).trunc() - 24.
    } else {
        top
    }
}

/// Expand the box from its speaker attachment while keeping frame slices
/// full-size. Text and the speech pointer appear after expansion.
fn expanding_rect(rect: [f32; 4], origin: [f32; 2], fraction: f32) -> [f32; 4] {
    let half = [
        ((rect[2] - rect[0]) / 2.).trunc(),
        ((rect[3] - rect[1]) / 2.).trunc(),
    ];
    let center: [f32; 2] =
        std::array::from_fn(|i| (origin[i] + (rect[i] + half[i] - origin[i]) * fraction).trunc());
    [
        (center[0] - half[0] * fraction).trunc(),
        (center[1] - half[1] * fraction).trunc(),
        (center[0] + half[0] * fraction).trunc(),
        (center[1] + half[1] * fraction).trunc(),
    ]
}

fn project_dialogue_point(
    camera: &Transform,
    fov_degrees: f32,
    point: Vec3,
    resolution: super::Resolution,
) -> [f32; 2] {
    let projection =
        Mat4::perspective_rh(fov_degrees.to_radians(), resolution.aspect(), 100., 40000.);
    let ndc = projection.project_point3(camera.to_matrix().inverse().transform_point3(point));
    if resolution == super::Resolution::default() {
        return [
            (320. * (ndc.x + 1.)).round().clamp(0., 640.),
            (224. * (1. - ndc.y)).round().clamp(0., 480.),
        ];
    }
    // Project into the 640×448 scene, then use those pixel coordinates directly
    // in the 640×480 overlay. Scaling Y here would push dialogue downward.
    let ui = resolution.ui_size();
    [
        (320. + ui.x * 0.5 * ndc.x).round().clamp(0., 640.),
        (240. - ui.y * 0.5 + ui.y * (224. / 480.) * (1. - ndc.y))
            .round()
            .clamp(0., 480.),
    ]
}

fn layout(
    font: &BitmapFont,
    request: &Dialogue,
    player: &DialoguePlayer,
    session: &FieldSession,
    head_height: f32,
    resolution: super::Resolution,
) -> Result<([f32; 4], Option<[f32; 2]>)> {
    let mut width = 0f32;
    let mut height = 25f32;
    for page in &player.pages {
        let mut x = 0f32;
        let mut lines = 1.;
        for (index, glyph) in page.glyphs.iter().enumerate() {
            if glyph.character == '\n' {
                width = width.max(x);
                x = 0.;
                // A trailing newline terminates the last row without adding an empty one.
                if index + 1 < page.glyphs.len() {
                    lines += 1.;
                }
            } else {
                // Box sizing retains control boundaries; glyph placement does not.
                let measured =
                    glyph.measured_character(page.glyphs.get(index + 1).map(|next| next.character));
                let advance = font
                    .glyphs
                    .get(&measured)
                    .with_context(|| format!("uncooked dialogue glyph {:?}", measured))?
                    .advance;
                x += body_advance(advance);
            }
        }
        width = width.max(x);
        height = height.max(lines * 25.);
    }
    if let Some(size) = request.dimensions {
        [width, height] = size.map(f32::from);
    }
    let camera = session
        .events
        .world
        .field_camera
        .as_ref()
        .context("dialogue needs a field camera")?;
    let transform = Transform::from_translation(Vec3::from_array(camera.position))
        .looking_at(Vec3::from_array(camera.target), Vec3::Z);
    let project =
        |point| project_dialogue_point(&transform, camera.fov_degrees(), point, resolution);
    let actor = request
        .speaker_actor
        .and_then(|id| session.events.world.actors.get(&id).map(|a| (id, a)));
    let mut pointer = actor
        .filter(|_| {
            request.flags & flags::POINTER != 0
                || matches!(request.anchor, DialogueAnchor::Actor(_))
        })
        .map(|(_, actor)| project(Vec3::from_array(actor.position) + Vec3::Z * 80.));
    // Center integer pixel dimensions without introducing half-pixel offsets.
    // Truncating only after subtraction shifts odd-sized boxes by one pixel.
    let half_width = (width / 2.).trunc();
    let half_height = (height / 2.).trunc();
    let (mut left, mut top, clamp) = match request.anchor {
        DialogueAnchor::ScreenGrid(grid) => (
            [106., 318., 530.][usize::from(grid % 3)] - half_width,
            [80., 240., 400.][usize::from(grid / 3)] - half_height,
            true,
        ),
        DialogueAnchor::Screen([x, y]) => {
            let [x, y] = if x == 0. && y == 0. {
                [320., 240.]
            } else {
                [x, y]
            };
            (x - half_width, y - half_height, false)
        }
        DialogueAnchor::Actor(_) => {
            let above = box_above_speaker(request.flags, pointer);
            let [x, y] = actor.map_or([320., 240.], |(_, actor)| {
                project(
                    Vec3::from_array(actor.position)
                        + Vec3::Z
                            * if above {
                                head_height + f32::from(request.height_offset)
                            } else {
                                0.
                            },
                )
            });
            if let Some(pointer) = &mut pointer {
                // Attached pointers share the box anchor's X. The torso-height
                // probe still selects whether the box belongs above or below.
                pointer[0] = x;
            }
            // Anchor an upper box at the head and a lower box below the feet;
            // position and pointer orientation must change together.
            (
                x - half_width,
                if above { y - 24. - height } else { y + 32. },
                true,
            )
        }
    };
    if clamp {
        left = left.clamp(41., (599. - width).max(41.));
        top = top.clamp(41., (439. - height).max(41.));
    }
    left = left.trunc();
    top = top.trunc();
    Ok(([left, top, left + width, top + height], pointer))
}

fn box_above_speaker(flags: u16, pointer: Option<[f32; 2]>) -> bool {
    if flags & flags::AUTO_SIDE != 0 {
        pointer.is_none_or(|[_, y]| y >= 176.)
    } else {
        flags & flags::BELOW == 0 && flags & flags::ABOVE != 0
    }
}

/// Saved window style and pattern assembled from prepared frame slices.
fn frame(
    batch: &mut [Batch; layer::TEXTURES.len()],
    [x0, top, x1, bottom]: [f32; 4],
    speaker: &str,
    font: &BitmapFont,
    pointer: Option<[f32; 2]>,
    flags: u16,
    preferences: Option<&resonance_content::menu_data::CustomizeSettings>,
) {
    let h = (bottom - top).max(48.);
    let y0 = frame_top(top, bottom - top);
    let y1 = y0 + h;
    let white = [1.; 4];
    let pointer_art = pointer
        .filter(|[x, _]| x - 16. >= x0 && x + 16. <= x1)
        .map(|[x, _]| {
            if box_above_speaker(flags, pointer) {
                (
                    [x - 16., y1 + 12., x + 16., y1 + 36.],
                    [112., 208., 144., 232.],
                    [64., 160., 96., 184.],
                )
            } else {
                (
                    [x - 16., y0 - 36., x + 16., y0 - 12.],
                    [144., 232., 176., 208.],
                    [64., 184., 96., 160.],
                )
            }
        });
    let defaults = resonance_content::menu_data::CustomizeSettings::default();
    let preferences = preferences.unwrap_or(&defaults);
    let colors = &preferences.colors;
    let tint = if flags & flags::GREEN != 0 {
        colors.popup
    } else if flags & flags::RED != 0 {
        colors.choice
    } else {
        colors.dialogue
    };
    let blue = tint.map(|c| f32::from(c) / 255.);
    if !speaker.is_empty() {
        let width: f32 = speaker
            .chars()
            .filter_map(|c| font.glyphs.get(&c))
            .map(|g| g.advance as f32 - 3.)
            .sum();
        for (rect, uv, overlay) in [
            (
                [x0 - 24., y0 - 28., x0 - 8., y0 + 4.],
                [80., 208., 96., 240.],
                [32., 160., 48., 192.],
            ),
            (
                [x0 - 8., y0 - 28., x0 + width, y0 + 4.],
                [94., 208., 96., 240.],
                [46., 160., 50., 192.],
            ),
            (
                [x0 + width, y0 - 28., x0 + width + 16., y0 + 4.],
                [96., 208., 112., 240.],
                [48., 160., 64., 192.],
            ),
        ] {
            batch[layer::SPEAKER].quad(rect, uv, white);
            batch[layer::SPEAKER_FILL].quad(rect, overlay, blue);
        }
    }
    for (slice, (rect, uv)) in [
        (
            [x0 - 16., y0 - 16., x0 + 8., y0 + 8.],
            [0., 208., 24., 232.],
        ),
        (
            [x1 - 8., y0 - 16., x1 + 16., y0 + 8.],
            [24., 208., 48., 232.],
        ),
        (
            [x0 - 16., y1 - 8., x0 + 8., y1 + 16.],
            [0., 232., 24., 256.],
        ),
        (
            [x1 - 8., y1 - 8., x1 + 16., y1 + 16.],
            [24., 232., 48., 256.],
        ),
        ([x0 - 16., y0 + 7., x0, y1 - 8.], [64., 208., 80., 224.]),
        (
            [x1 + 12., y0 + 7., x1 + 28., y1 - 8.],
            [64., 224., 80., 240.],
        ),
        ([x0 + 8., y0 - 16., x1 - 8., y0], [48., 208., 64., 224.]),
        (
            [x0 + 8., y1 + 12., x1 - 8., y1 + 28.],
            [48., 224., 64., 240.],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        // Artwork coverage clips this border at the speaker tab and pointer.
        // Rectangular clipping incorrectly removes its corner through the
        // tab's transparent tip while a narrow window is expanding.
        // Corners own their one-pixel overlap with strips; blending it twice
        // would darken the translucent window.
        batch[if slice < 4 {
            layer::CORNERS
        } else {
            layer::FRAME
        }]
        .quad(rect, uv, white);
    }
    if let Some((rect, uv, overlay)) = pointer_art {
        batch[layer::POINTER].quad(rect, uv, white);
        batch[layer::POINTER_FILL].quad(rect, overlay, blue);
    }
    // Background repeats the 64-pixel authored pattern in encoded color space.
    batch[layer::FILL].quad(
        [x0 - 14., y0 - 14., x1 + 14., y1 + 14.],
        [0., 0., x1 - x0, y1 - y0],
        blue,
    );
    // Frame styles share one atlas; background overlays occupy 32-pixel rows.
    let row = [96., 208., 152.][usize::from(preferences.window)];
    for index in [layer::FRAME, layer::CORNERS, layer::POINTER, layer::SPEAKER] {
        for uv in &mut batch[index].uv {
            uv[1] += row - 208.;
        }
    }
    let pattern = f32::from(preferences.background) * 32.;
    for index in [layer::SPEAKER_FILL, layer::POINTER_FILL] {
        for uv in &mut batch[index].uv {
            uv[1] += pattern - 160.;
            if index == layer::SPEAKER_FILL && preferences.window == 2 {
                uv[0] -= 32.;
            }
        }
    }
    if preferences.window == 0 {
        let color = |rgb: [u8; 3]| {
            [
                f32::from(rgb[0]) / 255.,
                f32::from(rgb[1]) / 255.,
                f32::from(rgb[2]) / 255.,
                blue[3],
            ]
        };
        let rgb = [tint[0], tint[1], tint[2]];
        let light = color(rgb.map(|v| v.saturating_add(64)));
        let dark = color(rgb.map(|v| (v / 2).saturating_sub(64)));
        let tab = color(rgb.map(|v| v.saturating_add(128)));
        batch[layer::SPEAKER].colors.fill(tab);
        batch[layer::SPEAKER_FILL] = Batch::default();
        batch[layer::POINTER] = Batch::default();
        if pointer_art.is_some() {
            let above = box_above_speaker(flags, pointer);
            let pattern = if preferences.background == 5 {
                2
            } else {
                preferences.background
            };
            let y = f32::from(pattern) * 32.;
            let [a, b] = if above {
                [y + 6., y + 30.]
            } else {
                [y + 24., y + 2.]
            };
            batch[layer::POINTER_FILL].uv = vec![[64., a], [96., a], [96., b], [64., b]];
            batch[layer::POINTER_FILL]
                .colors
                .fill(if above { dark } else { light });
        }
        let [left, top, right, bottom] = [x0 - 14., y0 - 14., x1 + 14., y1 + 14.];
        for (rect, color) in [
            ([left, top, right, top + 4.], light),
            ([left, bottom - 4., right, bottom], dark),
            ([left, top + 4., left + 4., bottom - 4.], light),
            ([right - 4., top + 4., right, bottom - 4.], dark),
        ] {
            batch[layer::BEVEL].quad(rect, [0., 0., (x1 - x0) * 2., (y1 - y0) * 2.], color);
        }
        batch[layer::FILL].colors = [
            rgb,
            rgb.map(|v| (u16::from(v) * 3 / 4) as u8),
            rgb.map(|v| v / 2),
            rgb.map(|v| (u16::from(v) * 3 / 4) as u8),
        ]
        .map(color)
        .to_vec();
        if preferences.background == 5 {
            batch[layer::FILL].uv = vec![[0., 0.], [32., 0.], [32., 32.], [0., 32.]];
        }
    }
}

// ASCII body glyphs use 21×25 quads with proportional advances.
fn body_advance(advance: u32) -> f32 {
    (advance as f32 * 0.84).trunc()
}

#[derive(Default, Clone, PartialEq)]
struct Batch {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}
impl Batch {
    /// Clip axis-aligned quads after composition, including their gradient colours.
    fn clip_vertical(&mut self, start: usize, top: f32, bottom: f32) {
        for i in (start..self.positions.len()).step_by(4) {
            let y0 = self.positions[i][1];
            let y1 = self.positions[i + 2][1];
            if y0 <= y1 {
                continue;
            }
            for (a, b) in [(0, 3), (1, 2)] {
                let (uv0, uv1) = (self.uv[i + a], self.uv[i + b]);
                let (c0, c1) = (self.colors[i + a], self.colors[i + b]);
                for j in [i + a, i + b] {
                    let y = self.positions[j][1].clamp(bottom, top);
                    let t = ((y0 - y) / (y0 - y1)).clamp(0., 1.);
                    self.positions[j][1] = y;
                    self.uv[j] = std::array::from_fn(|c| uv0[c] + (uv1[c] - uv0[c]) * t);
                    self.colors[j] = std::array::from_fn(|c| c0[c] + (c1[c] - c0[c]) * t);
                }
            }
        }
    }
    fn append(&mut self, other: Self) {
        let offset = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.uv.extend(other.uv);
        self.colors.extend(other.colors);
        self.indices
            .extend(other.indices.into_iter().map(|i| i + offset));
    }
    fn quad(
        &mut self,
        [left, top, right, bottom]: [f32; 4],
        [u0, v0, u1, v1]: [f32; 4],
        color: [f32; 4],
    ) {
        let start = self.positions.len() as u32;
        self.positions.extend([
            [left - 320., 240. - top, 0.],
            [right - 320., 240. - top, 0.],
            [right - 320., 240. - bottom, 0.],
            [left - 320., 240. - bottom, 0.],
        ]);
        self.uv.extend([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
        self.colors.extend([color; 4]);
        self.indices
            .extend([start, start + 2, start + 1, start, start + 3, start + 2]);
    }
    fn mesh(mut self, [width, height]: [u32; 2]) -> Mesh {
        for uv in &mut self.uv {
            uv[0] /= width as f32;
            uv[1] /= height as f32;
        }
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_speaker_gets_an_outlined_pointer_above_the_box() {
        let pointer = Some([312., 167.]);
        assert!(!box_above_speaker(0x40, pointer));
        assert!(box_above_speaker(0x40, Some([312., 176.])));
        assert!(box_above_speaker(0x80, pointer));
        assert!(!box_above_speaker(0x100, pointer));
        let font = BitmapFont {
            version: 1,
            texture: String::new(),
            width: 1,
            height: 1,
            line_height: 25,
            glyphs: Default::default(),
            source_sha256: String::new(),
            executable_sha256: String::new(),
        };
        let mut batches = std::array::from_fn(|_| Batch::default());
        frame(
            &mut batches,
            [173., 240., 451., 290.],
            "",
            &font,
            pointer,
            0x40,
            None,
        );
        // The upper outline uses atlas rows 232 → 208; row 184 is transparent.
        assert_eq!(
            &batches[layer::POINTER].uv[batches[layer::POINTER].uv.len() - 4..],
            &[[144., 232.], [176., 232.], [176., 208.], [144., 208.]]
        );
        let p = &batches[layer::POINTER].positions[batches[layer::POINTER].positions.len() - 4..];
        assert_eq!(p[0][1], 36.);
        assert_eq!(p[2][1], 12.);
    }

    #[test]
    fn attached_box_projects_to_the_independent_colette_checkpoint() {
        // colette-turn-silent: camera, actor and retained attachment height
        // read from the paired state. Its body rect is [169,120,492,145].
        let camera = Transform::from_xyz(-104.55858, -1278., 186.)
            .looking_at(Vec3::new(-88., -329., 87.), Vec3::Z);
        let point = project_dialogue_point(
            &camera,
            27.,
            Vec3::new(-73., -134., 135.),
            Default::default(),
        );
        assert_eq!(point, [330., 169.]);
        assert_eq!([point[0] - 161., point[1] - 24. - 25.], [169., 120.]);
    }

    #[test]
    fn opening_starts_at_the_attachment_and_preserves_integer_half_dimensions() {
        let rect = [169., 120., 492., 145.];
        assert_eq!(
            expanding_rect(rect, [330., 220.], 0.),
            [330., 220., 330., 220.]
        );
        assert_eq!(
            expanding_rect(rect, [330., 220.], 1.),
            [169., 120., 491., 144.]
        );
    }
}

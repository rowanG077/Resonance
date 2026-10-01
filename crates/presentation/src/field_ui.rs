//! Bitmap dialogue composition from cooked images and high-level text state.
#[path = "battle_ui.rs"]
mod battle_ui;
#[path = "field_ui_coverage.rs"]
mod coverage;
#[path = "credits.rs"]
pub(crate) mod credits;
#[path = "field_ui_damage.rs"]
mod damage;
#[path = "field_ui_failure.rs"]
mod failure;
#[path = "session_screen.rs"]
pub(crate) mod session_screen;
pub(super) use failure::update as transition_failure;
#[path = "field_ui_fade.rs"]
mod fade;
pub(super) use battle_ui::{Artwork as BattleHud, EnemyHud as BattleEnemyHud, PartyHudInput};
#[path = "game_over_ui.rs"]
mod game_over_ui;
pub(super) use game_over_ui::Artwork as GameOverArt;
#[path = "field_ui_menu.rs"]
mod menu;
#[cfg(test)]
pub(super) use menu::MenuDraws;
pub(super) fn install(app: &mut App) {
    app.add_plugins(bevy::sprite_render::Material2dPlugin::<Surface>::default());
    bevy::asset::embedded_asset!(app, "field_ui.wgsl");
    menu::install(app);
}
#[path = "field_ui_overlay.rs"]
mod overlay;
pub(super) fn model_preview_depth() -> f32 {
    menu::model_preview_depth()
}
#[path = "field_ui_prompt.rs"]
mod prompt;
#[path = "field_ui_skit.rs"]
mod skit;
#[path = "field_ui_world.rs"]
mod world;
use anyhow::{Context, Result};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d},
};
use coverage::Coverage;
use resonance_content::font::{BitmapFont, DialogueArt};
use resonance_events::dialogue::{DIALOGUE_SLOTS, Dialogue, DialogueAnchor, TextToken, flags};
use resonance_game::{dialogue::DialoguePlayer, field::FieldSession};
use std::{collections::BTreeMap, fs, path::Path};
pub(super) use world::Artwork as WorldArtwork;

/// Decode before activation. Callers decide whether failure is fatal or optional.
fn ui_image(
    path: &str,
    size: [u32; 2],
    repeat: bool,
    read: &impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Image> {
    use bevy::image::{CompressedImageFormats, ImageType};
    let extension = path
        .rsplit_once('.')
        .context("UI image has no extension")?
        .1;
    let address = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    let image = Image::from_buffer(
        &read(path)?,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        false,
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: address,
            address_mode_v: address,
            ..ImageSamplerDescriptor::linear()
        }),
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_context(|| format!("UI image {path}"))?;
    anyhow::ensure!(
        [image.width(), image.height()] == size,
        "UI image dimensions differ from its descriptor: {path}"
    );
    Ok(image)
}

/// Retained dialogue text may outlive its visible window or request owner.
pub(super) fn displayed_dialogue<'a>(
    player: &DialoguePlayer,
    request: Option<&'a Dialogue>,
) -> Option<&'a Dialogue> {
    request.filter(|request| {
        request.operation.id() == player.operation.id()
            && player.operation.is_pending()
            && player.window_visible()
    })
}

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
    // Cooked atlas indices: fill, frame, color overlay, font and cursor.
    pub const TEXTURES: [usize; 9] = [7, 7, 0, 1, 0, 0, 1, 0, 9];
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(SurfaceKey)]
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
    opaque: bool,
    additive: bool,
    /// Channel swap: both texture and raster become [R,R,R,A].
    red_channel: bool,
}
impl Surface {
    pub(super) fn images_ready(&self, images: &Assets<Image>) -> bool {
        [
            &self.source,
            &self.sampling,
            &self.frame_mask,
            &self.color_mask,
        ]
        .into_iter()
        .all(|image| images.contains(image.id()))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct SurfaceKey(bool, bool, bool);
impl From<&Surface> for SurfaceKey {
    fn from(surface: &Surface) -> Self {
        Self(surface.opaque, surface.additive, surface.red_channel)
    }
}
impl Material2d for Surface {
    fn vertex_shader() -> ShaderRef {
        "embedded://resonance_presentation/field_ui.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/field_ui.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::sprite_render::Material2dKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.label = Some("resonance/field-ui".into());
        if key.bind_group_data.2
            && let Some(fragment) = &mut descriptor.fragment
        {
            fragment.shader_defs.push("RED_CHANNEL".into());
        }
        if key.bind_group_data.1
            && let Some(fragment) = &mut descriptor.fragment
        {
            // Radar halo uses source-alpha additive blending.
            let component = BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState {
                    color: component,
                    alpha: component,
                });
            }
        }
        if key.bind_group_data.0
            && let Some(fragment) = &mut descriptor.fragment
        {
            fragment.shader_defs.push("OPAQUE_IMAGE".into());
        }
        Ok(())
    }
}

#[derive(Resource)]
pub(super) struct Artwork {
    pub(super) resolution: super::Resolution,
    pub(super) attached_positions: BTreeMap<i32, Vec3>,
    pub font: BitmapFont,
    spec: DialogueArt,
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    layers: BTreeMap<(u8, usize), Layer>,
    head_heights: BTreeMap<u64, f32>,
    subtitles: Vec<resonance_content::font::SubtitleCue>,
    subtitle_layer: Option<Layer>,
    fade_surface: Handle<Surface>,
    fade_layer: Option<Layer>,
    overlays: overlay::Artwork,
    menu: menu::MenuArtwork,
    prompt_layers: Vec<Layer>,
    damage_layer: Option<Layer>,
    skits: skit::Artwork,
    credits: Option<credits::Artwork>,
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
    artwork: menu::MenuArtwork,
}
impl MenuOverlay {
    pub fn load(
        root: &Path,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        Self::load_with(
            |path| Ok(fs::read(root.join(path))?),
            server,
            materials,
            diagnostics,
        )
    }
    pub fn load_with(
        read: impl Fn(&str) -> Result<Vec<u8>>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        dialogue.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&dialogue.font)?)?;
        font.validate()?;
        let image = menu::menu_image(server, font.texture.clone(), true);
        let surface = materials.add(Surface {
            source: image.clone(),
            sampling: image.clone(),
            frame_mask: image.clone(),
            color_mask: image,
            coverage: Coverage::default(),
            additive: false,
            red_channel: false,
            opaque: false,
        });
        let artwork = menu::MenuArtwork::load(
            read,
            Default::default(),
            server,
            materials,
            (&surface, [font.width, font.height]),
            diagnostics,
        )?;
        Ok(Self {
            font,
            dialogue,
            artwork,
        })
    }
    #[cfg(test)]
    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.artwork.images()
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.artwork.ready(images)
    }
    #[cfg(test)]
    pub fn drawn_images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.artwork.drawn_images()
    }
    pub fn drawn_images_ready(&self, images: &Assets<Image>, server: &AssetServer) -> Result<bool> {
        self.artwork.drawn_images_ready(images, server)
    }
    pub fn menu_settled(&self, menu: &resonance_game::menu::Menu, tick: u32) -> bool {
        self.artwork.visually_settled(menu, tick)
    }
    pub fn render(
        &mut self,
        menu: Option<&resonance_game::menu::Menu>,
        presentation_tick: u32,
        resolution: super::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.artwork.render(
            menu::Source::Title(menu),
            &self.font,
            &self.dialogue,
            presentation_tick,
            resolution,
            commands,
            meshes,
        )
    }
    pub fn despawn(self, world: &mut World) {
        for layer in self.artwork.layers.into_values() {
            world.despawn(layer.entity);
        }
    }
    pub fn hide(&mut self, commands: &mut Commands) {
        self.artwork.clear_page(commands);
    }
}
impl Layer {
    fn update_mesh(
        &mut self,
        batch: Batch,
        size: [u32; 2],
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        if batch.indices.is_empty() {
            // Hidden layers retain valid warm/previous geometry. Bevy's mesh
            // allocator skips zero-vertex allocations but still tries to upload
            // them; publishing an empty mesh therefore reports use-after-free.
            meshes
                .get(&self.mesh)
                .context("retained UI mesh was removed")?;
            self.uploaded = Some((batch, size));
            return Ok(());
        }
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
        let visible = visible
            && self
                .uploaded
                .as_ref()
                .is_none_or(|(batch, _)| !batch.indices.is_empty());
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
fn subtitle_cues(
    files: &resonance_content::prepared::Files,
) -> Result<Vec<resonance_content::font::SubtitleCue>> {
    let result = (|| {
        let subtitles: resonance_content::font::MovieSubtitles =
            files.json("ui/story-subtitles.json")?;
        subtitles.validate()?;
        Ok(subtitles.cues)
    })();
    Ok(files
        .diagnostics()
        .attempt("movie subtitle cues", result)?
        .unwrap_or_default())
}

impl Artwork {
    pub fn despawn(&mut self, world: &mut World) {
        self.overlays.despawn(world);
        for layer in std::mem::take(&mut self.layers)
            .into_values()
            .chain(std::mem::take(&mut self.menu.layers).into_values())
            .chain(self.prompt_layers.drain(..))
            .chain(self.damage_layer.take())
            .chain(self.skits.layers.drain(..))
            .chain(self.skits.warm.drain(..))
            .chain(
                self.credits
                    .iter_mut()
                    .flat_map(|credits| credits.layers.drain(..)),
            )
        {
            world.despawn(layer.entity);
        }
        if let Some(layer) = self.fade_layer.take() {
            world.despawn(layer.entity);
        }
        if let Some(layer) = self.subtitle_layer.take() {
            world.despawn(layer.entity);
        }
        self.head_heights.clear();
        self.attached_positions.clear();
    }
    pub fn load_with(
        field: &resonance_content::field::FieldAssets,
        session: std::sync::Arc<resonance_content::session::SessionData>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
        files: &resonance_content::prepared::Files,
    ) -> Result<Self> {
        let mut art = Self::load_shared(
            files,
            session.experience.clone().into(),
            &field.overlays,
            server,
            materials,
            image_assets,
        )?;
        art.subtitles = subtitle_cues(files)?;
        if files.contains_key(resonance_content::credits::PATH) {
            art.credits = Some(credits::Artwork::load(
                files,
                &art.font,
                &art.surfaces[9],
                server,
                materials,
            )?);
        }
        Ok(art)
    }
    fn load_shared(
        files: &resonance_content::prepared::Files,
        experience: std::sync::Arc<[u32]>,
        overlays: &BTreeMap<i32, String>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
    ) -> Result<Self> {
        let read = |path: &str| -> Result<Vec<u8>> { Ok(files.read(path)?.to_vec()) };
        let spec: DialogueArt = serde_json::from_slice(
            &read("ui/dialogue.json").context("classroom dialogue art is missing; run cook-all")?,
        )?;
        spec.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&spec.font)?)?;
        font.validate()?;
        let mut images: Vec<Handle<Image>> = spec
            .textures
            .iter()
            .map(|t| t.path.clone())
            .chain([font.texture.clone()])
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
                    additive: false,
                    red_channel: false,
                    opaque: false,
                })
            })
            .collect();
        let menu = menu::MenuArtwork::load(
            read,
            experience,
            server,
            materials,
            (&surfaces[9], [font.width, font.height]),
            files.diagnostics(),
        )?;
        let skits = skit::Artwork::load(read, server, materials, &surfaces[9], image_assets)?;
        let (fade_image, fade_surface) = fade::surface(materials, image_assets);
        images.push(fade_image);
        Ok(Self {
            skits,
            credits: None,
            resolution: Default::default(),
            attached_positions: BTreeMap::new(),
            font,
            spec,
            images,
            surfaces,
            layers: BTreeMap::new(),
            head_heights: BTreeMap::new(),
            subtitles: Vec::new(),
            subtitle_layer: None,
            fade_surface,
            fade_layer: None,
            overlays: overlay::Artwork::load(overlays, read, server, materials, image_assets)?,
            menu,
            prompt_layers: Vec::new(),
            damage_layer: None,
        })
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id()))
            && self.overlays.ready(images)
            && self
                .credits
                .as_ref()
                .is_none_or(|credits| credits.ready(images))
    }
    pub(super) fn essential_ready(
        &self,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        let mut ready = true;
        for image in self.images.iter().chain(self.overlays.images()) {
            ready &= crate::field_view::image_ready(server, images, image)?;
        }
        Ok(ready)
    }
    pub(super) fn menu_published(&self, session: &FieldSession) -> bool {
        self.menu.published(session)
    }
    pub(super) fn skit_ready(
        &self,
        session: &FieldSession,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        self.skits
            .ready(session.active_skit.as_ref(), images, server)
    }
    pub(super) fn clear_skit(&mut self, commands: &mut Commands) {
        self.skits.clear(commands);
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_menu(
        &mut self,
        session: &FieldSession,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        let result = self
            .menu
            .render(
                menu::Source::Field(session),
                &self.font,
                &self.spec,
                tick,
                self.resolution,
                commands,
                meshes,
            )
            .and_then(|()| self.menu.drawn_images_ready(images, server));
        if result.is_err() {
            self.menu.clear_page(commands);
        }
        result
    }
    pub(super) fn render_skit(
        &mut self,
        session: &FieldSession,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        let result = self.skit_ready(session, images, server).and_then(|ready| {
            if !ready {
                return Ok(false);
            }
            self.skits.render(
                session.active_skit.as_ref(),
                &self.font,
                self.resolution,
                commands,
                meshes,
                images,
            )?;
            Ok(true)
        });
        if !matches!(result, Ok(true)) {
            self.clear_skit(commands);
        }
        result
    }
    pub fn menu_settled(&self, menu: &resonance_game::menu::Menu, tick: u32) -> bool {
        self.menu.visually_settled(menu, tick)
    }
    /// Allocate every supported dialogue slot/layer before its first request.
    pub(super) fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) {
        self.skits.prepare(commands, meshes);
        if let Some(credits) = &mut self.credits {
            credits.prepare(commands, meshes);
        }
        self.prepare_prompt(commands, meshes, materials);
        self.prepare_damage(commands, meshes);
        self.overlays.prepare(commands, meshes);
        self.prepare_fade(commands, meshes);
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
    ) -> impl Iterator<Item = (&Handle<Mesh>, &Handle<Surface>, bool)> {
        self.layers
            .values()
            .chain(self.subtitle_layer.iter())
            .chain(self.fade_layer.iter())
            .chain(&self.overlays.layers)
            .chain(&self.overlays.warm)
            .chain(&self.prompt_layers)
            .chain(self.damage_layer.iter())
            .chain(self.credits.iter().flat_map(|credits| &credits.layers))
            .map(|layer| (&layer.mesh, &layer.material, true))
            .chain(
                self.menu
                    .layers
                    .values()
                    .chain(&self.skits.layers)
                    .chain(&self.skits.warm)
                    .map(|layer| (&layer.mesh, &layer.material, false)),
            )
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
                        &session.events.world,
                        &self.attached_positions,
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
    pub fn render_overlays(
        &mut self,
        world: &resonance_events::GameWorld,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<Vec<i32>> {
        self.overlays.render(world, commands, meshes)
    }
    pub fn render(
        &mut self,
        session: &FieldSession,
        heads: &BTreeMap<i32, Vec3>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        if let Some(credits) = &mut self.credits {
            credits.render(session.events.world.screen_request.as_ref(), commands);
        }
        self.render_fade(&session.events.world, commands, meshes)?;
        self.render_prompt(session, commands, meshes)?;
        self.render_damage(session, commands, meshes)?;
        let (world, dialogue) = session.dialogue_scene();
        self.render_dialogue(
            world,
            dialogue,
            &session.events.world,
            world.tick,
            heads,
            commands,
            meshes,
            materials,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn render_dialogue(
        &mut self,
        world: &resonance_events::GameWorld,
        dialogue: &BTreeMap<u8, DialoguePlayer>,
        camera_world: &resonance_events::GameWorld,
        presentation_tick: u32,
        heads: &BTreeMap<i32, Vec3>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        let mut used = std::collections::BTreeSet::new();
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
            let Some(request) = displayed_dialogue(player, world.dialogue.get(&slot)) else {
                continue;
            };
            let (rect, pointer) = layout(
                &self.font,
                request,
                player,
                camera_world,
                &self.attached_positions,
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
                && let Some(lines) = choice.selection.lines()
                && player.page + 1 == player.pages.len()
                && player.fully_revealed()
                && (player.accepts_input() || !choice.operation.is_pending())
            {
                let y =
                    super::ui_coordinates::drawing_y(top + f32::from(lines.selected_line) * 25.);
                let overlay = super::ui_coordinates::overlay_rect;
                let mut style = self.spec.selection.clone();
                style.mode = window;
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
                highlight.cursor([left, y], 1.);
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
                if let Some(choice) = world.choices.get(&slot)
                    && let resonance_events::dialogue::Selection::Number(number) = &choice.selection
                    && let Some(range) = &player.current().number
                    && index + 1 + usize::from(number.place) == range.end
                    && player.fully_revealed()
                    && (player.accepts_input() || !choice.operation.is_pending())
                {
                    highlight.quad(
                        [x, y, x + body_advance(spec.advance), y + 25.],
                        [0.5; 4],
                        [0.5, 0.625, 1., 0.5],
                    );
                }
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
                && player.continue_marker_visible()
                && request.flags & flags::FRAMELESS == 0
                && (player.page + 1 < player.pages.len()
                    || !world
                        .choices
                        .get(&slot)
                        .is_some_and(|c| c.operation.is_pending()))
            {
                // The continue marker’s pulse follows scene age, not window age.
                let bottom = frame_top(top, rect[3] - top) + (rect[3] - top).max(48.);
                let phase = (presentation_tick % 90) as f32 * 4.0f32.to_radians();
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
                let source = &self.images[texture_index];
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
    mut failures: super::field_view::Failures,
) {
    let (movie, session) = playback;
    let Some(art) = &mut art else {
        return;
    };
    let position = movie
        .audio_sink(&sinks)
        .map(super::audio_output::Sink::position);
    let frame = movie.timeline_frame(position);
    let story = movie.resource == Some(1);
    let enabled = session
        .as_ref()
        .and_then(|s| s.field().events.world.party.as_ref())
        .is_none_or(|p| p.settings.preferences.movie_subtitles);
    let ready = art.ready(&images);
    let Artwork {
        font,
        subtitles,
        subtitle_layer,
        ..
    } = &mut **art;
    let cue = frame.filter(|_| story && enabled).and_then(|frame| {
        // Subtitle cue frames are one-based; the decoded movie frame is zero-based.
        subtitles.iter().rev().find(|cue| cue.frame <= frame + 1)
    });
    if cue.is_none() || !ready {
        if let Some(layer) = subtitle_layer {
            layer.show(false, &mut commands);
        }
        return;
    }
    draw_subtitle(
        cue.unwrap(),
        font,
        subtitle_layer,
        &mut commands,
        &mut meshes,
        &mut failures,
    );
}

fn draw_subtitle(
    cue: &resonance_content::font::SubtitleCue,
    font: &BitmapFont,
    layer: &mut Option<Layer>,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    failures: &mut super::field_view::Failures,
) {
    let result = (|| -> Result<()> {
        let mut batch = Batch::default();
        for line in &cue.lines {
            let [mut x, y] = line.position;
            for character in line.text.chars() {
                let glyph = font
                    .glyphs
                    .get(&character)
                    .with_context(|| format!("uncooked subtitle glyph {character:?}"))?;
                let (width, scale) = if resonance_content::font::is_single_byte(character) {
                    (18., 18. / 17.)
                } else {
                    (25., 1.)
                };
                batch.quad([x, y, x + width, y + 25.], glyph_uv(glyph.rect), [1.; 4]);
                x += (glyph.advance as f32 * scale).trunc() - 1.;
            }
        }
        if batch.positions.is_empty() {
            if let Some(layer) = layer {
                layer.show(false, commands);
            }
            return Ok(());
        }
        let layer = layer.as_mut().context("subtitle layer was not prepared")?;
        layer.update_mesh(batch, [font.width, font.height], meshes)?;
        layer.show(true, commands);
        Ok(())
    })();
    if let Err(error) = result {
        if let Some(layer) = layer {
            layer.show(false, commands);
        }
        failures.skip("movie subtitle rendering", error);
    }
}
fn glyph_uv([x, y, width, height]: [u32; 4]) -> [f32; 4] {
    [
        x as f32,
        y as f32,
        x as f32 + width as f32,
        y as f32 + height as f32,
    ]
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
    camera_world: &resonance_events::GameWorld,
    attached_positions: &BTreeMap<i32, Vec3>,
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
    // Screen-anchored skit dialogue does not require a field camera.
    let project = |point| {
        camera_world
            .field_camera
            .as_ref()
            .map_or([320., 240.], |camera| {
                let transform = super::field_view::camera_transform(camera);
                project_dialogue_point(&transform, camera.fov_degrees(), point, resolution)
            })
    };
    let actor = request.speaker_actor.and_then(|id| {
        camera_world.actors.get(&id).map(|actor| {
            attached_positions
                .get(&id)
                .copied()
                .unwrap_or_else(|| Vec3::from_array(actor.position))
        })
    });
    let mut pointer = actor
        .filter(|_| {
            request.flags & flags::POINTER != 0
                || matches!(request.anchor, DialogueAnchor::Actor(_))
        })
        .map(|position| project(position + Vec3::Z * 80.));
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
            let [x, y] = actor.map_or([320., 240.], |position| {
                project(
                    position
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
    fn cursor(&mut self, [x, y]: [f32; 2], alpha: f32) {
        for (offset, color) in [(2., [0., 0., 0., alpha * 0.6]), (0., [1., 1., 1., alpha])] {
            let start = self.positions.len() as u32;
            for [dx, dy] in [[-14., 7.], [-4., 14.], [-14., 21.]] {
                let [x, y, _, _] =
                    super::ui_coordinates::overlay_rect([x + dx + offset, y + dy + offset, 0., 0.]);
                self.positions.push([x - 320., 240. - y, 0.]);
            }
            self.uv.extend([[0.5; 2]; 3]);
            self.colors.extend([color; 3]);
            self.indices.extend([start, start + 2, start + 1]);
        }
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
use crate::test_support::field_checkpoint;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally cooked field/menu assets; CPU field advancement and image failure only"]
    fn optional_images_do_not_freeze_field_and_failed_page_recovers_by_policy() -> Result<()> {
        use crate::{
            field_view::{Controls, advance_live},
            new_game::Session,
        };
        use bevy::ecs::system::RunSystemOnce;
        use resonance_content::{diagnostics::Diagnostics, prepared::Files};
        use resonance_game::menu::Page;
        use std::sync::{Arc, atomic::Ordering};
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        );
        for paranoid in [false, true] {
            let mut files = Files::load_with_diagnostics(
                &root,
                &["fields/map-332.preload.json"],
                &mut Default::default(),
                || false,
                Diagnostics::new(paranoid),
            )?;
            let checkpoint = field_checkpoint(&files)?;
            let mut session = Session::load_prepared(
                &root,
                Arc::new(files.clone()),
                Some(checkpoint.clone()),
                None,
                &mut Default::default(),
            )?;
            session.audio = None;
            let mut skit_catalog: resonance_content::skit::SkitCatalog =
                files.json("game/skits.json")?;
            // Obtain a real completion token from a script-owned preview request.
            let program = symphonia_script::Program::decode(
                &[
                    4u16, 0, 0, 0, 0x0200, 600, 0, 0x3000, 0x4000, 0x20ee, 0x20ff,
                ]
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
            )?;
            let mut caller = resonance_events::EventRuntime::new(
                Arc::new(program),
                Arc::new(resonance_events::ResourceLibrary {
                    skits: Some(Arc::new(skit_catalog.clone())),
                    ..Default::default()
                }),
            )?;
            let request = caller
                .world
                .skit_request
                .take()
                .context("preview did not request skit")?;
            let completion = request.operation.clone();
            session.field_mut().events.world.skit_request = Some(request);
            for _ in 0..300 {
                session.field_mut().step(Default::default())?;
                if session
                    .field()
                    .active_skit
                    .as_ref()
                    .is_some_and(|playback| {
                        playback
                            .events
                            .world
                            .skit
                            .as_ref()
                            .is_some_and(|scene| !scene.portraits.is_empty())
                    })
                {
                    break;
                }
            }
            let portrait = session
                .field()
                .active_skit
                .as_ref()
                .and_then(|playback| playback.events.world.skit.as_ref())
                .and_then(|scene| scene.portraits.values().next())
                .context("skit did not publish a portrait")?
                .resource;
            let mut active_skit = session.field_mut().active_skit.take();
            session.reload(&root, checkpoint)?;
            session.audio = None;
            skit_catalog.portraits.get_mut(&portrait).unwrap().images[0].texture =
                "missing-active-skit.ktx2".into();
            files.insert(
                "game/skits.json".into(),
                serde_json::to_vec(&skit_catalog)?.into(),
            );
            let mut menu_spec: resonance_content::menu::MenuArt = files.json("ui/menu.json")?;
            let missing = resonance_content::menu::MenuArt::WORLD_MAP_TEXTURES.start;
            menu_spec.textures.get_mut(&missing).unwrap().width = 0;
            let unused = resonance_content::menu::MenuArt::PORTRAIT_TEXTURES.end - 1;
            menu_spec.textures.get_mut(&unused).unwrap().path =
                "missing-unused-menu-portrait.ktx2".into();
            files.insert(
                "ui/menu.json".into(),
                serde_json::to_vec(&menu_spec)?.into(),
            );
            let mut dialogue_spec: DialogueArt = files.json("ui/dialogue.json")?;
            dialogue_spec.textures[0].path = "missing-essential-dialogue.ktx2".into();
            files.insert(
                "ui/dialogue.json".into(),
                serde_json::to_vec(&dialogue_spec)?.into(),
            );
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                AssetPlugin {
                    file_path: root.to_string_lossy().into_owned(),
                    ..Default::default()
                },
            ))
            .init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<Surface>()
            .init_asset::<bevy::gltf::Gltf>()
            .init_asset::<crate::sparse_animation::Clip>()
            .init_asset::<crate::materials::TitleSurface>()
            .register_asset_loader(bevy::image::ImageLoader::new(
                bevy::image::CompressedImageFormats::NONE,
            ))
            .init_resource::<Controls>()
            .init_resource::<crate::scene::SampledImages>()
            .init_resource::<crate::field_warm::Shared>()
            .add_message::<AppExit>();
            let diagnostics = Diagnostics::new(paranoid);
            app.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
            let resident = crate::loading::Resident::default();
            app.insert_resource(resident);
            app.world_mut().spawn(crate::menu_backdrop::Quad);
            let server = app.world().resource::<AssetServer>().clone();
            let field_art = crate::field_view::prepared_test_art(session.field_assets(), &server);
            let mut images = app.world_mut().remove_resource::<Assets<Image>>().unwrap();
            let mut materials = app
                .world_mut()
                .remove_resource::<Assets<Surface>>()
                .unwrap();
            let mut meshes = app.world_mut().remove_resource::<Assets<Mesh>>().unwrap();
            let mut art = Artwork::load_with(
                session.field_assets(),
                session.data().clone(),
                &server,
                &mut materials,
                &mut images,
                &files,
            )?;
            let unused_image: Handle<Image> = server.load(menu_spec.textures[&unused].path.clone());
            let skit_image: Handle<Image> = server.load("missing-active-skit.ktx2");
            let essential_image = art.images[0].clone();
            // Only completed image publication is injected; the missing path uses
            // the real AssetServer failure. No GPU fidelity is claimed here.
            for image in art
                .images
                .iter()
                .chain(art.overlays.images())
                .chain(art.menu.images())
                .filter(|image| {
                    ![&unused_image, &essential_image]
                        .iter()
                        .any(|missing| image.id() == missing.id())
                })
            {
                images.insert(image.id(), Image::default())?;
            }
            art.prepare(&mut app.world_mut().commands(), &mut meshes, &mut materials);
            app.world_mut().flush();
            app.insert_resource(images)
                .insert_resource(materials)
                .insert_resource(meshes)
                .insert_resource(art)
                .insert_resource(field_art)
                .insert_resource(session);
            let start = std::time::Instant::now();
            while [&unused_image, &skit_image, &essential_image]
                .iter()
                .any(|image| {
                    !matches!(
                        server.get_load_state(image.id()),
                        Some(bevy::asset::LoadState::Failed(_))
                    )
                })
            {
                anyhow::ensure!(
                    start.elapsed().as_secs() < 10,
                    "missing image did not finish failing"
                );
                app.update();
                std::thread::yield_now();
            }
            let world = app.world_mut();
            // A cold start must diagnose failed essential artwork before it
            // creates a GPU wait. Observe the recovery request, then restore
            // the input to exercise a successful start in the same fixture.
            assert!(!crate::field_warm::begin_test_startup(world));
            assert!(
                !world
                    .resource::<crate::loading::Resident>()
                    .active
                    .load(Ordering::Acquire)
            );
            assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
            assert_eq!(
                world
                    .remove_resource::<crate::field_view::RecoverField>()
                    .is_some(),
                !paranoid
            );
            assert_eq!(diagnostics.entries().len(), 1);
            assert_eq!(diagnostics.entries()[0].scope, "field essential artwork");
            world.resource_mut::<Messages<AppExit>>().clear();
            world
                .resource_mut::<Assets<Image>>()
                .insert(essential_image.id(), Image::default())?;
            let diagnostics = Diagnostics::new(paranoid);
            world.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
            assert!(
                !world
                    .resource::<Artwork>()
                    .menu
                    .ready(world.resource::<Assets<Image>>()),
                "fixture must fail the old global menu image gate"
            );
            assert!(crate::field_warm::begin_test_startup(world));
            let essential_materials: Vec<_> = world
                .resource::<Artwork>()
                .prepared_layers()
                .filter(|(_, _, essential)| *essential)
                .map(|(_, material, _)| material.clone())
                .collect();
            let draws: Vec<_> = world
                .query::<(
                    &MeshMaterial2d<Surface>,
                    &bevy::camera::visibility::RenderLayers,
                )>()
                .iter(world)
                .filter(|(_, layers)| {
                    layers.intersects(&bevy::camera::visibility::RenderLayers::layer(30))
                })
                .map(|(material, _)| material.0.clone())
                .collect();
            assert!(!draws.is_empty(), "startup must prepare actual UI draws");
            assert!(
                essential_materials
                    .iter()
                    .all(|material| draws.contains(material)),
                "startup must retain every essential UI material"
            );
            for material in draws {
                let surface = world.resource::<Assets<Surface>>().get(&material).unwrap();
                assert_ne!(surface.source.id(), unused_image.id());
                assert!(surface.images_ready(world.resource::<Assets<Image>>()));
            }
            assert!(
                !world
                    .resource::<crate::loading::Resident>()
                    .active
                    .load(Ordering::Acquire)
            );
            crate::field_warm::complete_test_startup(world);
            assert!(
                world
                    .resource::<crate::loading::Resident>()
                    .active
                    .load(Ordering::Acquire)
            );
            let before = world.resource::<Session>().field().events.tick();
            world.run_system_once(advance_live).unwrap();
            assert!(world.resource::<Session>().field().events.tick() > before);
            assert!(
                diagnostics.entries().is_empty(),
                "unused optional image was diagnosed"
            );
            world.resource_mut::<Session>().field_mut().step(
                resonance_game::field::FieldInput {
                    pressed_buttons: [resonance_events::input::Button::Menu].into(),
                    ..Default::default()
                },
            )?;
            let draw = |mut commands: Commands,
                        mut art: ResMut<Artwork>,
                        mut live: ResMut<Session>,
                        mut meshes: ResMut<Assets<Mesh>>,
                        images: Res<Assets<Image>>,
                        server: Res<AssetServer>,
                        mut failures: crate::field_view::Failures| {
                if let Err(error) = art.render_menu(
                    live.field(),
                    100,
                    &mut commands,
                    &mut meshes,
                    &images,
                    &server,
                ) {
                    failures.menu_page(live.field_mut(), error);
                }
            };
            world.run_system_once(draw).unwrap();
            world.flush();
            assert!(!world.resource::<Artwork>().menu.drawn_images_ready(
                world.resource::<Assets<Image>>(),
                world.resource::<AssetServer>()
            )?);
            // This CPU fixture completes only the images selected by the drawing.
            let drawn: Vec<_> = world
                .resource::<Artwork>()
                .menu
                .drawn_images()
                .cloned()
                .collect();
            for image in drawn {
                world
                    .resource_mut::<Assets<Image>>()
                    .insert(image.id(), Image::default())?;
            }
            world
                .resource::<MenuDraws>()
                .0
                .lock()
                .unwrap()
                .completed
                .store(true, std::sync::atomic::Ordering::Release);
            assert!(world.resource::<Artwork>().menu.drawn_images_ready(
                world.resource::<Assets<Image>>(),
                world.resource::<AssetServer>()
            )?);
            assert!(
                world
                    .resource::<Artwork>()
                    .menu
                    .layers
                    .values()
                    .any(|layer| layer.visible)
            );
            let retained = serde_json::to_value(
                world
                    .resource::<Session>()
                    .field()
                    .menu
                    .as_ref()
                    .unwrap()
                    .party(),
            )?;
            world
                .resource_mut::<Session>()
                .field_mut()
                .menu
                .as_mut()
                .unwrap()
                .page = Page::WorldMap;
            world.run_system_once(advance_live).unwrap();
            world.run_system_once(draw).unwrap();
            world.flush();
            assert!(
                world
                    .resource::<Artwork>()
                    .menu
                    .layers
                    .values()
                    .all(|layer| !layer.visible
                        && world.get::<Visibility>(layer.entity) == Some(&Visibility::Hidden))
            );
            assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
            let menu = world.resource::<Session>().field().menu.as_ref().unwrap();
            assert_eq!(
                menu.page,
                if paranoid { Page::WorldMap } else { Page::Main }
            );
            assert_eq!(serde_json::to_value(menu.party())?, retained);
            assert_eq!(diagnostics.entries().len(), 1);
            assert!(
                diagnostics.entries()[0]
                    .message
                    .contains("empty menu texture")
            );
            if !paranoid {
                world.run_system_once(draw).unwrap();
                world.flush();
                world
                    .resource_mut::<Session>()
                    .field_mut()
                    .menu
                    .as_mut()
                    .unwrap()
                    .closed = true;
                world.run_system_once(advance_live).unwrap();
                assert!(world.resource::<Session>().field().menu.is_none());
                world.run_system_once(draw).unwrap();
                world.run_system_once(advance_live).unwrap();
                assert!(world.resource::<Session>().field().events.tick() > before + 1);
            }
            world.resource_mut::<Messages<AppExit>>().clear();
            {
                let mut live = world.resource_mut::<Session>();
                live.field_mut().menu = None;
                live.field_mut().active_skit = active_skit.take();
            }
            let stale = {
                let mut art = world.resource_mut::<Artwork>();
                art.skits.layers[0].visible = true;
                art.skits.layers[0].entity
            };
            world.entity_mut(stale).insert(Visibility::Visible);
            let before_skit = world.resource::<Session>().field().events.tick();
            world.run_system_once(advance_live).unwrap();
            world.flush();
            assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
            assert_eq!(completion.is_pending(), paranoid);
            assert_eq!(world.get::<Visibility>(stale), Some(&Visibility::Hidden));
            assert_eq!(
                world.resource::<Session>().field().active_skit.is_some(),
                paranoid
            );
            assert!(
                world
                    .resource::<Artwork>()
                    .skits
                    .layers
                    .iter()
                    .all(|layer| !layer.visible)
            );
            if !paranoid {
                caller.step()?;
                assert!(
                    caller.main_finished(),
                    "skit failure stranded its script caller"
                );
                world.run_system_once(draw).unwrap();
                world.run_system_once(advance_live).unwrap();
                assert!(world.resource::<Session>().field().events.tick() > before_skit);
            }
        }
        Ok(())
    }

    #[test]
    fn optional_subtitle_loading_keeps_missing_and_corrupt_cues_local_to_error_policy() -> Result<()>
    {
        use resonance_content::{
            diagnostics::Diagnostics,
            field_preload::{SHARED_PATH, Shared, VERSION},
            prepared::Files,
        };
        let root =
            std::env::temp_dir().join(format!("resonance-subtitle-loading-{}", std::process::id()));
        fs::create_dir_all(&root)?;
        fs::write(
            root.join(SHARED_PATH),
            serde_json::to_vec(&Shared::<resonance_content::field_preload::File> {
                version: VERSION,
                files: BTreeMap::new(),
            })?,
        )?;
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut files = Files::load_with_diagnostics(
                &root,
                &[],
                &mut Default::default(),
                || false,
                diagnostics.clone(),
            )?;
            for bytes in [
                None,
                Some(b"{".as_slice()),
                Some(br#"{"version":0,"movie":1,"cues":[]}"#.as_slice()),
            ] {
                files.remove("ui/story-subtitles.json");
                if let Some(bytes) = bytes {
                    files.insert("ui/story-subtitles.json".into(), bytes.into());
                }
                let result = subtitle_cues(&files);
                if paranoid {
                    assert!(result.is_err());
                } else {
                    assert!(result?.is_empty());
                }
            }
            assert_eq!(diagnostics.entries().len(), 3);
            assert!(
                diagnostics
                    .entries()
                    .iter()
                    .all(|entry| entry.scope == "movie subtitle cues")
            );
            files.insert("ui/story-subtitles.json".into(), br#"{"version":1,"movie":1,"cues":[{"frame":1,"lines":[{"position":[0,0],"text":"Ready"}]}]}"#.as_slice().into());
            let cues = subtitle_cues(&files)?;
            assert_eq!(cues.len(), 1);
            assert_eq!(cues[0].lines[0].text, "Ready");
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn invalid_subtitles_hide_stale_text_and_honor_error_policy() {
        use bevy::ecs::system::RunSystemOnce;
        use resonance_content::font::{Glyph, SubtitleCue, SubtitleLine};
        for paranoid in [false, true] {
            for missing_mesh in [false, true] {
                let mut world = World::new();
                let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
                world.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
                world.init_resource::<Messages<AppExit>>();
                world.init_resource::<Assets<Mesh>>();
                let entity = world.spawn(Visibility::Inherited).id();
                world
                    .run_system_once(
                        move |mut commands: Commands,
                              mut meshes: ResMut<Assets<Mesh>>,
                              mut failures: crate::field_view::Failures| {
                            let font = BitmapFont {
                                version: 1,
                                texture: "font.png".into(),
                                width: 1,
                                height: 1,
                                line_height: 1,
                                source_sha256: String::new(),
                                executable_sha256: String::new(),
                                glyphs: if missing_mesh {
                                    [(
                                        'A',
                                        Glyph {
                                            rect: [0, 0, 1, 1],
                                            advance: 1,
                                        },
                                    )]
                                    .into()
                                } else {
                                    Default::default()
                                },
                            };
                            let cue = SubtitleCue {
                                frame: 1,
                                lines: vec![SubtitleLine {
                                    position: [0.; 2],
                                    text: "A".into(),
                                }],
                            };
                            let mut layer = Some(Layer {
                                entity,
                                mesh: Handle::default(),
                                material: Handle::default(),
                                uploaded: None,
                                visible: true,
                            });
                            draw_subtitle(
                                &cue,
                                &font,
                                &mut layer,
                                &mut commands,
                                &mut meshes,
                                &mut failures,
                            );
                        },
                    )
                    .unwrap();
                world.flush();
                assert_eq!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
                assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
                assert!(diagnostics.entries()[0].message.contains(if missing_mesh {
                    "retained UI mesh was removed"
                } else {
                    "uncooked subtitle glyph"
                }));
            }
        }
    }

    #[test]
    fn empty_layer_hides_without_uploading_empty_geometry_and_can_show_again() {
        let mut world = World::new();
        let entity = world.spawn(Visibility::Hidden).id();
        let mut meshes = Assets::<Mesh>::default();
        let mut warm = Batch::default();
        warm.quad([0., 0., 1., 1.], [0.; 4], [1.; 4]);
        let mesh = meshes.add(warm.clone().mesh([1, 1]));
        let mut layer = Layer {
            entity,
            mesh: mesh.clone(),
            material: Handle::default(),
            uploaded: None,
            visible: false,
        };
        for previous in [None, Some(warm.clone())] {
            if let Some(batch) = previous {
                layer.update_mesh(batch, [1, 1], &mut meshes).unwrap();
                layer.show(true, &mut world.commands());
                world.flush();
                assert_eq!(
                    world.get::<Visibility>(entity),
                    Some(&Visibility::Inherited)
                );
            }
            let packed = meshes
                .get(&mesh)
                .unwrap()
                .create_packed_vertex_buffer_data();
            layer
                .update_mesh(Batch::default(), [1, 1], &mut meshes)
                .unwrap();
            // Some callers, such as a skit's text panel, request visibility
            // even when their logical batch is empty.
            layer.show(true, &mut world.commands());
            world.flush();
            assert_eq!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
            assert_eq!(
                meshes
                    .get(&mesh)
                    .unwrap()
                    .create_packed_vertex_buffer_data(),
                packed
            );
            assert_eq!(meshes.get(&mesh).unwrap().indices().unwrap().len(), 6);
        }
        let mut visible = Batch::default();
        visible.quad([20., 30., 60., 90.], [0., 0., 8., 16.], [0.5; 4]);
        let expected = visible
            .clone()
            .mesh([8, 16])
            .create_packed_vertex_buffer_data();
        layer.update_mesh(visible, [8, 16], &mut meshes).unwrap();
        layer.show(true, &mut world.commands());
        world.flush();
        assert_eq!(
            world.get::<Visibility>(entity),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            meshes
                .get(&mesh)
                .unwrap()
                .create_packed_vertex_buffer_data(),
            expected
        );
        meshes.remove(mesh.id());
        assert!(
            layer
                .update_mesh(Batch::default(), [1, 1], &mut meshes)
                .is_err()
        );
    }

    #[test]
    fn ordinary_surface_keeps_vertices_without_secondary_stream() {
        let mut batch = Batch::default();
        batch.quad(
            [10., 20., 30., 40.],
            [2., 4., 6., 8.],
            [0.25, 0.5, 0.75, 1.],
        );
        let mesh = batch.mesh([8, 16]);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_none());
        let bevy::mesh::VertexAttributeValues::Float32x2(uv) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap()
        else {
            panic!("wrong UV format")
        };
        assert_eq!(uv, &[[0.25, 0.25], [0.75, 0.25], [0.75, 0.5], [0.25, 0.5]]);
        let image = Handle::<Image>::default();
        let surface = Surface {
            source: image.clone(),
            sampling: image.clone(),
            frame_mask: image.clone(),
            color_mask: image,
            coverage: Coverage::default(),
            opaque: false,
            additive: false,
            red_channel: false,
        };
        assert_eq!(SurfaceKey::from(&surface), SurfaceKey(false, false, false));
    }

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

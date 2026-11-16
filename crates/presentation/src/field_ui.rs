//! Bitmap dialogue composition from cooked images and high-level text state.
use crate::native::MeshMaterial2d;
#[path = "battle_ui.rs"]
mod battle_ui;
mod bitmap_text;
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
pub(super) use battle_ui::{
    ActorMarkers, Artwork as BattleHud, EnemyHud as BattleEnemyHud, HudTime, PartyHudInput,
    RibbonInput, TransitionPrediction,
};
#[path = "game_over_ui.rs"]
mod game_over_ui;
pub(super) use game_over_ui::Artwork as GameOverArt;
#[path = "field_ui_menu.rs"]
mod menu;
pub(crate) mod resident;
#[cfg(test)]
pub(super) use menu::MenuDraws;
pub(crate) use resident::Resident;
pub(super) fn install(app: &mut App) {
    app.init_resource::<Resident>();
    crate::native::init_material::<Surface>(app);
    crate::input_icons::install(app);
    skit::install(app);
    app.add_systems(
        PreUpdate,
        crate::input_icons::track_device.after(bevy::input::InputSystems),
    );
    menu::install(app);
}
#[path = "field_ui_overlay.rs"]
mod overlay;
pub(super) fn model_preview_depth() -> f32 {
    menu::model_preview_depth()
}
#[path = "field_ui_pause.rs"]
pub(crate) mod pause;
#[path = "field_ui_prompt.rs"]
mod prompt;
#[path = "field_ui_skit.rs"]
mod skit;
#[path = "field_ui_world.rs"]
mod world;
use anyhow::{Context as ContextExt, Result};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    mesh::VertexBufferLayout,
    prelude::*,
    render::render_resource::{
        BlendComponent, BlendFactor, BlendOperation, BlendState, ShaderType, VertexAttribute,
        VertexFormat, VertexStepMode,
    },
};
use coverage::Coverage;
use resonance_content::font::{BitmapFont, DialogueArt};
use resonance_events::dialogue::{DIALOGUE_SLOTS, Dialogue, DialogueAnchor, TextToken, flags};
use resonance_game::{dialogue::DialoguePlayer, field::FieldSession};
use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
pub(super) use world::Artwork as WorldArtwork;

/// Decode before activation. Callers decide whether failure is fatal or optional.
fn ui_image(
    path: &str,
    size: [u32; 2],
    repeat: bool,
    formats: bevy::image::CompressedImageFormats,
    read: &impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Image> {
    let address = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    anyhow::ensure!(
        size.iter().all(|v| (1..=4096).contains(v)),
        "invalid UI image coordinates: {path}"
    );
    let bytes = read(path)?;
    let image = crate::texture::decode(
        path,
        &ImageLoaderSettings {
            is_srgb: false,
            sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: address,
                address_mode_v: address,
                ..ImageSamplerDescriptor::linear()
            }),
            asset_usage: RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            ..default()
        },
        formats,
        bytes,
    )
    .with_context(|| format!("UI image {path}"))?;
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
    pub const TOP_FRAME: usize = 7; // Speaker tab and advance icon.
    pub const FONT: usize = 8;
    // Cooked atlas indices: fill, frame, color overlay, font and cursor.
    pub const CURSOR: usize = 9;
    pub const TEXTURES: [usize; 10] = [7, 7, 0, 1, 0, 0, 1, 0, 9, 10];
}

#[derive(Asset, TypePath, Debug, Clone)]
pub(super) struct Surface {
    source: Handle<Image>,
    /// Reuse another prepared image's sampler without duplicating its pixels.
    sampling: Handle<Image>,
    frame_mask: Handle<Image>,
    color_mask: Handle<Image>,
    coverage: Coverage,
    interpolation: UiInterpolation,
    opaque: bool,
    additive: bool,
    texture_effect: TextureEffect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum UiInterpolation {
    /// Retain pixel-center lookups for bitmap text.
    Center,
    /// Keep artwork lookups inside the primitive and its atlas cell.
    Centroid,
    /// Evaluate dialogue textures and clipping masks at matching MSAA samples.
    Sample,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum TextureEffect {
    #[default]
    Plain,
    /// Channel swap: both texture and raster become [R,R,R,A].
    RedChannel,
    /// A translucent atlas pattern clipped by the glyph's alpha.
    GlyphPattern,
}
impl Surface {
    /// Source, sampler source, frame mask and color mask images; the pipeline
    /// key; and the coverage table when it masks anything.
    pub(crate) fn native(&self) -> ([AssetId<Image>; 4], SurfaceKey) {
        (
            [
                self.source.id(),
                self.sampling.id(),
                self.frame_mask.id(),
                self.color_mask.id(),
            ],
            SurfaceKey::from(self),
        )
    }
    /// Append the coverage table's uniform bytes when it masks anything.
    pub(crate) fn native_coverage(&self, bytes: &mut Vec<u8>) -> bool {
        if !self.coverage.is_active() {
            return false;
        }
        let start = bytes.len();
        bytes.resize(start + Coverage::min_size().get() as usize, 0);
        bevy::render::render_resource::encase::UniformBuffer::new(&mut bytes[start..])
            .write(&self.coverage)
            .expect("coverage encoding");
        true
    }
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
pub(super) struct SurfaceKey(bool, bool, TextureEffect, UiInterpolation);
/// Pipeline state of a UI surface for the native display-list renderer.
pub(crate) struct NativeUiState {
    pub defs: Vec<&'static str>,
    pub layout: VertexBufferLayout,
    pub blend: BlendState,
}

impl SurfaceKey {
    /// The shader definitions and blending this surface draws with.
    pub(crate) fn native_state(self, samples: u32) -> NativeUiState {
        let mut defs = Vec::new();
        if samples > 1 {
            match self.3 {
                UiInterpolation::Center => {}
                UiInterpolation::Centroid => defs.push("CENTROID_UI"),
                UiInterpolation::Sample => defs.push("MULTISAMPLED_UI"),
            }
        }
        if self.2 == TextureEffect::RedChannel {
            defs.push("RED_CHANNEL");
        }
        if self.2 == TextureEffect::GlyphPattern {
            defs.push("GLYPH_PATTERN");
        }
        if self.3 == UiInterpolation::Centroid {
            defs.push("CLAMP_UI_ATLAS");
        }
        if self.0 {
            defs.push("OPAQUE_IMAGE");
        }
        let additive = BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        NativeUiState {
            defs,
            layout: UiVertex::layout(),
            blend: if self.1 {
                BlendState {
                    color: additive,
                    alpha: additive,
                }
            } else {
                BlendState::ALPHA_BLENDING
            },
        }
    }
}

impl From<&Surface> for SurfaceKey {
    fn from(surface: &Surface) -> Self {
        Self(
            surface.opaque,
            surface.additive,
            surface.texture_effect,
            surface.interpolation,
        )
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
    choice_trail: super::choice_cursor::Trail,
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
#[derive(Clone)]
struct Layer {
    entity: Entity,
    geometry: UiGeometry,
    material: Handle<Surface>,
    uploaded: Option<(Batch, [u32; 2])>,
    visible: bool,
}

/// The same menu renderer can be used before a field session exists.
#[derive(Resource)]
pub(super) struct MenuOverlay {
    font: BitmapFont,
    dialogue: DialogueArt,
    artwork: menu::MenuArtwork,
}
impl MenuOverlay {
    pub(crate) fn set_prompt_device(&mut self, device: crate::input_icons::PromptDevice) {
        self.artwork.set_prompt_device(device);
    }
    pub fn load_with(
        read: impl Fn(&str) -> Result<Vec<u8>>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
        resident: &Resident,
    ) -> Result<Self> {
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        dialogue.prepare_images(&read)?;
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
            interpolation: UiInterpolation::Center,
            additive: false,
            texture_effect: Default::default(),
            opaque: false,
        });
        let inputs = crate::input_icons::prepare(&read, server)?;
        let artwork = menu::MenuArtwork::load(
            read,
            Default::default(),
            server,
            materials,
            (&surface, [font.width, font.height]),
            &dialogue.cursor,
            diagnostics,
            inputs,
            resident,
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
        prediction: Option<&mut crate::field_view::Prediction>,
        commands: &mut Commands,
    ) -> Result<()> {
        self.artwork.render(
            menu::Source::Title(menu),
            &self.font,
            &self.dialogue,
            presentation_tick,
            resolution,
            prediction,
            commands,
        )
    }
    pub fn despawn(self, world: &mut World) {
        for layer in self.artwork.layers.into_values() {
            resident::retire(world, layer.entity);
        }
    }
    pub fn hide(&mut self, commands: &mut Commands) {
        self.artwork.clear_page(commands);
    }
}
impl Layer {
    fn new(material: Handle<Surface>, depth: f32, commands: &mut Commands) -> Self {
        Self::with_batch(material, depth, Batch::default(), [1, 1], commands)
    }
    fn with_batch(
        material: Handle<Surface>,
        depth: f32,
        batch: Batch,
        size: [u32; 2],
        commands: &mut Commands,
    ) -> Self {
        let geometry = batch.geometry(size);
        let transform = Transform::from_xyz(0., 0., depth);
        let entity = commands
            .spawn((
                geometry.clone(),
                MeshMaterial2d(material.clone()),
                transform,
                GlobalTransform::from(transform),
                Visibility::Hidden,
            ))
            .id();
        Self {
            entity,
            geometry,
            material,
            uploaded: Some((batch, size)),
            visible: false,
        }
    }
    fn update(&mut self, batch: Batch, size: [u32; 2]) {
        if self
            .uploaded
            .as_ref()
            .is_none_or(|(old, old_size)| *old != batch || *old_size != size)
        {
            self.geometry.replace(batch.vertices(size));
            self.uploaded = Some((batch, size));
        }
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
    fn common_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers
            .values()
            .chain(&self.subtitle_layer)
            .chain(&self.fade_layer)
            .chain(&self.prompt_layers)
            .chain(&self.damage_layer)
            .chain(&self.skits.layers)
            .chain(&self.skits.warm)
    }
    pub(crate) fn set_prompt_device(&mut self, device: crate::input_icons::PromptDevice) {
        self.menu.set_prompt_device(device);
    }
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
            resident::retire(world, layer.entity);
        }
        if let Some(layer) = self.fade_layer.take() {
            resident::retire(world, layer.entity);
        }
        if let Some(layer) = self.subtitle_layer.take() {
            resident::retire(world, layer.entity);
        }
        self.choice_trail = Default::default();
        self.head_heights.clear();
        self.attached_positions.clear();
    }
    pub fn load_with(
        field: &resonance_content::field::FieldAssets,
        session: std::sync::Arc<resonance_content::session::SessionData>,
        skits: std::sync::Arc<resonance_content::skit::SkitCatalog>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
        files: &resonance_content::prepared::Files,
        resident: &Resident,
    ) -> Result<Self> {
        let mut art = Self::load_shared(
            files,
            skits,
            session.experience.clone().into(),
            &field.overlays,
            server,
            materials,
            image_assets,
            resident,
        )?;
        art.subtitles = subtitle_cues(files)?;
        if resonance_content::credits::Manifest::captured(files) {
            art.credits = Some(credits::Artwork::load(
                files,
                &art.font,
                &art.surfaces[9],
                server,
                materials,
            )?);
        }
        {
            let game_over = resonance_game::game_over::Assets::from_prepared(files.clone())?;
            for path in [&game_over.art.background.path, &game_over.font.texture] {
                if files.contains_key(path) {
                    art.images.push(
                        server
                            .load_builder()
                            .with_settings(|settings: &mut ImageLoaderSettings| {
                                settings.is_srgb = false
                            })
                            .load(path.clone()),
                    );
                }
            }
        }
        Ok(art)
    }
    fn load_shared(
        files: &resonance_content::prepared::Files,
        skits: std::sync::Arc<resonance_content::skit::SkitCatalog>,
        experience: std::sync::Arc<[u32]>,
        overlays: &BTreeMap<i32, String>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
        resident: &Resident,
    ) -> Result<Self> {
        if let Some(art) = resident.field(files, overlays, server, materials, image_assets)? {
            return Ok(art);
        }
        let read = |path: &str| -> Result<Vec<u8>> { Ok(files.read(path)?.to_vec()) };
        let spec: DialogueArt = serde_json::from_slice(
            &read("ui/dialogue.json").context("classroom dialogue art is missing; run cook-all")?,
        )?;
        spec.prepare_images(&read)?;
        let font: BitmapFont = serde_json::from_slice(&read(&spec.font)?)?;
        font.validate()?;
        let mut images: Vec<Handle<Image>> = spec
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
                    interpolation: UiInterpolation::Center,
                    additive: false,
                    texture_effect: Default::default(),
                    opaque: false,
                })
            })
            .collect();
        let inputs = crate::input_icons::prepare(&read, server)?;
        let menu = menu::MenuArtwork::load(
            read,
            experience,
            server,
            materials,
            (&surfaces[9], [font.width, font.height]),
            &spec.cursor,
            files.diagnostics(),
            inputs.clone(),
            resident,
        )?;
        let skits = skit::Artwork::load(skits, server, materials, &surfaces[9], image_assets)?;
        let (fade_image, fade_surface) = fade::surface(materials, image_assets);
        images.push(fade_image);
        images.push(menu.input_image());
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
            choice_trail: Default::default(),
            subtitles: Vec::new(),
            subtitle_layer: None,
            fade_surface,
            fade_layer: None,
            overlays: overlay::Artwork::load(
                overlays,
                read,
                server,
                materials,
                image_assets,
                &inputs,
            )?,
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_menu(
        &mut self,
        session: &FieldSession,
        tick: u32,
        prediction: Option<&mut crate::field_view::Prediction>,
        commands: &mut Commands,

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
                prediction,
                commands,
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
        prediction: Option<&crate::field_view::Prediction>,
        commands: &mut Commands,

        images: &mut Assets<Image>,
        materials: &mut Assets<Surface>,
        server: &AssetServer,
    ) -> Result<bool> {
        self.skit_ready(session, images, server).and_then(|ready| {
            if !ready {
                return Ok(false);
            }
            self.skits.render(
                session.active_skit.as_ref(),
                prediction.and_then(|p| p.field()?.active_skit.as_ref().map(|s| (s, p.alpha))),
                &self.font,
                self.resolution,
                commands,
                images,
                materials,
            )?;
            Ok(true)
        })
    }
    pub fn menu_settled(&self, menu: &resonance_game::menu::Menu, tick: u32) -> bool {
        self.menu.visually_settled(menu, tick)
    }
    /// Allocate every supported dialogue slot/layer before its first request.
    pub(super) fn prepare(&mut self, commands: &mut Commands, materials: &mut Assets<Surface>) {
        self.skits.prepare(commands);
        if let Some(credits) = &mut self.credits {
            credits.prepare(commands);
        }
        self.prepare_prompt(commands, materials);
        self.prepare_damage(commands);
        self.overlays.prepare(commands);
        self.prepare_fade(commands);
        prepare_dialogue(&mut self.layers, &self.surfaces, commands, materials);
        if self.subtitle_layer.is_none() {
            let layer = Layer::new(
                self.surfaces[layer::TEXTURES[layer::FONT]].clone(),
                1.,
                commands,
            );
            commands
                .entity(layer.entity)
                .insert(bevy::camera::visibility::RenderLayers::layer(3));
            self.subtitle_layer = Some(layer);
        }
    }
    pub(super) fn prepared_layers(&self) -> impl Iterator<Item = (&Handle<Surface>, bool)> {
        self.layers
            .values()
            .chain(self.subtitle_layer.iter())
            .chain(self.fade_layer.iter())
            .chain(&self.overlays.layers)
            .chain(&self.overlays.warm)
            .chain(&self.prompt_layers)
            .chain(self.damage_layer.iter())
            .chain(self.credits.iter().flat_map(|credits| &credits.layers))
            .map(|layer| (&layer.material, true))
            .chain(
                self.menu
                    .layers
                    .values()
                    .chain(&self.skits.layers)
                    .chain(&self.skits.warm)
                    .map(|layer| (&layer.material, false)),
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
                        height,
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
        prediction: Option<&mut crate::field_view::Prediction>,
        commands: &mut Commands,
    ) -> Result<Vec<i32>> {
        self.overlays
            .render(world, prediction, commands, self.menu.prompt_device())
    }
    pub fn render(
        &mut self,
        session: &FieldSession,
        mut prediction: Option<&mut crate::field_view::Prediction>,
        heads: &BTreeMap<i32, Vec3>,
        commands: &mut Commands,

        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        if let Some(credits) = &mut self.credits {
            credits.render(
                session.events.world.screen_request.as_ref(),
                prediction.as_ref().map_or(0., |p| p.alpha),
                commands,
            );
        }
        self.render_fade(&session.events.world, prediction.as_deref(), commands)?;
        self.render_prompt(session, prediction.as_deref_mut(), commands)?;
        self.render_damage(session, prediction.as_deref(), commands);
        let (world, dialogue) = session.dialogue_scene();
        self.render_dialogue(
            world,
            dialogue,
            &session.events.world,
            prediction.as_deref(),
            world.tick,
            heads,
            commands,
            materials,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn render_dialogue(
        &mut self,
        world: &resonance_events::GameWorld,
        dialogue: &BTreeMap<u8, DialoguePlayer>,
        camera_world: &resonance_events::GameWorld,
        prediction: Option<&crate::field_view::Prediction>,
        presentation_tick: u32,
        heads: &BTreeMap<i32, Vec3>,
        commands: &mut Commands,

        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        let future = prediction.and_then(|p| {
            let players = p
                .field()
                .map(|f| f.dialogue_scene().1)
                .or_else(|| p.world()?.session.active_skit.as_ref().map(|s| &s.dialogue))?;
            let next_world = p.field().map_or(camera_world, |f| &f.events.world);
            let continuous = camera_world
                .field_camera
                .as_ref()
                .is_none_or(|c| p.camera(c).is_some());
            Some(DialogueEndpoint {
                players,
                camera: continuous.then(|| DialogueCamera::field(next_world)),
                alpha: p.alpha,
            })
        });
        DialogueDraw {
            resolution: self.resolution,
            attached_positions: &self.attached_positions,
            font: &self.font,
            spec: &self.spec,
            images: &self.images,
            layers: &mut self.layers,
            head_heights: &mut self.head_heights,
            choice_trail: &mut self.choice_trail,
            menu: &self.menu,
        }
        .render(
            world,
            dialogue,
            DialogueCamera::field(camera_world),
            future,
            presentation_tick,
            heads,
            commands,
            materials,
        )
    }
}

/// Scene-owned dialogue surfaces using the same layout and drawing as fields.
/// Battle borrows no retained field entities or attachment history.
#[derive(Clone)]
pub(super) struct DialogueOverlay {
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    layers: BTreeMap<(u8, usize), Layer>,
    head_heights: BTreeMap<u64, f32>,
    choice_trail: super::choice_cursor::Trail,
}

impl DialogueOverlay {
    fn load(
        spec: &DialogueArt,
        font: &BitmapFont,
        files: &resonance_content::prepared::Files,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
        formats: bevy::image::CompressedImageFormats,
    ) -> Result<Self> {
        let read = |path: &str| Ok(files.read(path)?.to_vec());
        let images = spec
            .textures
            .iter()
            .map(|texture| (texture.path.as_str(), [texture.width, texture.height]))
            .chain([
                (font.texture.as_str(), [font.width, font.height]),
                (
                    spec.cursor.path.as_str(),
                    [spec.cursor.width, spec.cursor.height],
                ),
            ])
            .enumerate()
            .map(|(index, (path, size))| {
                let mut image = ui_image(path, size, true, formats, &read)?;
                if index < 9 {
                    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::nearest()
                    });
                }
                Ok(image_assets.add(image))
            })
            .collect::<Result<Vec<_>>>()?;
        let surfaces = images
            .iter()
            .map(|image| {
                materials.add(Surface {
                    source: image.clone(),
                    sampling: image.clone(),
                    frame_mask: images[0].clone(),
                    color_mask: images[1].clone(),
                    coverage: Coverage::default(),
                    interpolation: UiInterpolation::Center,
                    opaque: false,
                    additive: false,
                    texture_effect: Default::default(),
                })
            })
            .collect();
        Ok(Self {
            images,
            surfaces,
            layers: BTreeMap::new(),
            head_heights: BTreeMap::new(),
            choice_trail: Default::default(),
        })
    }
    pub(super) fn prepare(&mut self, commands: &mut Commands, materials: &mut Assets<Surface>) {
        prepare_dialogue(&mut self.layers, &self.surfaces, commands, materials);
    }
    pub(super) fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.layers.values().map(|layer| layer.entity)
    }

    pub(super) fn hide(&mut self, commands: &mut Commands) {
        for layer in self.layers.values_mut() {
            layer.show(false, commands);
        }
        self.head_heights.clear();
        self.choice_trail = Default::default();
    }
    pub(super) fn despawn(self, world: &mut World) {
        for entity in self.entities() {
            resident::retire(world, entity);
        }
    }
}

fn prepare_dialogue(
    layers: &mut BTreeMap<(u8, usize), Layer>,
    surfaces: &[Handle<Surface>],
    commands: &mut Commands,

    materials: &mut Assets<Surface>,
) {
    for slot in 0..DIALOGUE_SLOTS {
        for (index, texture) in layer::TEXTURES.into_iter().enumerate() {
            if layers.contains_key(&(slot, index)) {
                continue;
            }
            let mut template = materials.get(&surfaces[texture]).unwrap().clone();
            // Only sliced windows need sample-frequency masking. Bitmap text
            // and cursors retain pixel-center sampling, even on MSAA UI cameras.
            template.interpolation = if index < layer::FONT {
                UiInterpolation::Sample
            } else {
                UiInterpolation::Center
            };
            let material = materials.add(template);
            layers.insert(
                (slot, index),
                Layer::new(
                    material,
                    10. + f32::from(slot) * 20. + index as f32,
                    commands,
                ),
            );
        }
    }
}

struct DialogueEndpoint<'a> {
    players: &'a BTreeMap<u8, DialoguePlayer>,
    camera: Option<DialogueCamera<'a>>,
    alpha: f32,
}

struct DialogueDraw<'a> {
    resolution: super::Resolution,
    attached_positions: &'a BTreeMap<i32, Vec3>,
    font: &'a BitmapFont,
    spec: &'a DialogueArt,
    images: &'a [Handle<Image>],
    layers: &'a mut BTreeMap<(u8, usize), Layer>,
    head_heights: &'a mut BTreeMap<u64, f32>,
    choice_trail: &'a mut super::choice_cursor::Trail,
    menu: &'a menu::MenuArtwork,
}
impl DialogueDraw<'_> {
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        world: &resonance_events::GameWorld,
        dialogue: &BTreeMap<u8, DialoguePlayer>,
        camera: DialogueCamera,
        future: Option<DialogueEndpoint>,
        presentation_tick: u32,
        heads: &BTreeMap<i32, Vec3>,
        commands: &mut Commands,

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
                && let Some(head) = heads.get(&id)
                && let Some(position) = camera.speaker_position(id, self.attached_positions)
            {
                // Measure once so animated head motion does not move the box.
                self.head_heights
                    .entry(request.operation.id())
                    .or_insert(((*head - position).dot(camera.up)).trunc() + HEAD_ANCHOR_PADDING);
            }
        }
        for (&slot, player) in dialogue {
            let Some(request) = displayed_dialogue(player, world.dialogue.get(&slot)) else {
                continue;
            };
            let (rect, mut pointer, above) = layout_with_camera(
                self.font,
                request,
                player,
                &camera,
                self.attached_positions,
                self.head_heights.get(&request.operation.id()).copied(),
                self.resolution,
            )?;
            let opening = player.opening_fraction();
            let expand = |rect: [f32; 4], pointer: Option<[f32; 2]>, fraction: Option<f32>| {
                fraction.map_or(rect, |fraction| {
                    expanding_rect(
                        rect,
                        pointer.unwrap_or([
                            rect[0] + ((rect[2] - rect[0]) / 2.).trunc(),
                            rect[1] + ((rect[3] - rect[1]) / 2.).trunc(),
                        ]),
                        fraction,
                    )
                })
            };
            let committed_rect = expand(rect, pointer, opening);
            let mut next_rect = committed_rect;
            let mut rect = committed_rect;
            if let Some(endpoint) = &future
                && let Some(next_camera) = &endpoint.camera
                && let Some(next) = endpoint
                    .players
                    .get(&slot)
                    .filter(|n| n.operation.id() == player.operation.id() && n.page == player.page)
            {
                let camera_world = camera.world;
                let next_world = next_camera.world;
                let same_actor = request.speaker_actor.is_none_or(|id| {
                    match (camera_world.actors.get(&id), next_world.actors.get(&id)) {
                        (Some(a), Some(b)) => crate::field_view::prediction::continuous_actor(a, b),
                        (None, None) => true,
                        _ => false,
                    }
                });
                if same_actor {
                    let (target, target_pointer, target_above) = layout_with_camera(
                        self.font,
                        request,
                        next,
                        next_camera,
                        self.attached_positions,
                        self.head_heights.get(&request.operation.id()).copied(),
                        self.resolution,
                    )?;
                    // Switching sides of the speaker is a discrete layout change.
                    if above == target_above {
                        next_rect = expand(target, target_pointer, next.opening_fraction());
                        rect = std::array::from_fn(|i| rect[i].lerp(next_rect[i], endpoint.alpha));
                        pointer = pointer.map(|from| {
                            target_pointer.map_or(from, |to| {
                                std::array::from_fn(|i| {
                                    from[i] + (to[i] - from[i]) * endpoint.alpha
                                })
                            })
                        });
                    }
                }
            }
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
                    self.font,
                    pointer.filter(|_| opening.is_none()),
                    (request.flags & !flags::PLACEMENT)
                        | if above { flags::ABOVE } else { flags::BELOW },
                    preferences,
                );
            }
            // Mask fills with the frame artwork: even translucent ornaments own
            // their pixels and must not receive a second layer of background color.
            let speaker_coverage =
                Coverage::frame(&batches[layer::TOP_FRAME], self.spec, opening.is_some())?;
            let speaker_fill_coverage = speaker_coverage.with_color(
                &batches[layer::SPEAKER_FILL],
                self.spec,
                opening.is_some(),
            )?;
            let corner_coverage = speaker_fill_coverage.with_frame(
                &batches[layer::CORNERS],
                self.spec,
                opening.is_some(),
            )?;
            let pointer_coverage = corner_coverage.with_frame(
                &batches[layer::POINTER],
                self.spec,
                opening.is_some(),
            )?;
            let ornament_coverage = pointer_coverage.with_color(
                &batches[layer::POINTER_FILL],
                self.spec,
                opening.is_some(),
            )?;
            let frame_coverage = ornament_coverage.with_frame(
                &batches[layer::FRAME],
                self.spec,
                opening.is_some(),
            )?;
            let fill_coverage =
                frame_coverage.with_solid(&batches[layer::BEVEL], self.spec, opening.is_some())?;
            let [left, top, _, _] = rect;
            if let Some(choice) = world.choices.get(&slot)
                && let Some(lines) = choice.selection.lines()
                && player.page + 1 == player.pages.len()
                && player.fully_revealed()
                && (player.accepts_input() || !choice.operation.is_pending())
            {
                let y = super::choice_cursor::drawing_y(top + f32::from(lines.selected_line) * 25.);
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
                let position = |rect: [f32; 4], tick| {
                    let bob = super::choice_cursor::bob(&style, tick);
                    let y = super::choice_cursor::drawing_y(
                        rect[1] + f32::from(lines.selected_line) * 25.,
                    );
                    Vec2::new(rect[0] + bob, y - bob)
                };
                let current = position(committed_rect, presentation_tick);
                let next_tick = presentation_tick.wrapping_add(1);
                let next = position(next_rect, next_tick);
                let fraction = future.as_ref().map_or(0., |endpoint| endpoint.alpha);
                let [x, y] = current.lerp(next, fraction).to_array();
                let slots = self
                    .choice_trail
                    .slots(presentation_tick, current.as_ivec2().to_array());
                let mut predicted_trail = *self.choice_trail;
                let next_slots = predicted_trail.slots(next_tick, next.as_ivec2().to_array());
                for ((from, a), (to, b)) in slots.into_iter().zip(next_slots) {
                    let alpha = f32::from(a).lerp(f32::from(b), fraction);
                    if alpha == 0. {
                        continue;
                    }
                    let [tx, ty] =
                        std::array::from_fn(|i| (from[i] as f32).lerp(to[i] as f32, fraction));
                    batches[layer::CURSOR].quad(
                        overlay([tx + 4. - w, ty + 8., tx + 4., ty + 8. + h]),
                        [0., 0., w, h],
                        [1., 1., 1., alpha / 255.],
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
                let advance = body_advance(glyph.character, spec.advance);
                if let Some(choice) = world.choices.get(&slot)
                    && let resonance_events::dialogue::Selection::Number(number) = &choice.selection
                    && let Some(range) = &player.current().number
                    && index + 1 + usize::from(number.place) == range.end
                    && player.fully_revealed()
                    && (player.accepts_input() || !choice.operation.is_pending())
                {
                    highlight.quad(
                        [x, y, x + advance, y + 25.],
                        [0.5; 4],
                        [0.5, 0.625, 1., 0.5],
                    );
                }
                batches[layer::FONT].quad(
                    [x, y, x + body_glyph_width(glyph.character), y + 25.],
                    glyph_uv(spec.rect),
                    [
                        f32::from(glyph.color[0]) / 255.,
                        f32::from(glyph.color[1]) / 255.,
                        f32::from(glyph.color[2]) / 255.,
                        future
                            .as_ref()
                            .and_then(|endpoint| {
                                let (players, a) = (endpoint.players, endpoint.alpha);
                                let next = players.get(&slot).filter(|p| {
                                    p.operation.id() == player.operation.id()
                                        && p.page == player.page
                                })?;
                                let v = f32::from(player.glyph_alpha(index));
                                Some((v + (f32::from(next.glyph_alpha(index)) - v) * a) / 255.)
                            })
                            .unwrap_or(f32::from(player.glyph_alpha(index)) / 255.),
                    ],
                );
                x += advance;
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
                && player.fully_revealed()
                && player.accepts_input()
                && request.flags & flags::FRAMELESS == 0
                && (player.page + 1 < player.pages.len()
                    || request.flags & flags::TIMED == 0
                        && !world
                            .choices
                            .get(&slot)
                            .is_some_and(|c| c.operation.is_pending()))
            {
                // Add the pulsing icon after the frame masks so the tail shows through it.
                let bottom = frame_top(top, rect[3] - top) + (rect[3] - top).max(48.);
                let pulse = |tick| {
                    let phase = (tick % 90) as f32 * 4.0f32.to_radians();
                    (phase.sin().abs() * 255.).trunc() / 255.
                };
                let opacity = pulse(presentation_tick);
                let opacity = future.as_ref().map_or(opacity, |endpoint| {
                    opacity + (pulse(presentation_tick.wrapping_add(1)) - opacity) * endpoint.alpha
                });
                batches[layer::TOP_FRAME].quad(
                    [rect[2] - 28., bottom, rect[2] - 4., bottom + 24.],
                    if preferences.is_some_and(|s| s.window == 2) {
                        [112., 176., 136., 200.]
                    } else {
                        [224., 120., 248., 144.]
                    },
                    [1., 1., 1., opacity],
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
                layer.update(batch, size);
                layer.show(true, commands);
            }
        }
        for (key, layer) in self.layers.iter_mut() {
            if !used.contains(key) {
                layer.show(false, commands);
            }
        }
        Ok(())
    }
}

struct DialogueCamera<'a> {
    world: &'a resonance_events::GameWorld,
    transform: Option<Transform>,
    fov_degrees: f32,
    up: Vec3,
}
impl<'a> DialogueCamera<'a> {
    fn speaker_position(&self, id: i32, attached: &BTreeMap<i32, Vec3>) -> Option<Vec3> {
        let actor = self.world.field_model_actor(id)?;
        Some(
            attached
                .get(&id)
                .copied()
                .unwrap_or_else(|| Vec3::from_array(actor.field_model_pose().position)),
        )
    }

    fn field(world: &'a resonance_events::GameWorld) -> Self {
        Self {
            world,
            transform: world
                .field_camera
                .as_ref()
                .map(super::field_view::camera_transform),
            fov_degrees: world
                .field_camera
                .as_ref()
                .map_or(0., |camera| camera.fov_degrees()),
            up: Vec3::Z,
        }
    }
    fn battle(
        world: &'a resonance_events::GameWorld,
        frame: &resonance_battle::BattleFrame,
    ) -> Result<Self> {
        let camera = frame.camera.context("battle dialogue has no camera")?;
        Ok(Self {
            world,
            transform: Some(
                Transform::from_translation(Vec3::from_array(camera.eye))
                    .looking_at(Vec3::from_array(camera.focus), Vec3::Y),
            ),
            fov_degrees: frame
                .camera_fov_degrees
                .unwrap_or(resonance_battle::VERTICAL_FOV_DEGREES),
            up: Vec3::Y,
        })
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
        &mut failures,
    );
}

fn draw_subtitle(
    cue: &resonance_content::font::SubtitleCue,
    font: &BitmapFont,
    layer: &mut Option<Layer>,
    commands: &mut Commands,

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
        layer.update(batch, [font.width, font.height]);
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
const MIN_FRAME_HEIGHT: f32 = 48.;
const POINTER_REACH: f32 = 36.;
const FRAME_MARGIN: f32 = 41.;
const HEAD_ANCHOR_PADDING: f32 = 30.;
const HEAD_CLEARANCE: f32 = 12.;

fn frame_top(top: f32, height: f32) -> f32 {
    if height < MIN_FRAME_HEIGHT {
        top + (height / 2.).trunc() - MIN_FRAME_HEIGHT / 2.
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

fn scene_ndc(
    camera: &Transform,
    fov_degrees: f32,
    point: Vec3,
    resolution: super::Resolution,
) -> Vec3 {
    let projection =
        bevy::math::proj::perspective(fov_degrees.to_radians(), resolution.aspect(), 100., 40000.);
    projection.project_point3(camera.to_matrix().inverse().transform_point3(point))
}

/// Apply the fixed pixels' rounding error to the continuously projected point.
fn pixel_aligned_position(from: Vec2, to: Vec2, shown: Vec2, fraction: f32) -> Vec2 {
    if fraction <= 0. {
        from.trunc()
    } else if fraction >= 1. {
        to.trunc()
    } else {
        shown + (from.trunc() - from).lerp(to.trunc() - to, fraction)
    }
}

// The 640×448 scene sits inside the 640×480 overlay.
fn scene_ui_point(ndc: Vec3, resolution: super::Resolution) -> Vec2 {
    let ui = resolution.ui_size();
    Vec2::new(
        320. + ui.x * 0.5 * ndc.x,
        240. - ui.y * 0.5 + ui.y * (224. / 480.) * (1. - ndc.y),
    )
}

fn project_dialogue_point(
    camera: &Transform,
    fov_degrees: f32,
    point: Vec3,
    resolution: super::Resolution,
) -> [f32; 2] {
    let ndc = scene_ndc(camera, fov_degrees, point, resolution);
    if resolution == super::Resolution::default() {
        return [
            (320. * (ndc.x + 1.)).round().clamp(0., 640.),
            (224. * (1. - ndc.y)).round().clamp(0., 480.),
        ];
    }
    let [x, y] = scene_ui_point(ndc, resolution).to_array();
    [x.round().clamp(0., 640.), y.round().clamp(0., 480.)]
}

fn layout(
    font: &BitmapFont,
    request: &Dialogue,
    player: &DialoguePlayer,
    camera_world: &resonance_events::GameWorld,
    attached_positions: &BTreeMap<i32, Vec3>,
    head_height: Option<f32>,
    resolution: super::Resolution,
) -> Result<([f32; 4], Option<[f32; 2]>)> {
    layout_with_camera(
        font,
        request,
        player,
        &DialogueCamera::field(camera_world),
        attached_positions,
        head_height,
        resolution,
    )
    .map(|(rect, pointer, _)| (rect, pointer))
}

fn dialogue_dimensions(font: &BitmapFont, player: &DialoguePlayer) -> Result<[f32; 2]> {
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
                x += body_advance(glyph.character, advance);
            }
        }
        width = width.max(x);
        height = height.max(lines * 25.);
    }
    Ok([width, height])
}

fn layout_with_camera(
    font: &BitmapFont,
    request: &Dialogue,
    player: &DialoguePlayer,
    camera: &DialogueCamera,
    attached_positions: &BTreeMap<i32, Vec3>,
    head_height: Option<f32>,
    resolution: super::Resolution,
) -> Result<([f32; 4], Option<[f32; 2]>, bool)> {
    let [width, height] = match request.dimensions {
        Some(size) => size.map(f32::from),
        None => dialogue_dimensions(font, player)?,
    };
    // Screen anchors remain usable without a scene camera.
    let project = |point| {
        camera.transform.as_ref().map_or([320., 240.], |transform| {
            project_dialogue_point(transform, camera.fov_degrees, point, resolution)
        })
    };
    let actor = request
        .speaker_actor
        .and_then(|id| camera.speaker_position(id, attached_positions));
    let mut pointer = actor
        .filter(|_| {
            request.flags & flags::POINTER != 0
                || matches!(request.anchor, DialogueAnchor::Actor(_))
        })
        .map(|position| project(position + camera.up * 80.));
    let mut above = box_above_speaker(request.flags, pointer);
    // Center integer pixel dimensions without introducing half-pixel offsets.
    // Truncating only after subtraction shifts odd-sized boxes by one pixel.
    let half_width = (width / 2.).trunc();
    let half_height = (height / 2.).trunc();
    let (mut left, mut top, clamp) = match request.anchor {
        DialogueAnchor::ScreenGrid(grid) => {
            let [x, y] = match grid {
                0..=8 => [
                    [106., 318., 530.][(grid % 3) as usize],
                    [80., 240., 400.][(grid / 3) as usize],
                ],
                _ => [320., 240.],
            };
            (x - half_width, y - half_height, true)
        }
        DialogueAnchor::Screen([x, y]) => {
            let [x, y] = if x == 0. && y == 0. {
                [320., 240.]
            } else {
                [x, y]
            };
            (x - half_width, y - half_height, false)
        }
        DialogueAnchor::Actor(_) => {
            let head = actor.map_or([320., 240.], |position| {
                project(
                    position
                        + camera.up
                            * (head_height.unwrap_or(170.) + f32::from(request.height_offset)),
                )
            });
            let mut upper_top = head[1] - 24. - height;
            if above
                && request.flags & flags::FRAMELESS == 0
                && let Some((position, head_height)) = actor.zip(head_height)
            {
                // Keep the intended placement unless the visible tail intrudes
                // into the sampled head's screen-space clearance. Unmeasured
                // anchors, including battle dialogue, retain their placement.
                upper_top = upper_top.clamp(
                    FRAME_MARGIN,
                    (480. - FRAME_MARGIN - height).max(FRAME_MARGIN),
                );
                let head_y = project(position + camera.up * (head_height - HEAD_ANCHOR_PADDING))[1];
                let tip_y =
                    frame_top(upper_top, height) + height.max(MIN_FRAME_HEIGHT) + POINTER_REACH;
                let correction = (tip_y - (head_y - HEAD_CLEARANCE)).max(0.);
                upper_top -= correction;
                // At the upper margin the pointer can reach the speaker.
                // Move below only if the window itself would cover their head.
                if upper_top < FRAME_MARGIN {
                    let frame_bottom =
                        frame_top(FRAME_MARGIN, height) + height.max(MIN_FRAME_HEIGHT) + 16.;
                    above = frame_bottom <= head_y;
                }
            }
            let [x, y] = actor.map_or(
                [320., 240.],
                |position| {
                    if above { head } else { project(position) }
                },
            );
            if let Some(pointer) = &mut pointer {
                // Attached pointers share the box anchor's X. The torso-height
                // probe still selects whether the box belongs above or below.
                pointer[0] = x;
            }
            (
                x - half_width,
                if above { upper_top } else { y + 32. },
                true,
            )
        }
    };
    if clamp {
        left = left.clamp(
            FRAME_MARGIN,
            (640. - FRAME_MARGIN - width).max(FRAME_MARGIN),
        );
        top = top.clamp(
            FRAME_MARGIN,
            (480. - FRAME_MARGIN - height).max(FRAME_MARGIN),
        );
    }
    left = left.trunc();
    top = top.trunc();
    Ok(([left, top, left + width, top + height], pointer, above))
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
    let h = (bottom - top).max(MIN_FRAME_HEIGHT);
    let y0 = frame_top(top, bottom - top);
    let y1 = y0 + h;
    let white = [1.; 4];
    let pointer_art = pointer
        .filter(|[x, _]| x - 16. >= x0 && x + 16. <= x1)
        .map(|[x, _]| {
            if box_above_speaker(flags, pointer) {
                (
                    [x - 16., y1 + 12., x + 16., y1 + POINTER_REACH],
                    [112., 208., 144., 232.],
                    [64., 160., 96., 184.],
                )
            } else {
                (
                    [x - 16., y0 - POINTER_REACH, x + 16., y0 - 12.],
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
            batch[layer::TOP_FRAME].quad(rect, uv, white);
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
    for index in [
        layer::FRAME,
        layer::CORNERS,
        layer::POINTER,
        layer::TOP_FRAME,
    ] {
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
        batch[layer::TOP_FRAME].colors.fill(tab);
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

fn body_glyph_width(character: char) -> f32 {
    if resonance_content::font::is_single_byte(character) {
        21.
    } else {
        25.
    }
}

fn body_advance(character: char, advance: u32) -> f32 {
    (advance as f32 * (body_glyph_width(character) / 25.)).trunc()
}

/// Packed UI vertices retain atlas bounds and glyph coordinates independently of
/// the 3D surface stream. No Mesh asset or render-world repacking is involved.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct UiVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub pattern_uv: [f32; 2],
    pub color: [f32; 4],
    pub bounds: [f32; 4],
}
pub(crate) type UiGeometry = crate::materials::GeometryData<UiVertex>;
impl UiVertex {
    fn layout() -> VertexBufferLayout {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: [
                (
                    0,
                    VertexFormat::Float32x3,
                    std::mem::offset_of!(Self, position),
                ),
                (2, VertexFormat::Float32x2, std::mem::offset_of!(Self, uv)),
                (
                    3,
                    VertexFormat::Float32x2,
                    std::mem::offset_of!(Self, pattern_uv),
                ),
                (
                    4,
                    VertexFormat::Float32x4,
                    std::mem::offset_of!(Self, color),
                ),
                (
                    5,
                    VertexFormat::Float32x4,
                    std::mem::offset_of!(Self, bounds),
                ),
            ]
            .into_iter()
            .map(|(shader_location, format, offset)| VertexAttribute {
                shader_location,
                format,
                offset: offset as u64,
            })
            .collect(),
        }
    }
}

#[derive(Default, Clone, PartialEq)]
struct Batch {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    /// Source cells before geometry clipping, keyed by the quad's first vertex.
    clipped_bounds: BTreeMap<usize, [f32; 4]>,
    pattern_uv: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}
impl Batch {
    /// Geometry and color may move; changing glyphs, atlas frames and topology
    /// remain discrete so input speculation cannot morph unrelated UI content.
    fn interpolate(&mut self, next: &Self, alpha: f32) {
        if self.indices != next.indices
            || self.positions.len() != next.positions.len()
            || self.uv.len() != next.uv.len()
        {
            return;
        }
        for start in (0..self.positions.len()).step_by(4) {
            let end = (start + 4).min(self.positions.len());
            if self.uv[start..end] != next.uv[start..end] {
                continue;
            }
            for i in start..end {
                for axis in 0..3 {
                    self.positions[i][axis] +=
                        (next.positions[i][axis] - self.positions[i][axis]) * alpha;
                }
                for channel in 0..4 {
                    self.colors[i][channel] +=
                        (next.colors[i][channel] - self.colors[i][channel]) * alpha;
                }
            }
        }
    }

    /// Clip axis-aligned quads after composition, including their gradient colours.
    fn clip_vertical(&mut self, start: usize, top: f32, bottom: f32) {
        for i in (start..self.positions.len()).step_by(4) {
            let y0 = self.positions[i][1];
            let y1 = self.positions[i + 2][1];
            if y0 <= y1 {
                continue;
            }
            if y0 > top || y1 < bottom {
                self.clipped_bounds
                    .entry(i)
                    .or_insert_with(|| Self::uv_bounds(&self.uv[i..i + 4]));
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
        self.clipped_bounds.extend(
            other
                .clipped_bounds
                .into_iter()
                .map(|(first, bounds)| (first + offset as usize, bounds)),
        );
        self.pattern_uv.extend(other.pattern_uv);
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
    fn uv_bounds(quad: &[[f32; 2]]) -> [f32; 4] {
        let lower: [f32; 2] = std::array::from_fn(|axis| {
            quad.iter().map(|uv| uv[axis]).fold(f32::INFINITY, f32::min)
        });
        let upper: [f32; 2] = std::array::from_fn(|axis| {
            quad.iter()
                .map(|uv| uv[axis])
                .fold(f32::NEG_INFINITY, f32::max)
        });
        [lower[0], lower[1], upper[0], upper[1]]
    }
    fn vertices(&self, [width, height]: [u32; 2]) -> impl Iterator<Item = UiVertex> + '_ {
        let normalize = move |uv: [f32; 2]| [uv[0] / width as f32, uv[1] / height as f32];
        let mut cell = (usize::MAX, [0.; 4]);
        self.indices.iter().map(move |&index| {
            let index = index as usize;
            // Clipping retains the unclipped cell: fragment clamping must not
            // change the authored UV slope or bleed into a neighbouring glyph.
            let first = index / 4 * 4;
            if cell.0 != first {
                cell = (
                    first,
                    self.clipped_bounds.get(&first).copied().unwrap_or_else(|| {
                        Self::uv_bounds(&self.uv[first..(first + 4).min(self.uv.len())])
                    }),
                );
            }
            let [left, top, right, bottom] = cell.1;
            UiVertex {
                position: self.positions[index],
                uv: normalize(self.uv[index]),
                pattern_uv: normalize(self.pattern_uv.get(index).copied().unwrap_or_default()),
                color: self.colors[index],
                bounds: [
                    left / width as f32,
                    top / height as f32,
                    right / width as f32,
                    bottom / height as f32,
                ],
            }
        })
    }

    fn geometry(&self, size: [u32; 2]) -> UiGeometry {
        let geometry = UiGeometry::default();
        geometry.replace(self.vertices(size));
        geometry
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
        use resonance_content::diagnostics::Diagnostics;
        use resonance_game::menu::Page;
        use std::sync::{Arc, atomic::Ordering};
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        );
        for paranoid in [false, true] {
            let package = Arc::new(crate::new_game::FieldPackage::prepare_with_diagnostics(
                &root,
                332,
                &mut Default::default(),
                || false,
                Diagnostics::new(paranoid),
            )?);
            let mut files = (*package.files).clone();
            let checkpoint = field_checkpoint(&files)?;
            let mut skit_catalog = (*package.skits).clone();
            let mut session = Session::load_prepared(package, Some(checkpoint.clone()), None)?;
            session.audio = None;
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
            let mut menu_spec: resonance_content::menu::MenuArt = files.json("ui/menu.json")?;
            let missing = resonance_content::menu::MenuArt::WORLD_MAP_TEXTURES.start;
            menu_spec.textures.get_mut(&missing).unwrap().path =
                "missing-selected-world-map.ktx2".into();
            let unused = resonance_content::menu::MenuArt::PORTRAIT_TEXTURES.end - 1;
            menu_spec.textures.get_mut(&unused).unwrap().path =
                "missing-unused-menu-portrait.ktx2".into();
            files.insert(
                "ui/menu.json".into(),
                serde_json::to_vec(&menu_spec)?.into(),
            );
            let mut dialogue_spec: DialogueArt = files.json("ui/dialogue.json")?;
            // Keep the captured header available to the layout reader while the
            // real AssetServer below exercises the missing on-disk image.
            let essential_bytes = files.read(&dialogue_spec.textures[0].path)?;
            dialogue_spec.textures[0].path = "missing-essential-dialogue.ktx2".into();
            files.insert(dialogue_spec.textures[0].path.clone(), essential_bytes);
            files.insert(
                "ui/dialogue.json".into(),
                serde_json::to_vec(&dialogue_spec)?.into(),
            );
            let mut app = App::new();
            crate::texture::decoded_files(&mut app, &root);
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
            .init_asset::<WorldAsset>()
            .init_asset::<crate::sparse_animation::Clip>()
            .init_asset::<crate::materials::TitleSurface>()
            .init_asset::<crate::menu_backdrop::Material>()
            .register_asset_loader(bevy::image::ImageLoader::new(
                bevy::image::CompressedImageFormats::NONE,
            ))
            .init_resource::<Controls>()
            .init_resource::<crate::display::Display>()
            .init_resource::<crate::scene::SampledImages>()
            .init_resource::<crate::field_warm::Shared>()
            .add_message::<AppExit>();
            let diagnostics = Diagnostics::new(paranoid);
            app.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
            let resident = crate::loading::Resident::default();
            app.insert_resource(resident);
            let backdrop_mesh = app
                .world_mut()
                .resource_mut::<Assets<Mesh>>()
                .add(Rectangle::new(640., 480.));
            let backdrop_image = app
                .world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::default());
            let backdrop_material = app
                .world_mut()
                .resource_mut::<Assets<crate::menu_backdrop::Material>>()
                .add(crate::menu_backdrop::Material::visible(backdrop_image));
            app.world_mut().spawn((
                crate::menu_backdrop::Quad,
                Mesh2d(backdrop_mesh),
                MeshMaterial2d(backdrop_material),
            ));
            let server = app.world().resource::<AssetServer>().clone();
            let field_art = crate::field_view::prepared_test_art(session.field_assets(), &server);
            let mut images = app.world_mut().remove_resource::<Assets<Image>>().unwrap();
            let mut materials = app
                .world_mut()
                .remove_resource::<Assets<Surface>>()
                .unwrap();
            let mut art = Artwork::load_with(
                session.field_assets(),
                session.data().clone(),
                Arc::new(skit_catalog),
                &server,
                &mut materials,
                &mut images,
                &files,
                &crate::field_ui::Resident::default(),
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
            art.prepare(&mut app.world_mut().commands(), &mut materials);
            app.world_mut().flush();
            app.insert_resource(images)
                .insert_resource(materials)
                .insert_resource(art)
                .insert_resource(field_art)
                .insert_resource(session);
            crate::field_weapons::without_models_for_test(app.world_mut());
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
                .filter(|(_, essential)| *essential)
                .map(|(material, _)| material.clone())
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
                        images: Res<Assets<Image>>,
                        server: Res<AssetServer>,
                        mut failures: crate::field_view::Failures| {
                if let Err(error) =
                    art.render_menu(live.field(), 100, None, &mut commands, &images, &server)
                {
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
                    .contains("missing-selected-world-map.ktx2")
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
            let before_skit = world.resource::<Session>().field().events.tick();
            world.run_system_once(advance_live).unwrap();
            world.flush();
            assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
            assert_eq!(
                world.contains_resource::<crate::field_view::RecoverField>(),
                !paranoid
            );
            // Required active art stops this scene; its owner handles return to
            // title. Do not complete the script as if the skit had played.
            assert!(completion.is_pending());
            assert!(world.resource::<Session>().field().active_skit.is_some());
            assert_eq!(
                world.resource::<Session>().field().events.tick(),
                before_skit
            );
            assert_eq!(
                diagnostics.entries().last().unwrap().scope,
                "field skit artwork"
            );
        }
        Ok(())
    }

    #[test]
    fn optional_subtitle_loading_keeps_missing_and_corrupt_cues_local_to_error_policy() -> Result<()>
    {
        use resonance_content::diagnostics::Diagnostics;
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut files = resonance_content::prepared::Files::new(diagnostics.clone());
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
            assert_eq!(
                diagnostics
                    .entries()
                    .iter()
                    .map(|entry| entry.occurrences)
                    .sum::<u64>(),
                3
            );
            assert!(
                diagnostics
                    .entries()
                    .iter()
                    .all(|entry| entry.scope == "movie subtitle cues")
            );
            files.insert(
                "ui/story-subtitles.json".into(),
                br#"[{"frame":1,"lines":[{"position":[0,0],"text":"Ready"}]}]"#
                    .as_slice()
                    .into(),
            );
            let cues = subtitle_cues(&files)?;
            assert_eq!(cues.len(), 1);
            assert_eq!(cues[0].lines[0].text, "Ready");
        }
        Ok(())
    }

    #[test]
    fn invalid_subtitles_hide_stale_text_and_honor_error_policy() {
        use bevy::ecs::system::RunSystemOnce;
        use resonance_content::font::{Glyph, SubtitleCue, SubtitleLine};
        for paranoid in [false, true] {
            for unprepared in [false, true] {
                let mut world = World::new();
                let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
                world.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
                world.init_resource::<Messages<AppExit>>();
                let entity = world.spawn(Visibility::Inherited).id();
                world
                    .run_system_once(
                        move |mut commands: Commands, mut failures: crate::field_view::Failures| {
                            let font = BitmapFont {
                                texture: "font.png".into(),
                                width: 1,
                                height: 1,
                                line_height: 1,
                                glyphs: if unprepared {
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
                            let mut layer = (!unprepared).then(|| Layer {
                                entity,
                                geometry: UiGeometry::default(),
                                material: Handle::default(),
                                uploaded: None,
                                visible: true,
                            });
                            draw_subtitle(&cue, &font, &mut layer, &mut commands, &mut failures);
                        },
                    )
                    .unwrap();
                world.flush();
                if !unprepared {
                    assert_eq!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
                }
                assert_eq!(world.resource::<Messages<AppExit>>().is_empty(), !paranoid);
                assert!(diagnostics.entries()[0].message.contains(if unprepared {
                    "subtitle layer was not prepared"
                } else {
                    "uncooked subtitle glyph"
                }));
            }
        }
    }

    #[test]
    fn empty_layer_hides_and_can_show_again_without_mesh_assets() {
        let mut world = World::new();
        let mut layer = Layer::new(Handle::default(), 0., &mut world.commands());
        world.flush();
        let mut batch = Batch::default();
        batch.quad([20., 30., 60., 90.], [0., 0., 8., 16.], [0.5; 4]);
        for _ in 0..2 {
            layer.update(batch.clone(), [8, 16]);
            layer.show(true, &mut world.commands());
            world.flush();
            assert_eq!(
                world.get::<Visibility>(layer.entity),
                Some(&Visibility::Inherited)
            );
            let mut frame = crate::materials::GeometryFrame::default();
            world
                .get::<UiGeometry>(layer.entity)
                .unwrap()
                .append_to(&mut frame);
            frame.finish();
            assert_eq!(frame.vertices.len(), 6);
            assert_eq!(frame.vertices[0].position, [-300., 210., 0.]);
            assert_eq!(frame.vertices[1].uv, [1., 1.]);
            layer.update(Batch::default(), [1, 1]);
            layer.show(true, &mut world.commands());
            world.flush();
            assert_eq!(
                world.get::<Visibility>(layer.entity),
                Some(&Visibility::Hidden)
            );
            frame.begin();
            world
                .get::<UiGeometry>(layer.entity)
                .unwrap()
                .append_to(&mut frame);
            frame.finish();
            assert!(frame.vertices.is_empty());
        }
    }

    #[test]
    fn clipped_cells_retain_source_bounds_after_reclipping_and_append() {
        let mut clipped = Batch::default();
        clipped.quad([0., 0., 4., 4.], [4., 4., 0., 0.], [1.; 4]);
        clipped.clip_vertical(0, 239., 237.);
        clipped.clip_vertical(0, 238.5, 237.5);
        assert_eq!(clipped.uv[0], [4., 2.5]);
        assert_eq!(clipped.uv[2], [0., 1.5]);

        let mut batch = Batch::default();
        batch.quad([0., 0., 1., 1.], [0.5; 4], [1.; 4]);
        batch.append(clipped);
        batch.quad([0., 0., 1., 1.], [1., 1., 2., 2.], [1.; 4]);
        let vertices = batch.vertices([4, 4]).collect::<Vec<_>>();
        let bounds = vertices.iter().map(|v| v.bounds).collect::<Vec<_>>();
        assert_eq!(bounds[..6], [[0.125; 4]; 6]);
        assert_eq!(bounds[6..12], [[0., 0., 1., 1.]; 6]);
        assert_eq!(bounds[12..], [[0.25, 0.25, 0.5, 0.5]; 6]);
        assert_eq!(vertices.len(), 18);
    }

    #[test]
    fn streamed_ui_keeps_uvs_and_pattern_coordinates_independent() {
        let mut batch = Batch::default();
        batch.quad(
            [10., 20., 30., 40.],
            [2., 4., 6., 8.],
            [0.25, 0.5, 0.75, 1.],
        );
        let vertices = batch.vertices([8, 16]).collect::<Vec<_>>();
        assert!(vertices.iter().all(|v| v.pattern_uv == [0.; 2]));
        let uv = vertices.iter().map(|v| v.uv).collect::<Vec<_>>();
        assert_eq!(
            uv,
            [
                [0.25, 0.25],
                [0.75, 0.5],
                [0.75, 0.25],
                [0.25, 0.25],
                [0.25, 0.5],
                [0.75, 0.5]
            ]
        );
        batch.pattern_uv = vec![[4., 8.], [8., 8.], [8., 16.], [4., 16.]];
        let pattern_uv = batch
            .vertices([8, 16])
            .map(|v| v.pattern_uv)
            .collect::<Vec<_>>();
        assert_eq!(
            pattern_uv,
            [
                [0.5, 0.5],
                [1., 1.],
                [1., 0.5],
                [0.5, 0.5],
                [0.5, 1.],
                [1., 1.]
            ]
        );
        let image = Handle::<Image>::default();
        let surface = Surface {
            source: image.clone(),
            sampling: image.clone(),
            frame_mask: image.clone(),
            color_mask: image,
            coverage: Coverage::default(),
            interpolation: UiInterpolation::Center,
            opaque: false,
            additive: false,
            texture_effect: Default::default(),
        };
        assert_eq!(
            SurfaceKey::from(&surface),
            SurfaceKey(false, false, TextureEffect::Plain, UiInterpolation::Center)
        );
    }

    #[test]
    fn distant_speaker_gets_an_outlined_pointer_above_the_box() {
        let pointer = Some([312., 167.]);
        assert!(!box_above_speaker(0x40, pointer));
        assert!(box_above_speaker(0x40, Some([312., 176.])));
        assert!(box_above_speaker(0x80, pointer));
        assert!(!box_above_speaker(0x100, pointer));
        let font = BitmapFont {
            texture: String::new(),
            width: 1,
            height: 1,
            line_height: 25,
            glyphs: Default::default(),
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
    fn dialogue_layout_sizes_mixed_text_and_clears_actor_heads() -> Result<()> {
        use resonance_events::{Actor, GameWorld, dialogue::ResolvedMessage};
        let mut world = GameWorld::default();
        let mut speaker = Actor::new(0, [0.; 3]);
        speaker.sample_field_model(false);
        // The script moves its speaker after the model and its head were sampled.
        speaker.position = [0., 400., 400.];
        world.actors.insert(0, speaker);
        world
            .show_notice(
                ResolvedMessage {
                    tokens: vec![TextToken::Text {
                        text: "A“".into()
                    }],
                },
                flags::ABOVE,
            )
            .unwrap();
        let mut request = world.dialogue[&0].clone();
        request.anchor = DialogueAnchor::Actor(0);
        request.speaker_actor = Some(0);
        let player = DialoguePlayer::new(&request, 0)?;
        let mut font = BitmapFont {
            texture: String::new(),
            width: 24,
            height: 24,
            line_height: 25,
            glyphs: BTreeMap::new(),
        };
        for resolution in ["640x480", "1920x1080"] {
            let resolution = resolution.parse().unwrap();
            for up in [Vec3::Z, Vec3::Y] {
                let camera = DialogueCamera {
                    world: &world,
                    transform: Some(
                        Transform::from_translation(Vec3::X * 1600.).looking_at(Vec3::ZERO, up),
                    ),
                    fov_degrees: 45.,
                    up,
                };
                for height in [25, 50, 75] {
                    request.dimensions = Some([250, height]);
                    for (flags, offset) in [
                        (flags::ABOVE, 0),
                        (flags::ABOVE, 100),
                        (flags::BELOW, 0),
                        (flags::ABOVE | flags::FRAMELESS, 0),
                    ] {
                        request.flags = flags;
                        request.height_offset = offset;
                        let baseline = layout_with_camera(
                            &font,
                            &request,
                            &player,
                            &camera,
                            &BTreeMap::new(),
                            None,
                            resolution,
                        )?;
                        let (rect, pointer, above) = layout_with_camera(
                            &font,
                            &request,
                            &player,
                            &camera,
                            &BTreeMap::new(),
                            Some(170.),
                            resolution,
                        )?;
                        if flags != flags::ABOVE || offset != 0 {
                            assert_eq!((rect, pointer, above), baseline);
                            continue;
                        }
                        assert!(above);
                        assert!(rect[1] < baseline.0[1]);
                        let mut batches = std::array::from_fn(|_| Batch::default());
                        frame(&mut batches, rect, "", &font, pointer, flags, None);
                        let tip_y = 240. - batches[layer::POINTER].positions[2][1];
                        let projected = project_dialogue_point(
                            camera.transform.as_ref().unwrap(),
                            45.,
                            up * 140.,
                            resolution,
                        );
                        assert_eq!(tip_y, projected[1] - HEAD_CLEARANCE);
                    }
                }
                request.flags = flags::ABOVE;
                request.height_offset = 0;
                let (rect, _, above) = layout_with_camera(
                    &font,
                    &request,
                    &player,
                    &camera,
                    &BTreeMap::new(),
                    Some(300.),
                    resolution,
                )?;
                assert!(above, "a clamped pointer may reach the speaker");
                assert_eq!(rect[1], FRAME_MARGIN);
                let (_, _, above) = layout_with_camera(
                    &font,
                    &request,
                    &player,
                    &camera,
                    &BTreeMap::new(),
                    Some(400.),
                    resolution,
                )?;
                assert!(!above, "an overlapping close-up box must fit below instead");
                request.dimensions = Some([250, 50]);
                for (anchor, [x, y]) in [
                    (DialogueAnchor::Screen([200., 200.]), [200., 200.]),
                    (DialogueAnchor::ScreenGrid(4), [318., 240.]),
                    (DialogueAnchor::ScreenGrid(200), [320., 240.]),
                    (DialogueAnchor::ScreenGrid(-1), [320., 240.]),
                ] {
                    request.anchor = anchor;
                    let draw = |head| {
                        layout_with_camera(
                            &font,
                            &request,
                            &player,
                            &camera,
                            &BTreeMap::new(),
                            head,
                            resolution,
                        )
                    };
                    assert_eq!(draw(Some(170.))?, draw(None)?);
                    assert_eq!(draw(None)?.0, [x - 125., y - 25., x + 125., y + 25.]);
                }
                request.anchor = DialogueAnchor::Actor(0);
            }
        }
        // Fixed-size windows above need no metrics. For an automatically sized
        // window A measures the lead byte as a space (8 pixels), and “ uses all 15.
        for (character, advance) in [(' ', 10), ('“', 15)] {
            font.glyphs.insert(
                character,
                resonance_content::font::Glyph {
                    rect: [0, 0, 24, 24],
                    advance,
                },
            );
        }
        assert_eq!(dialogue_dimensions(&font, &player)?, [23., 25.]);
        assert_eq!(body_glyph_width('“'), 25.);
        Ok(())
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

#[cfg(test)]
mod interpolation_tests {
    use super::*;
    #[test]
    fn ui_motion_and_fades_interpolate_but_glyph_changes_are_discrete() {
        let mut start = Batch::default();
        start.quad([10., 20., 30., 40.], [0., 0., 1., 1.], [1.; 4]);
        let mut next = Batch::default();
        next.quad([20., 30., 40., 50.], [0., 0., 1., 1.], [0.; 4]);
        let mut middle = start.clone();
        middle.interpolate(&next, 0.5);
        for (a, b) in start.positions.iter().zip(&middle.positions) {
            assert_eq!(b[0], a[0] + 5.);
            assert_eq!(b[1], a[1] - 5.);
        }
        assert_eq!(middle.colors, vec![[0.5; 4]; 4]);
        next.uv[0][0] = 0.25;
        let mut unchanged = start.clone();
        unchanged.interpolate(&next, 0.5);
        assert!(unchanged == start);
    }
}

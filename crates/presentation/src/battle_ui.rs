//! Party HUD composed on bitmap UI surfaces.
use super::*;
use anyhow::ensure;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use resonance_battle::{Activity, BattleFrame, Side};
use resonance_content::battle_ui::Art;

const HOURGLASS_FLASH_STEP: u8 = 16;
const HOURGLASS_FLASH_RISE: u8 = u8::MAX.div_ceil(HOURGLASS_FLASH_STEP);
const HOURGLASS_FLASH_PEAK: u8 = 6;
const HOURGLASS_FLASH_DURATION: u8 = 2 * HOURGLASS_FLASH_RISE + HOURGLASS_FLASH_PEAK - 1;

#[path = "battle_ui/layers.rs"]
mod layers;
use layers::{Depth, HudLayer, LayerDefinition, Palette, UiLayers};

pub(super) fn game_over_depth() -> f32 {
    Depth::GameOver.value()
}

pub(super) fn menu_depth() -> f32 {
    Depth::Menus.value()
}

#[path = "battle_ui/combat.rs"]
mod combat;
#[path = "battle_ui/combat_numbers.rs"]
mod combat_numbers;
#[path = "battle_ui/command_strip.rs"]
mod command_strip;
#[path = "battle_ui/cutin.rs"]
mod cutin;
#[path = "battle_ui/equipment.rs"]
mod equipment;
#[cfg(test)]
#[path = "battle_ui/fixtures.rs"]
mod fixtures;
#[path = "battle_ui/items.rs"]
mod items;
#[path = "battle_ui/meters.rs"]
mod meters;
#[path = "battle_ui/notice.rs"]
mod notice;
#[path = "battle_ui/overlays.rs"]
mod overlays;
#[path = "battle_ui/party.rs"]
mod party;
#[path = "battle_ui/results.rs"]
mod results;
#[path = "battle_ui/ribbons.rs"]
mod ribbons;
pub(crate) use ribbons::Input as RibbonInput;
#[path = "battle_ui/strategy.rs"]
mod strategy;
#[path = "battle_ui/techniques.rs"]
mod techniques;
#[path = "battle_ui/text.rs"]
mod text;
#[path = "battle_ui/unison.rs"]
mod unison;
/// Prepared enemy-group artwork; loaded before the battle's GPU warmup.
#[derive(Clone)]
pub(crate) struct EnemyHud {
    pub actor: usize,
    pub group: u8,
    pub name: String,
    pub icon: Option<resonance_content::font::UiTexture>,
}

fn enemy_group_count(enemies: &[EnemyHud]) -> usize {
    enemies
        .iter()
        .map(|enemy| usize::from(enemy.group) + 1)
        .max()
        .unwrap_or(0)
}

/// Party-owned state consumed by the ordinary battle HUD rows.
#[derive(Clone, Copy)]
pub(crate) struct PartyHudInput<'a> {
    pub characters: &'a [u8],
    pub queued_techniques: &'a [bool],
    pub names: &'a [Option<&'a str>],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ActorMarkers {
    pub target: Option<[f32; 3]>,
    pub head: Option<[f32; 3]>,
    pub effect_tick: u16,
}

#[derive(Resource)]
pub(crate) struct Artwork {
    resolution: crate::Resolution,
    art: Art,
    ui: UiLayers,
    font: BitmapFont,
    dialogue: DialogueArt,
    menu_data: std::sync::Arc<resonance_content::menu_data::MenuData>,
    windows: menu::MenuArtwork,
    settings: resonance_content::menu_data::CustomizeSettings,
    results: Option<results::Results>,
    notices: notice::Notices,
    labels: Vec<(usize, overlays::ActorLabel)>,
    combat: combat::Hud,
    numbers: combat_numbers::Numbers,
    scan: Option<items::Scan>,
    hourglass_remaining: u16,
    hourglass_flash_age: Option<u8>,
    unison: unison::State,
    cutin: cutin::State,
    command_motion: command_strip::Motion,
    ribbon_motion: ribbons::State,
    pending_images: Vec<PendingImage>,
}

struct PendingImage {
    image: Handle<Image>,
    size: [u32; 2],
    required: bool,
}

impl Artwork {
    pub(crate) fn displayed_vitals(&self, actor: usize) -> Option<[i32; 2]> {
        self.numbers
            .actors
            .get(actor)
            .and_then(|numbers| numbers.vitals)
    }

    pub(crate) fn portrait_observation(
        &self,
        actor: &resonance_battle::ActorFrame,
        result: Option<resonance_battle::BattleResult>,
    ) -> party::Portrait {
        party::portrait(actor, result, self.numbers.presentation_tick)
    }

    /// Build surfaces from the artwork and menu catalogue admitted by battle preparation.
    #[expect(
        clippy::too_many_arguments,
        reason = "Preparation borrows independently owned content and runtime resources."
    )]
    pub fn load(
        art: Art,
        menu_data: std::sync::Arc<resonance_content::menu_data::MenuData>,
        session: std::sync::Arc<resonance_content::session::SessionData>,
        read: impl Fn(&str) -> Result<Vec<u8>>,
        enemies: &[EnemyHud],
        stack_enemies: bool,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
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
        let white = image_assets.add(white);
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        dialogue.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&dialogue.font)?)?;
        font.validate()?;
        art.validate_required_dialogue(&font)?;
        command_strip::validate_font(&art.commands, &font)?;
        font.validate_text("Cancel orders")?;
        for enemy in enemies {
            font.validate_text(&enemy.name)?;
        }
        art.font.validate_text("HP TP +0123456789.")?;
        let mut pending_images = Vec::new();
        let mut load_image = |path: String, size, repeat, required| {
            let image = menu::menu_image(server, path, repeat);
            pending_images.push(PendingImage {
                image: image.clone(),
                size,
                required,
            });
            image
        };
        let atlas = load_image(
            art.font.texture.clone(),
            [art.font.width, art.font.height],
            false,
            true,
        );
        let main_font = load_image(font.texture.clone(), [font.width, font.height], false, true);
        let palette = Palette {
            solid: LayerDefinition::new(white, [1, 1], Depth::Background),
            font: LayerDefinition::new(atlas, [art.font.width, art.font.height], Depth::Background),
            main_font: LayerDefinition::new(
                main_font,
                [font.width, font.height],
                Depth::Background,
            ),
            nearest,
        };
        let solid = |depth| palette.solid.clone().at(depth);
        let text = |depth| palette.font.clone().at(depth).foreground();
        let main_text = |depth| palette.main_font.clone().at(depth).foreground();
        let mut load = |texture: &resonance_content::font::UiTexture, repeat, required| {
            load_image(
                texture.path.clone(),
                [texture.width, texture.height],
                repeat,
                required,
            )
        };
        let mut portraits = Vec::new();
        for portrait in &art.portraits {
            let layer = if let Some(texture) = portrait {
                let image = load(texture, true, false);
                LayerDefinition::new(image, [texture.width, texture.height], Depth::Party)
                    .foreground()
            } else {
                solid(Depth::Party)
            };
            portraits.push(layer);
        }
        let stat_icons = art
            .stat_icons
            .iter()
            .enumerate()
            .map(|(index, texture)| {
                let image = load(texture, false, true);
                let definition =
                    LayerDefinition::new(image, [texture.width, texture.height], Depth::Vitals);
                if index == party::StatusIcon::UpArrow as usize
                    || index == party::StatusIcon::DownArrow as usize
                {
                    definition.foreground()
                } else {
                    definition
                }
            })
            .collect::<Vec<_>>();
        let rhythm_texture = &art.unison_buttons[0];
        let rhythm_prompt = LayerDefinition::new(
            load(rhythm_texture, false, true),
            [rhythm_texture.width, rhythm_texture.height],
            Depth::Vitals,
        );
        let notice = notice::layers(&art, enemies, &palette, materials, &mut load);
        let unison = unison::layers(&art, &palette, &notice, materials, &mut load);
        let scan = items::layers(&art, &palette, materials, &mut load);
        let command_texture = load(&art.commands.texture, false, true);
        let petrified_portraits = portraits
            .iter()
            .cloned()
            .map(|layer| layer.effect(TextureEffect::RedChannel))
            .collect();
        let mut make = |definition| palette.layer(definition, materials);
        let ui = UiLayers {
            notice,
            labels: [make(solid(Depth::Labels)), make(text(Depth::Labels))],
            gauges: make(solid(Depth::Vitals)),
            font: make(text(Depth::Vitals)),
            patterned_vitals: make(
                text(Depth::Vitals)
                    .foreground()
                    .effect(TextureEffect::GlyphPattern),
            ),
            floating: make(text(Depth::Meters)),
            recovery: make(text(Depth::Recovery)),
            scan,
            transition: make(solid(Depth::Transition)),
            commands: [
                make(solid(Depth::Commands)),
                make(
                    LayerDefinition::new(
                        command_texture,
                        [art.commands.texture.width, art.commands.texture.height],
                        Depth::Commands,
                    )
                    .foreground(),
                ),
                make(main_text(Depth::Commands).foreground()),
                make(text(Depth::Commands).foreground()),
            ],
            ribbons: [
                make(solid(Depth::Commands)),
                make(main_text(Depth::Commands)),
                make(text(Depth::Commands)),
            ],
            results: [
                make(solid(Depth::Results)),
                make(main_text(Depth::Results)),
                make(text(Depth::Results)),
            ],
            portraits: palette.layers(portraits, materials),
            petrified_portraits: palette.layers(petrified_portraits, materials),
            stat_icons: palette.layers(stat_icons, materials),
            rhythm_prompt: palette.layer(rhythm_prompt, materials),
            unison,
        };
        let combat =
            combat::Hud::load(&art, enemies, stack_enemies, &palette, materials, &mut load)?;
        let windows = menu::MenuArtwork::load(
            &read,
            session.experience.clone().into(),
            server,
            materials,
            (&ui.results[1].material, [font.width, font.height]),
            &dialogue.cursor,
            diagnostics,
        )?;
        Ok(Self {
            resolution: Default::default(),
            art,
            ui,
            font,
            dialogue,
            menu_data,
            windows,
            settings: Default::default(),
            results: None,
            notices: Default::default(),
            labels: Vec::new(),
            combat,
            numbers: Default::default(),
            scan: None,
            hourglass_remaining: 0,
            hourglass_flash_age: None,
            unison: Default::default(),
            cutin: Default::default(),
            command_motion: Default::default(),
            ribbon_motion: Default::default(),
            pending_images,
        })
    }

    /// Complete the same required image checks before the HUD enters GPU warmup.
    pub(crate) fn images_ready(
        &mut self,
        server: &AssetServer,
        images: &mut Assets<Image>,
        materials: &mut Assets<Surface>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<bool> {
        let mut pending = Vec::new();
        let mut omitted = Vec::new();
        for image in self.pending_images.drain(..) {
            let result = (|| -> Result<bool> {
                if !crate::field_view::image_ready(server, images, &image.image)? {
                    return Ok(false);
                }
                let loaded = images
                    .get(&image.image)
                    .context("battle HUD image unavailable")?;
                ensure!(
                    [loaded.width(), loaded.height()] == image.size,
                    "battle HUD image dimensions differ from descriptor: {:?}",
                    server.get_path(image.image.id())
                );
                Ok(true)
            })();
            match result {
                Ok(true) => {}
                Ok(false) => pending.push(image),
                Err(error) if image.required => return Err(error),
                Err(error) => {
                    diagnostics.report("optional battle HUD artwork", error)?;
                    omitted.push(image.image);
                }
            }
        }
        self.pending_images = pending;
        if !omitted.is_empty() {
            for (portrait, layer) in self.art.portraits.iter_mut().zip(&self.ui.portraits) {
                if omitted.contains(&layer.definition.image) {
                    *portrait = None;
                }
            }
            self.combat.omit_images(&omitted);
            // Omitted layers emit no geometry. Reuse the resident solid texture
            // so their preparation draws still have a valid material binding.
            let fallback = self.ui.gauges.definition.image.clone();
            for layer in self
                .layers_mut()
                .filter(|layer| omitted.contains(&layer.definition.image))
            {
                let original = std::mem::replace(&mut layer.definition.image, fallback.clone());
                let mut surface = materials
                    .get_mut(&layer.material)
                    .context("battle HUD surface unavailable")?;
                let surface = &mut *surface;
                for handle in [
                    &mut surface.source,
                    &mut surface.sampling,
                    &mut surface.frame_mask,
                    &mut surface.color_mask,
                ] {
                    if *handle == original {
                        *handle = fallback.clone();
                    }
                }
            }
        }
        Ok(self.pending_images.is_empty())
    }

    /// Call before activation; the scene loader owns GPU readiness and warmup.
    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        resolution: crate::Resolution,
    ) {
        self.resolution = resolution;
        for layer in self.layers_mut() {
            layer.prepare(commands, meshes);
        }
        self.windows.prepare_windows(resolution);
    }

    pub(crate) fn menu_images_ready(
        &self,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        self.windows.drawn_images_ready(images, server)
    }
    fn layers(&self) -> impl Iterator<Item = &HudLayer> {
        self.ui
            .iter()
            .chain(&self.combat.layers)
            .chain(self.cutin.layers.values())
    }
    fn layers_mut(&mut self) -> impl Iterator<Item = &mut HudLayer> {
        self.ui
            .iter_mut()
            .chain(&mut self.combat.layers)
            .chain(self.cutin.layers.values_mut())
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.layers()
            .filter_map(|layer| layer.rendered.as_ref().map(|layer| layer.entity))
            .chain(self.windows.layers.values().map(|layer| layer.entity))
    }

    /// Character IDs are one-based, in the same order as the frame's party actors.
    /// Model visibility intentionally does not hide this separate HUD pass.
    #[expect(
        clippy::too_many_arguments,
        reason = "The HUD borrows separate frame inputs and independently owned rendering resources."
    )]
    pub fn render(
        &mut self,
        frame: &BattleFrame,
        party: PartyHudInput<'_>,
        command: Option<&resonance_game::battle::command::Frame>,
        inspection: Option<(resonance_battle::ActorId, [f32; 3])>,
        marker_anchors: &[ActorMarkers],
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        ensure!(
            self.layers().all(|layer| layer.rendered.is_some()),
            "battle HUD was not prepared"
        );
        self.clear_menu(commands);
        if frame.summon.as_ref().is_some_and(|summon| summon.hide_hud)
            || frame
                .enemy_presentation
                .as_ref()
                .is_some_and(|scene| scene.hide_hud)
        {
            self.hide_for_event(commands);
            return Ok(());
        }
        self.render_party(frame, party, commands, meshes)?;
        self.render_results(commands, meshes)?;
        self.render_overlays(frame, commands, meshes)?;
        if self.results.is_some() {
            for layer in [&mut self.ui.floating, &mut self.ui.recovery] {
                layer.show(false, commands);
            }
        } else {
            self.render_numbers(frame, commands, meshes)?;
        }
        self.render_notices(frame, party, commands, meshes)?;
        self.render_combat(frame, command, marker_anchors, commands, meshes)?;
        self.render_scan(frame, command, inspection, commands, meshes)?;
        self.render_unison_attack(frame, party, commands, meshes)?;
        self.cutin.render(commands, meshes)?;
        Ok(())
    }

    pub(crate) fn render_ribbons(
        &mut self,
        frame: &BattleFrame,
        input: RibbonInput<'_>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let batches = if frame.summon.as_ref().is_some_and(|summon| summon.hide_hud)
            || frame
                .enemy_presentation
                .as_ref()
                .is_some_and(|scene| scene.hide_hud)
        {
            std::array::from_fn(|_| Batch::default())
        } else {
            self.ribbon_motion
                .draw(input, &self.font, &self.combat.enemies, frame)?
        };
        for (layer, batch) in self.ui.ribbons.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }

    pub(crate) fn load_event_dialogue(
        &self,
        files: &resonance_content::prepared::Files,
        materials: &mut Assets<Surface>,
        images: &mut Assets<Image>,
    ) -> Result<DialogueOverlay> {
        DialogueOverlay::load(&self.dialogue, &self.font, files, materials, images)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_event_dialogue(
        &self,
        overlay: &mut DialogueOverlay,
        world: &resonance_events::GameWorld,
        dialogue: &BTreeMap<u8, DialoguePlayer>,
        frame: &BattleFrame,
        resolution: crate::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        DialogueDraw {
            resolution,
            attached_positions: &BTreeMap::new(),
            font: &self.font,
            spec: &self.dialogue,
            images: &overlay.images,
            layers: &mut overlay.layers,
            head_heights: &mut overlay.head_heights,
            choice_trail: &mut overlay.choice_trail,
            menu: &self.windows,
        }
        .render(
            world,
            dialogue,
            DialogueCamera::battle(world, frame)?,
            world.tick,
            // Battle dialogue uses a fixed attachment height plus scripted offsets.
            &BTreeMap::new(),
            commands,
            meshes,
            materials,
        )
    }

    pub(crate) fn hide_for_event(&mut self, commands: &mut Commands) {
        for layer in self.layers_mut() {
            layer.show(false, commands);
        }
        self.clear_menu(commands);
    }

    pub fn settings(&mut self, settings: resonance_content::menu_data::CustomizeSettings) {
        self.settings = settings;
    }

    pub fn render_transition(
        &mut self,
        frame: Option<&resonance_battle::TransitionFrame>,
        letterbox: u16,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let flash = self.hourglass_flash_age.map(|age| {
            let peak_end = HOURGLASS_FLASH_RISE + HOURGLASS_FLASH_PEAK;
            resonance_battle::TransitionFrame {
                color: [255; 3],
                alpha: if age < HOURGLASS_FLASH_RISE {
                    age * HOURGLASS_FLASH_STEP
                } else if age < peak_end {
                    255
                } else {
                    255 - (age - peak_end + 1) * HOURGLASS_FLASH_STEP
                },
            }
        });
        // Event, summon and departure fades retain ownership of the screen.
        let frame = frame.or(flash.as_ref());
        let layer = &mut self.ui.transition;
        let mut batch = Batch::default();
        if letterbox != 0 {
            let [left, top, right, bottom] = self.resolution.ui_rect();
            let height = f32::from(letterbox).min((bottom - top) * 0.5);
            for bounds in [
                [left, top, right, top + height],
                [left, bottom - height, right, bottom],
            ] {
                batch.quad(bounds, [0.5; 4], [0., 0., 0., 1.]);
            }
        }
        if let Some(frame) = frame.filter(|frame| frame.alpha != 0) {
            let [r, g, b] = frame.color.map(|channel| f32::from(channel) / 255.);
            batch.quad(
                self.resolution.ui_rect(),
                [0.5; 4],
                [r, g, b, f32::from(frame.alpha) / 255.],
            );
        }
        layer.upload(batch, commands, meshes)?;
        Ok(())
    }

    pub fn despawn(self, world: &mut World) {
        for entity in self.entities() {
            world.despawn(entity);
        }
    }
}

fn hud_color(color: [u8; 4]) -> [f32; 4] {
    std::array::from_fn(|channel| {
        f32::from(color[channel]) / 255. * if channel == 3 { 1. } else { 2. }
    })
}

impl Artwork {
    /// Consume a completed update once. Drawing never ages feedback.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<()> {
        // The screen flash keeps running during command/target holds and after time stop ends.
        self.hourglass_flash_age = if frame.hourglass_remaining > self.hourglass_remaining {
            Some(0)
        } else {
            self.hourglass_flash_age
                .map(|age| age + 1)
                .filter(|&age| age < HOURGLASS_FLASH_DURATION)
        };
        self.hourglass_remaining = frame.hourglass_remaining;
        let elapsed = u32::from(!paused);
        self.cutin.advance(frame)?;
        self.advance_scan(frame, elapsed);
        self.advance_results(elapsed);
        self.numbers.advance(frame, elapsed);
        self.combat.advance(frame, elapsed)?;
        self.notices.advance(frame, elapsed);
        self.advance_labels(frame.clock, elapsed);
        // Apply this update's notifications after advancing existing labels.
        for cue in &frame.cues {
            match cue {
                resonance_battle::Cue::CombatRetired => self.labels.clear(),
                resonance_battle::Cue::Hit { actor, result, .. }
                    if result.guard == resonance_battle::GuardResult::Broken =>
                {
                    let target = frame
                        .actors
                        .get(actor.index())
                        .context("unknown Guard Break label actor")?;
                    let position = target.condition_label_origin();
                    overlays::request_at(
                        &mut self.labels,
                        actor.index(),
                        overlays::LabelKind::GuardBreak,
                        position,
                    );
                }
                resonance_battle::Cue::ConditionLabel {
                    actor,
                    kind,
                    position,
                } => {
                    ensure!(
                        actor.index() < frame.actors.len(),
                        "unknown condition label actor"
                    );
                    overlays::condition(&mut self.labels, actor.index(), *kind, *position);
                }
                resonance_battle::Cue::OverLimitEntered { actor, position } => {
                    ensure!(
                        actor.index() < frame.actors.len(),
                        "unknown Over Limit label actor"
                    );
                    overlays::request_at(
                        &mut self.labels,
                        actor.index(),
                        overlays::LabelKind::OverLimit,
                        *position,
                    );
                }
                resonance_battle::Cue::ExSkillLabel { actor, position } => {
                    ensure!(
                        actor.index() < frame.actors.len(),
                        "unknown EX skill label actor"
                    );
                    overlays::request_at(
                        &mut self.labels,
                        actor.index(),
                        overlays::LabelKind::ExSkill,
                        *position,
                    );
                }
                resonance_battle::Cue::CustomLabel {
                    actor,
                    position,
                    left,
                    right,
                    duration,
                } => {
                    ensure!(
                        actor.index() < frame.actors.len(),
                        "unknown custom label actor"
                    );
                    overlays::request_custom(
                        &mut self.labels,
                        actor.index(),
                        [left.clone(), right.clone()],
                        *position,
                        *duration,
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn overlay_request(
        &mut self,
        kind: resonance_game::battle::lifecycle::Request,
    ) -> Result<()> {
        use resonance_game::battle::lifecycle::Request::*;
        match kind {
            DefeatNotice => self.notices.defeat_age = Some(0),
            // Scene transitions and prepared effects own these outcomes.
            VictoryBanner | EscapeNotice => {}
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
        let mut panel = Batch::default();
        let mut text = Batch::default();
        for (_, label) in &self.labels {
            let camera = frame.camera.as_ref().context("actor label has no camera")?;
            let projected = resonance_battle::project_screen_point(*camera, label.position);
            label.draw(
                &self.art.font,
                self.art.stat_colors,
                self.art.stat_backdrops,
                projected,
                &mut panel,
                &mut text,
            )?;
        }
        for (layer, batch) in self.ui.labels.iter_mut().zip([panel, text]) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
fn test_artwork() -> Result<(Artwork, Assets<Surface>, App)> {
    test_artwork_with(
        |_| {},
        &resonance_content::diagnostics::Diagnostics::new(true),
    )
}

#[cfg(test)]
fn test_artwork_with(
    edit: impl FnOnce(&mut serde_json::Value),
    diagnostics: &resonance_content::diagnostics::Diagnostics,
) -> Result<(Artwork, Assets<Surface>, App)> {
    let root = fixtures::asset_root();
    let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(
        root.join(resonance_content::battle_ui::PATH),
    )?)?;
    edit(&mut document);
    let mut files = resonance_content::prepared::Files::new(diagnostics.clone());
    files.insert(
        resonance_content::battle_ui::PATH.into(),
        serde_json::to_vec(&document)?.into(),
    );
    for portrait in document["portraits"].as_array().into_iter().flatten() {
        if let Some(path) = portrait["path"].as_str()
            && let Ok(bytes) = std::fs::read(root.join(path))
        {
            files.insert(path.into(), bytes.into());
        }
    }
    let art = Art::load(&files)?;
    let menu_data: std::sync::Arc<resonance_content::menu_data::MenuData> = std::sync::Arc::new(
        serde_json::from_slice(&std::fs::read(root.join("game/menu-data.json"))?)?,
    );
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: root.to_string_lossy().into_owned(),
            ..default()
        },
    ))
    .init_asset::<Image>()
    .register_asset_loader(bevy::image::ImageLoader::new(
        bevy::image::CompressedImageFormats::NONE,
    ));
    let server = app.world().resource::<AssetServer>().clone();
    let mut materials = Assets::<Surface>::default();
    let mut artwork = Artwork::load(
        art,
        menu_data,
        std::sync::Arc::new(serde_json::from_slice(&std::fs::read(
            root.join("game/session-data.json"),
        )?)?),
        |path| {
            ensure!(
                ![
                    resonance_content::battle_ui::PATH,
                    "game/menu-data.json",
                    "game/session-data.json",
                ]
                .contains(&path),
                "HUD reread a prepared descriptor"
            );
            Ok(std::fs::read(root.join(path))?)
        },
        &[],
        false,
        &server,
        &mut materials,
        &mut app.world_mut().resource_mut::<Assets<Image>>(),
        diagnostics,
    )?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        if artwork.images_ready(
            &server,
            &mut app.world_mut().resource_mut::<Assets<Image>>(),
            &mut materials,
            diagnostics,
        )? {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "test HUD images did not load"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok((artwork, materials, app))
}

#[test]
#[ignore = "requires current font publications; CPU only"]
fn required_fonts_decode_before_hud_activation() -> Result<()> {
    let root = fixtures::asset_root();
    let read = |path: &str| Ok(std::fs::read(root.join(path))?);
    for mut font in [fixtures::battle_font()?, fixtures::dialogue_font()?] {
        ui_image(&font.texture, [font.width, font.height], false, &read)?;
        assert!(
            ui_image(
                &font.texture,
                [font.width, font.height],
                false,
                &|_| anyhow::bail!("missing font")
            )
            .is_err()
        );
        assert!(
            ui_image(&font.texture, [font.width, font.height], false, &|_| Ok(
                b"invalid image".to_vec()
            ))
            .is_err()
        );
        font.width += 1;
        assert!(ui_image(&font.texture, [font.width, font.height], false, &read).is_err());
    }
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    assert!(
        test_artwork_with(
            |art| art["font"]["texture"] = serde_json::json!("ui/dialogue.json"),
            &diagnostics
        )
        .is_err()
    );
    assert!(
        test_artwork_with(
            |art| art["font"]["width"] =
                serde_json::json!(art["font"]["width"].as_u64().unwrap() + 1),
            &diagnostics
        )
        .is_err()
    );
    Ok(())
}

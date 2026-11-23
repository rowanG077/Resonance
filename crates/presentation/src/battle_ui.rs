//! Party HUD composed on bitmap UI surfaces.
use super::*;
use anyhow::ensure;
use resonance_battle::{Activity, ActorId, BattleFrame, Character, Side};
use resonance_content::battle_ui::Art;
use std::collections::BTreeMap;

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
#[path = "battle_ui/text.rs"]
mod text;
#[path = "battle_ui/unison.rs"]
mod unison;
/// Prepared enemy-group artwork; loaded before the battle's GPU warmup.
#[derive(Clone)]
pub(crate) struct EnemyHud {
    pub actor: ActorId,
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
    pub characters: &'a [Character],
    pub queued_techniques: &'a [bool],
    pub names: &'a [Option<&'a str>],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ActorMarkers {
    pub target: Option<[f32; 3]>,
    pub presented_target: Option<[f32; 3]>,
    pub head: Option<[f32; 3]>,
    pub effect_tick: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GraphicsKey(
    AssetId<Image>,
    [u32; 2],
    u32,
    bool,
    TextureEffect,
    UiInterpolation,
);
impl GraphicsKey {
    fn of(d: &LayerDefinition) -> Self {
        Self(
            d.image.id(),
            d.size,
            d.depth.to_bits(),
            d.nearest,
            d.texture_effect,
            d.interpolation,
        )
    }
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
    labels: BTreeMap<ActorId, overlays::ActorLabel>,
    tp_warnings: BTreeMap<ActorId, overlays::TpWarning>,
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
    required: bool,
}

pub(crate) struct TransitionPrediction<'a> {
    pub cover: Option<&'a resonance_battle::TransitionFrame>,
    pub letterbox: u16,
    pub hourglass_remaining: u16,
    pub alpha: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct HudTime<'a> {
    pub tick: u32,
    pub next: Option<&'a crate::battle::prediction::Endpoint>,
    pub visit: f32,
    pub update: f32,
    pub feedback: f32,
}

fn hit_feedback_time<T>(clock: resonance_battle::BattleClock, visit: T, feedback: T) -> T {
    if clock == resonance_battle::BattleClock::DefeatHold {
        visit
    } else {
        feedback
    }
}

impl HudTime<'_> {
    fn hit_feedback(self) -> f32 {
        self.next.map_or(self.feedback, |next| {
            hit_feedback_time(next.frame.clock, self.visit, self.feedback)
        })
    }
}

impl Artwork {
    pub(crate) fn displayed_vitals(&self, actor: usize) -> Option<[i32; 2]> {
        self.numbers
            .actors
            .get(actor)
            .and_then(|numbers| numbers.vitals)
    }

    pub(crate) fn party_portrait(
        &self,
        actor: &resonance_battle::ActorFrame,
        future: Option<(&resonance_battle::ActorFrame, f32)>,
    ) -> party::Portrait {
        party::portrait(
            actor,
            self.results.is_some(),
            self.numbers.presentation_tick,
            future,
        )
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
        resident: &Resident,
    ) -> Result<Self> {
        let [white, nearest] = resident.images(image_assets);
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        dialogue.prepare_images(&read)?;
        dialogue.validate()?;
        let font: BitmapFont = serde_json::from_slice(&read(&dialogue.font)?)?;
        font.validate()?;
        art.validate_required_dialogue(&font)?;
        command_strip::validate_font(&art.commands, &font)?;
        font.validate_text("Cancel orders")?;
        for enemy in enemies {
            font.validate_text(&enemy.name)?;
        }
        art.font
            .validate_text("HP TP XX +0123456789. RESERVE. FORMATION DEFAULT REVERSE")?;
        let inputs = crate::input_icons::prepare(&read, server)?;
        let mut pending_images = vec![PendingImage {
            image: inputs.image.clone(),
            required: true,
        }];
        let mut load_image = |path: String, repeat, required| {
            let image = menu::menu_image(server, path, repeat);
            pending_images.push(PendingImage {
                image: image.clone(),
                required,
            });
            image
        };
        let atlas = load_image(art.font.texture.clone(), false, true);
        let main_font = load_image(font.texture.clone(), false, true);
        let palette = Palette {
            solid: LayerDefinition::new(white, [1, 1], Depth::Background),
            font: LayerDefinition::new(atlas, [art.font.width, art.font.height], Depth::Background)
                .pixel_center(),
            main_font: LayerDefinition::new(
                main_font,
                [font.width, font.height],
                Depth::Background,
            )
            .pixel_center(),
            nearest,
            resident: resident.clone(),
        };
        let solid = |depth| palette.solid.clone().at(depth);
        let text = |depth| palette.font.clone().at(depth).offset_depth(1.);
        let main_text = |depth| palette.main_font.clone().at(depth).offset_depth(1.);
        let input_atlas = &inputs.atlas;
        let prompt_image = inputs.image.clone();
        let mut load = |texture: &resonance_content::font::UiTexture, repeat, required| {
            load_image(texture.path.clone(), repeat, required)
        };
        let mut portraits = Vec::new();
        for portrait in &art.portraits {
            let layer = if let Some(texture) = portrait {
                let image = load(texture, true, false);
                LayerDefinition::new(image, [texture.width, texture.height], Depth::Party)
                    .offset_depth(1.)
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
                    definition.offset_depth(1.)
                } else {
                    definition
                }
            })
            .collect::<Vec<_>>();
        let rhythm_prompt =
            LayerDefinition::new(prompt_image.clone(), input_atlas.size, Depth::Vitals);
        let notice = notice::layers(&art, enemies, &palette, materials, &mut load);
        let unison = unison::layers(
            prompt_image.clone(),
            input_atlas.size,
            &palette,
            &notice,
            materials,
        );
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
                    .offset_depth(1.)
                    .effect(TextureEffect::GlyphPattern),
            ),
            reserved_commands: make(text(Depth::Vitals).offset_depth(2.)),
            // Above the unison frame and escape art, which also sit at
            // `Meters + 1`; an equal depth left the order to the renderer.
            floating: make(text(Depth::Meters).offset_depth(0.5)),
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
                    .offset_depth(1.),
                ),
                make(main_text(Depth::Commands).offset_depth(1.)),
                make(text(Depth::Commands).offset_depth(1.)),
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
                make(
                    LayerDefinition::new(prompt_image, input_atlas.size, Depth::Results)
                        .offset_depth(2.),
                ),
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
            inputs,
            resident,
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
            labels: BTreeMap::new(),
            tp_warnings: BTreeMap::new(),
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
            match crate::field_view::image_ready(server, images, &image.image) {
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
            // Cut-ins are optional only during boot prewarming. A live
            // encounter still requires its selected portrait.
            self.cutin
                .layers
                .retain(|_, layer| !omitted.contains(&layer.definition.image));
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
    pub fn prepare(&mut self, commands: &mut Commands, resolution: crate::Resolution) {
        self.resolution = resolution;
        if let Some(resident) = self.windows.resident() {
            let inventory = resident.0.lock().unwrap();
            let mut used = std::collections::HashMap::new();
            for layer in self.layers_mut() {
                let key = GraphicsKey::of(&layer.definition);
                let index = used.entry(key).or_insert(0usize);
                if let Some(common) = inventory
                    .hud
                    .get(&key)
                    .and_then(|layers| layers.get(*index))
                    .filter(|_| layer.rendered.is_none())
                {
                    let mut rendered = common.clone();
                    rendered.uploaded = None;
                    rendered.visible = false;
                    layer.material = rendered.material.clone();
                    let transform = Transform::from_xyz(0., 0., layer.definition.depth);
                    commands
                        .entity(rendered.entity)
                        .insert((transform, GlobalTransform::from(transform)));
                    layer.rendered = Some(rendered);
                }
                *index += 1;
            }
        }
        for layer in self.layers_mut() {
            layer.prepare(commands);
        }
        self.windows.prepare_windows(resolution);
    }

    pub(super) fn remember_graphics(&self, resident: &Resident) {
        let mut inventory = resident.0.lock().unwrap();
        for layer in self.layers() {
            if let Some(rendered) = &layer.rendered {
                inventory
                    .hud
                    .entry(GraphicsKey::of(&layer.definition))
                    .or_default()
                    .push(rendered.clone());
            }
        }
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
        (committed, frame): (&BattleFrame, &BattleFrame),
        party: PartyHudInput<'_>,
        command: Option<&resonance_game::battle::command::Frame>,
        inspection: Option<(resonance_battle::ActorId, [f32; 3])>,
        marker_anchors: &[ActorMarkers],
        time: HudTime,
        commands: &mut Commands,
    ) -> Result<()> {
        ensure!(
            self.layers().all(|layer| layer.rendered.is_some()),
            "battle HUD was not prepared"
        );
        if frame.summon.as_ref().is_some_and(|summon| summon.hide_hud)
            || frame
                .enemy_presentation
                .as_ref()
                .is_some_and(|scene| scene.hide_hud)
        {
            self.hide_for_event(commands);
            return Ok(());
        }
        self.render_party(frame, party, time, commands)?;
        self.render_results(time.feedback, commands)?;
        self.render_overlays((committed, frame), time, commands)?;
        if self.results.is_some() {
            for layer in [&mut self.ui.floating, &mut self.ui.recovery] {
                layer.show(false, commands);
            }
        } else {
            self.render_numbers(frame, time, commands)?;
        }
        self.render_notices(frame, party, time.feedback, commands)?;
        self.render_combat(frame, command, marker_anchors, time, commands)?;
        self.render_scan(frame, command, inspection, time, commands)?;
        self.render_unison_attack(frame, party, time, commands)?;
        self.cutin.render(time.update, commands)?;
        Ok(())
    }

    pub(crate) fn render_command_page(
        &mut self,
        candidate: &resonance_game::battle::results::Candidate,
        core: &resonance_battle::Battle,
        command: &resonance_game::battle::command::Frame,
        character: usize,
        time: HudTime,
        commands: &mut Commands,
    ) -> Result<()> {
        use resonance_game::battle::command::View;
        let future = time
            .next
            .and_then(|n| n.menu.as_deref())
            .filter(|n| n.actor == command.actor);
        let [current, next] = [Some(command), future].map(|command| {
            let command = command?;
            Some(match &command.view {
                View::Inventory(list) => menu::BattlePage::Items(list, &self.menu_data),
                View::Strategy(state) => menu::BattlePage::Strategy(candidate.strategy_page(state)),
                View::Tech(state) => menu::BattlePage::Tech(candidate.battle_tech_view(
                    state,
                    &command.connected,
                    core,
                )),
                View::Unison(state) => menu::BattlePage::Unison(candidate.unison_page(state)),
                View::Equipment(state) => {
                    menu::BattlePage::Equipment(candidate.equipment_page(core, state, character))
                }
                _ => return None,
            })
        });
        self.windows.render_battle_page(
            current.context("missing shared battle page")?,
            next.map(|p| (p, time.visit)),
            &self.font,
            &self.dialogue,
            &self.settings,
            time.tick,
            commands,
        )
    }

    pub(crate) fn render_ribbons(
        &mut self,
        frame: &BattleFrame,
        input: RibbonInput<'_>,
        opening_fraction: f32,
        commands: &mut Commands,
    ) -> Result<()> {
        let batches = if frame.summon.as_ref().is_some_and(|summon| summon.hide_hud)
            || frame
                .enemy_presentation
                .as_ref()
                .is_some_and(|scene| scene.hide_hud)
        {
            std::array::from_fn(|_| Batch::default())
        } else {
            self.ribbon_motion.draw(
                input,
                &self.font,
                &self.combat.enemies,
                frame,
                opening_fraction,
                self.resolution.ui_rect(),
            )?
        };
        for (layer, batch) in self.ui.ribbons.iter_mut().zip(batches) {
            layer.upload(batch, commands)?;
        }
        Ok(())
    }

    pub(crate) fn load_event_dialogue(
        &self,
        files: &resonance_content::prepared::Files,
        materials: &mut Assets<Surface>,
        images: &mut Assets<Image>,
        formats: bevy::image::CompressedImageFormats,
    ) -> Result<DialogueOverlay> {
        if let Some(resident) = self.windows.resident()
            && let Some(dialogue) = &resident.0.lock().unwrap().dialogue
        {
            return Ok(dialogue.clone());
        }
        DialogueOverlay::load(
            &self.dialogue,
            &self.font,
            files,
            materials,
            images,
            formats,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_event_dialogue(
        &self,
        overlay: &mut DialogueOverlay,
        world: &resonance_events::GameWorld,
        dialogue: &BTreeMap<u8, DialoguePlayer>,
        frame: &BattleFrame,
        next: Option<(&crate::battle::prediction::Endpoint, f32)>,
        resolution: crate::Resolution,
        commands: &mut Commands,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        let future = if let Some((endpoint, alpha)) = next
            && let Some(event) = &endpoint.event
            && event.events.tick() == world.tick.wrapping_add(1)
        {
            let continuous = frame
                .camera
                .zip(endpoint.frame.camera)
                .is_some_and(|(a, b)| crate::battle::continuous_camera(a, b));
            Some(DialogueEndpoint {
                players: &event.dialogue,
                camera: continuous
                    .then(|| DialogueCamera::battle(&event.events.world, &endpoint.frame))
                    .transpose()?,
                alpha,
            })
        } else {
            None
        };
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
            future,
            world.tick,
            // Battle dialogue uses a fixed attachment height plus scripted offsets.
            &BTreeMap::new(),
            commands,
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
    pub(crate) fn set_prompt_device(&mut self, device: crate::input_icons::PromptDevice) -> bool {
        self.windows.set_prompt_device(device)
    }

    pub fn render_transition(
        &mut self,
        frame: Option<&resonance_battle::TransitionFrame>,
        letterbox: u16,
        prediction: Option<TransitionPrediction<'_>>,
        commands: &mut Commands,
    ) -> Result<()> {
        let flash_at = |age| {
            let peak_end = HOURGLASS_FLASH_RISE + HOURGLASS_FLASH_PEAK;
            resonance_battle::TransitionFrame {
                color: [255; 3],
                wipe: None,
                alpha: if age < HOURGLASS_FLASH_RISE {
                    age * HOURGLASS_FLASH_STEP
                } else if age < peak_end {
                    255
                } else {
                    255 - (age - peak_end + 1) * HOURGLASS_FLASH_STEP
                },
            }
        };
        let flash = self.hourglass_flash_age.map(flash_at);
        // Event, summon and departure fades retain ownership of the screen.
        let frame = frame.or(flash.as_ref());
        let mut cover = frame.map(|frame| {
            Vec4::new(
                frame.color[0] as f32,
                frame.color[1] as f32,
                frame.color[2] as f32,
                frame.alpha as f32,
            ) / 255.
        });
        let mut letterbox = f32::from(letterbox);
        if let Some(next) = prediction {
            let age = if next.hourglass_remaining > self.hourglass_remaining {
                Some(0)
            } else {
                self.hourglass_flash_age
                    .map(|age| age + 1)
                    .filter(|&age| age < HOURGLASS_FLASH_DURATION)
            };
            let flash = age.map(flash_at);
            let future = next.cover.or(flash.as_ref());
            let same_color = frame.zip(future).is_none_or(|(a, b)| a.color == b.color);
            if same_color {
                let future = future.map(|frame| {
                    Vec4::new(
                        frame.color[0] as f32,
                        frame.color[1] as f32,
                        frame.color[2] as f32,
                        frame.alpha as f32,
                    ) / 255.
                });
                cover = match (cover, future) {
                    (Some(a), Some(b)) => Some(a.lerp(b, next.alpha)),
                    (Some(mut a), None) => {
                        a.w *= 1. - next.alpha;
                        Some(a)
                    }
                    (None, Some(mut b)) => {
                        b.w *= next.alpha;
                        Some(b)
                    }
                    _ => None,
                };
            }
            letterbox += (f32::from(next.letterbox) - letterbox) * next.alpha;
        }
        let layer = &mut self.ui.transition;
        let mut batch = Batch::default();
        if letterbox != 0. {
            let [left, top, right, bottom] = self.resolution.ui_rect();
            let height = letterbox.min((bottom - top) * 0.5);
            for bounds in [
                [left, top, right, top + height],
                [left, bottom - height, right, bottom],
            ] {
                batch.quad(bounds, [0.5; 4], [0., 0., 0., 1.]);
            }
        }
        if let Some(wipe) = frame
            .filter(|frame| frame.alpha != 0)
            .and_then(|frame| frame.wipe.as_ref())
        {
            let [left, top, right, bottom] = self.resolution.ui_rect();
            let row_height = (bottom - top) / 480.;
            for (row, offset) in wipe.offsets.iter().enumerate() {
                let width = f32::from(wipe.progress.saturating_sub(*offset));
                if width > 0. {
                    let y = top + row as f32 * row_height;
                    let start = batch.colors.len();
                    batch.quad(
                        [
                            left,
                            y,
                            left + width / 640. * (right - left),
                            y + row_height,
                        ],
                        [0.5; 4],
                        [0., 0., 0., 1.],
                    );
                    // Rows fade from opaque at the left to clear at
                    // their moving endpoint, including endpoints offscreen.
                    batch.colors[start + 1][3] = 0.;
                    batch.colors[start + 2][3] = 0.;
                }
            }
        }
        if let Some(color) = cover.filter(|color| color.w > 0.) {
            batch.quad(self.resolution.ui_rect(), [0.5; 4], color.to_array());
        }
        layer.upload(batch, commands)?;
        Ok(())
    }

    pub fn despawn(self, world: &mut World) {
        for entity in self.entities() {
            resident::retire(world, entity);
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
        overlays::advance_tp_warnings(&mut self.tp_warnings, frame, elapsed)?;
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
                        *actor,
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
                    overlays::condition(&mut self.labels, *actor, *kind, *position);
                }
                resonance_battle::Cue::OverLimitEntered { actor, position } => {
                    ensure!(
                        actor.index() < frame.actors.len(),
                        "unknown Over Limit label actor"
                    );
                    overlays::request_at(
                        &mut self.labels,
                        *actor,
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
                        *actor,
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
                        *actor,
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
            DefeatNotice => {
                self.notices.outcome = Some((resonance_battle::BattleResult::Defeat, 0))
            }
            EscapeNotice => {
                self.notices.outcome = Some((resonance_battle::BattleResult::Escaped, 0))
            }
            // Scene transitions and prepared effects own these outcomes.
            VictoryBanner => {}
            _ => anyhow::bail!("request is not a battle overlay"),
        }
        Ok(())
    }
    fn render_overlays(
        &mut self,
        frames: (&BattleFrame, &BattleFrame),
        time: HudTime,
        commands: &mut Commands,
    ) -> Result<()> {
        let mut panel = Batch::default();
        let mut text = Batch::default();
        let next = time.next.map(|n| (&n.frame, time.visit));
        for (&owner, label) in &self.labels {
            let projected = overlays::project_anchor(frames, next, |_| Some(label.position))?;
            label.draw(
                &self.art,
                if time
                    .next
                    .is_some_and(|n| label.runs_during_hold(owner, n.frame.clock))
                {
                    time.visit
                } else {
                    time.feedback
                },
                projected,
                &mut panel,
                &mut text,
            )?;
        }
        for (&actor, warning) in &self.tp_warnings {
            let projected = overlays::project_anchor(frames, next, |frame| {
                Some(frame.actors.get(actor.index())?.condition_label_origin())
            })?;
            warning.draw(&self.art.font, projected, &mut text)?;
        }
        for (layer, batch) in self.ui.labels.iter_mut().zip([panel, text]) {
            layer.upload(batch, commands)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn test_tiled_surface(image: Handle<Image>, size: [u32; 2]) -> Surface {
    let mut materials = Assets::default();
    let layer = HudLayer::load(
        LayerDefinition::new(image.clone(), size, Depth::Notice),
        &image,
        &mut materials,
        &Resident::default(),
    );
    materials.get(&layer.material).unwrap().clone()
}

#[cfg(test)]
fn test_artwork() -> Result<(Artwork, Assets<Surface>, App)> {
    test_artwork_with(
        resonance_content::battle_ui::document::PATH,
        |_| {},
        &resonance_content::diagnostics::Diagnostics::new(true),
    )
}

#[cfg(test)]
fn test_artwork_with(
    document_path: &str,
    edit: impl FnOnce(&mut serde_json::Value),
    diagnostics: &resonance_content::diagnostics::Diagnostics,
) -> Result<(Artwork, Assets<Surface>, App)> {
    let root = fixtures::asset_root();
    let mut snapshot = resonance_content::library::Library::open(&root, &[])?.snapshot();
    let mut document: serde_json::Value =
        snapshot.json(document_path, resonance_content::preparation::JSON_LIMIT)?;
    edit(&mut document);
    resonance_content::preparation::gameplay(&mut snapshot, diagnostics, &|| false)?;
    resonance_content::preparation::ui(&mut snapshot, diagnostics, &|| false)?;
    let art = resonance_content::battle_ui::document::read(
        &mut |path| {
            if path == document_path {
                return Ok(serde_json::to_vec(&document)?.into());
            }
            snapshot.read(path, resonance_content::preparation::ASSET_LIMIT)
        },
        diagnostics,
    )?;
    snapshot.read(
        &art.font.texture,
        resonance_content::preparation::ASSET_LIMIT,
    )?;
    let files =
        resonance_content::prepared::Files::from_snapshot(snapshot, || false, diagnostics.clone())?;
    let session = std::sync::Arc::new(resonance_content::session::SessionData::load(&files)?);
    let menu_data = session
        .rules
        .as_ref()
        .context("missing session rules")?
        .clone();
    let mut app = App::new();
    crate::texture::decoded_files(&mut app, &root);
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
        session,
        |path| {
            ensure!(
                ![
                    resonance_content::battle_ui::document::PATH,
                    resonance_content::menu_data::GRADE_SHOP_PATH,
                    resonance_content::menu_data::CRAFTING_PATH,
                    resonance_content::menu_data::WORLD_MAP_PATH,
                    resonance_content::menu_data::PRESENTATION_PATH,
                    resonance_content::menu_data::MONSTER_BOOK_PATH,
                    resonance_content::characters::PATH,
                    resonance_content::items::PATH,
                ]
                .contains(&path),
                "HUD reread a prepared descriptor"
            );
            Ok(files.read(path)?.to_vec())
        },
        &[],
        false,
        &server,
        &mut materials,
        &mut app.world_mut().resource_mut::<Assets<Image>>(),
        diagnostics,
        &Resident::default(),
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
        ui_image(
            &font.texture,
            [font.width, font.height],
            false,
            bevy::image::CompressedImageFormats::NONE,
            &read,
        )?;
        assert!(
            ui_image(
                &font.texture,
                [font.width, font.height],
                false,
                bevy::image::CompressedImageFormats::NONE,
                &|_| anyhow::bail!("missing font")
            )
            .is_err()
        );
        assert!(
            ui_image(
                &font.texture,
                [font.width, font.height],
                false,
                bevy::image::CompressedImageFormats::NONE,
                &|_| Ok(b"invalid image".to_vec())
            )
            .is_err()
        );
        font.width += 1;
        // Authored coordinates may differ from the stored image resolution.
        ui_image(
            &font.texture,
            [font.width, font.height],
            false,
            bevy::image::CompressedImageFormats::NONE,
            &read,
        )?;
    }
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    assert!(
        test_artwork_with(
            resonance_content::battle_ui::document::FONT,
            |font| font["texture"] = serde_json::json!("ui/dialogue.json"),
            &diagnostics
        )
        .is_err()
    );
    assert!(
        test_artwork_with(
            resonance_content::battle_ui::document::FONT,
            |font| font["width"] = serde_json::json!(font["width"].as_u64().unwrap() + 1),
            &diagnostics
        )
        .is_ok()
    );
    for character in ['X', 'V', 'F'] {
        let missing_glyph = test_artwork_with(
            resonance_content::battle_ui::document::FONT,
            |font| {
                let mut bitmap: BitmapFont = serde_json::from_value(font.clone()).unwrap();
                bitmap.glyphs.remove(&character).unwrap();
                *font = serde_json::to_value(bitmap).unwrap();
            },
            &diagnostics,
        );
        match missing_glyph {
            Err(error) => assert!(
                error
                    .to_string()
                    .contains(&format!("glyph {character:?} was not prepared"))
            ),
            Ok(_) => panic!("HUD admitted a font missing required glyph {character:?}"),
        }
    }
    Ok(())
}

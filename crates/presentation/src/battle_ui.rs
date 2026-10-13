//! Party HUD composed on bitmap UI surfaces.
use super::*;
use anyhow::ensure;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use resonance_battle::{Activity, BattleFrame, Side};
use resonance_content::battle_ui::Art;
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

/// Party-owned state consumed by the ordinary battle HUD rows.
#[derive(Clone, Copy)]
pub(crate) struct PartyHudInput<'a> {
    pub characters: &'a [u8],
    pub queued_techniques: &'a [bool],
    pub names: &'a [Option<&'a str>],
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
    unison: unison::State,
    cutin: cutin::State,
    command_motion: command_strip::Motion,
}

impl Artwork {
    pub(crate) fn portrait_observation(
        &self,
        actor: &resonance_battle::ActorFrame,
        result: Option<resonance_battle::BattleResult>,
    ) -> party::Portrait {
        party::portrait(actor, result)
    }

    /// Build surfaces from the artwork and menu catalogue admitted by battle preparation.
    #[expect(
        clippy::too_many_arguments,
        reason = "Preparation borrows independently owned content and runtime resources."
    )]
    pub fn load(
        mut art: Art,
        menu_data: std::sync::Arc<resonance_content::menu_data::MenuData>,
        session: std::sync::Arc<resonance_content::session::SessionData>,
        read: impl Fn(&str) -> Result<Vec<u8>>,
        enemies: &[EnemyHud],
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
        for enemy in enemies {
            font.validate_text(&enemy.name)?;
        }
        art.font.validate_text("HP TP +0123456789.")?;
        let atlas = image_assets.add(ui_image(
            &art.font.texture,
            [art.font.width, art.font.height],
            false,
            &read,
        )?);
        let main_font = image_assets.add(ui_image(
            &font.texture,
            [font.width, font.height],
            false,
            &read,
        )?);
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
        let mut load = |texture: &resonance_content::font::UiTexture, repeat| -> Result<_> {
            Ok(diagnostics
                .attempt(
                    "optional battle HUD artwork",
                    ui_image(
                        &texture.path,
                        [texture.width, texture.height],
                        repeat,
                        &read,
                    ),
                )?
                .map(|image| image_assets.add(image)))
        };
        let mut portraits = Vec::new();
        for portrait in &mut art.portraits {
            let layer = if let Some(texture) = portrait
                && let Some(image) = load(texture, true)?
            {
                LayerDefinition::new(image, [texture.width, texture.height], Depth::Party)
                    .foreground()
            } else {
                *portrait = None;
                solid(Depth::Party)
            };
            portraits.push(layer);
        }
        let stat_icons = art
            .stat_icons
            .iter()
            .enumerate()
            .map(|(index, texture)| {
                let image = load(texture, false)?.context("battle stat icon atlas is missing")?;
                let definition =
                    LayerDefinition::new(image, [texture.width, texture.height], Depth::Vitals);
                Ok(if index == 2 {
                    definition.foreground()
                } else {
                    definition
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let notice = notice::layers(&art, enemies, &palette, materials, &mut load)?;
        let unison = unison::layers(&art, &palette, &notice, materials, &mut load)?;
        let scan = items::layers(&art, &palette, materials, &mut load)?;
        let command_texture =
            load(&art.commands.texture, false)?.context("battle command atlas is missing")?;
        let mut make = |definition| palette.layer(definition, materials);
        let ui = UiLayers {
            notice,
            labels: [make(solid(Depth::Labels)), make(text(Depth::Labels))],
            gauges: make(solid(Depth::Vitals)),
            font: make(text(Depth::Vitals)),
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
            results: [
                make(solid(Depth::Results)),
                make(main_text(Depth::Results)),
                make(text(Depth::Results)),
            ],
            portraits: palette.layers(portraits, materials),
            stat_icons: palette.layers(stat_icons, materials),
            unison,
        };
        let combat = combat::Hud::load(&art, enemies, &palette, materials, &mut load)?;
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
            unison: Default::default(),
            cutin: Default::default(),
            command_motion: Default::default(),
        })
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
    pub fn render(
        &mut self,
        frame: &BattleFrame,
        party: PartyHudInput<'_>,
        command: Option<&resonance_game::battle::command::Frame>,
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
        if self.results.is_some() {
            for layer in self
                .ui
                .labels
                .iter_mut()
                .chain([&mut self.ui.floating, &mut self.ui.recovery])
            {
                layer.show(false, commands);
            }
        } else {
            self.render_overlays(frame, commands, meshes)?;
            self.render_numbers(frame, commands, meshes)?;
        }
        self.render_notices(frame, party, commands, meshes)?;
        self.render_combat(frame, command, commands, meshes)?;
        self.render_scan(frame, command, commands, meshes)?;
        self.render_unison_attack(frame, party, commands, meshes)?;
        self.cutin.render(commands, meshes)?;
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
        heads: &BTreeMap<i32, Vec3>,
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
            heads,
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
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let layer = &mut self.ui.transition;
        let mut batch = Batch::default();
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

/// Keep floating feedback inside the play area above the party cards.
fn floating_origin([x, y]: [f32; 2], [width, height]: [f32; 2]) -> [f32; 2] {
    const MARGIN: f32 = 8.;
    [
        (x - width / 2.).clamp(MARGIN, 640. - MARGIN - width),
        (y - height).clamp(MARGIN, party::TOP - MARGIN - height),
    ]
}

fn hud_color(color: [u8; 4]) -> [f32; 4] {
    std::array::from_fn(|channel| {
        f32::from(color[channel]) / 255. * if channel == 3 { 1. } else { 2. }
    })
}

impl Artwork {
    /// Consume a completed update once. Drawing never ages feedback.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<()> {
        let elapsed = u32::from(!paused);
        self.cutin.advance(frame)?;
        self.advance_scan(frame, elapsed);
        self.numbers.advance(frame, elapsed);
        self.combat.advance(frame, elapsed)?;
        self.notices.advance(frame, elapsed);
        self.labels.retain_mut(|(_, label)| {
            label.advance(elapsed);
            label.visible()
        });
        // Apply this update's notifications after advancing existing labels.
        for cue in &frame.cues {
            match cue {
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
            // Scene transitions and prepared effects already own outcome presentation.
            VictoryBanner | DefeatNotice | EscapeNotice => {}
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
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>();
    let server = app.world().resource::<AssetServer>().clone();
    let mut materials = Assets::<Surface>::default();
    let artwork = Artwork::load(
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
        &server,
        &mut materials,
        &mut app.world_mut().resource_mut::<Assets<Image>>(),
        diagnostics,
    )?;
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
    Ok(())
}

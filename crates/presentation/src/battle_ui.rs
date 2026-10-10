//! Party HUD composed on bitmap UI surfaces.
use super::*;
use anyhow::ensure;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use resonance_battle::{
    Activity, BattleFrame, Side,
    conditions::{Condition, ConditionSet},
};
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

const POISON: ConditionSet = ConditionSet::of(&[Condition::PoisonMild, Condition::PoisonSevere]);

#[path = "battle_ui/combat.rs"]
mod combat;
#[path = "battle_ui/combat_numbers.rs"]
mod combat_numbers;
#[path = "battle_ui/command_strip.rs"]
mod command_strip;
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
#[path = "battle_ui/selector.rs"]
mod selector;
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
    flash: Option<overlays::Flash>,
    combat: combat::Hud,
    numbers: combat_numbers::Numbers,
    scan: Option<items::Scan>,
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
        command_strip::validate_font(&font)?;
        items::validate_font(&font)?;
        for enemy in enemies {
            font.validate_text(&enemy.name)?;
        }
        art.font
            .validate_text("HP TP +0123456789. DOWN STONE POISON WEAK QUEUED STORED READY")?;
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
            solid: LayerDefinition::new(white, [1, 1], Depth::Flash),
            font: LayerDefinition::new(atlas, [art.font.width, art.font.height], Depth::Flash),
            main_font: LayerDefinition::new(main_font, [font.width, font.height], Depth::Flash),
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
        let mut make = |definition| palette.layer(definition, materials);
        let ui = UiLayers {
            flash: make(solid(Depth::Flash)),
            notice: [make(solid(Depth::Notice)), make(main_text(Depth::Notice))],
            labels: [make(solid(Depth::Labels)), make(text(Depth::Labels))],
            gauges: make(solid(Depth::Party)),
            font: make(text(Depth::Vitals)),
            party_names: make(main_text(Depth::Names)),
            floating: make(text(Depth::Meters)),
            recovery: make(text(Depth::Recovery)),
            selector: [
                make(solid(Depth::Selector)),
                make(main_text(Depth::Selector)),
            ],
            scan: [make(solid(Depth::Scan)), make(main_text(Depth::Scan))],
            transition: make(solid(Depth::Transition)),
            commands: [
                make(solid(Depth::Commands)),
                make(main_text(Depth::Commands)),
            ],
            results: [make(solid(Depth::Results)), make(main_text(Depth::Results))],
            portraits: palette.layers(portraits, materials),
        };
        let combat = combat::Hud::load(&art, enemies, &palette, materials, &mut load)?;
        let windows = menu::MenuArtwork::load(
            &read,
            session.experience.clone().into(),
            server,
            materials,
            (&ui.results[1].material, [font.width, font.height]),
            diagnostics,
        )?;
        Ok(Self {
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
            flash: None,
            combat,
            numbers: Default::default(),
            scan: None,
        })
    }

    /// Call before activation; the scene loader owns GPU readiness and warmup.
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        for layer in self.layers_mut() {
            layer.prepare(commands, meshes);
        }
        self.windows.prepare_windows();
    }

    pub(crate) fn menu_images_ready(
        &self,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        self.windows.drawn_images_ready(images, server)
    }
    fn layers(&self) -> impl Iterator<Item = &HudLayer> {
        self.ui.iter().chain(&self.combat.layers)
    }
    fn layers_mut(&mut self) -> impl Iterator<Item = &mut HudLayer> {
        self.ui.iter_mut().chain(&mut self.combat.layers)
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
        self.render_party(frame, party, commands, meshes)?;
        self.render_results(commands, meshes)?;
        if self.results.is_some() {
            for layer in self.ui.labels.iter_mut().chain([
                &mut self.ui.flash,
                &mut self.ui.floating,
                &mut self.ui.recovery,
            ]) {
                layer.show(false, commands);
            }
        } else {
            self.render_overlays(frame, commands, meshes)?;
            self.render_numbers(frame, commands, meshes)?;
        }
        self.render_notices(frame, party, commands, meshes)?;
        self.render_combat(frame, commands, meshes)?;
        self.render_selector(frame, command, commands, meshes)?;
        if command.is_none() {
            self.render_scan(frame, commands, meshes)?;
        } else {
            for layer in &mut self.ui.scan {
                layer.show(false, commands);
            }
        }
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
        let layer = &mut self.ui.transition;
        let mut batch = Batch::default();
        if let Some(frame) = frame.filter(|frame| frame.alpha != 0) {
            batch.quad(
                [0., 0., 640., 480.],
                [0.5; 4],
                hud_color([frame.color[0], frame.color[1], frame.color[2], frame.alpha]),
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
        self.flash = self.flash.and_then(|mut flash| {
            flash.age += elapsed;
            (flash.alpha() > 0.).then_some(flash)
        });
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
                    self.flash = Some(overlays::Flash {
                        age: 0,
                        color: [1., 0.8, 0.35],
                    });
                }
                resonance_battle::Cue::Rescued { .. } => {
                    self.flash = Some(overlays::Flash {
                        age: 0,
                        color: [0.6, 0.85, 1.],
                    });
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
            VictoryBanner => {
                self.flash = Some(overlays::Flash {
                    age: 0,
                    color: [1., 0.95, 0.75],
                })
            }
            DefeatNotice => self.notices.banner = Some("PARTY DEFEATED"),
            EscapeNotice => self.notices.banner = Some("ESCAPED"),
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
        let mut flash = Batch::default();
        if let Some(state) = &self.flash {
            state.draw(&mut flash);
        }
        self.ui.flash.upload(flash, commands, meshes)?;
        let mut panel = Batch::default();
        let mut text = Batch::default();
        for (_, label) in &self.labels {
            let camera = frame.camera.as_ref().context("actor label has no camera")?;
            let projected = resonance_battle::project_screen_point(*camera, label.position);
            label.draw(&self.art.font, projected, &mut panel, &mut text)?;
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

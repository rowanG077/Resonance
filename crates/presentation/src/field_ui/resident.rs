//! Fixed UI graphics. Views borrow canvases; controllers own no cache policy.
use super::*;
#[cfg(all(test, feature = "preparation-trace"))]
#[path = "resident_probe.rs"]
mod probe;
#[cfg(test)]
#[path = "resident_scenario.rs"]
mod scenario;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

#[derive(Component)]
pub(super) struct Canvas;

#[derive(Resource, Clone, Default)]
pub(crate) struct Resident(pub(super) Arc<Mutex<Inventory>>);

#[derive(Default)]
pub(super) struct Inventory {
    pub menu: Option<menu::MenuArtwork>,
    pub menus: HashMap<(u8, usize, menu::DrawRole, menu::MaterialKey), Layer>,
    pub field: Option<Artwork>,
    pub world: Option<super::world::Graphics>,
    pub hud: HashMap<battle_ui::GraphicsKey, Vec<Layer>>,
    pub dialogue: Option<DialogueOverlay>,
    pub game_over: Option<GameOverArt>,
    images: Option<[Handle<Image>; 2]>,
}

impl Resident {
    pub(super) fn downgrade(&self) -> Weak<Mutex<Inventory>> {
        Arc::downgrade(&self.0)
    }

    pub(super) fn images(&self, images: &mut Assets<Image>) -> [Handle<Image>; 2] {
        self.0
            .lock()
            .unwrap()
            .images
            .get_or_insert_with(|| {
                use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
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
                [images.add(white), images.add(nearest)]
            })
            .clone()
    }

    pub(crate) fn game_over(&self) -> Option<GameOverArt> {
        self.0.lock().unwrap().game_over.clone()
    }

    pub(super) fn menu_canvas(
        &self,
        band: u8,
        key: (usize, menu::DrawRole, menu::MaterialKey),
    ) -> Option<Layer> {
        self.0
            .lock()
            .unwrap()
            .menus
            .get(&(band, key.0, key.1, key.2))
            .cloned()
    }

    pub(super) fn field(
        &self,
        files: &resonance_content::prepared::Files,
        overlays: &BTreeMap<i32, String>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        images: &mut Assets<Image>,
    ) -> Result<Option<Artwork>> {
        let inventory = self.0.lock().unwrap();
        let Some(common) = &inventory.field else {
            return Ok(None);
        };
        let inputs = common.menu.prepared_inputs();
        Ok(Some(Artwork {
            resolution: Default::default(),
            attached_positions: Default::default(),
            font: common.font.clone(),
            spec: common.spec.clone(),
            images: common.images.clone(),
            surfaces: common.surfaces.clone(),
            layers: common.layers.clone(),
            head_heights: Default::default(),
            choice_trail: Default::default(),
            subtitles: common.subtitles.clone(),
            subtitle_layer: common.subtitle_layer.clone(),
            fade_surface: common.fade_surface.clone(),
            fade_layer: common.fade_layer.clone(),
            overlays: overlay::Artwork::load(
                overlays,
                |path| Ok(files.read(path)?.to_vec()),
                server,
                materials,
                images,
                &inputs,
            )?,
            menu: common.menu.fresh(common.menu.experience()),
            prompt_layers: common.prompt_layers.clone(),
            damage_layer: common.damage_layer.clone(),
            skits: common.skits.clone(),
            credits: None,
        }))
    }

    pub(crate) fn check(&self, world: &World) -> Result<()> {
        for entity in self.entities() {
            anyhow::ensure!(
                world.get::<Canvas>(entity).is_some(),
                "common UI canvas was released"
            );
            let mesh = world
                .get::<Mesh2d>(entity)
                .context("common UI mesh binding missing")?;
            let material = world
                .get::<MeshMaterial2d<Surface>>(entity)
                .context("common UI material binding missing")?;
            anyhow::ensure!(
                world.resource::<Assets<Mesh>>().contains(&mesh.0),
                "common UI mesh missing"
            );
            let surface = world
                .resource::<Assets<Surface>>()
                .get(&material.0)
                .context("common UI material missing")?;
            anyhow::ensure!(
                surface.images_ready(world.resource::<Assets<Image>>()),
                "common UI image missing"
            );
        }
        Ok(())
    }

    pub(super) fn entities(&self) -> Vec<Entity> {
        self.canvases()
            .into_iter()
            .map(|(entity, _)| entity)
            .collect()
    }
    fn canvases(&self) -> Vec<(Entity, bevy::camera::visibility::RenderLayers)> {
        use bevy::camera::visibility::RenderLayers;
        let battle = || RenderLayers::layer(crate::battle_view::LAYER);
        let i = self.0.lock().unwrap();
        i.menus
            .iter()
            .map(|(&(band, _, _, _), l)| {
                (
                    l.entity,
                    if band == menu::DepthBand::Field as u8 {
                        RenderLayers::default()
                    } else {
                        battle()
                    },
                )
            })
            .chain(i.hud.values().flatten().map(|l| (l.entity, battle())))
            .chain(i.field.iter().flat_map(|a| {
                a.common_layers().map(move |l| {
                    let layers = if a
                        .subtitle_layer
                        .as_ref()
                        .is_some_and(|subtitle| subtitle.entity == l.entity)
                    {
                        RenderLayers::layer(3)
                    } else {
                        RenderLayers::default()
                    };
                    (l.entity, layers)
                })
            }))
            .chain(
                i.world
                    .iter()
                    .flat_map(|g| g.layers().map(|l| (l.entity, RenderLayers::default()))),
            )
            .chain(
                i.dialogue
                    .iter()
                    .flat_map(DialogueOverlay::entities)
                    .map(|e| (e, battle())),
            )
            .chain(
                i.game_over
                    .iter()
                    .flat_map(|a| a.layers.iter().map(|l| (l.entity, battle()))),
            )
            .collect()
    }
}

pub(super) fn retire(world: &mut World, entity: Entity) {
    if world.get::<Canvas>(entity).is_some() {
        world.entity_mut(entity).insert(Visibility::Hidden);
    } else {
        world.despawn(entity);
    }
}

pub(crate) struct Warm {
    shared: crate::gpu_prepare::Shared,
    field: Option<Artwork>,
    battle: Option<BattleHud>,
    queued: bool,
}

impl Warm {
    pub(crate) fn new(world: &mut World, common: &crate::loading::startup::Common) -> Result<Self> {
        let resident = world.resource::<Resident>().clone();
        let server = world.resource::<AssetServer>().clone();
        let mut field = world.resource_scope(|world, mut materials: Mut<Assets<Surface>>| {
            Artwork::load_shared(
                &common.files,
                common.skits.clone(),
                common.data.experience.clone().into(),
                &BTreeMap::new(),
                &server,
                &mut materials,
                &mut world.resource_mut::<Assets<Image>>(),
                &resident,
            )
        })?;
        field.subtitles = subtitle_cues(&common.files)?;
        let battle = world.resource_scope(|world, mut materials: Mut<Assets<Surface>>| {
            let mut hud = BattleHud::load(
                common.battle_ui.clone(),
                common
                    .data
                    .rules
                    .as_ref()
                    .context("common UI menu rules missing")?
                    .clone(),
                common.data.clone(),
                |path| Ok(common.files.read(path)?.to_vec()),
                &[],
                false,
                &server,
                &mut materials,
                &mut world.resource_mut::<Assets<Image>>(),
                common.files.diagnostics(),
                &resident,
            )?;
            for (id, texture) in common.cutins.iter().enumerate() {
                hud.add_cutin(
                    resonance_battle::TechniqueId(u16::try_from(id)?),
                    texture,
                    &mut materials,
                    &server,
                    false,
                );
            }
            Ok::<_, anyhow::Error>(hud)
        })?;
        Ok(Self {
            shared: crate::gpu_prepare::Shared::new(world),
            field: Some(field),
            battle: Some(battle),
            queued: false,
        })
    }

    pub(crate) fn update(&mut self, world: &mut World) -> Result<bool> {
        use std::sync::atomic::Ordering;
        let resident = world.resource::<Resident>().clone();
        if self.queued {
            {
                let report = self.shared.0.lock().unwrap();
                if let Some(error) = &report.error {
                    anyhow::bail!("common UI pipeline: {error}")
                }
                if !report.completed.load(Ordering::Acquire) {
                    return Ok(false);
                }
            }
            resident.check(world)?;
            return Ok(true);
        }
        let field = self.field.as_mut().unwrap();
        let battle = self.battle.as_mut().unwrap();
        {
            let images = world.resource::<Assets<Image>>();
            let server = world.resource::<AssetServer>();
            for image in &field.images {
                if !crate::field_view::image_ready(server, images, image)? {
                    return Ok(false);
                }
            }
        }
        let common = world
            .resource::<crate::loading::Resident>()
            .common
            .get()
            .unwrap()
            .clone();
        let server = world.resource::<AssetServer>().clone();
        if !world.resource_scope(|world, mut materials: Mut<Assets<Surface>>| {
            battle.images_ready(
                &server,
                &mut world.resource_mut::<Assets<Image>>(),
                &mut materials,
                common.files.diagnostics(),
            )
        })? {
            return Ok(false);
        }
        if !field.menu.warm_canvases(&resident, world)? {
            return Ok(false);
        }
        // World UI is speculative here, including its declarations. Live world
        // entry still prepares it normally when this preload leaves a miss.
        let world_images = common
            .files
            .diagnostics()
            .attempt("startup world UI", field.menu.prepare_world_maps())?;
        let warm_world = world_images.is_some_and(|images| {
            images.iter().all(|image| {
                crate::field_view::image_ready(&server, world.resource::<Assets<Image>>(), image)
                    .unwrap_or(false)
            })
        });
        let mut dialogue = DialogueOverlay {
            images: field.images.clone(),
            surfaces: field.surfaces.clone(),
            layers: Default::default(),
            head_heights: Default::default(),
            choice_trail: Default::default(),
        };
        let texture_formats = world
            .get_resource::<bevy::image::CompressedImageFormatSupport>()
            .map_or(bevy::image::CompressedImageFormats::NONE, |formats| {
                formats.0
            });
        let game_over_data =
            resonance_game::game_over::Assets::from_prepared((*common.files).clone())?;
        let mut game_over =
            world.resource_scope(|world, mut materials: Mut<Assets<Surface>>| {
                GameOverArt::load(
                    &game_over_data,
                    &mut materials,
                    &mut world.resource_mut::<Assets<Image>>(),
                    texture_formats,
                )
            })?;
        let resolution = world.resource::<crate::display::Display>().0;
        let world_graphics = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            world.resource_scope(|world, mut materials: Mut<Assets<Surface>>| {
                field.prepare(&mut world.commands(), &mut meshes, &mut materials);
                dialogue.prepare(&mut world.commands(), &mut meshes, &mut materials);
                battle.prepare(&mut world.commands(), &mut meshes, resolution);
                game_over.prepare(&mut world.commands(), &mut meshes);
                if warm_world {
                    common.files.diagnostics().attempt(
                        "startup world UI",
                        super::world::Graphics::prepare(
                            &field.menu,
                            &field.surfaces[9],
                            &mut world.commands(),
                            &mut meshes,
                        ),
                    )
                } else {
                    Ok(None)
                }
            })
        })?;
        world.flush();
        battle.remember_graphics(&resident);
        {
            let mut i = resident.0.lock().unwrap();
            i.field = self.field.take();
            i.world = world_graphics;
            i.dialogue = Some(dialogue);
            i.game_over = Some(game_over);
        }
        // The inventory's HUD layers own the meshes, materials and images.
        self.battle = None;
        let mut bindings = Vec::new();
        for (entity, layers) in resident.canvases() {
            world.entity_mut(entity).insert((
                Canvas,
                layers,
                Visibility::Hidden,
                bevy::camera::visibility::NoFrustumCulling,
            ));
            bindings.push((
                world.get::<Mesh2d>(entity).unwrap().0.id(),
                world
                    .get::<MeshMaterial2d<Surface>>(entity)
                    .unwrap()
                    .0
                    .id()
                    .untyped(),
            ));
        }
        let samples = world
            .resource::<crate::anti_aliasing::AntiAliasingCapabilities>()
            .effective(
                world
                    .resource::<crate::anti_aliasing::AntiAliasingSettings>()
                    .0,
            )
            .samples();
        let mut report = self.shared.0.lock().unwrap();
        *report = crate::gpu_prepare::Report {
            armed: true,
            ..Default::default()
        };
        for (mesh, material) in bindings {
            report.prepare(
                crate::gpu_prepare::View::Field,
                mesh,
                material,
                true,
                Some(samples),
            );
        }
        self.queued = true;
        Ok(false)
    }
}

#[cfg(test)]
#[path = "resident_tests.rs"]
mod tests;

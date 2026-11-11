//! One fixed owner for common GPU assets. Scene instances keep their own state.
use super::*;
use crate::field_view::Part;
use crate::gpu_prepare as gpu;
use bevy::image::ImageLoaderSettings;
use resonance_game::battle::encounter::CommonModelKind;
use std::collections::HashSet;

#[derive(Resource)]
struct Graphics {
    shared: Batch,
    battle: Batch,
}

pub(super) struct Batch {
    uploads: Arc<Mutex<gpu::Report>>,
    parts: Vec<Model>,
    images: Vec<Handle<Image>>,
    loads: LoadTasks,
    since: Instant,
    diagnostics: Option<resonance_content::diagnostics::Diagnostics>,
}

pub(crate) struct Model {
    // Failed optional models keep their slot so incremental pipeline warmup
    // can visit the batch in order while other models are still resolving.
    pub part: Option<Part>,
    pub additive: bool,
    pub kind: CommonModelKind,
}

pub(super) fn begin(world: &mut World, common: &startup::Common) -> Result<()> {
    let mut parts = Vec::new();
    let mut textures = HashSet::new();
    parts.extend(
        common
            .field
            .iter()
            .flat_map(|field| &field.actors)
            .flat_map(|actor| actor.parts.iter()),
    );
    if let Some(package) = &common.world {
        let definition = &package.world.definition;
        parts.extend(
            definition
                .worlds
                .iter()
                .flatten()
                .flat_map(|tile| std::iter::once(&tile.base).chain(&tile.alternate))
                .flat_map(|tile| tile.parts.iter()),
        );
        let visuals = &definition.visuals;
        parts.extend(visuals.actors.values().flatten());
        parts.extend(
            visuals
                .markers
                .iter()
                .flat_map(|world| world.values().flatten()),
        );
        parts.extend(visuals.skies.iter().flatten());
        parts.extend(
            visuals
                .cinematics
                .values()
                .flat_map(|scene| scene.actors.values().flatten()),
        );
        textures.extend(visuals.dust.iter().cloned());
    }
    let shared = Batch::optional(
        world,
        parts
            .into_iter()
            .map(|part| (part, false, CommonModelKind::Body)),
        textures,
        common.files.clone(),
    )?;
    let battle = Batch::optional(
        world,
        common
            .battle
            .iter()
            .flat_map(|battle| battle.render_layers()),
        common
            .battle
            .iter()
            .flat_map(|battle| battle.render_images())
            .map(str::to_owned),
        common.files.clone(),
    )?;
    world.insert_resource(Graphics { shared, battle });
    Ok(())
}

impl Batch {
    pub(super) fn new<'a>(
        world: &World,
        parts: impl IntoIterator<Item = &'a resonance_content::ScenePart>,
        textures: impl IntoIterator<Item = String>,
        files: Arc<Files>,
    ) -> Self {
        Self::load(
            world,
            parts
                .into_iter()
                .map(|part| (part, false, CommonModelKind::Body)),
            textures,
            files,
        )
    }

    fn load<'a>(
        world: &World,
        parts: impl IntoIterator<Item = (&'a resonance_content::ScenePart, bool, CommonModelKind)>,
        textures: impl IntoIterator<Item = String>,
        files: Arc<Files>,
    ) -> Self {
        let loads = world.resource::<Resident>().scope(files);
        let server = world.resource::<AssetServer>();
        let parts = parts
            .into_iter()
            .map(|(part, additive, kind)| Model {
                part: Some(Part::load(geometry_only(part), server, &loads)),
                additive,
                kind,
            })
            .collect();
        let images = textures
            .into_iter()
            .map(|path| {
                server
                    .load_builder()
                    .with_guard(loads.ticket())
                    .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = false)
                    .load(path)
            })
            .collect();
        let uploads = Arc::new(Mutex::new(gpu::Report::default()));
        world.resource::<crate::gpu_prepare::Registry>().watch(
            &uploads,
            |report| report,
            None,
            |report, _, observer| observer.uploaded(report),
        );
        Self {
            parts,
            images,
            loads,
            since: Instant::now(),
            uploads,
            diagnostics: None,
        }
    }

    // Prewarming art follows the selected scene's diagnostic policy.
    // A rejected warmup never removes the source from a later live encounter.
    fn optional<'a>(
        world: &World,
        parts: impl IntoIterator<Item = (&'a resonance_content::ScenePart, bool, CommonModelKind)>,
        textures: impl IntoIterator<Item = String>,
        files: Arc<Files>,
    ) -> Result<Self> {
        let diagnostics = files.diagnostics();
        let mut prepared = Vec::new();
        for (part, additive, kind) in parts {
            let inputs = std::iter::once(&part.mesh).chain(
                part.materials
                    .iter()
                    .flat_map(|material| material.color.iter().chain(&material.multiply))
                    .map(|binding| &part.textures[binding.texture]),
            );
            if diagnostics
                .attempt(
                    "common graphics",
                    inputs
                        .into_iter()
                        .try_for_each(|path| files.read(path).map(|_| ())),
                )?
                .is_some()
            {
                prepared.push((part, additive, kind));
            }
        }
        let mut images = Vec::new();
        for path in textures {
            if diagnostics
                .attempt("common graphics", files.read(&path))?
                .is_some()
            {
                images.push(path);
            }
        }
        let diagnostics = diagnostics.clone();
        let mut batch = Self::load(world, prepared, images, files);
        batch.diagnostics = Some(diagnostics);
        Ok(batch)
    }

    pub(super) fn update(&mut self, world: &mut World) -> Result<bool> {
        // A finished batch keeps being polled while other startup stages run.
        if self
            .uploads
            .lock()
            .unwrap()
            .completed
            .load(Ordering::Acquire)
        {
            return Ok(true);
        }
        anyhow::ensure!(
            self.since.elapsed().as_secs() < 120,
            "GPU upload batch timed out"
        );
        if self.uploads.lock().unwrap().armed {
            return Ok(false);
        }
        let mut state =
            bevy::ecs::system::SystemState::<crate::model_asset::ModelAssets>::new(world);
        let mut assets = state.get_mut(world)?;
        let mut ready = self.loads.complete();
        for model in &mut self.parts {
            let Some(part) = &mut model.part else {
                continue;
            };
            match part.resolve(&mut assets) {
                Ok(loaded) => {
                    ready &= loaded;
                }
                Err(error) => {
                    match &self.diagnostics {
                        Some(diagnostics) => diagnostics.report("common graphics", error)?,
                        None => return Err(error),
                    }
                    model.part = None;
                }
            }
        }
        let mut index = 0;
        while index < self.images.len() {
            match crate::field_view::image_ready(
                &assets.server,
                &assets.images,
                &self.images[index],
            ) {
                Ok(loaded) => {
                    ready &= loaded;
                    index += 1;
                }
                Err(error) => {
                    match &self.diagnostics {
                        Some(diagnostics) => diagnostics.report("common graphics", error)?,
                        None => return Err(error),
                    }
                    self.images.swap_remove(index);
                }
            }
        }
        if !ready {
            return Ok(false);
        }
        let mut uploads = self.uploads.lock().unwrap();
        for part in self.parts.iter().filter_map(|model| model.part.as_ref()) {
            uploads.meshes.extend(
                part.layout
                    .as_ref()
                    .unwrap()
                    .meshes
                    .iter()
                    .flatten()
                    .map(Handle::id),
            );
            uploads.images.extend(
                part.materials
                    .iter()
                    .flat_map(|material| material.sampled.iter().flatten().map(Handle::id)),
            );
        }
        uploads.images.extend(self.images.iter().map(Handle::id));
        info!(
            meshes = uploads.meshes.len(),
            images = uploads.images.len(),
            "GPU assets prepared; waiting for uploads"
        );
        uploads.armed = true;
        Ok(false)
    }
}

pub(super) fn update(world: &mut World) -> Result<bool> {
    world.resource_scope(|world, mut graphics: Mut<Graphics>| {
        let shared = graphics.shared.update(world)?;
        let battle = graphics.battle.update(world)?;
        Ok(shared && battle)
    })
}

pub(super) fn prepare_battle(
    world: &mut World,
    warm: &mut crate::battle_view::startup::Warm,
) -> Result<bool> {
    world
        .resource_scope(|world, graphics: Mut<Graphics>| warm.update(world, &graphics.battle.parts))
}

#[cfg(test)]
pub(crate) fn check(world: &World) -> Result<()> {
    let graphics = world.resource::<Graphics>();
    anyhow::ensure!(
        !graphics.shared.parts.is_empty(),
        "common GPU inventory is empty"
    );
    let meshes = world.resource::<Assets<Mesh>>();
    let images = world.resource::<Assets<Image>>();
    for batch in [&graphics.shared, &graphics.battle] {
        let report = batch.uploads.lock().unwrap();
        anyhow::ensure!(
            report.completed.load(Ordering::Acquire),
            "common GPU assets were not uploaded at startup"
        );
        anyhow::ensure!(
            report.meshes.iter().all(|id| meshes.contains(*id)),
            "common mesh retired"
        );
        anyhow::ensure!(
            report.images.iter().all(|id| images.contains(*id)),
            "common image retired"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::diagnostics::Diagnostics;

    #[test]
    fn optional_warmup_skips_missing_and_broken_images_but_keeps_strict_errors() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("broken.ktx2"), b"invalid image")?;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.path().to_string_lossy().into_owned(),
                ..default()
            },
            bevy::image::ImagePlugin::default(),
        ))
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<WorldAsset>()
        .init_asset::<crate::sparse_animation::Clip>()
        .init_resource::<crate::scene::SampledImages>()
        .init_resource::<Resident>()
        .init_resource::<crate::gpu_prepare::Registry>()
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ));
        app.finish();
        app.cleanup();
        let diagnostics = Diagnostics::new(false);
        let mut files = Files::new(diagnostics.clone());
        files.insert("broken.ktx2".into(), Arc::from(&b"invalid image"[..]));
        let files = Arc::new(files);
        let mut optional = Batch::optional(
            app.world(),
            Vec::new(),
            ["missing.ktx2".into(), "broken.ktx2".into()],
            files.clone(),
        )?;
        assert!(
            app.world()
                .resource::<AssetServer>()
                .get_handle::<Image>("missing.ktx2")
                .is_none()
        );
        let since = Instant::now();
        while !optional.uploads.lock().unwrap().armed {
            app.update();
            optional.update(app.world_mut())?;
            anyhow::ensure!(
                since.elapsed().as_secs() < 5,
                "image failure was not retired"
            );
            std::thread::yield_now();
        }
        assert!(optional.images.is_empty());
        assert!(diagnostics.has_errors());
        let mut required = Batch::new(app.world(), Vec::new(), ["broken.ktx2".into()], files);
        loop {
            app.update();
            if required.update(app.world_mut()).is_err() {
                break;
            }
            anyhow::ensure!(
                since.elapsed().as_secs() < 5,
                "required image error was lost"
            );
            std::thread::yield_now();
        }
        Ok(())
    }
}

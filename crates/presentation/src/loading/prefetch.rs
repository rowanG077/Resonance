//! Checkpoint handoff: captured graphics begin loading before CPU preparation ends.
use super::*;
use resonance_content::{ScenePart, field::FieldAssets, library::Snapshot};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource)]
pub(crate) struct FieldPrefetch {
    pub map: u32,
    pub models: BTreeMap<u32, Vec<crate::field_view::Part>>,
    pub toon_ramp: Handle<Image>,
    pub _loads: LoadTasks,
    pub disabled: bool,
    pub changed: BTreeSet<(u32, usize)>,
}
impl FieldPrefetch {
    fn capture(
        snapshot: &Snapshot,
        field: &FieldAssets,
        selected: &BTreeSet<u32>,
        resident: &Resident,
        server: &AssetServer,
    ) -> Result<Self> {
        let files = Arc::new(Files::capture_snapshot(
            snapshot,
            || false,
            resident.diagnostics.clone(),
        )?);
        let loads = resident.scope(files);
        let geometry = |part: &ScenePart| -> Result<crate::field_view::Part> {
            let spec = geometry_only(part);
            spec.validate(|_| true)?;
            Ok(crate::field_view::Part::load(spec, server, &loads))
        };
        let models = field
            .actors
            .iter()
            .filter(|actor| {
                !crate::field_view::is_costume(actor.resource) || selected.contains(&actor.resource)
            })
            .map(|actor| (actor.resource, actor.parts.iter().collect::<Vec<_>>()))
            .chain(field.parts.iter().map(|part| {
                (
                    resonance_content::field::SCENERY_RESOURCE_BASE + u32::from(part.resource),
                    vec![part],
                )
            }))
            .map(|(id, parts)| Ok((id, parts.into_iter().map(&geometry).collect::<Result<_>>()?)))
            .collect::<Result<_>>()?;
        let toon_ramp = server
            .load_builder()
            .with_guard(loads.ticket())
            .with_settings(|settings: &mut bevy::image::ImageLoaderSettings| {
                settings.is_srgb = false;
                settings.sampler = bevy::image::ImageSampler::linear();
            })
            .load(field.toon_ramp.clone());
        Ok(Self {
            map: field.map_id,
            models,
            toon_ramp,
            _loads: loads,
            disabled: false,
            changed: Default::default(),
        })
    }
}

#[derive(Resource)]
pub(crate) struct FieldPending {
    pub map: u32,
    task: Task<Arc<crate::new_game::FieldPackage>>,
    graphics: Arc<Mutex<Option<FieldPrefetch>>>,
}
impl FieldPending {
    #[cfg(test)]
    pub fn field(
        root: PathBuf,
        script_root: Option<PathBuf>,
        map: u32,
        resident: &Resident,
    ) -> Result<Self> {
        Self::prefetch(root, script_root, map, resident, None, BTreeSet::new())
    }
    pub fn prefetch(
        root: PathBuf,
        script_root: Option<PathBuf>,
        map: u32,
        resident: &Resident,
        server: Option<AssetServer>,
        selected: BTreeSet<u32>,
    ) -> Result<Self> {
        let library = resident.library(&root)?;
        let cache = resident.cache.clone();
        let resident = resident.clone();
        let graphics = Arc::new(Mutex::new(None));
        let published = graphics.clone();
        let task = Task::spawn(format!("field {map}"), move |stop| {
            let mut cache = cache.lock().unwrap();
            cache.configure_scripts(script_root);
            crate::new_game::FieldPackage::capture_publishing(
                &library,
                map,
                &mut cache,
                || stop.load(Ordering::Relaxed),
                resident.diagnostics.clone(),
                &mut |snapshot, field| {
                    if let Some(server) = &server {
                        *published.lock().unwrap() = Some(FieldPrefetch::capture(
                            snapshot, field, &selected, &resident, server,
                        )?);
                    }
                    Ok(())
                },
            )
            .map(Arc::new)
        })?;
        Ok(Self {
            map,
            task,
            graphics,
        })
    }
    pub fn take_graphics(&self) -> Option<FieldPrefetch> {
        self.graphics.lock().unwrap().take()
    }
}

impl std::ops::Deref for FieldPending {
    type Target = Task<Arc<crate::new_game::FieldPackage>>;
    fn deref(&self) -> &Self::Target {
        &self.task
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        publish
            .after(crate::new_game::transition)
            .before(crate::field_view::FieldPreparation),
    )
    .add_systems(Last, retire);
}
fn publish(world: &mut World) {
    let graphics = world
        .get_resource::<FieldPending>()
        .and_then(FieldPending::take_graphics);
    if let Some(graphics) = graphics {
        world.insert_resource(graphics);
    }
}
fn retire(world: &mut World) {
    let Some(graphics) = world.get_resource::<FieldPrefetch>() else {
        return;
    };
    if world
        .get_resource::<FieldPending>()
        .is_some_and(|pending| pending.map == graphics.map)
        || world
            .get_resource::<crate::new_game::Session>()
            .is_some_and(|session| session.is_field() && session.map_id() == graphics.map)
    {
        return;
    }
    world.remove_resource::<FieldPrefetch>();
}

//! Build and submit both ordinary worlds at boot through the live renderer.
use super::*;

#[derive(Resource)]
pub(super) struct Draws(crate::gpu_prepare::Shared);

impl Draws {
    pub(super) fn fail(&self, scope: &str, error: anyhow::Error) {
        self.0
            .0
            .lock()
            .unwrap()
            .error
            .get_or_insert_with(|| format!("{scope}: {error:#}"));
    }
}

pub(crate) fn abort(world: &mut World) {
    if let Some(mut art) = world.get_resource_mut::<Art>() {
        art.ready = false;
    }
    retire(world);
}

fn retire(world: &mut World) {
    world.remove_resource::<Draws>();
    world.remove_resource::<capture::Scene>();
    super::retire(world);
    world
        .resource::<crate::loading::Resident>()
        .active
        .store(false, Ordering::Release);
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        PostUpdate,
        draw.after(super::animate)
            .before(bevy::transform::TransformSystems::Propagate),
    );
}
fn draw(world: &mut World) {
    if !world.contains_resource::<Draws>() {
        return;
    }
    let roots: Vec<_> = world
        .query_filtered::<Entity, With<WorldRoot>>()
        .iter(world)
        .collect();
    for root in roots {
        world.entity_mut(root).insert(Visibility::Hidden);
    }
    let submitted = world.resource::<Draws>().0.clone();
    let mut report = submitted.0.lock().unwrap();
    if report.armed {
        return;
    }
    if world
        .get_resource::<Art>()
        .is_none_or(|art| !art.ready || !art.spawned)
        || world
            .query::<&Instance>()
            .iter(world)
            .any(|instance| !instance.prepared)
    {
        return;
    }
    let roots: Vec<_> = world
        .query_filtered::<Entity, With<Instance>>()
        .iter(world)
        .collect();
    let mut nodes = roots;
    let mut index = 0;
    while index < nodes.len() {
        if let Some(children) = world.get::<Children>(nodes[index]) {
            nodes.extend(children.iter());
        }
        index += 1;
    }
    let samples = world
        .query_filtered::<&Msaa, With<crate::FieldCamera>>()
        .single(world)
        .unwrap()
        .samples();
    for entity in nodes {
        if let Some((mesh, material)) = world
            .get::<Mesh3d>(entity)
            .zip(world.get::<MeshMaterial3d<TitleSurface>>(entity))
        {
            report.prepare(
                crate::gpu_prepare::View::Field,
                mesh.0.id(),
                material.0.id().untyped(),
                false,
                Some(samples),
            );
        }
    }
    report.armed = true;
}

pub(crate) struct Warm {
    package: Arc<game::Prepared>,
    index: usize,
    since: std::time::Instant,
}
impl Warm {
    pub(crate) fn new(world: &mut World) -> Result<Option<Self>> {
        let Some(package) = world
            .resource::<crate::loading::Resident>()
            .common
            .get()
            .and_then(|common| common.world.as_ref())
            .map(|package| package.world.clone())
        else {
            return Ok(None);
        };
        let warm = Self {
            package,
            index: 0,
            since: std::time::Instant::now(),
        };
        warm.enter(world)?;
        Ok(Some(warm))
    }
    fn enter(&self, world: &mut World) -> Result<()> {
        let selected = [game::World::Sylvarant, game::World::TetheAlla][self.index];
        let persistent = resonance_events::PersistentState {
            party: Some(resonance_events::party::Party::new(
                self.package
                    .resources
                    .session_data
                    .as_ref()
                    .context("world startup data")?,
                Default::default(),
            )?),
            ..Default::default()
        };
        let state = resonance_content::overworld::TravelState {
            world: selected,
            position: Position::from_map([2., 0., 0.])?,
            heading: 0.,
            camera_yaw: 0.,
            alternate_perspective: false,
            map_display: Default::default(),
            mount: Mount::Foot,
            altitude: 0.,
        };
        let session = game::Session::enter(
            self.package.assets(selected, &persistent)?,
            state,
            persistent,
            Default::default(),
        )?;
        world.insert_resource(capture::Scene(Scene::new(session, self.package.clone())?));
        let gpu = crate::gpu_prepare::Shared::new(world);
        world.insert_resource(Draws(gpu));
        Ok(())
    }
    pub(crate) fn update(&mut self, world: &mut World) -> Result<bool> {
        anyhow::ensure!(
            self.since.elapsed().as_secs() < 120,
            "world boot preparation timed out"
        );
        {
            let report = world.resource::<Draws>().0.0.lock().unwrap();
            if let Some(error) = &report.error {
                anyhow::bail!("world boot pipeline: {error}");
            }
            if !report.completed.load(Ordering::Acquire) {
                return Ok(false);
            }
        }
        retire(world);
        self.index += 1;
        if self.index < 2 {
            self.enter(world)?;
            return Ok(false);
        }
        Ok(true)
    }
}

//! Frame-coherent material records of title surfaces, which the native frame
//! renderer reads by row. TitleSurface remains the CPU interface used by
//! gameplay and resident leases.
use super::record_buffer::{RecordBuffer, RecordStats};
use super::*;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp,
    render_resource::Buffer,
    renderer::{RenderDevice, RenderQueue},
};
type FrameWrites = super::record_buffer::FrameWrites<SurfaceUniform>;
const MAX_UPLOAD_GAP: usize = 65_536;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SurfacePublication;

/// The images and pipeline choices of a title surface.
struct Recipe {
    textures: [Option<Handle<Image>>; 4],
    key: SurfaceKey,
}
impl Recipe {
    fn from_surface(surface: &TitleSurface) -> Self {
        Self {
            textures: [
                surface.color.clone(),
                surface.sampling.clone(),
                surface.multiply.clone(),
                surface.toon_ramp.clone(),
            ],
            key: SurfaceKey::from(surface),
        }
    }
}

struct Entry {
    state: SurfaceUniform,
    recipe: Recipe,
    row: Option<u32>,
    /// The state changed after its row was written; written once drawn.
    dirty: bool,
}
/// A retained title surface as the native display-list renderer draws it.
pub(crate) struct NativeSurface {
    pub row: u32,
    /// Color, sampler source, multiply and toon ramp images.
    pub textures: [Option<AssetId<Image>>; 4],
    pub key: SurfaceKey,
}

#[derive(Resource, Default)]
pub(crate) struct Records {
    storage: RecordBuffer<SurfaceUniform>,
    entries: HashMap<AssetId<TitleSurface>, Entry>,
}
impl Records {
    pub(crate) fn buffer(&self) -> Option<&Buffer> {
        self.storage.buffer.as_ref()
    }

    /// Resolve a visible draw's record, writing a deferred state change.
    pub(crate) fn native(&mut self, id: AssetId<TitleSurface>) -> Option<NativeSurface> {
        let entry = self.entries.get_mut(&id)?;
        let row = entry.row?;
        if entry.dirty {
            self.storage
                .pending
                .get_mut()
                .unwrap()
                .insert(row, entry.state.clone());
            entry.dirty = false;
        }
        Some(NativeSurface {
            row,
            textures: entry
                .recipe
                .textures
                .each_ref()
                .map(|image| image.as_ref().map(Handle::id)),
            key: entry.recipe.key,
        })
    }
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<super::GeometryMesh>()
        .init_resource::<Records>()
        .add_systems(Startup, initialize)
        // CPU mutations have emitted their source asset events. Publish
        // records before extraction, including same-frame births.
        .add_systems(Last, publish.in_set(SurfacePublication));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .init_resource::<FrameWrites>()
            .init_resource::<RecordStats>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(Render, upload.in_set(crate::renderer::FrameUploads));
    }
}
fn initialize(device: Option<Res<RenderDevice>>, mut records: ResMut<Records>) {
    records
        .storage
        .initialize(device.as_deref(), "title material records");
}

fn publish(
    mut events: MessageReader<AssetEvent<TitleSurface>>,
    sources: Res<Assets<TitleSurface>>,
    mut records: ResMut<Records>,
    mut changed: Local<HashSet<AssetId<TitleSurface>>>,
) {
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                changed.insert(id);
            }
            AssetEvent::Unused { id } | AssetEvent::Removed { id } => {
                changed.remove(&id);
                if let Some(entry) = records.entries.remove(&id)
                    && let Some(row) = entry.row
                {
                    records.storage.release(row);
                }
            }
            _ => {}
        }
    }
    for id in changed.drain() {
        let Some(source) = sources.get(id) else {
            continue;
        };
        let state = SurfaceUniform::from(source);
        let recipe = Recipe::from_surface(source);
        if let Some(entry) = records.entries.get_mut(&id) {
            if entry.state != state {
                entry.dirty = entry.row.is_some();
                entry.state = state;
            }
            entry.recipe = recipe;
            continue;
        }
        let row = records.storage.allocate(state.clone());
        records.entries.insert(
            id,
            Entry {
                state,
                recipe,
                row,
                dirty: false,
            },
        );
    }
}
fn extract(source: Extract<Res<Records>>, mut frame: ResMut<FrameWrites>) {
    frame.extract(&source.storage);
}

fn upload(
    mut frame: ResMut<FrameWrites>,
    queue: Res<RenderQueue>,
    mut uploads: ResMut<crate::renderer::Uploads>,
    mut stats: ResMut<RecordStats>,
) {
    *stats = frame.upload(&queue, &mut uploads, MAX_UPLOAD_GAP);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    fn state(tint: Vec4) -> SurfaceUniform {
        SurfaceUniform::from(&TitleSurface { tint, ..default() })
    }
    #[test]
    fn hidden_material_changes_catch_up_when_first_drawn() {
        let mut world = World::new();
        world.init_resource::<Assets<TitleSurface>>();
        world.init_resource::<Records>();
        world.init_resource::<Messages<AssetEvent<TitleSurface>>>();
        let source = world
            .resource_mut::<Assets<TitleSurface>>()
            .add(TitleSurface::default());
        world.write_message(AssetEvent::<TitleSurface>::Added { id: source.id() });
        world.run_system_once(publish).unwrap();
        // Exercise the storage-buffer path without requiring a GPU in this test.
        world
            .resource_mut::<Records>()
            .entries
            .get_mut(&source.id())
            .unwrap()
            .row = Some(7);
        for tint in [Vec4::ZERO, Vec4::splat(0.5)] {
            world
                .resource_mut::<Assets<TitleSurface>>()
                .get_mut(&source)
                .unwrap()
                .tint = tint;
            world.write_message(AssetEvent::<TitleSurface>::Modified { id: source.id() });
            world.run_system_once(publish).unwrap();
            assert!(
                world
                    .resource::<Records>()
                    .storage
                    .pending
                    .lock()
                    .unwrap()
                    .is_empty()
            );
        }
        let mut records = world.resource_mut::<Records>();
        assert_eq!(records.native(source.id()).unwrap().row, 7);
        assert_eq!(
            records.storage.pending.get_mut().unwrap().remove(&7),
            Some(state(Vec4::splat(0.5)))
        );
        assert!(!records.entries[&source.id()].dirty);
    }

    #[test]
    fn removed_surfaces_free_their_rows() {
        let mut world = World::new();
        world.init_resource::<Assets<TitleSurface>>();
        world.init_resource::<Messages<AssetEvent<TitleSurface>>>();
        let mut records = Records::default();
        for _ in 0..3 {
            records.storage.rows.allocate();
        }
        world.insert_resource(records);
        let source = world
            .resource_mut::<Assets<TitleSurface>>()
            .add(TitleSurface::default());
        world.resource_mut::<Records>().entries.insert(
            source.id(),
            Entry {
                state: state(Vec4::ONE),
                recipe: Recipe::from_surface(&TitleSurface::default()),
                row: Some(2),
                dirty: true,
            },
        );
        world
            .resource_mut::<Assets<TitleSurface>>()
            .remove(source.id());
        world.write_message(AssetEvent::<TitleSurface>::Removed { id: source.id() });
        world.run_system_once(publish).unwrap();
        let mut records = world.resource_mut::<Records>();
        assert!(records.entries.is_empty());
        assert_eq!(records.storage.rows.allocate(), 2);
        assert_eq!(records.storage.rows.allocate(), 3);
    }
}

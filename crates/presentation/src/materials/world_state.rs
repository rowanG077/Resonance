//! Frame-coherent endpoint uploads independent of immutable texture bindings.
use super::{WorldSurface, record_buffer::RecordBuffer, world::WorldUniform};
use bevy::{
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp,
        render_resource::Buffer,
        renderer::{RenderDevice, RenderQueue},
    },
};
use std::collections::HashMap;

type FrameWrites = super::record_buffer::FrameWrites<WorldUniform>;
// World endpoints retain one upload spanning all changed rows.
const MAX_UPLOAD_GAP: usize = usize::MAX;

#[derive(Resource, Default)]
pub(crate) struct WorldRecords {
    storage: RecordBuffer<WorldUniform>,
    owners: HashMap<AssetId<WorldSurface>, u32>,
}

impl WorldRecords {
    pub(crate) fn buffer(&self) -> Option<&Buffer> {
        self.storage.buffer.as_ref()
    }

    pub(crate) fn add(
        &mut self,
        mut surface: WorldSurface,
        assets: &mut Assets<WorldSurface>,
    ) -> Handle<WorldSurface> {
        let row = self.storage.allocate(WorldUniform::from(&surface));
        surface.row = row;
        let handle = assets.add(surface);
        if let Some(row) = row {
            self.owners.insert(handle.id(), row);
        }
        handle
    }

    pub(crate) fn update(&self, surface: &WorldSurface) {
        if let Some(row) = surface.row {
            self.storage
                .pending
                .lock()
                .unwrap()
                .insert(row, WorldUniform::from(surface));
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<WorldRecords>()
        .add_systems(Startup, initialize)
        .add_systems(PreUpdate, reclaim);
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .init_resource::<FrameWrites>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(Render, upload.in_set(crate::renderer::FrameUploads));
    }
}

fn initialize(device: Option<Res<RenderDevice>>, mut records: ResMut<WorldRecords>) {
    records
        .storage
        .initialize(device.as_deref(), "world material endpoints");
}

fn reclaim(mut events: MessageReader<AssetEvent<WorldSurface>>, mut records: ResMut<WorldRecords>) {
    for event in events.read() {
        if let AssetEvent::Removed { id } = *event
            && let Some(row) = records.owners.remove(&id)
        {
            records.storage.release(row);
        }
    }
}

fn extract(source: Extract<Res<WorldRecords>>, mut frame: ResMut<FrameWrites>) {
    frame.extract(&source.storage);
}

fn upload(
    mut frame: ResMut<FrameWrites>,
    queue: Res<RenderQueue>,
    mut uploads: ResMut<crate::renderer::Uploads>,
) {
    frame.upload(&queue, &mut uploads, MAX_UPLOAD_GAP);
}

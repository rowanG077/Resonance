//! The frame's buffer writes for the native frame, counted for profiling.
use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems, render_resource::Buffer, renderer::RenderQueue},
};

#[derive(Resource, Default)]
pub(crate) struct Uploads {
    pending: Stats,
    /// The previous frame's writes.
    pub stats: Stats,
}
#[derive(Default, Clone, Copy)]
pub(crate) struct Stats {
    pub copies: usize,
    pub bytes: usize,
}
impl Uploads {
    pub(crate) fn write(&mut self, queue: &RenderQueue, target: &Buffer, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.pending.copies += 1;
        self.pending.bytes += data.len();
        queue.write_buffer(target, offset, data);
    }
}

pub(crate) fn install(app: &mut App) {
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.init_resource::<Uploads>().add_systems(
            Render,
            flush
                .after(RenderSystems::Prepare)
                .before(RenderSystems::Render),
        );
    }
}
fn flush(mut uploads: ResMut<Uploads>) {
    uploads.stats = std::mem::take(&mut uploads.pending);
}

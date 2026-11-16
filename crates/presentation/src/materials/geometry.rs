//! Frame-coherent dynamic triangles, independent of Mesh asset preparation.
//! Surface triangles share one layout mesh; UI uses its fixed packed layout.
use bevy::prelude::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GeometryVertex {
    pub position: [f32; 4],
    pub normal: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

/// Main-world ownership permits buffer reuse without sharing a mutable render
/// snapshot. Extraction copies only visible vertices into its own retained arena.
#[derive(Component, Clone)]
#[require(Transform, Visibility)]
pub(crate) struct GeometryData<T: Send + Sync + 'static = GeometryVertex>(Arc<Mutex<Data<T>>>);
struct Data<T> {
    revision: u64,
    vertices: Vec<T>,
}
impl<T: Send + Sync + 'static> Default for GeometryData<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Data {
            revision: 0,
            vertices: Vec::new(),
        })))
    }
}
impl<T: Copy + Send + Sync + 'static> GeometryData<T> {
    pub(crate) fn replace(&self, vertices: impl Iterator<Item = T>) {
        static REVISION: AtomicU64 = AtomicU64::new(1);
        let mut data = self.0.lock().unwrap();
        data.vertices.clear();
        data.vertices.extend(vertices);
        data.revision = REVISION.fetch_add(1, Ordering::Relaxed);
    }
    /// Append this draw's vertices to a frame stream, returning their range.
    pub(crate) fn append_to(&self, stream: &mut GeometryFrame<T>) -> std::ops::Range<u32> {
        let data = self.0.lock().unwrap();
        let start = if stream.used == 0 {
            0
        } else {
            stream.sources[stream.used - 1].1.end
        };
        let range = start..start + data.vertices.len() as u32;
        let source = (data.revision, range.clone());
        if stream.sources.get(stream.used) != Some(&source) {
            stream.copied_vertices += data.vertices.len();
            if range.end as usize <= stream.vertices.len() {
                stream.vertices[range.start as usize..range.end as usize]
                    .copy_from_slice(&data.vertices);
            } else {
                stream.vertices.truncate(range.start as usize);
                stream.vertices.extend_from_slice(&data.vertices);
            }
            if stream.used < stream.sources.len() {
                stream.sources[stream.used] = source;
            } else {
                stream.sources.push(source);
            }
        }
        stream.used += 1;
        range
    }

    #[cfg(test)]
    pub(crate) fn storage(&self) -> (usize, usize) {
        let data = self.0.lock().unwrap();
        (data.vertices.as_ptr() as usize, data.vertices.capacity())
    }
}

/// An immutable frame snapshot with reusable storage. Unchanged ordered sources
/// keep their bytes; render uploads compare the same revisions before copying.
pub(crate) struct GeometryFrame<T> {
    pub vertices: Vec<T>,
    pub sources: Vec<(u64, std::ops::Range<u32>)>,
    pub copied_vertices: usize,
    used: usize,
}
impl<T> Default for GeometryFrame<T> {
    fn default() -> Self {
        Self {
            vertices: Vec::new(),
            sources: Vec::new(),
            copied_vertices: 0,
            used: 0,
        }
    }
}
impl<T> GeometryFrame<T> {
    pub fn begin(&mut self) {
        self.used = 0;
        self.copied_vertices = 0;
    }
    pub fn finish(&mut self) {
        self.sources.truncate(self.used);
        self.vertices.truncate(
            self.sources
                .last()
                .map_or(0, |(_, range)| range.end as usize),
        );
    }
}

/// Layout and upload readiness shared by every streamed battle draw.
#[derive(Resource)]
pub(crate) struct GeometryMesh(pub Handle<Mesh>);
impl FromWorld for GeometryMesh {
    fn from_world(world: &mut World) -> Self {
        use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
        Self(
            world.resource_mut::<Assets<Mesh>>().add(
                Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::default(),
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.; 3]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.; 2]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.; 2]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.; 4]; 3]),
            ),
        )
    }
}

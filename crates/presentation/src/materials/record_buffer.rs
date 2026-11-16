//! Shared row allocation and frame snapshots for retained material records.
use bevy::{
    prelude::Resource,
    render::{
        render_resource::{Buffer, BufferDescriptor, BufferUsages, ShaderType, encase},
        renderer::{RenderDevice, RenderQueue},
    },
};
use std::{collections::BTreeMap, ops::Range, sync::Mutex};

const CAPACITY: u32 = 65_536;

#[derive(Default)]
pub(super) struct Rows {
    next: u32,
    free: Vec<u32>,
}
impl Rows {
    pub fn allocate(&mut self) -> u32 {
        self.free.pop().unwrap_or_else(|| {
            assert!(self.next < CAPACITY, "material record capacity exhausted");
            let row = self.next;
            self.next += 1;
            row
        })
    }

    fn release(&mut self, row: u32) {
        self.free.push(row);
    }
}

pub(super) struct RecordBuffer<T> {
    pub buffer: Option<Buffer>,
    pub rows: Rows,
    pub pending: Mutex<BTreeMap<u32, T>>,
}
impl<T> Default for RecordBuffer<T> {
    fn default() -> Self {
        Self {
            buffer: None,
            rows: Rows::default(),
            pending: Mutex::default(),
        }
    }
}
impl<T> RecordBuffer<T> {
    pub fn allocate(&mut self, state: T) -> Option<u32> {
        self.buffer.as_ref()?;
        let row = self.rows.allocate();
        self.pending.get_mut().unwrap().insert(row, state);
        Some(row)
    }

    pub fn release(&mut self, row: u32) {
        self.pending.get_mut().unwrap().remove(&row);
        self.rows.release(row);
    }
}
impl<T: ShaderType> RecordBuffer<T> {
    pub fn initialize(&mut self, device: Option<&RenderDevice>, label: &str) {
        if let Some(device) =
            device.filter(|d| d.limits().max_storage_buffers_per_shader_stage >= 8)
        {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: T::min_size().get() * u64::from(CAPACITY),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }
}

#[derive(Resource)]
pub(super) struct FrameWrites<T: Send + Sync + 'static> {
    buffer: Option<Buffer>,
    pub records: BTreeMap<u32, T>,
    pub bytes: Vec<u8>,
    pub ranges: Vec<Range<usize>>,
}
impl<T: Send + Sync + 'static> Default for FrameWrites<T> {
    fn default() -> Self {
        Self {
            buffer: None,
            records: BTreeMap::new(),
            bytes: Vec::new(),
            ranges: Vec::new(),
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct RecordStats {
    pub updated: usize,
    pub bytes: usize,
    pub uploads: usize,
}

impl<T: Send + Sync + 'static> FrameWrites<T> {
    /// Snapshot at extraction so later main-frame writes cannot affect this render.
    pub fn extract(&mut self, source: &RecordBuffer<T>) {
        self.buffer = source.buffer.clone();
        let mut pending = source.pending.lock().unwrap();
        std::mem::swap(&mut self.records, &mut *pending);
        assert!(
            pending.is_empty(),
            "previous material writes were not uploaded"
        );
    }
}

impl<T: ShaderType + encase::internal::WriteInto + Send + Sync + 'static> FrameWrites<T> {
    pub fn encode(&mut self, max_gap: usize) {
        let stride = T::min_size().get() as usize;
        self.ranges.clear();
        if let Some((&last, _)) = self.records.last_key_value() {
            self.bytes
                .resize(self.bytes.len().max((last as usize + 1) * stride), 0);
        }
        // Preserve unchanged rows between updates so adjacent writes can share
        // one upload. Each caller chooses how large a gap is worth copying.
        for (&row, state) in &self.records {
            let start = row as usize * stride;
            encase::StorageBuffer::new(&mut self.bytes[start..start + stride])
                .write(state)
                .expect("material record encoding");
            if let Some(range) = self.ranges.last_mut()
                && start - range.end <= max_gap
            {
                range.end = start + stride;
            } else {
                self.ranges.push(start..start + stride);
            }
        }
        self.records.clear();
    }

    pub fn upload(
        &mut self,
        queue: &RenderQueue,
        uploads: &mut crate::renderer::Uploads,
        max_gap: usize,
    ) -> RecordStats {
        let Some(buffer) = self.buffer.clone() else {
            return RecordStats::default();
        };
        let updated = self.records.len();
        self.encode(max_gap);
        for range in &self.ranges {
            uploads.write(
                queue,
                &buffer,
                range.start as u64,
                &self.bytes[range.clone()],
            );
        }
        RecordStats {
            updated,
            bytes: self.ranges.iter().map(|range| range.len()).sum(),
            uploads: self.ranges.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_writes_preserve_unchanged_rows_and_bound_sparse_uploads() {
        for max_gap in [65_536, usize::MAX] {
            let mut frame = FrameWrites::<u32>::default();
            frame.records.extend([(0, 1), (1, 2), (2, 3), (20_000, 4)]);
            frame.encode(max_gap);
            if max_gap == usize::MAX {
                assert_eq!(frame.ranges.as_slice(), std::slice::from_ref(&(0..80_004)));
            } else {
                assert_eq!(frame.ranges, [0..12, 80_000..80_004]);
            }
            frame.records.extend([(0, 9), (2, 10)]);
            frame.encode(max_gap);
            assert_eq!(frame.ranges.as_slice(), std::slice::from_ref(&(0..12)));
            let expected: Vec<u8> = [9u32, 2, 10]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            assert_eq!(frame.bytes[..12], expected);
            assert!(frame.records.is_empty());
            frame.records.insert(2, 11);
            frame.encode(max_gap);
            assert_eq!(frame.ranges.as_slice(), std::slice::from_ref(&(8..12)));
            assert_eq!(frame.bytes[8..12], 11u32.to_le_bytes());
            frame.encode(max_gap);
            assert!(frame.ranges.is_empty());
        }
    }

    #[test]
    fn extraction_keeps_a_snapshot_until_the_next_frame() {
        let mut source = RecordBuffer::<u32>::default();
        source.pending.get_mut().unwrap().insert(7, 17);
        let mut frame = FrameWrites::default();
        frame.extract(&source);
        source.pending.get_mut().unwrap().insert(7, 23);
        assert_eq!(frame.records[&7], 17);
        frame.encode(usize::MAX);
        assert_eq!(frame.bytes[28..32], 17u32.to_le_bytes());
        frame.extract(&source);
        assert_eq!(frame.records[&7], 23);
        frame.encode(usize::MAX);
        assert_eq!(frame.bytes[28..32], 23u32.to_le_bytes());
    }
}

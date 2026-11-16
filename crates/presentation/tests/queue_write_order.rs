//! Queue buffer writes checked against a CPU model on the real GPU.
//!
//! Our wgpu-core fork records `Queue::write_buffer` copies in batches
//! behind a single barrier (`PendingWrites::defer_buffer_copy`). Overlapping
//! writes, writes around texture uploads, recorded command buffers,
//! submissions and destroyed buffers must still behave as if every write were
//! recorded on its own.
//!
//! Run with `--ignored` on a machine with a Vulkan adapter.

use std::sync::mpsc;

const SIZE: u64 = 16 * 1024;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn bytes(&mut self, len: u64) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}

fn device() -> (wgpu::Device, wgpu::Queue) {
    bevy::tasks::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("an explicitly invoked GPU test requires a Vulkan adapter");
        adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("create Vulkan test device")
    })
}

fn buffer(device: &wgpu::Device, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: SIZE,
        usage: usage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn read(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Vec<u8> {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: SIZE,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, SIZE);
    queue.submit([encoder.finish()]);
    let (sender, receiver) = mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    staging.slice(..).get_mapped_range().unwrap().to_vec()
}

fn verify(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffers: &[wgpu::Buffer],
    model: &[Vec<u8>],
    at: &str,
) {
    for (index, (buffer, expected)) in buffers.iter().zip(model).enumerate() {
        let actual = read(device, queue, buffer);
        if let Some(byte) = (0..actual.len()).find(|&i| actual[i] != expected[i]) {
            panic!(
                "buffer {index} differs from the model at byte {byte} {at}: \
                 GPU {:#04x}, model {:#04x}",
                actual[byte], expected[byte]
            );
        }
    }
}

/// Recorded buffer-to-buffer copies, applied to the model when submitted.
type Copies = Vec<(usize, usize, u64, u64, u64)>;

fn submit(
    queue: &wgpu::Queue,
    encoder: &mut Option<(wgpu::CommandEncoder, Copies)>,
    model: &mut [Vec<u8>],
) {
    match encoder.take() {
        Some((encoder, copies)) => {
            queue.submit([encoder.finish()]);
            // Queue writes issued before a submission run before its command
            // buffers, so the copies read the model as it is now.
            for (src, dst, src_offset, dst_offset, size) in copies {
                let (src_offset, dst_offset, size) =
                    (src_offset as usize, dst_offset as usize, size as usize);
                let data = model[src][src_offset..src_offset + size].to_vec();
                model[dst][dst_offset..dst_offset + size].copy_from_slice(&data);
            }
        }
        None => {
            queue.submit([]);
        }
    }
}

#[test]
#[ignore = "requires a GPU; checks queued buffer writes against a CPU model"]
fn queue_writes_land_in_program_order() {
    let (device, queue) = device();
    let buffers = [
        buffer(&device, wgpu::BufferUsages::STORAGE),
        buffer(&device, wgpu::BufferUsages::UNIFORM),
        buffer(
            &device,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX,
        ),
    ];
    let mut model = vec![vec![0u8; SIZE as usize]; buffers.len()];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut encoder: Option<(wgpu::CommandEncoder, Copies)> = None;
    for round in 0..240 {
        for _ in 0..rng.below(48) {
            let target = rng.below(buffers.len() as u64) as usize;
            // Offsets on a coarse grid with small lengths, so that writes
            // overlap often but not always.
            let offset = rng.below(64) * 64 + rng.below(4) * 4;
            let len = ((1 + rng.below(96)) * 4).min(SIZE - offset);
            match rng.below(12) {
                0..=6 => {
                    let data = rng.bytes(len);
                    queue.write_buffer(&buffers[target], offset, &data);
                    model[target][offset as usize..(offset + len) as usize].copy_from_slice(&data);
                }
                7 => {
                    let data = rng.bytes(len);
                    let mut view = queue
                        .write_buffer_with(
                            &buffers[target],
                            offset,
                            wgpu::BufferSize::new(len).unwrap(),
                        )
                        .unwrap();
                    view.copy_from_slice(&data);
                    drop(view);
                    model[target][offset as usize..(offset + len) as usize].copy_from_slice(&data);
                }
                8 => {
                    // Records into the same internal encoder, between the
                    // queued buffer writes.
                    let data = vec![round as u8; 16 * 16 * 4];
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: rng.below(4) as u32 * 16,
                                y: rng.below(4) as u32 * 16,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &data,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(16 * 4),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: 16,
                            height: 16,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                9 | 10 => {
                    let source = (target + 1 + rng.below(2) as usize) % buffers.len();
                    let source_offset = rng.below(64) * 64;
                    let len = len.min(SIZE - source_offset);
                    let (encoder, copies) = encoder.get_or_insert_with(|| {
                        (
                            device.create_command_encoder(&Default::default()),
                            Vec::new(),
                        )
                    });
                    encoder.copy_buffer_to_buffer(
                        &buffers[source],
                        source_offset,
                        &buffers[target],
                        offset,
                        len,
                    );
                    copies.push((source, target, source_offset, offset, len));
                }
                _ => submit(&queue, &mut encoder, &mut model),
            }
        }
        if round % 24 == 23 {
            submit(&queue, &mut encoder, &mut model);
            verify(
                &device,
                &queue,
                &buffers,
                &model,
                &format!("after round {round}"),
            );
        }
    }
}

#[test]
#[ignore = "requires a GPU; checks writes around destroyed buffers"]
fn writes_to_destroyed_and_dropped_buffers_leave_others_intact() {
    let (device, queue) = device();
    let kept = [
        buffer(&device, wgpu::BufferUsages::STORAGE),
        buffer(&device, wgpu::BufferUsages::UNIFORM),
    ];
    let mut model = vec![vec![0u8; SIZE as usize]; kept.len()];
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for round in 0..32 {
        let doomed = buffer(&device, wgpu::BufferUsages::STORAGE);
        for step in 0..24 {
            let offset = rng.below(64) * 64;
            let len = (1 + rng.below(64)) * 4;
            let data = rng.bytes(len);
            if step % 3 == 0 {
                queue.write_buffer(&doomed, offset, &data);
            } else {
                let target = step % kept.len();
                queue.write_buffer(&kept[target], offset, &data);
                model[target][offset as usize..(offset + len) as usize].copy_from_slice(&data);
            }
        }
        // Destroying or dropping a buffer with queued writes must neither
        // free its memory before the queued copy runs nor lose the others.
        if round % 2 == 0 {
            doomed.destroy();
        }
        drop(doomed);
        if round % 4 == 3 {
            verify(
                &device,
                &queue,
                &kept,
                &model,
                &format!("after round {round}"),
            );
        }
    }
    verify(&device, &queue, &kept, &model, "at the end");
}

#[test]
#[ignore = "requires a GPU; checks overlapping large buffer writes"]
fn large_overlapping_writes_keep_the_last_one() {
    let (device, queue) = device();
    // Large copies run as many workgroups, so on a driver that overlaps
    // unordered copies a missing write-after-write barrier shows up here.
    // Honeykrisp runs the copies of one batch in order, so on Apple GPUs this
    // only checks the result, not the barrier.
    const LARGE: u64 = 4 * 1024 * 1024;
    let target = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: LARGE,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: LARGE,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for frame in 0..8u8 {
        for layer in 0..12u8 {
            let value = frame.wrapping_mul(16).wrapping_add(layer);
            // Shifted ranges, so every write overlaps the previous one.
            let offset = u64::from(layer) * 4096;
            queue.write_buffer(&target, offset, &vec![value; (LARGE - offset) as usize]);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&target, 0, &readback, 0, LARGE);
        queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap()
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        {
            let data = readback.slice(..).get_mapped_range().unwrap();
            for (index, &byte) in data.iter().enumerate() {
                let layer = (index as u64 / 4096).min(11) as u8;
                let expected = frame.wrapping_mul(16).wrapping_add(layer);
                assert_eq!(
                    byte, expected,
                    "frame {frame}: byte {index} holds an earlier overlapping write"
                );
            }
        }
        readback.unmap();
    }
}

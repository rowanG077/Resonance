//! GPU-composited portrait canvases and bitmap text for the separate skit scene.
use super::*;
use anyhow::ensure;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    render_asset::RenderAssets,
    render_resource::{Extent3d, Origin3d, TextureDimension, TextureFormat},
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};
use resonance_content::skit::{MAX_PORTRAITS, PortraitAsset, PortraitTile, SkitCatalog, TILE_SIZE};
use std::sync::Arc;

pub(super) fn install(app: &mut App) {
    app.add_plugins(ExtractComponentPlugin::<Composition>::default());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(Render, composite.in_set(RenderSystems::PrepareResources));
    }
}

/// The block map one portrait canvas shows. Tiles are copied between textures
/// on the GPU, so compressed artwork is combined without decoding or filtering.
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(RenderApp)]
struct Composition {
    target: AssetId<Image>,
    /// Distinguishes this block map from the one last copied into the canvas.
    generation: u64,
    size: [u32; 2],
    format: TextureFormat,
    sources: Arc<[AssetId<Image>]>,
    runs: Arc<[Run]>,
}

/// Adjacent tiles of one source row, in stored pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Run {
    image: usize,
    from: [u32; 2],
    to: [u32; 2],
    size: [u32; 2],
}

fn composite(
    compositions: Query<&Composition>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    // The block map last copied into each canvas, and the texture it went to:
    // a canvas recreated by the renderer is painted again.
    mut copied: Local<
        bevy::platform::collections::HashMap<
            AssetId<Image>,
            (u64, bevy::render::render_resource::TextureId),
        >,
    >,
) {
    copied.retain(|target, _| compositions.iter().any(|c| c.target == *target));
    let mut encoder = None;
    for composition in &compositions {
        let current = images
            .get(composition.target)
            .map(|image| (composition.generation, image.texture.id()));
        if current.is_some() && copied.get(&composition.target) == current.as_ref() {
            continue;
        }
        // Wait until the canvas and every source are the textures this block
        // map was planned for; an image may be uploaded a frame later.
        let matches = |image: &GpuImage, size: Option<[u32; 2]>| {
            let descriptor = &image.texture_descriptor;
            descriptor.format == composition.format
                && size.is_none_or(|size| [descriptor.size.width, descriptor.size.height] == size)
        };
        let Some(target) = images
            .get(composition.target)
            .filter(|image| matches(image, Some(composition.size)))
        else {
            continue;
        };
        let Some(sources) = composition
            .sources
            .iter()
            .map(|source| images.get(*source).filter(|image| matches(image, None)))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let encoder =
            encoder.get_or_insert_with(|| device.create_command_encoder(&Default::default()));
        let origin = |[x, y]: [u32; 2]| Origin3d { x, y, z: 0 };
        for run in composition.runs.iter() {
            let mut from = sources[run.image].texture.as_image_copy();
            from.origin = origin(run.from);
            let mut to = target.texture.as_image_copy();
            to.origin = origin(run.to);
            encoder.copy_texture_to_texture(
                from,
                to,
                Extent3d {
                    width: run.size[0],
                    height: run.size[1],
                    depth_or_array_layers: 1,
                },
            );
        }
        copied.insert(
            composition.target,
            (composition.generation, target.texture.id()),
        );
    }
    if let Some(encoder) = encoder {
        queue.submit([encoder.finish()]);
    }
}

struct Canvas {
    image: Handle<Image>,
    surface: Handle<Surface>,
    layout: Option<([u32; 2], TextureFormat)>,
    painted: Option<(u32, Vec<PortraitTile>)>,
}

pub(super) struct Artwork {
    catalog: Arc<SkitCatalog>,
    images: BTreeMap<u32, Vec<Handle<Image>>>,
    canvases: Vec<Canvas>,
    generation: u64,
    background: Handle<Surface>,
    font: Handle<Surface>,
    pub layers: Vec<Layer>,
    pub warm: Vec<Layer>,
}
impl Artwork {
    pub fn load(
        catalog: Arc<SkitCatalog>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        font: &Handle<Surface>,
        image_assets: &mut Assets<Image>,
    ) -> Result<Self> {
        catalog.validate()?;
        let mut images = BTreeMap::new();
        for (&id, portrait) in &catalog.portraits {
            images.insert(
                id,
                portrait
                    .images
                    .iter()
                    .map(|source| {
                        server
                            .load_builder()
                            .with_settings(|settings: &mut ImageLoaderSettings| {
                                settings.is_srgb = false;
                                settings.asset_usage = RenderAssetUsages::RENDER_WORLD;
                            })
                            .load(source.texture.clone())
                    })
                    .collect(),
            );
        }
        // Each slot keeps one canvas and material. A canvas takes the size and
        // format of the portrait it shows, so its sampler clamps at the portrait's
        // own edges and no other material pipeline appears during playback.
        let mut canvases = Vec::new();
        for _ in 0..MAX_PORTRAITS {
            let image = image_assets.add(canvas_image([4; 2], TextureFormat::Rgba8Unorm));
            let surface = materials.add(Surface {
                source: image.clone(),
                sampling: image.clone(),
                frame_mask: image.clone(),
                color_mask: image.clone(),
                coverage: Coverage::default(),
                interpolation: UiInterpolation::Center,
                additive: false,
                texture_effect: Default::default(),
                opaque: false,
            });
            canvases.push(Canvas {
                image,
                surface,
                layout: None,
                painted: None,
            });
        }
        Ok(Self {
            catalog,
            images,
            canvases,
            generation: 0,
            background: font.clone(),
            font: font.clone(),
            layers: Vec::new(),
            warm: Vec::new(),
        })
    }
    pub fn ready(
        &self,
        playback: Option<&resonance_game::field::SkitPlayback>,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        let Some(scene) = playback.and_then(|playback| playback.events.world.skit.as_ref()) else {
            return Ok(true);
        };
        let mut ready = true;
        for portrait in scene.portraits.values() {
            for image in self
                .images
                .get(&portrait.resource)
                .context("uncooked skit portrait")?
            {
                ready &= crate::field_view::image_ready(server, images, image)?;
            }
        }
        Ok(ready)
    }
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
        }
        // New layers carry no block map yet.
        for canvas in &mut self.canvases {
            canvas.painted = None;
        }
        let mut allocate = |material: Handle<Surface>, depth| {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., depth),
                    Visibility::Hidden,
                ))
                .id();
            Layer {
                entity,
                mesh,
                material,
                uploaded: None,
                visible: false,
            }
        };
        self.layers.push(allocate(self.background.clone(), 4.));
        self.layers.push(allocate(self.font.clone(), 9.));
        for (index, canvas) in self.canvases.iter().enumerate() {
            self.layers
                .push(allocate(canvas.surface.clone(), 5. + index as f32 * 0.1));
            self.warm.push(allocate(canvas.surface.clone(), 4.));
        }
    }
    #[allow(clippy::too_many_arguments)] // Scene, layout and the drawing resources it updates.
    pub fn render(
        &mut self,
        playback: Option<&resonance_game::field::SkitPlayback>,
        font: &BitmapFont,
        resolution: crate::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        let Some(playback) = playback else {
            for layer in &mut self.layers {
                layer.show(false, commands);
            }
            return Ok(());
        };
        ensure!(
            self.layers.len() == MAX_PORTRAITS + 2,
            "skit rendering was not prepared"
        );
        let scene = playback
            .events
            .world
            .skit
            .as_ref()
            .context("skit has no portrait scene")?;
        ensure!(
            scene.portraits.len() <= MAX_PORTRAITS,
            "too many skit portraits to render"
        );
        let tick = playback.events.tick();
        let rect @ [left, _, right, _] = resolution.ui_rect();
        let mut background = Batch::default();
        background.quad(rect, [0.5; 4], [0., 0., 0., 0.5]);
        self.layers[0].update_mesh(background, [font.width, font.height], meshes)?;
        self.layers[0].show(true, commands);
        let mut text = Batch::default();
        if let Some(start) = scene.panel_started {
            let opacity = ((tick.saturating_sub(start) + 1) * 4).min(144) as f32 / 255.;
            text.quad([left, 384., right, 468.], [0.5; 4], [0., 0., 0., opacity]);
        }
        centered(
            &mut text,
            font,
            &playback.title,
            34.,
            25.,
            ((tick + 1) * 4).min(255) as f32 / 255.,
            false,
        )?;
        let lines: Vec<_> = scene.subtitle.split('\n').collect();
        let top = if lines.len() == 1 { 402. } else { 392. };
        let alpha = ((tick.saturating_sub(scene.subtitle_started) + 1) * 8).min(255) as f32 / 255.;
        for (i, line) in lines.iter().enumerate() {
            centered(
                &mut text,
                font,
                line,
                top + i as f32 * 21.,
                21.,
                alpha,
                true,
            )?;
            // A line break occupies one blank glyph before the following text.
            if i > 0 {
                let advance = (font.glyphs[&' '].advance as f32 * 13. / 17.).trunc() - 1.;
                let vertices = line.chars().count() * 4;
                for position in text.positions.iter_mut().rev().take(vertices) {
                    position[0] += advance;
                }
            }
        }
        self.layers[1].update_mesh(text, [font.width, font.height], meshes)?;
        self.layers[1].show(true, commands);
        for (index, layer) in self.layers[2..].iter_mut().enumerate() {
            let Some(portrait) = scene.portraits.values().nth(index) else {
                layer.show(false, commands);
                continue;
            };
            let asset = self
                .catalog
                .portraits
                .get(&portrait.resource)
                .context("uncooked skit portrait")?;
            let canvas = &mut self.canvases[index];
            if canvas.painted.as_ref().is_none_or(|(resource, tiles)| {
                *resource != portrait.resource || *tiles != portrait.tiles
            }) {
                let sources = &self.images[&portrait.resource];
                let format = source_format(asset, sources, images)?;
                let size = std::array::from_fn(|axis| asset.size[axis] * asset.scale[axis]);
                let runs = tile_runs(asset, &portrait.tiles, format)?;
                if canvas.layout != Some((size, format)) {
                    // A new image, not a resized one: the material can then
                    // never keep the previous texture bound.
                    canvas.image = images.add(canvas_image(size, format));
                    let surface = &mut *materials
                        .get_mut(&canvas.surface)
                        .context("portrait material missing")?;
                    for image in [
                        &mut surface.source,
                        &mut surface.sampling,
                        &mut surface.frame_mask,
                        &mut surface.color_mask,
                    ] {
                        *image = canvas.image.clone();
                    }
                    canvas.layout = Some((size, format));
                }
                self.generation += 1;
                commands.entity(layer.entity).insert(Composition {
                    target: canvas.image.id(),
                    generation: self.generation,
                    size,
                    format,
                    sources: sources.iter().map(Handle::id).collect(),
                    runs: runs.into(),
                });
                canvas.painted = Some((portrait.resource, portrait.tiles.clone()));
            }
            let [x, y] = portrait.position;
            let [w, h] = portrait.size.map(|v| v * portrait.scale);
            let mut batch = Batch::default();
            let mut color = portrait.color;
            color[3] = portrait.opacity;
            batch.quad(
                [x - w / 2., y - h / 2., x + w / 2., y + h / 2.],
                [0., 0., 1., 1.],
                color,
            );
            let (sin, cos) = portrait.angle.to_radians().sin_cos();
            for p in &mut batch.positions {
                let dx = p[0] - (x - 320.);
                let dy = p[1] - (240. - y);
                p[0] = x - 320. + dx * cos + dy * sin;
                p[1] = 240. - y - dx * sin + dy * cos;
            }
            layer.update_mesh(batch, [1, 1], meshes)?;
            layer.show(true, commands);
        }
        Ok(())
    }
}

fn canvas_image(size: [u32; 2], format: TextureFormat) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

/// Tiles are copied as stored, so a portrait's images share one texture format
/// and the resolution scale its document was prepared with.
fn source_format(
    asset: &PortraitAsset,
    sources: &[Handle<Image>],
    images: &Assets<Image>,
) -> Result<TextureFormat> {
    ensure!(
        !sources.is_empty() && sources.len() == asset.images.len(),
        "invalid portrait images"
    );
    let mut format = None;
    for (handle, source) in sources.iter().zip(&asset.images) {
        let descriptor = &images
            .get(handle)
            .context("portrait image not loaded")?
            .texture_descriptor;
        ensure!(
            [descriptor.size.width, descriptor.size.height]
                == std::array::from_fn(|axis| source.size[axis] * asset.scale[axis])
                && *format.get_or_insert(descriptor.format) == descriptor.format,
            "portrait image {} differs from its portrait's resolution or texture format",
            source.texture
        );
    }
    Ok(format.unwrap())
}

/// Plan the persistent block map as texture copies, in stored pixels. Drawing
/// patches as separate alpha-blended quads would change transparent pixels and
/// filter across unrelated neighbours in their source images.
fn tile_runs(
    asset: &PortraitAsset,
    tiles: &[PortraitTile],
    format: TextureFormat,
) -> Result<Vec<Run>> {
    let columns = asset.size[0].div_ceil(TILE_SIZE);
    ensure!(
        columns > 0 && tiles.len() == (columns * asset.size[1].div_ceil(TILE_SIZE)) as usize,
        "incomplete portrait block map"
    );
    let scale = asset.scale;
    let block = format.block_dimensions();
    let mut runs = Vec::<Run>::new();
    for (index, tile) in tiles.iter().enumerate() {
        let image = usize::from(tile.image);
        let [source_width, source_height] = asset
            .images
            .get(image)
            .context("portrait block image is absent")?
            .size;
        let source_columns = source_width.div_ceil(TILE_SIZE);
        let sx = tile.block % source_columns * TILE_SIZE;
        let sy = tile.block / source_columns * TILE_SIZE;
        let dx = index as u32 % columns * TILE_SIZE;
        let dy = index as u32 / columns * TILE_SIZE;
        let w = TILE_SIZE.min(asset.size[0] - dx);
        let h = TILE_SIZE.min(asset.size[1] - dy);
        ensure!(
            sx + w <= source_width && sy + h <= source_height,
            "portrait block requests uncooked source padding"
        );
        let run = Run {
            image,
            from: [sx * scale[0], sy * scale[1]],
            to: [dx * scale[0], dy * scale[1]],
            size: [w * scale[0], h * scale[1]],
        };
        ensure!(
            [run.from, run.to, run.size]
                .iter()
                .all(|v| v[0] % block.0 == 0 && v[1] % block.1 == 0),
            "portrait tiles must cover whole texture blocks"
        );
        match runs.last_mut() {
            Some(last)
                if last.image == image
                    && last.from[1] == run.from[1]
                    && last.to[1] == run.to[1]
                    && last.from[0] + last.size[0] == run.from[0]
                    && last.to[0] + last.size[0] == run.to[0] =>
            {
                last.size[0] += run.size[0];
            }
            _ => runs.push(run),
        }
    }
    Ok(runs)
}

pub(super) fn centered(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    y: f32,
    height: f32,
    alpha: f32,
    subtitle: bool,
) -> Result<()> {
    let glyphs = text
        .chars()
        .map(|c| {
            Ok((
                c,
                font.glyphs
                    .get(&c)
                    .with_context(|| format!("uncooked skit glyph {c:?}"))?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    // Single-byte text measurement looks ahead one character, including the
    // trailing blank. Drawing still advances by the current glyph's width.
    let width: f32 = glyphs
        .iter()
        .enumerate()
        .map(|(i, (c, glyph))| {
            let narrow = resonance_content::font::is_single_byte(*c);
            let glyph = if narrow {
                glyphs
                    .get(i + 1)
                    .map(|(_, g)| *g)
                    .unwrap_or(&font.glyphs[&' '])
            } else {
                *glyph
            };
            if subtitle {
                (glyph.advance as f32 * if narrow { 13. / 21. } else { height / 25. }).trunc()
                    + f32::from(narrow)
            } else {
                (glyph.advance as f32 * if narrow { 13. / 17. } else { height / 25. }).trunc() - 1.
            }
        })
        .sum();
    let mut x = 320. - (width / 2.).trunc();
    for (c, glyph) in glyphs {
        let narrow = resonance_content::font::is_single_byte(c);
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        let glyph_width = if narrow { 13. } else { height };
        batch.quad(
            [x, y, x + glyph_width, y + height],
            [u, v, u + w, v + h],
            [1., 1., 1., alpha],
        );
        x += (glyph.advance as f32 * if narrow { 13. / 17. } else { height / 25. }).trunc() - 1.;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::skit::PortraitImage;

    #[test]
    fn block_maps_become_block_aligned_copies_of_adjacent_tiles() -> Result<()> {
        let image = |size| PortraitImage {
            texture: String::new(),
            size,
        };
        let asset = PortraitAsset {
            size: [24, 16],
            scale: [2; 2],
            images: vec![image([24, 16]), image([8, 8])],
        };
        let tile = |image, block| PortraitTile { image, block };
        let mut tiles = vec![
            tile(0, 0),
            tile(1, 0),
            tile(0, 2),
            tile(0, 3),
            tile(0, 4),
            tile(0, 5),
        ];
        let run = |image, from, to, size| Run {
            image,
            from,
            to,
            size,
        };
        assert_eq!(
            tile_runs(&asset, &tiles, TextureFormat::Bc7RgbaUnorm)?,
            [
                run(0, [0, 0], [0, 0], [16, 16]),
                run(1, [0, 0], [16, 0], [16, 16]),
                run(0, [32, 0], [32, 0], [16, 16]),
                run(0, [0, 16], [0, 16], [48, 16]),
            ]
        );
        // A patch has no second block, and an incomplete map is not drawn.
        tiles[1].block = 1;
        assert!(tile_runs(&asset, &tiles, TextureFormat::Bc7RgbaUnorm).is_err());
        assert!(tile_runs(&asset, &tiles[1..], TextureFormat::Bc7RgbaUnorm).is_err());
        // Compressed blocks cannot be split.
        let odd = PortraitAsset {
            size: [6, 6],
            scale: [1; 2],
            images: vec![image([6, 6])],
        };
        assert!(tile_runs(&odd, &[tile(0, 0)], TextureFormat::Rgba8Unorm).is_ok());
        assert!(tile_runs(&odd, &[tile(0, 0)], TextureFormat::Bc7RgbaUnorm).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires a GPU with BC texture support"]
    fn compressed_tiles_are_copied_into_the_canvas_on_the_gpu() -> Result<()> {
        use bevy::render::render_resource::{
            BufferDescriptor, BufferUsages, MapMode, TexelCopyBufferInfo, TexelCopyBufferLayout,
        };
        let mut app = App::new();
        app.add_plugins(crate::test_support::render_plugins_with_features(
            wgpu::Features::TEXTURE_COMPRESSION_BC,
        ));
        install(&mut app);
        let began = std::time::Instant::now();
        while app.plugins_state() == bevy::app::PluginsState::Adding {
            ensure!(
                began.elapsed().as_secs() < 60,
                "GPU initialization timed out"
            );
            bevy::tasks::tick_global_task_pools_on_main_thread();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.finish();
        app.cleanup();
        let format = TextureFormat::Bc7RgbaUnorm;
        let blocks = |size, values: &[u8]| {
            let mut image = canvas_image(size, format);
            image.data = Some(values.iter().flat_map(|value| [*value; 16]).collect());
            image
        };
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let base = images.add(blocks([8, 4], &[1, 2]));
        let patch = images.add(blocks([4, 4], &[9]));
        let canvas = images.add(canvas_image([8, 4], format));
        let run = |image, to| Run {
            image,
            from: [0, 0],
            to,
            size: [8 - image as u32 * 4, 4],
        };
        app.world_mut().spawn(Composition {
            target: canvas.id(),
            generation: 1,
            size: [8, 4],
            format,
            sources: [base.id(), patch.id()].into(),
            runs: [run(0, [0, 0]), run(1, [4, 0])].into(),
        });
        app.update();
        app.update();
        let render = app.sub_app(RenderApp).world();
        let device = render.resource::<RenderDevice>();
        let buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: 256,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            render
                .resource::<RenderAssets<GpuImage>>()
                .get(canvas.id())
                .context("canvas was not uploaded")?
                .texture
                .as_image_copy(),
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            Extent3d {
                width: 8,
                height: 4,
                depth_or_array_layers: 1,
            },
        );
        render.resource::<RenderQueue>().submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely())?;
        assert_eq!(
            buffer.slice(..).get_mapped_range()?[..32],
            [[1; 16], [9; 16]].concat()
        );
        Ok(())
    }
}

//! Persistent portrait canvases and bitmap text for the separate skit scene.
use super::*;
use anyhow::ensure;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use resonance_content::skit::{MAX_PORTRAITS, PortraitAsset, PortraitTile, SkitCatalog, TILE_SIZE};

struct Canvas {
    image: Handle<Image>,
    surface: Handle<Surface>,
    painted: Option<(u32, Vec<PortraitTile>)>,
}

pub(super) struct Artwork {
    catalog: SkitCatalog,
    images: BTreeMap<u32, Vec<Handle<Image>>>,
    canvases: Vec<Canvas>,
    canvas_size: [u32; 2],
    background: Handle<Surface>,
    font: Handle<Surface>,
    pub layers: Vec<Layer>,
    pub warm: Vec<Layer>,
}
impl Artwork {
    pub fn load(
        read: impl Fn(&str) -> Result<Vec<u8>>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        font: &Handle<Surface>,
        image_assets: &mut Assets<Image>,
    ) -> Result<Self> {
        let catalog: SkitCatalog = serde_json::from_slice(&read("game/skits.json")?)?;
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
                                settings.asset_usage = RenderAssetUsages::MAIN_WORLD;
                            })
                            .load(source.texture.clone())
                    })
                    .collect(),
            );
        }
        // Allocate all slot textures during field loading. Portrait changes never
        // resize an image or introduce another material pipeline during playback.
        let canvas_size = std::array::from_fn(|axis| {
            catalog
                .portraits
                .values()
                .map(|asset| asset.size[axis])
                .max()
                .unwrap_or(1)
                + 2
        });
        let mut canvases = Vec::new();
        for _ in 0..MAX_PORTRAITS {
            let mut image = Image::new(
                Extent3d {
                    width: canvas_size[0],
                    height: canvas_size[1],
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                vec![0; (canvas_size[0] * canvas_size[1] * 4) as usize],
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            );
            image.sampler = ImageSampler::linear();
            let image = image_assets.add(image);
            let surface = materials.add(Surface {
                source: image.clone(),
                sampling: image.clone(),
                frame_mask: image.clone(),
                color_mask: image.clone(),
                coverage: Coverage::default(),
                opaque: false,
            });
            canvases.push(Canvas {
                image,
                surface,
                painted: None,
            });
        }
        Ok(Self {
            catalog,
            images,
            canvases,
            canvas_size,
            background: font.clone(),
            font: font.clone(),
            layers: Vec::new(),
            warm: Vec::new(),
        })
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images
            .values()
            .flatten()
            .all(|image| images.contains(image.id()))
    }
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
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
    pub fn render(
        &mut self,
        playback: Option<&resonance_game::field::SkitPlayback>,
        font: &BitmapFont,
        resolution: crate::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
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
                let sources = self.images[&portrait.resource]
                    .iter()
                    .map(|source| images.get(source).context("portrait image not loaded"))
                    .collect::<Result<Vec<_>>>()?;
                let pixels = compose(asset, &portrait.tiles, &sources, self.canvas_size)?;
                images
                    .get_mut(&canvas.image)
                    .context("portrait canvas missing")?
                    .data = Some(pixels);
                canvas.painted = Some((portrait.resource, portrait.tiles.clone()));
            }
            let [x, y] = portrait.position;
            let [w, h] = portrait.size.map(|v| v * portrait.scale);
            let mut batch = Batch::default();
            let mut color = portrait.color;
            color[3] = portrait.opacity;
            batch.quad(
                [x - w / 2., y - h / 2., x + w / 2., y + h / 2.],
                [1., 1., 1. + asset.size[0] as f32, 1. + asset.size[1] as f32],
                color,
            );
            let (sin, cos) = portrait.angle.to_radians().sin_cos();
            for p in &mut batch.positions {
                let dx = p[0] - (x - 320.);
                let dy = p[1] - (240. - y);
                p[0] = x - 320. + dx * cos + dy * sin;
                p[1] = 240. - y - dx * sin + dy * cos;
            }
            layer.update_mesh(batch, self.canvas_size, meshes)?;
            layer.show(true, commands);
        }
        Ok(())
    }
}
/// Reconstruct the persistent block map before filtering. Drawing patches as
/// separate alpha-blended quads would change transparent pixels and their edges.
fn compose(
    asset: &PortraitAsset,
    tiles: &[PortraitTile],
    images: &[&Image],
    canvas_size: [u32; 2],
) -> Result<Vec<u8>> {
    let [width, height] = asset.size;
    ensure!(
        width > 0
            && height > 0
            && width <= canvas_size[0].saturating_sub(2)
            && height <= canvas_size[1].saturating_sub(2)
            && images.len() == asset.images.len(),
        "invalid portrait canvas"
    );
    let sources = images
        .iter()
        .zip(&asset.images)
        .map(|(image, source)| {
            ensure!(
                image.texture_descriptor.format == TextureFormat::Rgba8Unorm
                    && [image.width(), image.height()] == source.size,
                "portrait source must retain its lossless RGBA pixels"
            );
            image
                .data
                .as_deref()
                .and_then(|bytes| bytes.get(..(source.size[0] * source.size[1] * 4) as usize))
                .context("portrait source pixels are unavailable")
        })
        .collect::<Result<Vec<_>>>()?;
    let columns = width.div_ceil(TILE_SIZE);
    ensure!(
        tiles.len() == (columns * height.div_ceil(TILE_SIZE)) as usize,
        "incomplete portrait block map"
    );
    let stride = canvas_size[0] as usize * 4;
    let mut pixels = vec![0; stride * canvas_size[1] as usize];
    for (index, tile) in tiles.iter().enumerate() {
        let source = asset
            .images
            .get(usize::from(tile.image))
            .context("portrait block image is absent")?;
        let [source_width, source_height] = source.size;
        let source_columns = source_width.div_ceil(TILE_SIZE);
        let sx = tile.block % source_columns * TILE_SIZE;
        let sy = tile.block / source_columns * TILE_SIZE;
        let dx = index as u32 % columns * TILE_SIZE;
        let dy = index as u32 / columns * TILE_SIZE;
        let w = TILE_SIZE.min(width - dx);
        let h = TILE_SIZE.min(height - dy);
        ensure!(
            sx + w <= source_width && sy + h <= source_height,
            "portrait block requests uncooked source padding"
        );
        for row in 0..h {
            let from = ((sy + row) * source_width + sx) as usize * 4;
            let to = (dy + row + 1) as usize * stride + (dx + 1) as usize * 4;
            pixels[to..to + w as usize * 4]
                .copy_from_slice(&sources[usize::from(tile.image)][from..from + w as usize * 4]);
        }
    }
    // One duplicated edge texel gives each portrait independent linear clamping
    // even when its preallocated canvas is larger than the portrait.
    for row in 1..=height as usize {
        let start = row * stride;
        pixels.copy_within(start + 4..start + 8, start);
        let end = start + width as usize * 4;
        pixels.copy_within(end..end + 4, end + 4);
    }
    let row_bytes = (width as usize + 2) * 4;
    pixels.copy_within(stride..stride + row_bytes, 0);
    let last = height as usize * stride;
    pixels.copy_within(last..last + row_bytes, last + stride);
    Ok(pixels)
}

fn centered(
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
    #[ignore = "requires cook-all and prepared skits; no graphics or audio device"]
    fn original_shared_portraits_decode_and_compose_without_atlases() -> Result<()> {
        use bevy::image::{CompressedImageFormats, ImageType};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
        let catalog: SkitCatalog =
            serde_json::from_slice(&std::fs::read(root.join("game/skits.json"))?)?;
        catalog.validate()?;
        ensure!(!catalog.portraits.is_empty(), "empty portrait corpus");
        for asset in catalog.portraits.values() {
            let images = asset
                .images
                .iter()
                .map(|source| {
                    Ok(Image::from_buffer(
                        &std::fs::read(root.join(&source.texture))?,
                        ImageType::Extension("ktx2"),
                        CompressedImageFormats::NONE,
                        false,
                        ImageSampler::linear(),
                        RenderAssetUsages::MAIN_WORLD,
                    )?)
                })
                .collect::<Result<Vec<_>>>()?;
            let [width, height] = asset.size;
            let tiles = (0..width.div_ceil(TILE_SIZE) * height.div_ceil(TILE_SIZE))
                .map(|block| PortraitTile { image: 0, block })
                .collect::<Vec<_>>();
            let canvas = compose(
                asset,
                &tiles,
                &images.iter().collect::<Vec<_>>(),
                [width + 2, height + 2],
            )?;
            let rows = canvas.chunks_exact((width as usize + 2) * 4).skip(1);
            for (source, row) in images[0]
                .data
                .as_ref()
                .unwrap()
                .chunks_exact(width as usize * 4)
                .zip(rows)
            {
                assert_eq!(source, &row[4..4 + width as usize * 4]);
            }
        }
        Ok(())
    }

    #[test]
    fn portrait_canvas_replaces_alpha_preserves_blocks_and_clamps_its_edges() -> Result<()> {
        let image = |width, color: [u8; 4]| {
            Image::new(
                Extent3d {
                    width,
                    height: TILE_SIZE,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                color.repeat((width * TILE_SIZE) as usize),
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::MAIN_WORLD,
            )
        };
        let base = image(16, [10, 20, 30, 255]);
        let patch = image(8, [80, 70, 60, 0]);
        let asset = PortraitAsset {
            size: [16, 8],
            images: vec![
                PortraitImage {
                    texture: "base.ktx2".into(),
                    size: [16, 8],
                },
                PortraitImage {
                    texture: "patch.ktx2".into(),
                    size: [8, 8],
                },
            ],
        };
        let mut tiles = vec![
            PortraitTile { image: 1, block: 0 },
            PortraitTile { image: 0, block: 1 },
        ];
        let canvas = compose(&asset, &tiles, &[&base, &patch], [24, 12])?;
        let pixel = |x: usize, y: usize| &canvas[(y * 24 + x) * 4..(y * 24 + x + 1) * 4];
        for y in 0..10 {
            for x in 0..18 {
                assert_eq!(
                    pixel(x, y),
                    if x <= 8 {
                        &[80, 70, 60, 0]
                    } else {
                        &[10, 20, 30, 255]
                    }
                );
            }
            assert_eq!(pixel(18, y), [0; 4]);
        }
        tiles[0].block = 1;
        assert!(compose(&asset, &tiles, &[&base, &patch], [24, 12]).is_err());
        Ok(())
    }
}

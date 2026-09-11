//! Prepared portrait atlases and bitmap text for the separate skit scene.
use super::*;
use anyhow::ensure;
use resonance_content::skit::SkitCatalog;

pub(super) struct Artwork {
    catalog: SkitCatalog,
    images: Vec<Handle<Image>>,
    surfaces: BTreeMap<u32, Handle<Surface>>,
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
    ) -> Result<Self> {
        let catalog: SkitCatalog = serde_json::from_slice(&read("game/skits.json")?)?;
        catalog.validate()?;
        let mut images = Vec::new();
        let mut surfaces = BTreeMap::new();
        for (&id, portrait) in &catalog.portraits {
            let image = server
                .load_builder()
                .with_settings(|settings: &mut ImageLoaderSettings| {
                    settings.is_srgb = false;
                    settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
                })
                .load(portrait.texture.clone());
            surfaces.insert(
                id,
                materials.add(Surface {
                    source: image.clone(),
                    sampling: image.clone(),
                    frame_mask: image.clone(),
                    color_mask: image.clone(),
                    coverage: Coverage::default(),
                }),
            );
            images.push(image);
        }
        Ok(Self {
            catalog,
            images,
            surfaces,
            background: font.clone(),
            font: font.clone(),
            layers: Vec::new(),
            warm: Vec::new(),
        })
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|i| images.contains(i.id()))
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
        for index in 0..32 {
            self.layers
                .push(allocate(self.font.clone(), 5. + index as f32 * 0.1));
        }
        for surface in self.surfaces.values() {
            self.warm.push(allocate(surface.clone(), 4.));
        }
    }
    pub fn render(
        &mut self,
        playback: Option<&resonance_game::field::SkitPlayback>,
        font: &BitmapFont,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let Some(playback) = playback else {
            for layer in &mut self.layers {
                layer.show(false, commands);
            }
            return Ok(());
        };
        ensure!(self.layers.len() == 34, "skit rendering was not prepared");
        let scene = playback
            .events
            .world
            .skit
            .as_ref()
            .context("skit has no portrait scene")?;
        ensure!(
            scene.portraits.len() <= 32,
            "too many skit portraits to render"
        );
        let tick = playback.events.tick();
        let mut background = Batch::default();
        background.quad([0., 0., 640., 480.], [0.5; 4], [0., 0., 0., 0.5]);
        self.layers[0].update_mesh(background, [font.width, font.height], meshes)?;
        self.layers[0].show(true, commands);
        let mut text = Batch::default();
        if let Some(start) = scene.panel_started {
            let opacity = ((tick.saturating_sub(start) + 1) * 4).min(144) as f32 / 255.;
            text.quad([0., 384., 640., 468.], [0.5; 4], [0., 0., 0., opacity]);
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
            let variant = asset
                .variants
                .iter()
                .find(|v| v.images == portrait.images)
                .context("uncooked portrait expression")?;
            let surface = self
                .surfaces
                .get(&portrait.resource)
                .context("portrait surface missing")?;
            if layer.material != *surface {
                layer.material = surface.clone();
                commands
                    .entity(layer.entity)
                    .insert(MeshMaterial2d(surface.clone()));
            }
            let [x, y] = portrait.position;
            let [w, h] = portrait.size.map(|v| v * portrait.scale);
            let [u, v, uw, vh] = variant.rect.map(|n| n as f32);
            let mut batch = Batch::default();
            let mut color = portrait.color;
            color[3] = portrait.opacity;
            batch.quad(
                [x - w / 2., y - h / 2., x + w / 2., y + h / 2.],
                [u, v, u + uw, v + vh],
                color,
            );
            let (sin, cos) = portrait.angle.to_radians().sin_cos();
            for p in &mut batch.positions {
                let dx = p[0] - (x - 320.);
                let dy = p[1] - (240. - y);
                p[0] = x - 320. + dx * cos + dy * sin;
                p[1] = 240. - y - dx * sin + dy * cos;
            }
            layer.update_mesh(batch, asset.atlas_size, meshes)?;
            layer.show(true, commands);
        }
        Ok(())
    }
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
            let glyph = if c.is_ascii() {
                glyphs
                    .get(i + 1)
                    .map(|(_, g)| *g)
                    .unwrap_or(&font.glyphs[&' '])
            } else {
                *glyph
            };
            if subtitle {
                (glyph.advance as f32
                    * if c.is_ascii() {
                        13. / 21.
                    } else {
                        height / 25.
                    })
                .trunc()
                    + f32::from(c.is_ascii())
            } else {
                (glyph.advance as f32
                    * if c.is_ascii() {
                        13. / 17.
                    } else {
                        height / 25.
                    })
                .trunc()
                    - 1.
            }
        })
        .sum();
    let mut x = 320. - (width / 2.).trunc();
    for (c, glyph) in glyphs {
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        let glyph_width = if c.is_ascii() { 13. } else { height };
        batch.quad(
            [x, y, x + glyph_width, y + height],
            [u, v, u + w, v + h],
            [1., 1., 1., alpha],
        );
        x += (glyph.advance as f32
            * if c.is_ascii() {
                13. / 17.
            } else {
                height / 25.
            })
        .trunc()
            - 1.;
    }
    Ok(())
}

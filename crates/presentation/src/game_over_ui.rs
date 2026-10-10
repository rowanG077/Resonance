//! Game-over image and system-font text on ordinary UI surfaces.
use super::*;
use resonance_game::game_over::{Assets as Prepared, Screen};

pub(crate) struct Artwork {
    data: Prepared,
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    layers: Vec<Layer>,
}
impl Artwork {
    pub fn load(
        data: &Prepared,
        materials: &mut Assets<Surface>,
        image_assets: &mut Assets<Image>,
    ) -> Result<Self> {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let read = |path: &str| Ok(data.files.read(path)?.to_vec());
        let font = image_assets.add(ui_image(
            &data.font.texture,
            [data.font.width, data.font.height],
            false,
            &read,
        )?);
        let black = image_assets.add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        ));
        let background = &data.art.background;
        let background = data
            .files
            .diagnostics()
            .attempt(
                "game-over background",
                ui_image(
                    &background.path,
                    [background.width, background.height],
                    false,
                    &read,
                ),
            )?
            .map(|image| image_assets.add(image))
            .unwrap_or_else(|| black.clone());
        let images = vec![background, font, black];
        let surfaces = images
            .iter()
            .map(|image| {
                materials.add(Surface {
                    source: image.clone(),
                    sampling: image.clone(),
                    frame_mask: image.clone(),
                    color_mask: image.clone(),
                    coverage: Coverage::default(),
                    additive: false,
                    red_channel: false,
                    opaque: false,
                })
            })
            .collect();
        Ok(Self {
            data: data.clone(),
            images,
            surfaces,
            layers: Vec::new(),
        })
    }
    pub fn load_menu(
        &self,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
    ) -> Result<MenuOverlay> {
        MenuOverlay::load_with(
            |path| Ok(self.data.files.read(path)?.to_vec()),
            server,
            materials,
            self.data.files.diagnostics(),
        )
    }
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
        }
        for (index, material) in self.surfaces.iter().enumerate() {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., super::battle_ui::game_over_depth() + index as f32),
                    Visibility::Hidden,
                ))
                .id();
            self.layers.push(Layer {
                entity,
                mesh,
                material: material.clone(),
                uploaded: None,
                visible: false,
            });
        }
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.layers.iter().map(|l| l.entity)
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.layers.len() == 3 && self.images.iter().all(|i| images.contains(i))
    }
    pub fn render(
        &mut self,
        screen: &Screen,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        anyhow::ensure!(self.layers.len() == 3, "game-over artwork was not prepared");
        let mut image = Batch::default();
        image.quad([0., 0., 640., 480.], [0., 0., 1., 1.], [1.; 4]);
        self.layers[0].update_mesh(image, [1, 1], meshes)?;
        let mut text = Batch::default();
        centered(
            &mut text,
            &self.data.font,
            &self.data.art.caption,
            13.,
            416.,
            255,
        )?;
        for (i, caption) in self.data.art.choices.iter().enumerate() {
            centered(
                &mut text,
                &self.data.font,
                caption,
                17.,
                240. + i as f32 * 50.,
                if screen.selected == i { 255 } else { 64 },
            )?;
        }
        self.layers[1].update_mesh(text, [self.data.font.width, self.data.font.height], meshes)?;
        let mut fade = Batch::default();
        fade.quad(
            [0., 0., 640., 480.],
            [0., 0., 1., 1.],
            [1., 1., 1., f32::from(screen.alpha) / 255.],
        );
        self.layers[2].update_mesh(fade, [1, 1], meshes)?;
        for layer in &mut self.layers {
            layer.show(true, commands);
        }
        Ok(())
    }
    pub fn hide(&mut self, commands: &mut Commands) {
        for layer in &mut self.layers {
            layer.show(false, commands);
        }
    }
    pub fn despawn(self, world: &mut World) {
        for layer in self.layers {
            world.despawn(layer.entity);
        }
    }
}

fn centered(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    single_width: f32,
    y: f32,
    intensity: u8,
) -> Result<()> {
    let start = batch.positions.len();
    let mut x = 0.;
    let color = [
        f32::from(intensity) / 255.,
        f32::from(intensity) / 255.,
        f32::from(intensity) / 255.,
        1.,
    ];
    for character in text.chars() {
        let glyph = font
            .glyphs
            .get(&character)
            .context("uncooked game-over glyph")?;
        let single = resonance_content::font::is_single_byte(character);
        let drawn = if single { single_width } else { 25. };
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        batch.quad([x, y, x + drawn, y + 25.], [u, v, u + w, v + h], color);
        x += (if single { single_width / 17. } else { 1. }) * glyph.advance as f32;
    }
    let vertices = &mut batch.positions[start..];
    if !vertices.is_empty() {
        let left = vertices.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min);
        let right = vertices
            .iter()
            .map(|v| v[0])
            .fold(f32::NEG_INFINITY, f32::max);
        for vertex in vertices {
            vertex[0] -= (left + right) / 2.;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn font() -> BitmapFont {
        BitmapFont {
            version: 1,
            texture: String::new(),
            width: 64,
            height: 64,
            line_height: 24,
            glyphs: [('A', 20), ('B', 10), (' ', 6)]
                .map(|(c, advance)| {
                    (
                        c,
                        resonance_content::font::Glyph {
                            rect: [1, 1, 24, 24],
                            advance,
                        },
                    )
                })
                .into(),
            source_sha256: String::new(),
            executable_sha256: String::new(),
        }
    }

    #[test]
    fn centered_text_uses_the_bounds_of_its_rendered_glyphs() -> Result<()> {
        let font = font();
        for text in ["A", "AB", "BA", ""] {
            let mut batch = Batch::default();
            centered(&mut batch, &font, text, 17., 240., 64)?;
            assert_eq!(batch.positions.len(), text.len() * 4);
            if !text.is_empty() {
                let left = batch
                    .positions
                    .iter()
                    .map(|v| v[0])
                    .fold(f32::INFINITY, f32::min);
                let right = batch
                    .positions
                    .iter()
                    .map(|v| v[0])
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!((left + right).abs() < 0.001, "{text} was not centered");
                assert!(
                    batch
                        .positions
                        .chunks_exact(4)
                        .all(|quad| (quad[1][0] - quad[0][0] - 17.).abs() < 0.001)
                );
                assert_eq!(batch.colors[0], [64. / 255., 64. / 255., 64. / 255., 1.]);
            }
        }
        Ok(())
    }
}

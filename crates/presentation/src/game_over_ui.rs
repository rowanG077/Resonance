//! 7202C full-screen image and 62DF4 system-font draws on ordinary UI surfaces.
use super::*;
use resonance_game::game_over::{Assets as Prepared, Screen};

pub(crate) struct Artwork {
    data: Prepared,
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    layers: Vec<Layer>,
    menu: Option<MenuOverlay>,
}
impl Artwork {
    pub fn load(
        data: &Prepared,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
    ) -> Result<Self> {
        let mut images: Vec<_> = [
            &data.art.background.path,
            &data.font.texture,
            &data.font.texture,
        ]
        .into_iter()
        .map(|path| {
            server
                .load_builder()
                .with_settings(|settings: &mut ImageLoaderSettings| {
                    settings.is_srgb = false;
                    settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::ClampToEdge,
                        address_mode_v: ImageAddressMode::ClampToEdge,
                        ..ImageSamplerDescriptor::linear()
                    });
                })
                .load(path.clone())
        })
        .collect();
        let surfaces = images
            .iter()
            .map(|image| {
                materials.add(Surface {
                    source: image.clone(),
                    sampling: image.clone(),
                    frame_mask: image.clone(),
                    color_mask: image.clone(),
                    coverage: Coverage::default(),
                    layered: false,
                    screen_break: false,
                    additive: false,
                    opaque: false,
                })
            })
            .collect();
        let menu = MenuOverlay::load_with(
            |path| Ok(data.files.read(path)?.to_vec()),
            server,
            materials,
        )?;
        images.extend(menu.images().cloned());
        Ok(Self {
            data: data.clone(),
            images,
            surfaces,
            layers: Vec::new(),
            menu: Some(menu),
        })
    }
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
        }
        if let Some(menu) = &mut self.menu {
            menu.prepare(commands, meshes);
        }
        for (index, material) in self.surfaces.iter().enumerate() {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., 800. + index as f32),
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
    pub fn images(&self) -> &[Handle<Image>] {
        &self.images
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.layers
            .iter()
            .map(|l| l.entity)
            .chain(self.menu.iter().flat_map(|m| m.entities()))
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
        // The shared font publisher retains the source's solid texel in its gutter.
        fade.quad(
            [0., 0., 640., 480.],
            [0.5; 4],
            [0., 0., 0., f32::from(screen.alpha) / 255.],
        );
        self.layers[2].update_mesh(fade, [self.data.font.width, self.data.font.height], meshes)?;
        for layer in &mut self.layers {
            layer.show(true, commands);
        }
        Ok(())
    }
    pub fn take_menu(&mut self) -> Option<MenuOverlay> {
        self.menu.take()
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
        if let Some(menu) = self.menu {
            menu.despawn(world);
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
    let chars: Vec<_> = text.chars().collect();
    let mut width = 0i32;
    // Exact 62B20 reads a big-endian halfword at a single-byte character: its
    // low byte selects the following glyph, including NUL mapped to a space.
    for (i, &character) in chars.iter().enumerate() {
        let single = resonance_content::font::is_single_byte(character);
        let measured = if single {
            chars.get(i + 1).copied().unwrap_or(' ')
        } else {
            character
        };
        let glyph = font
            .glyphs
            .get(&measured)
            .context("uncooked game-over measurement glyph")?;
        let scale = if single { single_width / 17. } else { 1. };
        width = scale.mul_add(glyph.advance as f32, width as f32) as i32 - 1;
    }
    let mut x = (320 - width / 2) as f32;
    let color = [
        f32::from(intensity) / 255.,
        f32::from(intensity) / 255.,
        f32::from(intensity) / 255.,
        1.,
    ];
    for character in chars {
        let glyph = font
            .glyphs
            .get(&character)
            .context("uncooked game-over glyph")?;
        let single = resonance_content::font::is_single_byte(character);
        let drawn = if single { single_width } else { 25. };
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        batch.quad([x, y, x + drawn, y + 25.], [u, v, u + w, v + h], color);
        x += ((if single { single_width / 17. } else { 1. }) * glyph.advance as f32).trunc() - 1.;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_center_measurement_looks_ahead_but_drawing_uses_current_metrics() -> Result<()> {
        let font = BitmapFont {
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
        };
        let mut batch = Batch::default();
        centered(&mut batch, &font, "AB", 17., 240., 64)?;
        // Measurement sees B and space: (10-1)+(6-1)=14, half7.
        // Drawing advances by A's20-1 while both bitmap quads remain17 wide.
        assert_eq!(batch.positions[0], [-7., 0., 0.]);
        assert_eq!(batch.positions[1], [10., 0., 0.]);
        assert_eq!(batch.positions[4], [12., 0., 0.]);
        assert_eq!(batch.positions[6], [29., -25., 0.]);
        assert_eq!(batch.colors[0], [64. / 255., 64. / 255., 64. / 255., 1.]);
        Ok(())
    }
}

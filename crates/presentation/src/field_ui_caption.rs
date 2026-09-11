//! Prepared screen overlays share the dialogue compositor and native UI coordinates.
use super::*;
use anyhow::ensure;
use resonance_content::{effect::LocationCaption, field::FieldAssets};
use resonance_events::{GameWorld, OverlayKind};

pub(super) struct Caption {
    art: LocationCaption,
    images: Vec<Handle<Image>>,
    surfaces: Vec<Handle<Surface>>,
    pub layers: Vec<Layer>,
}
impl Caption {
    pub fn load(
        field: &FieldAssets,
        read: impl Fn(&str) -> Result<Vec<u8>>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
    ) -> Result<BTreeMap<u32, Self>> {
        field
            .captions
            .iter()
            .map(|(&id, path)| {
                let art: LocationCaption = serde_json::from_slice(&read(path)?)?;
                art.validate()?;
                let images: Vec<_> = art
                    .textures
                    .iter()
                    .map(|texture| {
                        server
                            .load_builder()
                            .with_settings(|s: &mut ImageLoaderSettings| {
                                s.is_srgb = false;
                                s.sampler =
                                    ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
                            })
                            .load(texture.path.clone())
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
                        })
                    })
                    .collect();
                Ok((
                    id as u32,
                    Self {
                        art,
                        images,
                        surfaces,
                        layers: Vec::new(),
                    },
                ))
            })
            .collect()
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id()))
    }
    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
        }
        for (i, material) in self.surfaces.iter().enumerate() {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., 2. + i as f32 * 0.01),
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
    pub fn render(
        &mut self,
        resource: u32,
        world: &GameWorld,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<Vec<i32>> {
        let mut batches: Vec<_> = self.layers.iter().map(|_| Batch::default()).collect();
        let mut handled = Vec::new();
        for (&id, overlay) in &world.overlays {
            let actor = world.actors.get(&id).context("overlay actor is missing")?;
            if actor.resource != resource {
                continue;
            }
            ensure!(
                matches!(overlay.kind, OverlayKind::LocationCaption { .. })
                    && overlay.size == [-1, -1]
                    && overlay.angle == 0
                    && overlay.duration <= 1,
                "unsupported location caption presentation: {overlay:?}"
            );
            handled.push(id);
            if !actor.visible || actor.appearance.model_hidden {
                continue;
            }
            let alpha = overlay.alpha(world.tick);
            for sprite in self
                .art
                .frame(world.tick.saturating_sub(overlay.born) as usize)
            {
                let [x, y, _] = actor.position;
                let [l, t, r, b] = sprite.rect;
                // The authored controller replaces the reveal alpha while fading out.
                let alpha = if alpha == 255 { sprite.alpha } else { alpha };
                batches[sprite.texture].quad(
                    [x + l, y + t, x + r, y + b],
                    sprite.uv,
                    [1., 1., 1., f32::from(alpha) / 255.],
                );
            }
        }
        for (layer, batch) in self.layers.iter_mut().zip(batches) {
            let visible = !batch.indices.is_empty();
            if visible {
                layer.update_mesh(batch, [1, 1], meshes)?;
            }
            layer.show(visible, commands);
        }
        Ok(handled)
    }
}

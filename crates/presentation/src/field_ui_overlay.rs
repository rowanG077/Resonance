//! Prepared texture banks and per-instance screen quads in native UI coordinates.
use super::*;
use anyhow::ensure;
use bevy::image::ImageFilterMode;
use resonance_content::{
    TextureWrap,
    effect::OverlayArt,
    field::FieldAssets,
    texture::{Filter, Sampler},
};
use resonance_events::{GameWorld, OverlayKind};

struct Bank {
    art: OverlayArt,
    surfaces: Vec<Handle<Surface>>,
}

pub(super) struct Artwork {
    banks: BTreeMap<u32, Bank>,
    images: Vec<Handle<Image>>,
    pub layers: Vec<Layer>,
    pub warm: Vec<Layer>,
    prepared: bool,
}

impl Artwork {
    pub fn load(
        field: &FieldAssets,
        read: impl Fn(&str) -> Result<Vec<u8>>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        images: &mut Assets<Image>,
    ) -> Result<Self> {
        let mut banks = BTreeMap::new();
        let mut loaded = Vec::new();
        let mut samplers: Vec<(ImageSamplerDescriptor, Handle<Image>)> = Vec::new();
        for (&id, path) in &field.overlays {
            let art: OverlayArt = serde_json::from_slice(&read(path)?)?;
            art.validate()?;
            let mut surfaces = Vec::new();
            for texture in &art.textures {
                let descriptor = sampler(&texture.sampler)?;
                // Equal pixel files can have different native sampling. A tiny
                // separate sampler image avoids changing a shared image asset.
                let sampling = match samplers.iter().find(|(s, _)| *s == descriptor) {
                    Some((_, image)) => image.clone(),
                    None => {
                        let image = images.add(Image {
                            sampler: ImageSampler::Descriptor(descriptor.clone()),
                            ..Image::default()
                        });
                        samplers.push((descriptor, image.clone()));
                        loaded.push(image.clone());
                        image
                    }
                };
                let pages: Vec<Handle<Image>> = texture
                    .images
                    .iter()
                    .map(|image| {
                        server
                            .load_builder()
                            .with_settings(|settings: &mut ImageLoaderSettings| {
                                settings.is_srgb = false
                            })
                            .load(image.path.clone())
                    })
                    .collect();
                // Ordinary overlays select a texture, always using its first palette.
                let source = pages[0].clone();
                surfaces.push(materials.add(Surface {
                    source: source.clone(),
                    sampling,
                    frame_mask: source.clone(),
                    color_mask: source,
                    coverage: Coverage::default(),
                    layered: false,
                    screen_break: false,
                    additive: false,
                    opaque: false,
                }));
                loaded.extend(pages);
            }
            banks.insert(id as u32, Bank { art, surfaces });
        }
        Ok(Self {
            banks,
            images: loaded,
            layers: Vec::new(),
            warm: Vec::new(),
            prepared: false,
        })
    }

    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id()))
    }

    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if self.prepared {
            return;
        }
        for surface in self.banks.values().flat_map(|bank| &bank.surfaces) {
            self.warm.push(allocate(surface.clone(), commands, meshes));
        }
        self.prepared = true;
    }

    pub fn despawn(&mut self, world: &mut World) {
        for layer in self.layers.drain(..).chain(self.warm.drain(..)) {
            world.despawn(layer.entity);
        }
        self.prepared = false;
    }

    pub fn render(
        &mut self,
        world: &GameWorld,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<Vec<i32>> {
        ensure!(self.prepared, "field overlays were not prepared");
        let mut draws = Vec::new();
        let mut handled = Vec::new();
        for (&id, overlay) in &world.overlays {
            let actor = world.actors.get(&id).context("overlay actor is missing")?;
            let bank = self
                .banks
                .get(&actor.resource)
                .with_context(|| format!("uncooked overlay resource {:#x}", actor.resource))?;
            let alpha = overlay.alpha(world.tick);
            let mut emit = |texture: usize, rect, uv, color, scale, depth: f32| {
                if actor.visible && !actor.appearance.model_hidden && (0. ..=200.).contains(&depth)
                {
                    draws.push(Draw {
                        resource: actor.resource,
                        texture,
                        instance: actor.instance,
                        depth,
                        batch: quad(rect, uv, color, actor.position, scale, actor.heading),
                    });
                }
            };
            match &overlay.kind {
                OverlayKind::Sprite(sprite) => {
                    let texture = usize::from(sprite.image);
                    let image = &bank
                        .art
                        .textures
                        .get(texture)
                        .context("overlay texture index is outside its bank")?
                        .images[0];
                    let mut color = overlay.rgba.map(|v| f32::from(v) / 255.);
                    color[3] = f32::from(alpha) / 255.;
                    emit(
                        texture,
                        centered_rect(overlay.size, [image.width, image.height]),
                        [0., 0., 255. / 256., 255. / 256.],
                        color,
                        sprite.scale,
                        actor.position[2] + sprite.depth as i16 as f32 * sprite.scale[2],
                    );
                }
                OverlayKind::LocationCaption { .. } => {
                    ensure!(
                        overlay.size == [-1, -1] && actor.heading == 0. && overlay.duration <= 1,
                        "unsupported location caption presentation: {overlay:?}"
                    );
                    let sizes = bank.art.textures.iter().map(|texture| {
                        let image = &texture.images[0];
                        [image.width, image.height]
                    });
                    resonance_events::caption::frame(
                        sizes,
                        world.tick.saturating_sub(overlay.born) as usize,
                        |sprite| {
                            // The controller replaces reveal opacity while fading out.
                            let opacity = if alpha == 255 { sprite.alpha } else { alpha };
                            emit(
                                sprite.texture,
                                sprite.rect,
                                sprite.uv,
                                [1., 1., 1., f32::from(opacity) / 255.],
                                [1.; 3],
                                actor.position[2],
                            );
                        },
                    )?;
                }
            }
            handled.push(id);
        }
        // Native positive Z is farther away. Keep authored sprite order at
        // equal depth; resource IDs never affect composition. The UI blend
        // compositor does not yet reproduce inherited native depth writes.
        sort(&mut draws);
        let count = draws.len();
        for (index, draw) in draws.into_iter().enumerate() {
            let surface = self.banks[&draw.resource].surfaces[draw.texture].clone();
            if index == self.layers.len() {
                // All material/vertex-layout combinations were warmed before play.
                self.layers
                    .push(allocate(surface.clone(), commands, meshes));
            }
            let layer = &mut self.layers[index];
            if layer.material != surface {
                commands
                    .entity(layer.entity)
                    .insert(MeshMaterial2d(surface.clone()));
                layer.material = surface;
            }
            layer.update_mesh(draw.batch, [1, 1], meshes)?;
            commands.entity(layer.entity).insert(Transform::from_xyz(
                0.,
                0.,
                2. + index as f32 / (count + 1) as f32,
            ));
            layer.show(true, commands);
        }
        for layer in &mut self.layers[count..] {
            layer.show(false, commands);
        }
        Ok(handled)
    }
}

fn sampler(source: &Sampler) -> Result<ImageSamplerDescriptor> {
    source.validate()?;
    ensure!(
        source.lod.bias == 0. && !source.lod.edge,
        "overlay LOD bias and edge LOD need shader support"
    );
    let (min_filter, mipmap_filter, mipmapped) = match source.min_filter {
        Filter::Nearest => (ImageFilterMode::Nearest, ImageFilterMode::Nearest, false),
        Filter::Linear => (ImageFilterMode::Linear, ImageFilterMode::Nearest, false),
        Filter::NearestMipmapNearest => (ImageFilterMode::Nearest, ImageFilterMode::Nearest, true),
        Filter::LinearMipmapNearest => (ImageFilterMode::Linear, ImageFilterMode::Nearest, true),
        Filter::NearestMipmapLinear => (ImageFilterMode::Nearest, ImageFilterMode::Linear, true),
        Filter::LinearMipmapLinear => (ImageFilterMode::Linear, ImageFilterMode::Linear, true),
    };
    let wrap = |mode| match mode {
        TextureWrap::Clamp => ImageAddressMode::ClampToEdge,
        TextureWrap::Repeat => ImageAddressMode::Repeat,
        TextureWrap::Mirror => ImageAddressMode::MirrorRepeat,
    };
    Ok(ImageSamplerDescriptor {
        address_mode_u: wrap(source.wrap[0]),
        address_mode_v: wrap(source.wrap[1]),
        min_filter,
        mag_filter: if matches!(source.mag_filter, Filter::Nearest) {
            ImageFilterMode::Nearest
        } else {
            ImageFilterMode::Linear
        },
        mipmap_filter,
        lod_min_clamp: if mipmapped {
            f32::from(source.lod.min)
        } else {
            0.
        },
        lod_max_clamp: if mipmapped {
            f32::from(source.lod.max)
        } else {
            0.
        },
        ..default()
    })
}

struct Draw {
    resource: u32,
    texture: usize,
    instance: u64,
    depth: f32,
    batch: Batch,
}
fn sort(draws: &mut [Draw]) {
    draws.sort_by(|a, b| {
        b.depth
            .total_cmp(&a.depth)
            .then(a.instance.cmp(&b.instance))
    });
}

fn centered_rect(size: [i32; 2], image: [u32; 2]) -> [f32; 4] {
    let [width, height] = std::array::from_fn(|i| {
        if size[i] == -1 {
            image[i] as i32
        } else {
            size[i]
        }
    });
    [
        width.wrapping_neg() / 2,
        height.wrapping_neg() / 2,
        width / 2,
        height / 2,
    ]
    .map(|v| v as i16 as f32)
}

fn quad(
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    position: [f32; 3],
    scale: [f32; 3],
    angle: f32,
) -> Batch {
    let mut batch = Batch::default();
    batch.quad(rect, uv, color);
    let (sin, cos) = angle.to_radians().sin_cos();
    for vertex in &mut batch.positions {
        let [x, y] = [vertex[0] + 320., 240. - vertex[1]];
        vertex[0] = position[0] + scale[0] * (cos * x - sin * y) - 320.;
        vertex[1] = 240. - position[1] - scale[1] * (sin * x + cos * y);
    }
    batch
}

fn allocate(
    material: Handle<Surface>,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) -> Layer {
    let mut batch = Batch::default();
    batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
    let mesh = meshes.add(batch.mesh([1, 1]));
    let entity = commands
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material.clone()),
            Transform::from_xyz(0., 0., 2.),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_rotate_before_nonuniform_scale_and_keep_instance_order_across_banks() {
        let rect = centered_rect([-1, -1], [259, 65]);
        assert_eq!(rect, [-129., -32., 129., 32.]);
        let batch = quad(
            rect,
            [0., 0., 255. / 256., 255. / 256.],
            [0.2, 0.4, 0.6, 0.8],
            [320., 240., 0.],
            [2., 3., 1.],
            90.,
        );
        for (actual, expected) in batch.positions.iter().zip([
            [64., 387., 0.],
            [64., -387., 0.],
            [-64., -387., 0.],
            [-64., 387., 0.],
        ]) {
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < 0.001)
            );
        }
        assert_eq!(batch.uv[2], [255. / 256.; 2]);
        assert_eq!(batch.colors[0], [0.2, 0.4, 0.6, 0.8]);
        let mut draws = [(9, 2, 0.), (1, 3, 0.), (8, 1, 100.), (7, 2, 0.)].map(
            |(resource, instance, depth)| Draw {
                resource,
                instance,
                depth,
                texture: 0,
                batch: Batch::default(),
            },
        );
        sort(&mut draws);
        assert_eq!(draws.map(|d| d.resource), [8, 9, 7, 1]);
    }
}

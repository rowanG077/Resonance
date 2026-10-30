//! Prepared texture banks and per-instance screen quads in native UI coordinates.
use super::*;
use anyhow::ensure;
use bevy::image::ImageFilterMode;
use resonance_content::{
    TextureWrap,
    effect::OverlayArt,
    texture::{Filter, Sampler},
};
use resonance_events::{GameWorld, OverlayKind};

struct Bank {
    sizes: Vec<[u32; 2]>,
    surfaces: Vec<Option<Handle<Surface>>>,
    controls: bool,
}

pub(super) struct Artwork {
    banks: BTreeMap<u32, Bank>,
    images: Vec<Handle<Image>>,
    pub layers: Vec<Layer>,
    pub warm: Vec<Layer>,
    prepared: bool,
    input_atlas: Option<std::sync::Arc<crate::input_icons::Atlas>>,
}

impl Artwork {
    pub fn load(
        overlays: &BTreeMap<i32, String>,
        read: impl Fn(&str) -> Result<Vec<u8>>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        images: &mut Assets<Image>,
        inputs: &crate::input_icons::Prepared,
    ) -> Result<Self> {
        let mut banks = BTreeMap::new();
        let mut loaded = Vec::new();
        let mut samplers: Vec<(ImageSamplerDescriptor, Handle<Image>)> = Vec::new();
        let mut input_atlas = None;
        let mut input_surface = None;
        for (&id, path) in overlays {
            let controls = path == resonance_content::input_prompts::FIELD_BANK_PATH;
            if controls && input_surface.is_none() {
                let atlas = inputs.atlas.clone();
                let image = inputs.image.clone();
                loaded.push(image.clone());
                input_surface = Some(materials.add(Surface {
                    source: image.clone(),
                    sampling: image.clone(),
                    frame_mask: image.clone(),
                    color_mask: image,
                    coverage: Coverage::default(),
                    additive: false,
                    texture_effect: Default::default(),
                    opaque: false,
                }));
                input_atlas = Some(atlas);
            }
            let art = OverlayArt::load(path, &read)?;
            art.validate()?;
            let mut surfaces = Vec::new();
            let mut sizes = Vec::new();
            if controls {
                ensure!(
                    art.textures.len() == resonance_content::input_prompts::FIELD_GLYPHS,
                    "invalid scripted input bank"
                );
            }
            for (index, texture) in art.textures.iter().enumerate() {
                if controls && !resonance_content::input_prompts::PANEL_GLYPHS.contains(&index) {
                    sizes.push([24, 24]);
                    surfaces.push(input_surface.clone());
                    continue;
                }
                let Some(texture) = texture else {
                    surfaces.push(None);
                    sizes.push([0, 0]);
                    continue;
                };
                sizes.push(resonance_asset_writer::texture::dimensions(&read(
                    &texture.images[0],
                )?)?);
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
                let source: Handle<Image> = server
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = false)
                    .load(texture.images[0].clone());
                loaded.push(source.clone());
                surfaces.push(Some(materials.add(Surface {
                    source: source.clone(),
                    sampling,
                    frame_mask: source.clone(),
                    color_mask: source,
                    coverage: Coverage::default(),
                    additive: false,
                    texture_effect: Default::default(),
                    opaque: false,
                })));
            }
            banks.insert(
                id as u32,
                Bank {
                    sizes,
                    surfaces,
                    controls,
                },
            );
        }
        Ok(Self {
            banks,
            images: loaded,
            layers: Vec::new(),
            warm: Vec::new(),
            prepared: false,
            input_atlas,
        })
    }

    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.images.iter()
    }

    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id()))
    }

    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if self.prepared {
            return;
        }
        for surface in self
            .banks
            .values()
            .flat_map(|bank| bank.surfaces.iter().flatten())
        {
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
        device: crate::input_icons::PromptDevice,
    ) -> Result<Vec<i32>> {
        ensure!(self.prepared, "field overlays were not prepared");
        let defaults = resonance_content::input::Keymap::default();
        let keymap = world
            .party
            .as_ref()
            .map(|party| &party.settings.preferences.keymap)
            .unwrap_or(&defaults);
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
                if bank.surfaces.get(texture).is_some_and(Option::is_some)
                    && actor.visible
                    && !actor.appearance.model_hidden
                    && (0. ..=200.).contains(&depth)
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
                    let size = *bank
                        .sizes
                        .get(texture)
                        .context("overlay texture index is outside its bank")?;
                    let mut color = overlay.rgba.map(|v| f32::from(v) / 255.);
                    color[3] = f32::from(alpha) / 255.;
                    let rect = centered_rect(overlay.size, size);
                    let depth = actor.position[2] + sprite.depth as i16 as f32 * sprite.scale[2];
                    if bank.controls
                        && !resonance_content::input_prompts::PANEL_GLYPHS.contains(&texture)
                    {
                        let atlas = self
                            .input_atlas
                            .as_ref()
                            .context("scripted input atlas missing")?;
                        for prompt in crate::input_icons::controller_quads(
                            atlas, keymap, texture, rect, true, device,
                        )? {
                            let [u, v, w, h] = prompt.uv.map(|v| v as f32);
                            let [aw, ah] = atlas.size.map(|v| v as f32);
                            emit(
                                texture,
                                prompt.rect,
                                [u / aw, v / ah, (u + w) / aw, (v + h) / ah],
                                color,
                                sprite.scale,
                                depth,
                            );
                        }
                    } else {
                        emit(
                            texture,
                            rect,
                            [0., 0., 255. / 256., 255. / 256.],
                            color,
                            sprite.scale,
                            depth,
                        );
                    }
                }
                OverlayKind::LocationCaption { .. } => {
                    ensure!(
                        overlay.size == [-1, -1] && actor.heading == 0. && overlay.duration <= 1,
                        "unsupported location caption presentation: {overlay:?}"
                    );
                    resonance_events::caption::frame(
                        bank.sizes.iter().copied(),
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
            let surface = self.banks[&draw.resource].surfaces[draw.texture]
                .as_ref()
                .unwrap()
                .clone();
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
    fn controller_bank_never_loads_legacy_button_pixels_and_uses_cooked_prompts() -> Result<()> {
        use resonance_content::input_prompts as prompts;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>();
        crate::input_icons::install(&mut app);
        let server = app.world().resource::<AssetServer>().clone();
        let mut materials = Assets::<Surface>::default();
        let mut images = Assets::<Image>::default();
        let paths: Vec<_> = (0..32).map(|i| format!("legacy/{i}.ktx2")).collect();
        let metadata = serde_json::to_vec(&paths)?;
        let panel = resonance_asset_writer::ktx2::encode_rgba8(16, 24, &vec![255; 16 * 24 * 4])?;
        let reads = std::cell::RefCell::new(Vec::new());
        let read = |path: &str| -> Result<Vec<u8>> {
            reads.borrow_mut().push(path.to_string());
            if path == prompts::IMAGE_PATH {
                return Ok(crate::input_icons::test_image().to_vec());
            }
            if path == prompts::MANIFEST_PATH {
                return Ok(serde_json::to_vec(crate::input_icons::default_atlas())?);
            }
            if path == prompts::FIELD_BANK_PATH {
                return Ok(metadata.clone());
            }
            if ["legacy/19.ktx2", "legacy/20.ktx2", "legacy/21.ktx2"].contains(&path) {
                return Ok(panel.clone());
            }
            anyhow::bail!("absent {path}")
        };
        let inputs = crate::input_icons::prepare(&read, &server)?;
        let mut art = Artwork::load(
            &[(123, prompts::FIELD_BANK_PATH.into())].into(),
            read,
            &server,
            &mut materials,
            &mut images,
            &inputs,
        )?;
        assert!(art.input_atlas.is_some());
        assert!(std::sync::Arc::ptr_eq(
            art.input_atlas.as_ref().unwrap(),
            &inputs.atlas
        ));
        for (path, expected) in [(prompts::IMAGE_PATH, 0), (prompts::MANIFEST_PATH, 1)] {
            assert_eq!(
                reads
                    .borrow()
                    .iter()
                    .filter(|read| read.as_str() == path)
                    .count(),
                expected
            );
        }
        let bank = &art.banks[&123];
        assert!(bank.controls);
        for id in 0..32 {
            if prompts::PANEL_GLYPHS.contains(&id) {
                continue;
            }
            assert_eq!(bank.sizes[id], [24, 24]);
            assert_eq!(bank.surfaces[id], bank.surfaces[0]);
            assert!(!reads.borrow().contains(&format!("legacy/{id}.ktx2")));
        }
        let surface = materials.get(bank.surfaces[0].as_ref().unwrap()).unwrap();
        assert_eq!(
            server.get_path(surface.source.id()).unwrap().to_string(),
            prompts::IMAGE_PATH
        );
        // Exercise the real sprite path through a tiny authored overlay call.
        // This preserves native alpha, transform, size and image-index behavior.
        let mut words = vec![4u16, 0, 0, 0];
        for arg in [7i32, 123, 320, 240, -1, -1, 0, 255, 255, 255, 255, 0, 0] {
            words.extend([
                0x0200,
                arg as u16,
                (arg as u32 >> 16) as u16,
                0x3000,
                0x4000,
            ]);
        }
        words.extend([0x204b, 0x20ff]);
        let program = symphonia_script::Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )?;
        let mut resources = resonance_events::ResourceLibrary::default();
        resources
            .bindings
            .insert(123, (resonance_events::ResourceKind::Overlay, 123));
        let mut events = resonance_events::EventRuntime::new(
            std::sync::Arc::new(program),
            std::sync::Arc::new(resources),
        )?;
        let render_world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &render_world);
        let mut meshes = Assets::<Mesh>::default();
        art.prepare(&mut commands, &mut meshes);
        for id in 0..32 {
            if prompts::PANEL_GLYPHS.contains(&id) {
                continue;
            }
            let OverlayKind::Sprite(sprite) = &mut events.world.overlays.get_mut(&7).unwrap().kind
            else {
                unreachable!()
            };
            sprite.image = id as u8;
            assert_eq!(
                art.render(
                    &events.world,
                    &mut commands,
                    &mut meshes,
                    crate::input_icons::PromptDevice::Controller
                )?,
                [7]
            );
            let atlas = art.input_atlas.as_ref().unwrap();
            let expected = crate::input_icons::controller_quads(
                atlas,
                &Default::default(),
                id,
                [-12., -12., 12., 12.],
                true,
                crate::input_icons::PromptDevice::Controller,
            )?;
            let visible: Vec<_> = art.layers.iter().filter(|l| l.visible).collect();
            assert_eq!(visible.len(), expected.len(), "glyph {id}");
            for (layer, expected) in visible.into_iter().zip(expected) {
                let (batch, _) = layer.uploaded.as_ref().unwrap();
                assert_eq!(
                    batch.uv[0],
                    [
                        expected.uv[0] as f32 / atlas.size[0] as f32,
                        expected.uv[1] as f32 / atlas.size[1] as f32
                    ],
                    "glyph {id}"
                );
                assert_eq!(batch.colors[0][3], 1.);
                assert!(
                    batch
                        .positions
                        .iter()
                        .all(|p| p[0].abs() <= 12. && p[1].abs() <= 12.)
                );
            }
        }
        Ok(())
    }
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

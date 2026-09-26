//! Fixed ribbon/rope draw slots. History and sampled endpoints belong to combat.
use super::*;
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
use resonance_content::{
    battle_model::TrailMaterial,
    font::UiTexture,
    texture::{Filter, Sampler},
};

pub(crate) struct TrailAsset {
    pub resource: u32,
    pub material: TrailMaterial,
    /// The selected native palette page, resolved by encounter preparation.
    pub texture: Option<(UiTexture, Sampler)>,
    pub capacity: usize,
}
struct Ribbon {
    material: TrailMaterial,
    texture: Option<(Handle<Image>, UiTexture, Sampler)>,
    draws: Vec<Draw>,
    capacity: usize,
}
struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
}
pub(super) struct Trails {
    diagnostics: Diagnostics,
    ribbons: BTreeMap<u32, Ribbon>,
    ropes: Vec<Draw>,
    ready: bool,
}
impl Trails {
    pub fn load(
        assets: Vec<TrailAsset>,
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut ribbons = BTreeMap::new();
        for asset in assets {
            let resource = asset.resource;
            let result = (|| -> Result<()> {
                ensure!(
                    asset.capacity > 0
                        && asset.material.flags & !3 == 0
                        && asset.material.flags & 3 < 2,
                    "unsupported battle ribbon material"
                );
                ensure!(
                    (asset.material.texture < 0) == asset.texture.is_none(),
                    "battle ribbon texture is missing"
                );
                let texture = asset
                    .texture
                    .map(|(image, sampler)| -> Result<_> {
                        image.validate()?;
                        sampler.validate()?;
                        let handle = server
                            .load_builder()
                            .with_settings(|s: &mut ImageLoaderSettings| {
                                s.is_srgb = false;
                                s.sampler = ImageSampler::linear();
                            })
                            .load(image.path.clone());
                        Ok((handle, image, sampler))
                    })
                    .transpose()?;
                ensure!(
                    ribbons
                        .insert(
                            asset.resource,
                            Ribbon {
                                material: asset.material,
                                texture,
                                draws: Vec::new(),
                                capacity: asset.capacity
                            }
                        )
                        .is_none(),
                    "duplicate battle ribbon resource"
                );
                Ok(())
            })();
            if let Err(error) = result {
                diagnostics.report(&format!("battle ribbon {resource}"), error)?;
            }
        }
        Ok(Self {
            diagnostics,
            ribbons,
            ropes: Vec::new(),
            ready: false,
        })
    }
    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.ribbons
            .values()
            .filter_map(|r| r.texture.as_ref().map(|(h, _, _)| h))
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.ribbons
            .values()
            .flat_map(|r| &r.draws)
            .chain(&self.ropes)
            .map(|d| d.entity)
    }
    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        assets: &mut AssetsForView,
        sampled: &mut crate::scene::SampledImages,
    ) -> Result<()> {
        if self.ready {
            return Ok(());
        }
        for ribbon in self.ribbons.values_mut() {
            let color = ribbon.texture.as_ref().map(|(image, _, sampler)| {
                let binding = resonance_content::TextureBinding {
                    texture: 0,
                    wrap_u: sampler.wrap[0],
                    wrap_v: sampler.wrap[1],
                    nearest_min: matches!(
                        sampler.min_filter,
                        Filter::Nearest
                            | Filter::NearestMipmapNearest
                            | Filter::NearestMipmapLinear
                    ),
                    nearest_mag: matches!(sampler.mag_filter, Filter::Nearest),
                };
                crate::scene::sampled_image(
                    Some((image.clone(), binding)),
                    &mut assets.images,
                    sampled,
                )
                .unwrap()
            });
            let surface = assets.surfaces.add(TitleSurface {
                blend: true,
                additive: ribbon.material.flags & 3 == 1,
                depth_write: false,
                depth_equal: true,
                cull: resonance_content::CullFace::None,
                ..TitleSurface::textured(color)
            });
            for _ in 0..ribbon.capacity {
                ribbon
                    .draws
                    .push(Draw::prepare(commands, &mut assets.meshes, surface.clone()));
            }
        }
        let surface = assets.surfaces.add(TitleSurface {
            blend: true,
            // 155CC restores ordinary weapon depth writes before 46EA4's rope.
            depth_write: true,
            depth_equal: true,
            cull: resonance_content::CullFace::None,
            ..default()
        });
        // Eight native actors, each with at most two ordinary weapon slots.
        for _ in 0..16 {
            self.ropes
                .push(Draw::prepare(commands, &mut assets.meshes, surface.clone()));
        }
        self.ready = true;
        Ok(())
    }
    pub fn set_layer(&self, commands: &mut Commands, layer: usize, visible: bool) {
        for entity in self.entities() {
            commands.entity(entity).insert((
                RenderLayers::layer(layer),
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
    }
    pub fn apply(
        &self,
        frame: &BattleFrame,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        order: &ActorOrder,
        models: &BTreeMap<u32, super::Model>,
    ) -> Result<()> {
        for entity in self.entities() {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        let mut used = BTreeMap::<u32, usize>::new();
        for trail in &frame.trails {
            let result = (|| -> Result<()> {
                let ribbon = self
                    .ribbons
                    .get(&trail.resource)
                    .context("unprepared battle ribbon material")?;
                let count = used.entry(trail.resource).or_default();
                let draw = ribbon
                    .draws
                    .get(*count)
                    .context("battle ribbon exceeds prepared instance capacity")?;
                *count += 1;
                let size = ribbon
                    .texture
                    .as_ref()
                    .map_or([1, 1], |(_, image, _)| [image.width, image.height]);
                let data = ribbon_mesh(trail, &ribbon.material, size)?;
                draw.apply(data, order.get(trail.actor)? + 49152, commands, meshes)?;
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics
                    .report(&format!("battle ribbon draw {}", trail.resource), error)?;
            }
        }
        let camera = frame.camera.context("battle rope has no camera")?;
        let eye = Vec3::from_array(camera.eye);
        let rotation = Transform::from_translation(eye)
            .looking_at(Vec3::from_array(camera.focus), Vec3::Y)
            .rotation;
        let mut rope_slot = 0;
        for weapon in &frame.weapons {
            if !weapon.visible || weapon.links.is_empty() {
                continue;
            }
            let result = (|| -> Result<()> {
                let draw = self
                    .ropes
                    .get(rope_slot)
                    .context("battle rope exceeds prepared weapon slots")?;
                rope_slot += 1;
                let actor = frame
                    .actors
                    .get(weapon.owner.index())
                    .context("invalid battle rope owner")?;
                let distance = eye.distance(Vec3::from_array(actor.position));
                let width = if distance > 4000. {
                    1.
                } else if distance > 2800. {
                    2.
                } else if distance > 1600. {
                    3.
                } else if distance > 400. {
                    4.
                } else {
                    5.
                };
                let world = Mat4::from_cols_array_2d(&weapon.world);
                let mut data = Vertices::default();
                for &[a, b] in &weapon.links {
                    let endpoint = |bone| -> Result<Vec3> {
                        let bone = weapon
                            .bones
                            .get(usize::from(bone))
                            .context("invalid battle rope bone")?;
                        Ok((world * Mat4::from_cols_array_2d(bone)).transform_point3(Vec3::ZERO))
                    };
                    data.line(endpoint(a)?, endpoint(b)?, eye, rotation, width);
                }
                draw.apply(
                    data.mesh(),
                    order.get(weapon.owner)?
                        + if models
                            .get(&weapon.resource)
                            .context("missing battle rope model")?
                            .weapon_flags
                            .is_some_and(|flags| flags & 1 != 0)
                        {
                            8192
                        } else {
                            32768
                        }
                        + u32::from(weapon.slot) * 1024
                        + 1023,
                    commands,
                    meshes,
                )?;
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics
                    .report(&format!("battle rope draw {}", weapon.resource), error)?;
            }
        }
        Ok(())
    }
    pub fn despawn(self, commands: &mut Commands) {
        for entity in self.entities() {
            commands.entity(entity).despawn();
        }
    }
}
impl Draw {
    fn prepare(
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        material: Handle<TitleSurface>,
    ) -> Self {
        let mut data = Vertices::default();
        data.quad(
            [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::ONE],
            [[0.; 2]; 4],
            [[1.; 4]; 4],
        );
        let mesh = meshes.add(data.mesh());
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::default(),
                NoFrustumCulling,
                RenderLayers::layer(WARM_LAYER),
                DrawOrder(ACTORS, 0),
            ))
            .id();
        Self { entity, mesh }
    }
    fn apply(
        &self,
        mesh: Mesh,
        order: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        *meshes
            .get_mut(&self.mesh)
            .context("prepared battle ribbon mesh is missing")? = mesh;
        commands
            .entity(self.entity)
            .insert((Visibility::Inherited, DrawOrder(order, 0)));
        Ok(())
    }
}
#[derive(Default)]
struct Vertices {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
}
impl Vertices {
    fn quad(&mut self, positions: [Vec3; 4], uv: [[f32; 2]; 4], colors: [[f32; 4]; 4]) {
        // Native GX_QUADS expands the same two triangles in this strip order.
        for i in [0, 1, 2, 2, 1, 3] {
            self.positions.push(positions[i].to_array());
            self.normals.push([0., 1., 0.]);
            self.uv.push(uv[i]);
            self.colors.push(colors[i]);
        }
    }
    fn line(&mut self, a: Vec3, b: Vec3, eye: Vec3, camera: Quat, width: f32) {
        let inverse = camera.inverse();
        let av = inverse * (a - eye);
        let bv = inverse * (b - eye);
        if av.z >= 0. || bv.z >= 0. {
            return;
        }
        let projected = Vec2::new(bv.x / -bv.z - av.x / -av.z, bv.y / -bv.z - av.y / -av.z);
        let normal = Vec2::new(-projected.y, projected.x).normalize_or_zero();
        let scale =
            width * (13.33_f32.to_radians() * 0.5).tan() / resonance_content::SCENE_HEIGHT as f32;
        let offset = |depth: f32| camera * Vec3::new(normal.x, normal.y, 0.) * (scale * -depth);
        let aa = offset(av.z);
        let bb = offset(bv.z);
        self.quad(
            [a - aa, a + aa, b - bb, b + bb],
            [[0.; 2]; 4],
            [[1., 1., 1., 128. / 255.]; 4],
        );
    }
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    }
}
fn ribbon_mesh(
    trail: &resonance_battle::TrailFrame,
    material: &TrailMaterial,
    size: [u32; 2],
) -> Result<Mesh> {
    ensure!(
        (2..=3).contains(&trail.rows.len()) && size.iter().all(|&n| n > 0),
        "invalid battle ribbon geometry"
    );
    let [u, v, width, height] = material.uv;
    let du = f32::from(width) / 16.;
    // Three endpoint rows use native arithmetic-half-height, including negatives.
    let dv = height >> (trail.rows.len() - 2);
    let rgb = material.color.map(|v| f32::from(v) / 128.);
    let mut data = Vertices::default();
    for (row, pair) in trail.rows.windows(2).enumerate() {
        let top = v + height - row as i16 * dv;
        let bottom = top - dv;
        for column in 0..15 {
            let left = (f32::from(u) + column as f32 * du).trunc();
            let right = left + du.trunc();
            let points = [
                pair[0][column],
                pair[0][column + 1],
                pair[1][column],
                pair[1][column + 1],
            ];
            let uv = [
                [left, f32::from(top)],
                [right, f32::from(top)],
                [left, f32::from(bottom)],
                [right, f32::from(bottom)],
            ]
            .map(|[u, v]| [u / size[0] as f32, v / size[1] as f32]);
            data.quad(
                points.map(|p| Vec3::from_array(p.position)),
                uv,
                points.map(|p| [rgb[0], rgb[1], rgb[2], f32::from(p.alpha) / 255.]),
            );
        }
    }
    Ok(data.mesh())
}

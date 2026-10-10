//! Weapon ribbons own their short pose history; drawing never advances it.
use super::*;
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
use resonance_battle::{ActorId, Cue, WeaponFrame};
use resonance_content::{
    battle_model::TrailMaterial,
    font::UiTexture,
    texture::{Filter, Sampler},
};
use resonance_events::effect::Blend;
use std::collections::VecDeque;

struct Ribbon {
    bones: Vec<u16>,
    material: TrailMaterial,
    texture: Option<(Handle<Image>, UiTexture, Sampler)>,
    draw: Option<Draw>,
}
struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
}
pub(super) struct Trails {
    diagnostics: Diagnostics,
    ribbons: BTreeMap<u32, Ribbon>,
    history: BTreeMap<(ActorId, u8), Trail>,
    ropes: Vec<Draw>,
    ready: bool,
}
impl Trails {
    pub fn load(
        assets: Vec<RenderTrail>,
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut ribbons = BTreeMap::new();
        for asset in assets {
            let resource = asset.resource;
            let result = (|| -> Result<()> {
                ensure!(
                    (2..=3).contains(&asset.bones.len()),
                    "invalid weapon trail endpoints"
                );
                ensure!(
                    asset.material.texture.is_some() == asset.texture.is_some(),
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
                                bones: asset.bones,
                                material: asset.material,
                                texture,
                                draw: None,
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
            history: BTreeMap::new(),
            ropes: Vec::new(),
            ready: false,
        })
    }
    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.ribbons
            .values()
            .filter_map(|r| r.texture.as_ref().map(|(h, _, _)| h))
    }
    pub fn images_mut(&mut self) -> impl Iterator<Item = &mut Handle<Image>> {
        self.ribbons
            .values_mut()
            .filter_map(|r| r.texture.as_mut().map(|(h, _, _)| h))
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.ribbons
            .values()
            .filter_map(|r| r.draw.as_ref())
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
                blend: Some(if ribbon.material.additive {
                    Blend::Additive
                } else {
                    Blend::Alpha
                }),
                depth_write: false,
                depth_equal: true,
                cull: resonance_content::CullFace::None,
                ..TitleSurface::textured(color)
            });
            ribbon.draw = Some(Draw::prepare(commands, &mut assets.meshes, surface));
        }
        let surface = assets.surfaces.add(TitleSurface {
            blend: Some(Blend::Alpha),
            // Enable ordinary weapon depth writes before drawing the ribbon.
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
        for (&(actor, slot), trail) in &self.history {
            if trail.samples.len() < 2 {
                continue;
            }
            let result = (|| -> Result<()> {
                let ribbon = &self.ribbons[&trail.resource];
                let size = ribbon
                    .texture
                    .as_ref()
                    .map_or([1, 1], |(_, image, _)| [image.width, image.height]);
                let style = models
                    .get(&trail.resource)
                    .context("missing trail weapon model")?
                    .weapon_style;
                ribbon
                    .draw
                    .as_ref()
                    .context("weapon trail was not prepared")?
                    .apply(
                        ribbon_mesh(&trail.samples, &ribbon.material, size)?,
                        DrawOrder(weapon_layer(order.get(actor)?, slot, style, true), 0, 0),
                        commands,
                        meshes,
                    )
            })();
            if let Err(error) = result {
                self.diagnostics
                    .report("battle weapon trail drawing", error)?;
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
                    DrawOrder(
                        weapon_layer(
                            order.get(weapon.owner)?,
                            weapon.slot,
                            models
                                .get(&weapon.resource)
                                .context("missing battle rope model")?
                                .weapon_style,
                            true,
                        ),
                        0,
                        0,
                    ),
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
                DrawOrder(Layer::Effects, 0, 0),
            ))
            .id();
        Self { entity, mesh }
    }
    fn apply(
        &self,
        mesh: Mesh,
        order: DrawOrder,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        *meshes
            .get_mut(&self.mesh)
            .context("prepared battle ribbon mesh is missing")? = mesh;
        commands
            .entity(self.entity)
            .insert((Visibility::Inherited, order));
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
        // Two triangles form one ribbon segment.
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
// A tenth of a second at 60 updates/second.
const FADE_UPDATES: u8 = 6;

struct Sample {
    points: Vec<Vec3>,
    age: u8,
}
struct Trail {
    resource: u32,
    remaining: u16,
    samples: VecDeque<Sample>,
}

impl Ribbon {
    fn sample(&self, weapon: &WeaponFrame) -> Result<Vec<Vec3>> {
        let world = Mat4::from_cols_array_2d(&weapon.world);
        self.bones
            .iter()
            .map(|&bone| {
                let pose = weapon
                    .bones
                    .get(usize::from(bone))
                    .context("invalid weapon trail bone")?;
                let point = (world * Mat4::from_cols_array_2d(pose)).transform_point3(Vec3::ZERO);
                ensure!(point.is_finite(), "invalid weapon trail pose");
                Ok(point)
            })
            .collect()
    }
}

impl Trails {
    /// Consume a completed frame once. Commands and gameplay pauses hold all trail feedback.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<()> {
        if frame.recognized_result.is_some() || frame.outcome.is_some() {
            self.history.clear();
            return Ok(());
        }
        let weapon = |actor, slot| {
            frame.weapons.iter().find(|weapon| {
                weapon.owner == actor
                    && weapon.slot == slot
                    && self.ribbons.contains_key(&weapon.resource)
            })
        };
        for cue in &frame.cues {
            if let Cue::WeaponTrail {
                actor,
                slot,
                duration,
            } = *cue
                && duration > 0
                && let Some(weapon) = weapon(actor, slot)
            {
                let trail = self.history.entry((actor, slot)).or_insert_with(|| Trail {
                    resource: weapon.resource,
                    remaining: 0,
                    samples: VecDeque::new(),
                });
                trail.remaining = trail.remaining.max(duration);
            }
        }
        for (&(actor, slot), trail) in &mut self.history {
            let Some(weapon) = weapon(actor, slot).filter(|weapon| weapon.visible) else {
                trail.remaining = 0;
                trail.samples.clear();
                continue;
            };
            if trail.resource != weapon.resource {
                trail.resource = weapon.resource;
                trail.samples.clear();
            }
            if paused {
                continue;
            }
            for sample in &mut trail.samples {
                sample.age += 1;
            }
            trail.samples.retain(|sample| sample.age < FADE_UPDATES);
            if trail.remaining > 0 {
                trail.remaining -= 1;
                match self.ribbons[&trail.resource].sample(weapon) {
                    Ok(points) => trail.samples.push_front(Sample { points, age: 0 }),
                    Err(error) => {
                        trail.remaining = 0;
                        trail.samples.clear();
                        self.diagnostics.report("battle weapon trail pose", error)?;
                    }
                }
            }
        }
        self.history
            .retain(|_, trail| trail.remaining > 0 || !trail.samples.is_empty());
        Ok(())
    }
}

fn ribbon_mesh(
    samples: &VecDeque<Sample>,
    material: &TrailMaterial,
    size: [u32; 2],
) -> Result<Mesh> {
    let endpoints = samples.front().map_or(0, |sample| sample.points.len());
    ensure!(
        (2..=3).contains(&endpoints)
            && samples.len() >= 2
            && samples
                .iter()
                .all(|sample| sample.points.len() == endpoints)
            && size.iter().all(|&n| n > 0),
        "invalid battle ribbon geometry"
    );
    let [u, v, width, height] = material.uv.map(f32::from);
    let du = width / (samples.len() - 1) as f32;
    let dv = height / (endpoints - 1) as f32;
    let rgb = material.color.map(|v| f32::from(v) / 128.);
    let mut data = Vertices::default();
    for row in 0..endpoints - 1 {
        let top = v + height - row as f32 * dv;
        let bottom = top - dv;
        for column in 0..samples.len() - 1 {
            let left = u + column as f32 * du;
            let right = left + du;
            let a = &samples[column];
            let b = &samples[column + 1];
            let color = |sample: &Sample| {
                [
                    rgb[0],
                    rgb[1],
                    rgb[2],
                    1. - f32::from(sample.age) / f32::from(FADE_UPDATES),
                ]
            };
            let uv = [[left, top], [right, top], [left, bottom], [right, bottom]]
                .map(|[u, v]| [u / size[0] as f32, v / size[1] as f32]);
            data.quad(
                [
                    a.points[row],
                    b.points[row],
                    a.points[row + 1],
                    b.points[row + 1],
                ],
                uv,
                [color(a), color(b), color(a), color(b)],
            );
        }
    }
    Ok(data.mesh())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use resonance_battle::{BattleResult, ModelMaterial, PreparedBattle, Side};
    use std::sync::Arc;

    #[test]
    fn weapon_ribbons_follow_equipment_hold_fade_and_omit_bad_art() -> Result<()> {
        let mut frame = PreparedBattle::new(
            vec![(
                crate::test_support::actor(Side::Party, 100, 20),
                Default::default(),
            )],
            Default::default(),
            1,
        )?
        .finish()?
        .snapshot();
        let actor = ActorId::from_index(0)?;
        let material = TrailMaterial {
            texture: None,
            palette: 0,
            additive: false,
            color: [128; 3],
            uv: [0, 0, 64, 64],
        };
        let mut trails = Trails {
            diagnostics: Diagnostics::new(true),
            ribbons: [(7, vec![1, 0]), (8, vec![2, 0, 1])]
                .into_iter()
                .map(|(id, bones)| {
                    (
                        id,
                        Ribbon {
                            bones,
                            material,
                            texture: None,
                            draw: None,
                        },
                    )
                })
                .collect(),
            history: BTreeMap::new(),
            ropes: Vec::new(),
            ready: false,
        };
        frame.weapons.push(WeaponFrame {
            owner: actor,
            slot: 0,
            visible: true,
            tint: [255; 4],
            material: ModelMaterial::Normal,
            resource: 7,
            clip: None,
            frame: 0.,
            world: Mat4::from_translation(Vec3::new(10., 20., 30.)).to_cols_array_2d(),
            bones: Arc::new(vec![
                Mat4::IDENTITY.to_cols_array_2d(),
                Mat4::from_translation(Vec3::new(2., 0., 0.)).to_cols_array_2d(),
                Mat4::from_translation(Vec3::new(0., 2., 0.)).to_cols_array_2d(),
            ]),
            links: vec![],
        });
        let request = Cue::WeaponTrail {
            actor,
            slot: 0,
            duration: 20,
        };
        frame.cues = vec![request.clone()];
        trails.advance(&frame, false)?;
        frame.cues.clear();
        frame.weapons[0].world[3][0] += 2.;
        trails.advance(&frame, false)?;
        let mesh = |trails: &Trails| {
            ribbon_mesh(&trails.history[&(actor, 0)].samples, &material, [256, 256])
        };
        let before = mesh(&trails)?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            before.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("missing ribbon positions");
        };
        assert!(positions.contains(&[12., 20., 30.]));
        assert!(positions.contains(&[14., 20., 30.]));
        let Some(VertexAttributeValues::Float32x2(uvs)) = before.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("missing ribbon texture coordinates");
        };
        assert!(uvs.contains(&[0., 0.]) && uvs.contains(&[0.25, 0.25]));
        trails.advance(&frame, true)?;
        let held = mesh(&trails)?;
        for attribute in [Mesh::ATTRIBUTE_POSITION, Mesh::ATTRIBUTE_COLOR] {
            assert_eq!(before.attribute(attribute), held.attribute(attribute));
        }
        // Switching a held weapon must never connect its new pose to the old blade.
        frame.weapons[0].resource = 8;
        frame.weapons[0].world[3][0] = 300.;
        trails.advance(&frame, true)?;
        trails.advance(&frame, false)?;
        frame.weapons[0].world[3][0] += 2.;
        trails.advance(&frame, false)?;
        let replaced = mesh(&trails)?;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            replaced.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("missing replacement ribbon");
        };
        assert!(positions.iter().all(|p| p[0] >= 300.));
        for _ in 0..30 {
            trails.advance(&frame, false)?;
        }
        assert!(trails.history.is_empty());
        // Malformed optional artwork is isolated from simulation in tolerant mode.
        trails.ribbons.get_mut(&8).unwrap().bones[0] = u16::MAX;
        frame.cues = vec![request.clone()];
        for paranoid in [false, true] {
            trails.diagnostics = Diagnostics::new(paranoid);
            assert_eq!(trails.advance(&frame, false).is_err(), paranoid);
            assert!(trails.diagnostics.has_errors());
            assert!(
                trails
                    .history
                    .values()
                    .all(|trail| trail.samples.is_empty())
            );
        }
        trails.ribbons.get_mut(&8).unwrap().bones[0] = 2;
        trails.advance(&frame, false)?;
        frame.weapons[0].visible = false;
        trails.advance(&frame, true)?;
        assert!(trails.history.is_empty());
        frame.weapons[0].visible = true;
        trails.advance(&frame, false)?;
        frame.recognized_result = Some(BattleResult::Victory);
        trails.advance(&frame, true)?;
        assert!(trails.history.is_empty());
        Ok(())
    }
}

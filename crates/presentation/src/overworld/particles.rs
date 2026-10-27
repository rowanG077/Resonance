//! Small travel effects share five draw batches; no entity per particle.
use super::*;
use bevy::{
    asset::RenderAssetUsages, camera::visibility::NoFrustumCulling, mesh::PrimitiveTopology,
};

#[derive(Clone, Copy)]
enum Kind {
    Grass,
    Footprint(Quat),
    Dust(usize),
    Snow,
    Holy,
    Dark,
}
#[derive(Clone)]
struct Particle {
    id: u64,
    kind: Kind,
    position: Position,
    velocity: Vec3,
    rotation: f32,
    life: u16,
}
#[derive(Clone, Default)]
pub(super) struct Particles {
    next_id: u64,
    live: Vec<Particle>,
    stride: u32,
    random: u32,
}
impl Particles {
    fn random(&mut self, range: u32) -> u32 {
        self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.random >> 8) % range
    }
    fn spawn(&mut self, kind: Kind, position: Position, velocity: Vec3, rotation: f32) {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("world particle id exhausted");
        self.live.push(Particle {
            id: self.next_id,
            kind,
            position,
            velocity,
            rotation,
            life: if matches!(kind, Kind::Snow) { 100 } else { 60 },
        });
    }
    pub(super) fn step(
        &mut self,
        session: &game::Session,
        models: &BTreeMap<Model, Vec<ScenePart>>,
    ) -> Result<()> {
        for p in &mut self.live {
            p.life -= 1;
            p.position = p.position.translated(p.velocity.to_array())?;
            match p.kind {
                Kind::Grass => p.velocity.z -= 0.196,
                Kind::Dust(_) => p.velocity.z -= 0.049,
                Kind::Holy => p.rotation += 0.02,
                _ => {}
            }
        }
        self.live.retain(|p| p.life > 0);
        if session.cinematic.is_some() {
            self.live.clear();
            return Ok(());
        }
        let state = session.travel.state();
        if matches!(
            session.travel.displayed_mount(),
            Mount::Foot | Mount::Noishe
        ) {
            let party = session.events.world.party.as_ref().unwrap();
            if session.travel.speed().abs() > 0.01 && session.player_has_control() {
                let actor = if state.mount == Mount::Noishe {
                    202
                } else {
                    u16::from(party.field_leader)
                };
                let slot = if session.travel.speed().abs() > 8. {
                    2
                } else {
                    1
                };
                let duration = models[&Model::Actor(actor)]
                    .iter()
                    .flat_map(|p| &p.clips)
                    .find(|c| c.resource_slot == slot)
                    .context("world stride animation missing")?
                    .duration_seconds;
                let half_stride = (duration * resonance_content::ANIMATION_HZ / 2.)
                    .round()
                    .max(1.) as u32;
                if self.stride.is_multiple_of(half_stride) {
                    let terrain = session.travel.response();
                    if matches!(terrain, 4 | 5 | 10) {
                        let side = if (self.stride / half_stride).is_multiple_of(2) {
                            20.
                        } else {
                            -20.
                        };
                        let position = state.position.translated([
                            side * state.heading.cos(),
                            side * state.heading.sin(),
                            1.,
                        ])?;
                        if let Some(surface) = session.ground_surface(position)? {
                            let ground = Position::from_map([
                                position.map()[0],
                                position.map()[1],
                                surface.height + 1.,
                            ])?;
                            let rotation =
                                Quat::from_rotation_arc(Vec3::Z, Vec3::from_array(surface.normal))
                                    * Quat::from_rotation_z(state.heading);
                            self.spawn(Kind::Footprint(rotation), ground, Vec3::ZERO, 0.);
                        }
                    }
                    if matches!(terrain, 5..=7 | 10) {
                        let count = 3 + self.random(4);
                        for _ in 0..count {
                            let angle = self.random(360) as f32 * std::f32::consts::TAU / 360.;
                            let grass = matches!(terrain, 6 | 7);
                            let kind = if grass {
                                Kind::Grass
                            } else {
                                Kind::Dust(self.random(4) as usize)
                            };
                            let speed = if grass { 2.5 } else { 1.5 };
                            let rise = if grass {
                                6. + self.random(12) as f32 / 4.
                            } else {
                                1.5
                            };
                            self.spawn(
                                kind,
                                state.position,
                                Vec3::new(speed * angle.cos(), speed * angle.sin(), rise),
                                0.,
                            );
                        }
                    }
                }
                self.stride += 1;
            } else {
                self.stride = 0;
            }
            if let Some(modifier) = &party.encounter_modifier {
                let holy = modifier.rate == 1;
                let spread = if holy { 120 } else { 100 };
                let position = state.position.translated([
                    self.random(spread) as f32 - spread as f32 / 2.,
                    self.random(spread) as f32 - spread as f32 / 2.,
                    10.,
                ])?;
                self.spawn(
                    if holy { Kind::Holy } else { Kind::Dark },
                    position,
                    Vec3::Z * 4.,
                    0.,
                );
            }
        } else {
            self.stride = 0;
        }
        // Snowfall over the northern Tethe'alla mountain range.
        let snow_center = Position::from_map([66333., 14745., 1000.])?;
        if state.world == game::World::TetheAlla && state.position.distance_to(snow_center) < 18000.
        {
            for _ in 0..4 {
                let position = snow_center.translated([
                    self.random(10000) as f32 - 5000.,
                    self.random(10000) as f32 - 5000.,
                    0.,
                ])?;
                let angle = self.random(360) as f32 * std::f32::consts::TAU / 360.;
                self.spawn(
                    Kind::Snow,
                    position,
                    Vec3::new(1.5 * angle.cos(), 1.5 * angle.sin(), -10.),
                    0.,
                );
            }
        }
        Ok(())
    }
}

struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<TitleSurface>,
}
pub(super) struct Artwork {
    images: Vec<Handle<Image>>,
    draws: [Option<Draw>; 5],
}
impl Artwork {
    pub(super) fn load(
        paths: &[String; 4],
        server: &AssetServer,
        loads: &super::super::loading::LoadTasks,
    ) -> Self {
        Self {
            images: paths
                .iter()
                .map(|p| {
                    server
                        .load_builder()
                        .with_guard(loads.ticket())
                        .with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.is_srgb = false)
                        .load(p.clone())
                })
                .collect(),
            draws: std::array::from_fn(|_| None),
        }
    }
    pub(super) fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|h| images.contains(h))
    }
    pub(super) fn despawn(self, world: &mut World) {
        for draw in self.draws.into_iter().flatten() {
            world.despawn(draw.entity);
        }
    }
}

#[derive(Clone, Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
}
impl Batch {
    fn fan(
        &mut self,
        center: Vec3,
        rotation: Quat,
        size: Vec2,
        color: [f32; 4],
        edge_alpha: f32,
        star: bool,
    ) {
        for i in 0..8 {
            for vertex in [None, Some(i), Some((i + 1) % 8)] {
                let mut color = color;
                let point = vertex.map_or(Vec2::ZERO, |v| {
                    let angle = v as f32 * std::f32::consts::FRAC_PI_4;
                    color[3] = edge_alpha;
                    Vec2::new(angle.cos(), angle.sin()) * if star && v % 2 == 1 { 0.3 } else { 1. }
                });
                self.positions
                    .push((center + rotation * (point * size).extend(0.)).to_array());
                self.colors.push(color);
                self.uv.push([0.; 2]);
            }
        }
    }
    fn square(&mut self, center: Vec3, rotation: Quat, size: f32, color: [f32; 4]) {
        let corners = [
            Vec2::new(-0.5, 0.5),
            Vec2::new(0.5, 0.5),
            Vec2::new(0.5, -0.5),
            Vec2::new(-0.5, -0.5),
        ];
        for i in [0, 2, 1, 0, 3, 2] {
            let p = corners[i];
            self.positions
                .push((center + rotation * (p * size).extend(0.)).to_array());
            self.colors.push(color);
            self.uv.push([p.x + 0.5, 0.5 - p.y]);
        }
    }
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
    }
}

pub(super) fn render(
    mut commands: Commands,
    owner: State,
    prediction: Res<crate::field_view::Prediction>,
    mut art: Option<ResMut<Art>>,
    camera: Query<&Transform, With<super::super::FieldCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TitleSurface>>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    let Some(mut art) = art.take().filter(|a| a.ready) else {
        return;
    };
    let Ok(camera) = camera.single() else {
        return;
    };
    let mut batches: [Batch; 5] = Default::default();
    let origin = scene.session.travel.state().position;
    let future = prediction
        .world()
        .filter(|next| interpolation::continuous(scene, next));
    let next_origin = future.map_or(origin, |next| next.session.travel.state().position);
    let alpha = future.map_or(0., |_| prediction.alpha);
    let next = future.map_or(&scene.effects.particles.live, |next| {
        &next.effects.particles.live
    });
    for (from, to) in particle_pairs(&scene.effects.particles.live, next) {
        let p = from.or(to).unwrap();
        if matches!(p.kind, Kind::Snow) && origin.distance_to(p.position) >= 10000. {
            continue;
        }
        let center_at = |origin: Position, p: &Particle| {
            let delta = origin.displacement_to(p.position);
            Vec3::new(delta[0], delta[1], p.position.map()[2])
        };
        let center = center_at(origin, from.unwrap_or(p))
            .lerp(center_at(next_origin, to.unwrap_or(p)), alpha);
        let opacity = match (from.is_some(), to.is_some()) {
            (true, true) => 1.,
            (true, false) => 1. - alpha,
            _ => alpha,
        };
        let life = f32::from(from.unwrap_or(p).life)
            + (f32::from(to.unwrap_or(p).life) - f32::from(from.unwrap_or(p).life)) * alpha;
        let fade = (life / 10.).min(1.) * opacity;
        let rotation = Quat::from_rotation_z(from.unwrap_or(p).rotation)
            .slerp(Quat::from_rotation_z(to.unwrap_or(p).rotation), alpha);
        let batch = &mut batches[0];
        match p.kind {
            Kind::Footprint(rotation) => batch.fan(
                center,
                rotation,
                Vec2::new(10., 25.),
                [0., 0., 0., fade / 4.],
                fade / 4.,
                false,
            ),
            Kind::Grass => batch.fan(
                center,
                Quat::from_euler(
                    EulerRot::XYZ,
                    life * 3f32.to_radians(),
                    life * 4f32.to_radians(),
                    0.,
                ),
                Vec2::new(4., 10.),
                [16. / 255., 80. / 255., 16. / 255., fade],
                fade,
                false,
            ),
            Kind::Dust(texture) => batches[texture + 1].square(
                center,
                camera.rotation,
                100. - life,
                [
                    189. / 255.,
                    182. / 255.,
                    113. / 255.,
                    life / 60. * 128. / 255. * opacity,
                ],
            ),
            Kind::Snow => batch.fan(
                center,
                camera.rotation,
                Vec2::splat(10.),
                [224. / 255., 224. / 255., 224. / 255., fade],
                fade,
                false,
            ),
            Kind::Holy => batch.fan(
                center,
                camera.rotation * rotation,
                Vec2::splat(15.),
                [
                    240. / 255.,
                    240. / 255.,
                    240. / 255.,
                    (life / 30.).min(1.) * opacity,
                ],
                0.,
                true,
            ),
            Kind::Dark => batch.fan(
                center,
                camera.rotation,
                Vec2::splat(15.),
                [0., 0., 0., (life / 30.).min(1.) * opacity],
                0.,
                false,
            ),
        }
    }
    let far = scene.session.travel.camera_distance() + 10000.;
    for (i, batch) in batches.into_iter().enumerate() {
        if batch.positions.is_empty() {
            if let Some(draw) = &art.particles.draws[i] {
                commands.entity(draw.entity).insert(Visibility::Hidden);
            }
            continue;
        }
        if art.particles.draws[i].is_none() {
            let color = i.checked_sub(1).map(|i| art.particles.images[i].clone());
            let material = materials.add(TitleSurface {
                color: color.clone(),
                sampling: color,
                blend: Some(Blend::Alpha),
                field_fog: false,
                fog_color: super::FOG_COLOR,
                depth_test: true,
                depth_write: false,
                cull: resonance_content::CullFace::None,
                ..default()
            });
            let mesh = meshes.add(batch.mesh());
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    super::super::draw_order::DrawOrder(
                        super::super::draw_order::Layer::Scene(4 << 20),
                        0,
                        0,
                    ),
                    NoFrustumCulling,
                ))
                .id();
            art.particles.draws[i] = Some(Draw {
                entity,
                mesh,
                material,
            });
        } else {
            let draw = art.particles.draws[i].as_ref().unwrap();
            commands.entity(draw.entity).insert(Visibility::Inherited);
            *meshes.get_mut(&draw.mesh).unwrap() = batch.mesh();
        }
        materials
            .get_mut(&art.particles.draws[i].as_ref().unwrap().material)
            .unwrap()
            .fog_range = Vec4::new(far * 0.5, far * 0.75, 0., 0.);
    }
}

// Births and deaths retain their identity; deleting a particle never shifts another endpoint.
fn particle_pairs<'a>(
    from: &'a [Particle],
    to: &'a [Particle],
) -> impl Iterator<Item = (Option<&'a Particle>, Option<&'a Particle>)> {
    let mut from = from.iter().peekable();
    let mut to = to.iter().peekable();
    std::iter::from_fn(move || match (from.peek(), to.peek()) {
        (Some(a), Some(b)) if a.id == b.id => Some((from.next(), to.next())),
        (Some(a), Some(b)) if a.id < b.id => Some((from.next(), None)),
        (Some(_), Some(_)) => Some((None, to.next())),
        (Some(_), None) => Some((from.next(), None)),
        (None, Some(_)) => Some((None, to.next())),
        (None, None) => None,
    })
}

#[cfg(test)]
mod interpolation_tests {
    use super::*;
    #[test]
    fn particle_pairs_keep_identity_when_the_middle_particle_survives() -> Result<()> {
        let mut from = Particles::default();
        let origin = Position::from_map([0.; 3])?;
        for _ in 0..3 {
            from.spawn(Kind::Grass, origin, Vec3::ZERO, 0.);
        }
        let mut next = from.clone();
        next.live.remove(0);
        next.live.remove(1);
        next.spawn(Kind::Grass, origin, Vec3::ZERO, 0.);
        let pairs: Vec<_> = particle_pairs(&from.live, &next.live)
            .map(|(a, b)| (a.map(|p| p.id), b.map(|p| p.id)))
            .collect();
        assert_eq!(
            pairs,
            [
                (Some(1), None),
                (Some(2), Some(2)),
                (Some(3), None),
                (None, Some(4))
            ]
        );
        assert_eq!(from.live.len(), 3);
        Ok(())
    }
}

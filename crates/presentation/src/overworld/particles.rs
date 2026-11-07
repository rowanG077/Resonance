//! Small travel effects share five draw batches; no entity per particle.
use super::*;
use bevy::{
    asset::RenderAssetUsages, camera::visibility::NoFrustumCulling, mesh::PrimitiveTopology,
};
use resonance_content::overworld::TerrainSurface;

// Terrain-specific sounds shared by walking and running.
fn footstep_sound(surface: TerrainSurface) -> Option<i16> {
    use TerrainSurface::*;
    match surface {
        Grassland => Some(12),
        Road => Some(11),
        Wasteland => Some(18),
        Snowfield => Some(19),
        Desert | Beach => Some(17),
        Forest | DeepForest => Some(27),
        Bridge | Pier => Some(14),
        Barrier | Water => None,
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Grass,
    Footprint(Quat),
    Dust(usize),
    Snow,
    Holy,
    Dark,
}
struct Particle {
    kind: Kind,
    position: Position,
    velocity: Vec3,
    rotation: f32,
    life: u16,
}
struct Gait {
    actor: u16,
    slot: u16,
    start: u32,
    phase: Option<bool>,
}
#[derive(Default)]
pub(super) struct Particles {
    live: Vec<Particle>,
    gait: Option<Gait>,
    random: u32,
}
impl Particles {
    /// Rendering samples the same gait clock as sounds and footprints.
    pub(super) fn gait_start(&self, actor: u16, slot: u16) -> Option<u32> {
        self.gait
            .as_ref()
            .filter(|gait| gait.actor == actor && gait.slot == slot)
            .map(|gait| gait.start)
    }

    fn random(&mut self, range: u32) -> u32 {
        self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.random >> 8) % range
    }
    fn spawn(&mut self, kind: Kind, position: Position, velocity: Vec3, rotation: f32) {
        self.live.push(Particle {
            kind,
            position,
            velocity,
            rotation,
            life: if matches!(kind, Kind::Snow) { 100 } else { 60 },
        });
    }
    pub(super) fn step(
        &mut self,
        session: &mut game::Session,
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
            self.gait = None;
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
                let slot = locomotion_slot(
                    session.travel.displayed_mount(),
                    session.travel.speed(),
                    session.travel.speed_fraction(),
                );
                let clip = models[&Model::Actor(actor)]
                    .iter()
                    .flat_map(|p| &p.clips)
                    .find(|c| c.resource_slot == slot)
                    .context("world stride animation missing")?;
                let tick = session.events.tick();
                if self
                    .gait
                    .as_ref()
                    .is_none_or(|gait| gait.actor != actor || gait.slot != slot)
                {
                    self.gait = Some(Gait {
                        actor,
                        slot,
                        start: tick,
                        phase: None,
                    });
                }
                let gait = self.gait.as_mut().unwrap();
                let clip = resonance_events::AnimationClip::from(clip);
                let phase = clip.sample_tick(tick - gait.start, true) >= clip.duration_ticks / 2;
                let contact = gait.phase != Some(phase);
                gait.phase = Some(phase);
                if contact {
                    let terrain = session.travel.surface();
                    if let Some(id) = terrain.and_then(footstep_sound) {
                        session.events.world.audio_commands.push(
                            resonance_events::AudioCommand::Sound {
                                id,
                                volume: 127,
                                pan: 64,
                                slot: None,
                            },
                        );
                    }
                    if matches!(
                        terrain,
                        Some(
                            TerrainSurface::Snowfield
                                | TerrainSurface::Desert
                                | TerrainSurface::Beach
                        )
                    ) {
                        let side = if phase { -20. } else { 20. };
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
                    if matches!(
                        terrain,
                        Some(
                            TerrainSurface::Desert
                                | TerrainSurface::Forest
                                | TerrainSurface::DeepForest
                                | TerrainSurface::Beach
                        )
                    ) {
                        let count = 3 + self.random(4);
                        for _ in 0..count {
                            let angle = self.random(360) as f32 * std::f32::consts::TAU / 360.;
                            let grass = matches!(
                                terrain,
                                Some(TerrainSurface::Forest | TerrainSurface::DeepForest)
                            );
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
            } else {
                self.gait = None;
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
            self.gait = None;
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

#[derive(Default)]
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
    for p in &scene.effects.particles.live {
        if matches!(p.kind, Kind::Snow) && origin.distance_to(p.position) >= 10000. {
            continue;
        }
        let delta = origin.displacement_to(p.position);
        let center = Vec3::new(delta[0], delta[1], p.position.map()[2]);
        let fade = (f32::from(p.life) / 10.).min(1.);
        let rotation = Quat::from_rotation_z(p.rotation);
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
                    f32::from(p.life) * 3f32.to_radians(),
                    f32::from(p.life) * 4f32.to_radians(),
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
                100. - f32::from(p.life),
                [
                    189. / 255.,
                    182. / 255.,
                    113. / 255.,
                    f32::from(p.life) / 60. * 128. / 255.,
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
                    (f32::from(p.life) / 30.).min(1.),
                ],
                0.,
                true,
            ),
            Kind::Dark => batch.fan(
                center,
                camera.rotation,
                Vec2::splat(15.),
                [0., 0., 0., (f32::from(p.life) / 30.).min(1.)],
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

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; runs world movement without output devices"]
fn world_movement_emits_footsteps_and_stops_when_idle() -> Result<()> {
    use resonance_content::overworld::{TravelState, World as WorldKind};
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
    );
    let package = Package::prepare(
        &root,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let mut persistent = resonance_events::PersistentState {
        party: Some(resonance_events::party::Party::new(
            package.world.resources.session_data.as_ref().unwrap(),
            Default::default(),
        )?),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
    let state = TravelState {
        world: WorldKind::Sylvarant,
        position: Position::from_map([9770., 24160., 0.])?,
        heading: 0.,
        camera_yaw: 0.,
        alternate_perspective: false,
        map_display: Default::default(),
        mount: Mount::Foot,
        altitude: 0.,
    };
    let session = game::Session::enter(
        package.world.assets(state.world, &persistent)?,
        state,
        persistent,
        Default::default(),
    )?;
    let mut scene = Scene::new(session, package.world)?;
    let initial = scene.session.travel.state().clone();
    for mount in [Mount::Foot, Mount::Noishe] {
        scene.session.travel = game::travel::Travel::new(
            TravelState {
                mount,
                ..initial.clone()
            },
            Arc::new(scene.package.definition.movement.clone()),
        )?;
        let actor = if mount == Mount::Noishe {
            202
        } else {
            scene.leader()
        };
        scene.session.events.world.audio_commands.clear();
        for (pressure, slot) in [(0.5, 1), (0.5001, 2)] {
            scene.step(game::Input::default())?;
            scene.session.events.world.audio_commands.clear();
            let duration = scene.models[&Model::Actor(actor)]
                .iter()
                .flat_map(|part| &part.clips)
                .find(|clip| clip.resource_slot == slot)
                .unwrap()
                .duration_ticks();
            let expected = [0, duration / 2, duration + 1];
            let mut contacts = Vec::new();
            for elapsed in 0..=*expected.last().unwrap() {
                scene.step(game::Input {
                    travel: game::travel::Input {
                        stick: [pressure, 0.],
                        ..Default::default()
                    },
                    ..Default::default()
                })?;
                if elapsed == 0 {
                    assert_eq!(
                        scene.effects.particles.gait_start(actor, slot),
                        Some(scene.session.events.tick())
                    );
                }
                for command in std::mem::take(&mut scene.session.events.world.audio_commands) {
                    if let resonance_events::AudioCommand::Sound { id, slot: None, .. } = command
                        && resonance_content::field_audio::FOOTSTEP_EFFECTS.contains(&(id as u32))
                    {
                        contacts.push(elapsed);
                    }
                }
            }
            assert_eq!(contacts, expected, "mount={mount:?}, pressure={pressure}");
        }
        // Changing pressure within either gait must preserve the shared clock.
        for pressures in [[0.25, 0.5, 0.1], [0.625, 1., 0.75]] {
            scene.step(game::Input::default())?;
            let start = scene.session.events.tick() + 1;
            for pressure in pressures {
                scene.step(game::Input {
                    travel: game::travel::Input {
                        stick: [pressure, 0.],
                        ..Default::default()
                    },
                    ..Default::default()
                })?;
                let slot = if pressure > 0.5 { 2 } else { 1 };
                assert_eq!(scene.effects.particles.gait_start(actor, slot), Some(start));
            }
        }
        scene.session.events.world.audio_commands.clear();
        for _ in 0..60 {
            scene.step(game::Input::default())?;
            assert_eq!(scene.effects.particles.gait_start(actor, 2), None);
            assert!(!std::mem::take(&mut scene.session.events.world.audio_commands).iter().any(|command|
                matches!(command, resonance_events::AudioCommand::Sound { id, slot: None, .. }
                    if resonance_content::field_audio::FOOTSTEP_EFFECTS.contains(&(*id as u32)))));
        }
    }
    Ok(())
}

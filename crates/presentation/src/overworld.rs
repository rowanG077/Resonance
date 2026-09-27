//! Desktop ownership of the world scene. Travel remains in resonance-game.
use super::{
    field_view::Part,
    materials::{TitleOutput, TitleSurface},
    sparse_animation,
};
mod capture;
mod cinematic;
mod effects;
use anyhow::{Context, Result};
use bevy::{prelude::*, world_serialization::WorldInstanceReady};
pub use capture::{Probe, capture_overworld};
use resonance_content::{
    ScenePart,
    overworld::{Marker, Mount, TILE_SIZE},
};
use resonance_game::overworld::{self as game, Position};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};

#[derive(bevy::ecs::system::SystemParam)]
struct State<'w> {
    live: Option<Res<'w, super::new_game::Session>>,
    capture: Option<Res<'w, capture::Scene>>,
}
impl State<'_> {
    fn get(&self) -> Option<&Scene> {
        self.live
            .as_ref()
            .and_then(|s| s.overworld.as_ref())
            .or_else(|| self.capture.as_ref().map(|s| &s.0))
    }
}

pub(super) struct Package {
    pub world: Arc<game::Prepared>,
    pub audio: Arc<super::field_audio::Assets>,
}

pub(super) struct Scene {
    pub session: game::Session,
    pub package: Arc<game::Prepared>,
    models: BTreeMap<Model, Vec<ScenePart>>,
    landmarks: BTreeMap<u16, Position>,
    animation: Option<MountAnimation>,
    effects: effects::Effects,
}
struct MountAnimation {
    token: u64,
    slot: u16,
    start: u32,
    duration: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Model {
    Sky,
    Tile(usize),
    Actor(u16),
    Marker(u8),
    Cinematic(u8),
}
impl Scene {
    pub fn new(session: game::Session, package: Arc<game::Prepared>) -> Result<Self> {
        let persistent = session.events.persistent_state()?;
        let world = session.travel.state().world;
        let mut models: BTreeMap<_, _> = package
            .terrain(world, &persistent)?
            .into_iter()
            .map(|(tile, resources)| (Model::Tile(tile.index()), resources.parts.clone()))
            .collect();
        models.extend(
            package
                .definition
                .visuals
                .actors
                .iter()
                .map(|(&id, parts)| (Model::Actor(id), parts.clone())),
        );
        models.extend(
            package.definition.visuals.markers[world.index()]
                .iter()
                .map(|(&id, parts)| (Model::Marker(id), parts.clone())),
        );
        if let Some(playback) = &session.cinematic {
            models.extend(
                playback
                    .definition
                    .actors
                    .iter()
                    .map(|(&id, parts)| (Model::Cinematic(id), parts.clone())),
            );
        }
        let story = persistent.memory.read(0x40, symphonia_script::Width::S32)?;
        models.insert(
            Model::Sky,
            package.definition.visuals.skies[usize::from(story >= 22_605_000)].clone(),
        );
        let terrain = package.assets(world, &persistent)?.terrain.clone();
        let mut landmarks = BTreeMap::new();
        for landmark in &package.definition.landmarks.worlds[world.index()] {
            let mut position =
                Position::from_map([landmark.position[0], landmark.position[1], 0.])?;
            let height = match landmark.height {
                Some(height) => height,
                None => terrain
                    .query(position, package.definition.movement.collision_radius)?
                    .surface(game::collision::Mode::Ground)
                    .map_or(0., |surface| surface.height),
            };
            position = Position::from_map([landmark.position[0], landmark.position[1], height])?;
            landmarks.insert(landmark.id, position);
        }
        Ok(Self {
            session,
            package,
            models,
            landmarks,
            animation: None,
            effects: Default::default(),
        })
    }
    fn leader(&self) -> u16 {
        u16::from(
            self.session
                .events
                .world
                .party
                .as_ref()
                .unwrap()
                .field_leader,
        )
    }
    fn step(&mut self, input: game::Input) -> Result<()> {
        let previous_tick = self.session.events.tick();
        if let Some(animation) = &self.animation
            && self.session.events.tick().saturating_sub(animation.start) >= animation.duration
        {
            self.session.travel.finish_animation(animation.token);
            self.animation = None;
        }
        for cue in self.session.step(input)? {
            if let game::travel::Cue::Animation { token, mounting } = cue {
                let slot = if mounting { 4 } else { 5 };
                let clip = self.models[&Model::Actor(self.leader())]
                    .iter()
                    .flat_map(|p| &p.clips)
                    .find(|c| c.resource_slot == slot)
                    .context("party mount animation missing")?;
                self.animation = Some(MountAnimation {
                    token,
                    slot,
                    start: self.session.events.tick(),
                    duration: (f64::from(clip.duration_seconds) * resonance_game::clock::UPDATE_HZ)
                        .ceil() as u32,
                });
            }
        }
        if previous_tick != self.session.events.tick() {
            self.effects.step(&self.session, &self.models)?;
        }
        Ok(())
    }
}
pub(super) fn destination(
    location: u16,
    progress: &resonance_events::PersistentState,
) -> Result<game::World> {
    match location {
        0 => progress
            .party
            .as_ref()
            .and_then(|p| p.travel.overworld.as_ref())
            .map(|s| s.world)
            .context("world resume has no return position"),
        1..=98 | 513..=516 | 522..=525 => Ok(game::World::Sylvarant),
        257..=337 | 517..=521 | 526 => Ok(game::World::TetheAlla),
        _ => anyhow::bail!("world cinematic {location} requires its scripted scene"),
    }
}

pub(super) struct OverworldPlugin;
impl Plugin for OverworldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Controls>()
            .add_systems(PreUpdate, controls.after(bevy::input::InputSystems))
            .add_systems(
                FixedUpdate,
                advance
                    .run_if(super::dungeons::running)
                    .before(super::new_game::advance)
                    .before(super::field_view::advance_live),
            )
            .add_systems(
                Update,
                (
                    retire,
                    load,
                    prepare,
                    instances,
                    bind,
                    pose,
                    terrain_order,
                    camera,
                    ui,
                )
                    .chain()
                    .after(super::new_game::transition)
                    .after(super::field_view::FieldPreparation),
            )
            .add_systems(
                PostUpdate,
                animate.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}
#[derive(Resource, Default)]
pub(super) struct Controls(game::Input);
pub(super) fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut controls: ResMut<Controls>,
    dungeons: Option<Res<super::dungeons::Menu>>,
) {
    if dungeons.is_some_and(|menu| menu.blocked()) {
        *controls = Controls::default();
        return;
    }
    use GamepadButton::*;
    let held = |keys_: &[KeyCode], button| {
        keys_.iter().any(|k| keys.pressed(*k)) || pads.iter().any(|p| p.pressed(button))
    };
    let edge = |keys_: &[KeyCode], button| {
        keys_.iter().any(|k| keys.just_pressed(*k)) || pads.iter().any(|p| p.just_pressed(button))
    };
    let axis = |positive: &[KeyCode], negative: &[KeyCode]| {
        f32::from(positive.iter().any(|k| keys.pressed(*k)))
            - f32::from(negative.iter().any(|k| keys.pressed(*k)))
    };
    let mut direction = Vec2::new(
        axis(
            &[KeyCode::KeyD, KeyCode::ArrowRight],
            &[KeyCode::KeyA, KeyCode::ArrowLeft],
        ),
        axis(
            &[KeyCode::KeyW, KeyCode::ArrowUp],
            &[KeyCode::KeyS, KeyCode::ArrowDown],
        ),
    );
    let mut secondary = Vec2::ZERO;
    for pad in &pads {
        if pad.left_stick().length() > 0.2 {
            direction += pad.left_stick();
        }
        if pad.right_stick().length() > 0.2 {
            secondary += pad.right_stick();
        }
        direction += Vec2::new(
            f32::from(pad.pressed(DPadRight)) - f32::from(pad.pressed(DPadLeft)),
            f32::from(pad.pressed(DPadUp)) - f32::from(pad.pressed(DPadDown)),
        );
    }
    let input = &mut controls.0;
    input.travel.stick = direction.clamp_length_max(1.).to_array();
    input.travel.secondary = secondary.clamp_length_max(1.).to_array();
    input.travel.throttle = held(&[KeyCode::Space, KeyCode::Enter], South);
    input.travel.toggle_noishe |= edge(&[KeyCode::KeyX], West);
    input.travel.vehicle |= edge(&[KeyCode::KeyB, KeyCode::Escape], East);
    input.travel.toggle_perspective |= edge(&[KeyCode::KeyC], RightThumb);
    input.travel.cycle_map |= edge(&[KeyCode::KeyM], Start);
    input.travel.rotate_camera = axis(&[KeyCode::KeyE], &[KeyCode::KeyQ])
        + f32::from(pads.iter().any(|p| p.pressed(RightTrigger)))
        - f32::from(pads.iter().any(|p| p.pressed(LeftTrigger)));
    input.travel.rotate_camera = input.travel.rotate_camera.clamp(-1., 1.);
    input.confirm |= edge(&[KeyCode::Space, KeyCode::Enter], South);
    input.cancel |= edge(&[KeyCode::Escape, KeyCode::KeyB], East);
    input.skit |= edge(&[KeyCode::KeyZ], Z);
    input.skip_skit |= edge(&[KeyCode::Home], Start);
    input.accelerate_dialogue = input.travel.throttle;
}
fn advance(
    mut owner: Option<ResMut<super::new_game::Session>>,
    art: Option<Res<Art>>,
    resident: Res<super::loading::Resident>,
    loading_save: Option<Res<super::saves::WorldLoad>>,
    mut controls: ResMut<Controls>,
    mut menu_controls: ResMut<super::field_view::Controls>,
    mut exit: MessageWriter<AppExit>,
) {
    let mut input = controls.0;
    if owner.as_ref().is_some_and(|s| s.overworld.is_some()) {
        input.menu = menu_controls.consume();
    }
    controls.0.confirm = false;
    controls.0.cancel = false;
    controls.0.skit = false;
    controls.0.skip_skit = false;
    controls.0.travel.toggle_noishe = false;
    controls.0.travel.vehicle = false;
    controls.0.travel.toggle_perspective = false;
    controls.0.travel.cycle_map = false;
    if loading_save.is_some() || owner.as_ref().is_some_and(|s| s.audio.is_some()) {
        return;
    }
    let Some(scene) = owner.as_mut().and_then(|s| s.overworld.as_mut()) else {
        return;
    };
    if art.is_none_or(|a| !a.ready) || !resident.active.load(Ordering::Acquire) {
        return;
    }
    if let Err(error) = scene.step(input) {
        error!("Overworld update failed: {error:#}");
        exit.write(AppExit::error());
    }
}

#[derive(Resource)]
struct Art {
    package: Arc<game::Prepared>,
    world: game::World,
    cinematic: Option<u16>,
    models: BTreeMap<Model, Vec<Part>>,
    loads: super::loading::LoadTasks,
    ready: bool,
    spawned: bool,
    landmarks: std::collections::BTreeSet<(u16, Model)>,
    since: std::time::Instant,
    ui: Option<super::field_ui::WorldArtwork>,
}
#[derive(Component)]
struct Instance {
    model: Model,
    part: usize,
    landmark: Option<u16>,
    enemy: Option<usize>,
    flight: Option<usize>,
    wake: Option<usize>,
    instantiated: bool,
    prepared: bool,
    materials: Vec<Handle<TitleSurface>>,
    binding: sparse_animation::Binding,
    clip: Option<(u16, u32)>,
}
#[derive(Component)]
struct TerrainDraw {
    tile: usize,
    material: u32,
    secondary: bool,
}
fn retire(world: &mut World) {
    let current = world
        .get_resource::<super::new_game::Session>()
        .and_then(|s| s.overworld.as_ref())
        .or_else(|| world.get_resource::<capture::Scene>().map(|s| &s.0));
    let Some(art) = world.get_resource::<Art>() else {
        return;
    };
    if current.is_some_and(|s| {
        Arc::ptr_eq(&s.package, &art.package)
            && s.session.travel.state().world == art.world
            && s.session.cinematic.as_ref().map(|c| c.id) == art.cinematic
    }) {
        return;
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<Instance>>()
        .iter(world)
        .collect();
    for entity in entities {
        world.despawn(entity);
    }
    if let Some(art) = world.remove_resource::<Art>()
        && let Some(ui) = art.ui
    {
        ui.despawn(world);
    }
}
fn load(mut commands: Commands, owner: State, art: Option<Res<Art>>, server: Res<AssetServer>) {
    let Some(scene) = owner.get() else {
        return;
    };
    if art.is_some() {
        return;
    }
    let loads = super::loading::LoadTasks::default();
    commands.insert_resource(Art {
        package: scene.package.clone(),
        world: scene.session.travel.state().world,
        cinematic: scene.session.cinematic.as_ref().map(|c| c.id),
        models: scene
            .models
            .iter()
            .map(|(key, parts)| {
                (
                    *key,
                    parts
                        .iter()
                        .map(|p| Part::load(p.clone(), &server, &loads))
                        .collect(),
                )
            })
            .collect(),
        loads,
        ready: false,
        spawned: false,
        landmarks: Default::default(),
        since: std::time::Instant::now(),
        ui: None,
    });
}
fn prepare(
    mut art: Option<ResMut<Art>>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    clips: Res<Assets<sparse_animation::Clip>>,
    images: Res<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(art) = art.as_mut() else {
        return;
    };
    if art.ready {
        return;
    }
    let result = (|| -> Result<()> {
        let mut ready = art.loads.complete();
        for part in art.models.values_mut().flatten() {
            if let Some(error) = part.load_error(&server) {
                anyhow::bail!("{error}");
            }
            ready &= part.resolve(&server, &gltfs, &clips, &images)?;
        }
        art.ready = ready;
        anyhow::ensure!(
            ready || art.since.elapsed().as_secs() < 120,
            "world graphics preparation timed out"
        );
        Ok(())
    })();
    if let Err(error) = result {
        error!("World graphics failed: {error:#}");
        exit.write(AppExit::error());
    }
}
#[allow(clippy::too_many_arguments)]
fn instances(
    mut commands: Commands,
    owner: State,
    mut art: Option<ResMut<Art>>,
    mut images: ResMut<Assets<Image>>,
    mut sampled: ResMut<super::scene::SampledImages>,
    mut materials: ResMut<Assets<TitleSurface>>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ui_materials: ResMut<Assets<super::field_ui::Surface>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    let Some(art) = art.as_mut().filter(|a| a.ready) else {
        return;
    };
    if art.ui.is_none() {
        match super::field_ui::WorldArtwork::load(
            &scene.package.files,
            &server,
            &mut commands,
            &mut meshes,
            &mut ui_materials,
            &mut images,
        ) {
            Ok(ui) => art.ui = Some(ui),
            Err(error) => {
                error!("World interface failed: {error:#}");
                exit.write(AppExit::error());
                return;
            }
        }
    }
    let mut spawn = |model: Model, landmark, enemy, flight, wake| {
        let Some(parts) = art.models.get(&model) else {
            return;
        };
        for (index, part) in parts.iter().enumerate() {
            let surfaces = part
                .surfaces(&mut images, &mut sampled)
                .into_iter()
                .map(|mut s| {
                    if matches!(model, Model::Actor(203 | 213 | 214)) {
                        s.additive = true;
                        s.depth_write = false;
                    }
                    if matches!(model, Model::Marker(1 | 17)) {
                        // Draw portals and discovery circles
                        // without depth writes.
                        s.blend = true;
                        s.depth_write = false;
                    }
                    if let Model::Cinematic(actor) = model {
                        cinematic::surface(
                            scene.session.cinematic.as_ref().unwrap().id,
                            actor,
                            &mut s,
                        );
                    }
                    materials.add(s)
                })
                .collect();
            commands
                .spawn((
                    WorldAssetRoot(part.scene.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    Instance {
                        model,
                        part: index,
                        landmark,
                        enemy,
                        flight,
                        wake,
                        instantiated: false,
                        prepared: false,
                        materials: surfaces,
                        binding: Default::default(),
                        clip: None,
                    },
                ))
                .observe(
                    |event: On<WorldInstanceReady>, mut roots: Query<&mut Instance>| {
                        if let Ok(mut instance) = roots.get_mut(event.entity) {
                            instance.instantiated = true;
                            instance.prepared = false;
                        }
                    },
                );
        }
    };
    if !art.spawned {
        for &model in art.models.keys() {
            if matches!(model, Model::Sky | Model::Tile(_) | Model::Cinematic(_) | Model::Actor(1..=9 | 201..=202 | 204..=212 | 214))
            {
                spawn(model, None, None, None, None);
            }
        }
        for member in 0..4 {
            for id in [200, 203] {
                spawn(Model::Actor(id), None, None, Some(member), None);
            }
        }
        // Both appearances for each stable simulation slot are resident before
        // activation. Spawning or changing a symbol never starts asset loading.
        for slot in 0..game::enemies::SLOTS {
            for id in 100..=101 {
                spawn(Model::Actor(id), None, Some(slot), None, None);
            }
        }
        for index in 0..effects::WAKE_COUNT {
            spawn(Model::Actor(213), None, None, None, Some(index));
        }
    }
    let mut added = Vec::new();
    for &id in scene.landmarks.keys() {
        let model = match scene.session.locations.appearance(id).map(|a| a.marker) {
            Some(Marker::Model { id }) => Model::Marker(id),
            Some(Marker::FieldPoint) if scene.session.locations.reward(id).is_some() => {
                Model::Actor(99)
            }
            Some(Marker::FieldPoint) => Model::Marker(17),
            _ => continue,
        };
        if !art.landmarks.contains(&(id, model)) {
            spawn(model, Some(id), None, None, None);
            added.push((id, model));
        }
    }
    art.landmarks.extend(added);
    art.spawned = true;
}
#[allow(clippy::too_many_arguments)]
fn bind(
    mut commands: Commands,
    owner: State,
    art: Option<Res<Art>>,
    mut roots: Query<(Entity, &mut Instance)>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>,
    meshes: Query<&super::materials::MaterialSlot>,
    resident: Res<super::loading::Resident>,
    mut exit: MessageWriter<AppExit>,
    images: Res<Assets<Image>>,
) {
    let Some(art) = art.filter(|a| a.ready && a.spawned) else {
        return;
    };
    let mut ready = art.ui.as_ref().is_some_and(|ui| ui.ready(&images));
    for (root, mut instance) in &mut roots {
        if !instance.instantiated {
            ready = false;
            continue;
        }
        if instance.prepared {
            continue;
        }
        let result = (|| -> Result<()> {
            let part = &art.models[&instance.model][instance.part];
            instance.binding = sparse_animation::Binding::new(
                root,
                part.spec.bone_names.len(),
                &children,
                &nodes,
            )?;
            for entity in children.iter_descendants(root) {
                if let Ok(slot) = meshes.get(entity) {
                    let slot = slot.index(instance.materials.len())?;
                    commands.entity(entity).insert((
                        MeshMaterial3d(instance.materials[slot].clone()),
                        super::draw_order::DrawOrder(
                            part.spec.materials[slot].draw_order
                                + match instance.model {
                                    Model::Sky => 0,
                                    Model::Cinematic(actor)
                                        if art
                                            .cinematic
                                            .is_some_and(|id| cinematic::background(id, actor)) =>
                                    {
                                        0
                                    }
                                    // The native secondary terrain pass (water,
                                    // shore foam and foliage) follows all opaque
                                    // terrain and actors, with depth writes off.
                                    Model::Tile(_) if part.spec.resource == 2 => 3 << 20,
                                    Model::Tile(_) => 1 << 20,
                                    _ => 2 << 20,
                                },
                            instance.landmark.map_or(0, usize::from),
                        ),
                    ));
                    if let Model::Tile(tile) = instance.model {
                        let material = part.spec.materials[slot].draw_order;
                        anyhow::ensure!(material < 4096, "world tile draw order exceeds its span");
                        commands.entity(entity).insert(TerrainDraw {
                            tile,
                            material,
                            secondary: part.spec.resource == 2,
                        });
                    }
                    // Optional field weapons are geometry on kk nodes, not
                    // bones to remove from the animated skeleton.
                    if matches!(instance.model, Model::Actor(1..=9 | 204..=212))
                        && part.spec.material_nodes.get(slot).is_some_and(|nodes| {
                            nodes.iter().any(|&node| {
                                part.spec.bone_names[usize::from(node)].starts_with("kk")
                            })
                        })
                    {
                        commands.entity(entity).insert(Visibility::Hidden);
                    }
                }
            }
            instance.prepared = true;
            Ok(())
        })();
        if let Err(error) = result {
            error!("World model binding failed: {error:#}");
            exit.write(AppExit::error());
            return;
        }
    }
    if ready
        && owner.get().is_some_and(|scene| {
            scene.session.events.world.field_transition.is_none()
                && scene.session.events.world.world_transition.is_none()
                && scene.session.world_destination().is_none()
        })
    {
        resident.active.store(true, Ordering::Release);
    }
}

fn terrain_order(
    owner: State,
    mut draws: Query<(&TerrainDraw, &mut super::draw_order::DrawOrder)>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    let state = scene.session.travel.state();
    let direction = [state.camera_yaw.sin(), state.camera_yaw.cos()];
    let mut tiles: Vec<_> = scene
        .models
        .keys()
        .filter_map(|model| match model {
            Model::Tile(index) => {
                let center = Position::from_map([
                    (index % 12) as f32 * TILE_SIZE + TILE_SIZE / 2.,
                    (index / 12) as f32 * TILE_SIZE + TILE_SIZE / 2.,
                    0.,
                ])
                .unwrap();
                let [x, y] = state.position.displacement_to(center);
                // Submit complete tiles from far to near. Interleaving
                // their material priorities lets a translucent tree edge write
                // depth before the neighboring ground, leaving sky-colored holes.
                let depth = -x.hypot(y) * (x * direction[0] + y * direction[1]);
                Some((*index, depth))
            }
            _ => None,
        })
        .collect();
    tiles.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut rank = [0; 108];
    for (order, (tile, _)) in tiles.into_iter().enumerate() {
        rank[tile] = order as u32;
    }
    for (draw, mut order) in &mut draws {
        let key =
            ((if draw.secondary { 3 } else { 1 }) << 20) + rank[draw.tile] * 4096 + draw.material;
        if order.0 != key {
            order.0 = key;
        }
    }
}
fn pose(
    owner: State,
    art: Option<Res<Art>>,
    mut roots: Query<(&mut Instance, &mut Transform, &mut Visibility)>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    let Some(art) = art.filter(|a| a.ready) else {
        return;
    };
    let session = &scene.session;
    let state = session.travel.state();
    let mount = session.travel.displayed_mount();
    let tick = session.events.tick();
    let formation = &session.events.world.party.as_ref().unwrap().formation;
    let cinema = session.cinematic.as_ref();
    let origin = cinema.map_or(state.position, cinematic::origin);
    for (mut instance, mut transform, mut visibility) in &mut roots {
        if !instance.prepared {
            continue;
        }
        let part = &art.models[&instance.model][instance.part];
        let mut visible = true;
        let mut slot = None;
        let mut heading = 0.;
        let mut scale = Vec3::ONE;
        let mut uv_offset = 0.;
        let mut brightness = 1.;
        let mut alpha = 1.;
        let position = if let Some(index) = instance.wake {
            if let Some(wake) = &scene.effects.wakes[index] {
                let fraction = wake.age as f32 / wake.duration as f32;
                scale = Vec3::splat(0.6 * (1. + fraction));
                let fade = (1. - fraction) * 1.25;
                brightness = 64. / 255. * if fade <= 1. { fade } else { 4. * (1.25 - fade) };
                uv_offset = (tick & 255) as f32 / 256.;
                slot = Some(0);
                wake.position
            } else {
                visible = false;
                state.position
            }
        } else if let Some(member) = instance.flight {
            visible = mount == Mount::Rheairds && member < formation.len();
            let (position, lift) = flight_position(session, member);
            scale = Vec3::splat(lift);
            if instance.model == Model::Actor(203) {
                let fraction = session.travel.speed_fraction();
                scale *= Vec3::new(
                    0.5 + 0.5 * fraction,
                    0.25 + 0.75 * fraction,
                    0.5 + 0.5 * fraction,
                );
                uv_offset = (tick & 127) as f32 / 128.;
                brightness = (64. + 64. * fraction) / 255.;
                slot = Some(0);
            } else {
                uv_offset = member as f32 * 0.25;
            }
            position
        } else if let Some(index) = instance.enemy {
            if let Some(symbol) = session.enemies.get(index) {
                visible = instance.model == Model::Actor(100 + u16::from(symbol.variant));
                heading = symbol.heading;
                slot = Some(symbol.animation);
                symbol.position
            } else {
                visible = false;
                state.position
            }
        } else if let Some(id) = instance.landmark {
            visible = session
                .locations
                .appearance(id)
                .is_some_and(|a| match a.marker {
                    Marker::Model { id } => instance.model == Model::Marker(id),
                    Marker::FieldPoint => {
                        matches!(instance.model, Model::Actor(99) | Model::Marker(17))
                            && a.interaction != resonance_content::overworld::Interaction::Disabled
                    }
                    _ => false,
                });
            visible |= instance.model == Model::Actor(99)
                && session
                    .discovery()
                    .is_some_and(|(location, _)| location == id);
            if part.spec.autoplay || matches!(instance.model, Model::Actor(99) | Model::Marker(1)) {
                slot = Some(0);
            }
            if instance.model == Model::Marker(17) {
                let [near, far] = scene.package.definition.movement.camera_distances
                    [usize::from(state.alternate_perspective)];
                alpha = ((far - session.travel.camera_distance()) / (far - near)).clamp(0., 1.);
                visible &= !mount.long_range() && alpha > 0.;
            }
            if instance.model == Model::Marker(1) {
                let landmark = session.locations.definition(id).unwrap();
                let position = scene.landmarks[&id];
                // The portal archive also contains reference spheres more than
                // 22,000 units from its origin. Gate this
                // model at 10,000 + entrance radius, before mesh culling.
                visible &= origin.distance_to(position) < 10000. + landmark.radius;
                Position::from_map([position.map()[0], position.map()[1], 0.]).unwrap()
            } else {
                scene.landmarks[&id]
            }
        } else {
            match instance.model {
                Model::Sky => {
                    visible = cinema.is_none_or(|c| !matches!(c.id, 521 | 526));
                    cinema.map_or_else(
                        || Position::from_map([origin.map()[0], origin.map()[1], -100.]).unwrap(),
                        cinematic::sky_position,
                    )
                }
                Model::Tile(index) => {
                    if part.spec.autoplay {
                        slot = Some(0);
                    }
                    Position::from_map([
                        (index % 12) as f32 * TILE_SIZE + TILE_SIZE / 2.,
                        (index / 12) as f32 * TILE_SIZE + TILE_SIZE / 2.,
                        0.,
                    ])
                    .unwrap()
                }
                Model::Actor(id) => {
                    heading = state.heading;
                    visible = match mount {
                        Mount::Foot => id == scene.leader(),
                        Mount::Noishe => id == scene.leader() || id == 202,
                        Mount::Rheairds => {
                            (204..=212).contains(&id)
                                && formation.iter().take(4).any(|&c| u16::from(c) + 203 == id)
                        }
                        Mount::Ship => {
                            id == 201
                                || id == 214
                                || (state.mount == Mount::Foot && id == scene.leader())
                        }
                    };
                    slot = Some(
                        if let Some(animation) = &scene.animation
                            && id == scene.leader()
                        {
                            animation.slot
                        } else if mount == Mount::Noishe && id == scene.leader() {
                            4
                        } else if session.travel.speed().abs() > 8. {
                            2
                        } else if session.travel.speed().abs() > 0.01 {
                            1
                        } else {
                            0
                        },
                    );
                    if id == 201 {
                        let (position, size) =
                            session.travel.ship_pose().unwrap_or((state.position, 0.));
                        scale = Vec3::splat(size);
                        Position::from_map([position.map()[0], position.map()[1], 0.]).unwrap()
                    } else if id == 214 {
                        visible &=
                            state.mount == Mount::Ship && session.travel.player_has_control();
                        let fraction = session.travel.speed_fraction();
                        scale = Vec3::splat(0.3 * (1. + fraction));
                        brightness = 64. * fraction / 255.;
                        uv_offset = (tick & 63) as f32 / 64.;
                        slot = Some(0);
                        Position::from_map([state.position.map()[0], state.position.map()[1], 0.])
                            .unwrap()
                    } else if mount == Mount::Rheairds && (204..=212).contains(&id) {
                        let member = formation
                            .iter()
                            .position(|&c| u16::from(c) + 203 == id)
                            .unwrap_or(0);
                        flight_position(session, member).0
                    } else {
                        Position::from_map([
                            state.position.map()[0],
                            state.position.map()[1],
                            state.altitude,
                        ])
                        .unwrap()
                    }
                }
                Model::Cinematic(actor) => {
                    let playback = cinema.unwrap();
                    slot = Some(0);
                    alpha = cinematic::opacity(playback, actor);
                    visible = alpha > 0.;
                    if cinematic::background(playback.id, actor) {
                        cinematic::sky_position(playback)
                    } else {
                        state.position
                    }
                }
                Model::Marker(_) => unreachable!(),
            }
        };
        let delta = origin.displacement_to(position);
        if instance.model == Model::Actor(99) {
            // Keep discoveries visible across the same ground horizon as other
            // landmarks; the contact radius must not become their draw distance.
            // Long-range vehicle travel still suppresses their interaction/model.
            let distance = delta[0].hypot(delta[1]);
            alpha *= ((10000. - distance) / 2000.).clamp(0., 1.);
            visible &= !mount.long_range() && alpha > 0.;
        }
        if matches!(instance.model, Model::Actor(_)) && cinema.is_some() {
            visible = false;
        }
        // Cull actual mesh bounds through the camera, never an entire tile or
        // landmark from its origin. The native fog hides the far clipping plane.
        // The model transform uses native object +0x0c. The nearby [-60,-60,140]
        // values at +0x40 are auxiliary coordinates, not actor translation.
        transform.translation = Vec3::new(delta[0], delta[1], position.map()[2])
            + Vec3::from_array(part.spec.translation);
        transform.scale = scale;
        for (index, handle) in instance.materials.iter().enumerate() {
            let binding = &part.spec.materials[index];
            let mut offset = Vec4::new(
                0.,
                if binding.color.as_ref().is_some_and(|b| b.texture == 0) {
                    uv_offset
                } else {
                    0.
                },
                0.,
                if binding.multiply.as_ref().is_some_and(|b| b.texture == 0) {
                    uv_offset
                } else {
                    0.
                },
            );
            if instance.model == Model::Marker(17) {
                let scroll = (tick & 127) as f32 / 128.;
                offset = Vec4::new(scroll, 0., scroll, 0.);
            } else if let Model::Cinematic(actor) = instance.model {
                let uv = [&binding.color, &binding.multiply].map(|binding| {
                    binding.as_ref().map_or(Vec2::ZERO, |b| {
                        cinematic::uv(cinema.unwrap(), actor, b.texture)
                    })
                });
                offset = Vec4::new(uv[0].x, uv[0].y, uv[1].x, uv[1].y);
            } else if matches!(instance.model, Model::Tile(_)) {
                let uv = [&binding.color, &binding.multiply].map(|binding| {
                    binding
                        .as_ref()
                        .and_then(|binding| {
                            part.spec
                                .texture_animations
                                .iter()
                                .find(|animation| animation.texture == binding.texture)
                        })
                        .map_or([0.; 2], |animation| animation.offset(u64::from(tick)))
                });
                offset = Vec4::new(uv[0][0], uv[0][1], uv[1][0], uv[1][1]);
            }
            let tint = part.spec.outline_color.map_or(Vec4::ONE, |c| {
                Vec4::from_array(c.map(|v| f32::from(v) / 255.))
            }) * Vec4::new(brightness, brightness, brightness, alpha);
            let fog_range = if instance.model != Model::Sky && cinema.is_none() {
                let far = session.travel.camera_distance() + 10000.;
                Vec4::new(far * 0.5, far * 0.75, 0., 0.)
            } else {
                Vec4::ZERO
            };
            if surfaces.get(handle).is_some_and(|s| {
                s.uv_offsets != offset || s.tint != tint || s.fog_range != fog_range
            }) {
                let mut surface = surfaces.get_mut(handle).unwrap();
                surface.uv_offsets = offset;
                surface.tint = tint;
                surface.fog_color = Vec4::new(208. / 255., 208. / 255., 224. / 255., 1.);
                surface.fog_range = fog_range;
            }
        }
        transform.rotation = if instance.landmark.is_none()
            && instance.enemy.is_none()
            && matches!(instance.model, Model::Actor(_))
        {
            match mount {
                Mount::Rheairds => flight_rotation(
                    state.camera_yaw,
                    session.travel.pitch(),
                    session.travel.bank(),
                ),
                Mount::Ship => Quat::from_rotation_z(
                    std::f32::consts::PI
                        - state.camera_yaw
                        - (0.75 * session.travel.bank()).to_radians(),
                ),
                _ => Quat::from_rotation_z(heading),
            }
        } else {
            Quat::from_rotation_z(heading)
        };
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if instance.clip.map(|c| c.0) != slot {
            instance.clip = slot.map(|slot| (slot, tick));
        }
    }
}

fn flight_rotation(yaw: f32, pitch: f32, bank: f32) -> Quat {
    // Pitch and roll belong to the craft's local axes, before world heading.
    Quat::from_rotation_z(std::f32::consts::PI - yaw - (0.5 * bank).to_radians())
        * Quat::from_rotation_y(-0.015 * bank)
        * Quat::from_rotation_x((pitch / 3.).to_radians())
}

/// Native flight formation expands as the craft lifts clear of the terrain.
fn flight_position(session: &game::Session, member: usize) -> (Position, f32) {
    let state = session.travel.state();
    let lift = ((state.altitude - state.position.map()[2]) / 100.).clamp(0.05, 1.);
    let (radius, angle) = match member {
        1 => (450., 120f32.to_radians()),
        2 => (450., -120f32.to_radians()),
        3 => (575., std::f32::consts::PI),
        _ => (0., 0.),
    };
    let angle = angle + state.camera_yaw + (0.5 * session.travel.bank()).to_radians();
    (
        Position::from_map([
            state.position.map()[0] + radius * lift * angle.sin(),
            state.position.map()[1] - radius * lift * angle.cos(),
            state.altitude,
        ])
        .unwrap(),
        lift,
    )
}
fn animate(
    owner: State,
    art: Option<Res<Art>>,
    roots: Query<&Instance>,
    clips: Res<Assets<sparse_animation::Clip>>,
    mut nodes: Query<&mut Transform>,
    mut affine: ResMut<sparse_animation::affine::Locals>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    let Some(art) = art.filter(|a| a.ready) else {
        return;
    };
    for instance in &roots {
        if !instance.prepared {
            continue;
        }
        let Some((slot, start)) = instance.clip else {
            continue;
        };
        let part = &art.models[&instance.model][instance.part];
        let Some(index) = part.spec.clips.iter().position(|c| c.resource_slot == slot) else {
            continue;
        };
        let clip = clips.get(&part.clips[index]).unwrap();
        let elapsed = (scene.session.events.tick().saturating_sub(start) as f64
            / resonance_game::clock::UPDATE_HZ) as f32;
        let duration = part.spec.clips[index].duration_seconds;
        let seconds = if matches!(instance.model, Model::Cinematic(_)) {
            scene
                .session
                .cinematic
                .as_ref()
                .unwrap()
                .seconds()
                .min(duration)
        } else if let Some(index) = instance.wake {
            let Some(wake) = &scene.effects.wakes[index] else {
                continue;
            };
            (wake.age as f32 / resonance_game::clock::UPDATE_HZ as f32 * effects::WAKE_RATE)
                .min(duration)
        } else if instance.model == Model::Actor(99) {
            scene
                .session
                .discovery()
                .filter(|(id, _)| Some(*id) == instance.landmark)
                .map_or(0., |(_, start)| {
                    (scene.session.events.tick().saturating_sub(start) as f32
                        / resonance_game::clock::UPDATE_HZ as f32)
                        .min(duration)
                })
        } else if slot >= 4 {
            if scene.animation.is_none() {
                duration
            } else {
                elapsed.min(duration)
            }
        } else {
            elapsed % duration
        };
        if let Err(error) = instance
            .binding
            .sample(&clip.0, seconds, &mut nodes, &mut affine)
        {
            error!("World animation failed: {error:#}");
            exit.write(AppExit::error());
            return;
        }
    }
}
fn camera(
    owner: State,
    display: Res<super::display::Display>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<super::FieldCamera>>,
    mut outputs: ResMut<Assets<TitleOutput>>,
    mut clear: ResMut<ClearColor>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    clear.0 = Color::linear_rgb(60. / 255., 130. / 255., 30. / 255.);
    let state = scene.session.travel.state();
    let distance = scene.session.travel.camera_distance();
    let angle = scene.session.camera.angle();
    let direction = Vec3::new(state.camera_yaw.sin(), state.camera_yaw.cos(), 0.);
    let lead = scene.session.camera.lead(state.alternate_perspective);
    let target = -direction * lead + Vec3::Z * (state.altitude * 0.75 + 85.);
    let eye = target + direction * distance * angle.cos() + Vec3::Z * distance * angle.sin();
    let bank = if scene.session.travel.displayed_mount().airborne() {
        scene.session.travel.bank() / 400.
    } else {
        0.
    };
    let up = Vec3::new(-bank * direction.y, bank * direction.x, 1.);
    let (eye, target, up, fov, far) = if let Some(playback) = &scene.session.cinematic {
        let camera = playback.camera();
        let mut eye = Vec3::from_array(camera.position);
        let mut target = Vec3::from_array(camera.target);
        let center = Vec3::new(eye.x, eye.y, 0.);
        if playback.id == 520 || (playback.id == 517 && playback.ticks() >= 1620) {
            // Cosmetic shake is deterministic without consuming encounter RNG.
            let jitter = |axis: u32| {
                let value = playback
                    .ticks()
                    .wrapping_mul(1664525)
                    .wrapping_add(axis.wrapping_mul(1013904223));
                ((value ^ (value >> 16)) % 500) as f32 - 250.
            };
            eye += Vec3::new(jitter(0), jitter(1), jitter(2)) / 500.;
            target += Vec3::new(jitter(3), jitter(4), jitter(5)) / 50.;
        }
        (eye - center, target - center, Vec3::Z, 18.9f32, 12800.)
    } else {
        (eye, target, up, 31.668, distance + 10000.)
    };
    for (mut transform, mut projection) in &mut cameras {
        *transform = Transform::from_translation(eye).looking_at(target, up);
        *projection = Projection::custom(super::camera::TitleProjection(PerspectiveProjection {
            fov: fov.to_radians(),
            aspect_ratio: display.0.aspect(),
            near: 100.,
            far,
            ..default()
        }));
    }
    TitleOutput::update(&mut outputs, |brightness| {
        let (fade, white) = scene
            .session
            .cinematic
            .as_ref()
            .map_or((0., false), |c| c.fade());
        *brightness = Vec4::new(1. - fade, if white { fade } else { 0. }, 0., 0.)
    });
}

#[allow(clippy::too_many_arguments)]
fn ui(
    mut commands: Commands,
    owner: State,
    mut art: Option<ResMut<Art>>,
    display: Res<super::display::Display>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<super::field_ui::Surface>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(scene) = owner.get() else {
        return;
    };
    let Some(ui) = art.as_mut().and_then(|a| a.ui.as_mut()) else {
        return;
    };
    if !ui.ready(&images) {
        return;
    }
    if let Err(error) = ui.render(
        &scene.session,
        &scene.package.resources.text,
        display.0,
        &mut commands,
        &mut meshes,
        &mut images,
        &mut materials,
    ) {
        error!("World interface failed: {error:#}");
        exit.write(AppExit::error());
    }
}

fn embed_shaders(app: &mut App) {
    bevy::asset::embedded_asset!(app, "title_surface.wgsl");
    bevy::asset::embedded_asset!(app, "title_surface_vertex.wgsl");
    bevy::asset::embedded_asset!(app, "field_ui.wgsl");
    bevy::asset::embedded_asset!(app, "title_output.wgsl");
    bevy::shader::load_shader_library!(app, "surface_bindings.wgsl");
}

pub(super) fn ready(world: &mut World) -> bool {
    world
        .get_resource::<Art>()
        .is_some_and(|a| a.ready && a.spawned)
        && world
            .resource::<super::loading::Resident>()
            .active
            .load(Ordering::Acquire)
        && world.query::<&Instance>().iter(world).all(|p| p.prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flight_pitch_and_bank_follow_local_axes_at_every_heading() {
        for yaw in [0., std::f32::consts::FRAC_PI_2, std::f32::consts::PI] {
            let pitch = flight_rotation(yaw, 30., 0.);
            assert!(
                (pitch * Vec3::Y).z > 0.1,
                "pitch must lift the nose at yaw {yaw}"
            );
            assert!(
                (pitch * Vec3::X).z.abs() < 0.0001,
                "pitch must not roll the wings"
            );
            let bank = flight_rotation(yaw, 0., 10.);
            assert!(
                (bank * Vec3::X).z > 0.1,
                "bank must roll the wings at yaw {yaw}"
            );
            assert!(
                (bank * Vec3::Y).z.abs() < 0.0001,
                "bank must not pitch the nose"
            );
        }
    }
}

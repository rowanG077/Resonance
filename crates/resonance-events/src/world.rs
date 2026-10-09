use crate::animation::{Animation, slot};
use std::collections::BTreeMap;

/// Transient player dimensions. Field entry resets these; the scenario may
/// restore them for a narrow passage through native 0x79.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSize {
    #[default]
    Normal,
    Small,
}
impl PlayerSize {
    pub fn model_scale(self) -> f32 {
        match self {
            Self::Normal => 1.,
            Self::Small => 0.4,
        }
    }
    pub fn movement_scale(self) -> f32 {
        match self {
            Self::Normal => 1.,
            Self::Small => 1. / 3.,
        }
    }
    pub fn floor_clearance(self) -> f32 {
        match self {
            Self::Normal => 40.,
            Self::Small => 14.,
        }
    }
}
/// Interaction actors (native class 3) participate in sustained ring contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorRole {
    Ordinary,
    Interaction,
}

#[derive(Debug, Clone)]
pub struct Actor {
    /// A removed actor completes its current presentation before being reclaimed.
    pub(crate) retiring: bool,
    pub role: ActorRole,
    /// An authored scene object expires with its owning controller.
    pub(crate) operation: Option<crate::Operation>,
    /// Replacing an actor invalidates its retained presentation instance.
    pub instance: u64,
    pub(crate) update_order: usize,
    pub(crate) authored_handle: Option<i32>,
    pub resource: u32,
    pub position: [f32; 3],
    pub(crate) visual_lift: Option<crate::projectile::VisualLift>,
    pub chain_impulses: BTreeMap<u32, crate::projectile::ChainImpulse>,
    pub visible: bool,
    /// A spawned actor is presented after its first update.
    pub visible_from: u32,
    /// A non-rendered scene marker can still own a scenery interaction.
    pub interaction_anchor: bool,
    /// Read-only actors use a strict depth test; ordinary actors test and write
    /// depth, including equal-depth fragments. This is presentation state.
    pub depth_write: bool,
    pub blend: Option<crate::effect::Blend>,
    pub animation: Option<Animation>,
    /// Independent scenery motion layers, sampled over its base animation.
    pub scenery_animations: BTreeMap<i8, Animation>,
    pub scale_percent: [i32; 3],
    /// Scale already presented before this update's script edits.
    pub(crate) rendered_scale: Option<[i32; 3]>,
    pub tilt: [i32; 2],
    pub tint: [u8; 3],
    pub opacity: u8,
    pub heading_lock: u8,
    pub interaction_label: i32,
    pub pushable: bool,
    pub ring_contact_disabled: bool,
    pub interaction_disabled: bool,
    pub unlit: bool,
    pub toon_lighting: Option<u8>,
    pub draw_layer: i8,
    pub heading: f32,
    pub target_heading: f32,
    pub turn_speed: f32,
    pub appearance: Appearance,
    pub wings: Option<crate::Wings>,
    pub cull_outside_view: bool,
    /// Script overrides are separate from the actor's initial visibility policy.
    pub(crate) culling_flags: [Option<bool>; 2],
    /// Last actor update's view test; animation and secondary motion share it.
    pub animation_culled: bool,
    pub grounded: bool,
    pub collidable: bool,
    /// Native contact shape; disabling walking collision does not remove it.
    pub contact: ActorContact,
    /// Touching the player invokes registry (0, -2), enabled by property 20.
    pub contact_event: bool,
    /// Enabled by native property 18; separate from the actor's contact cylinder.
    pub model_collision: Option<std::sync::Arc<resonance_content::field::ModelCollision>>,
    /// Parent bone's rigid frame; script coordinates remain local to the attachment.
    pub(crate) collision_parent: Option<resonance_content::animation::Matrix>,
    pub radius: f32,
    pub path: crate::autonomy::Path,
    pub casts_shadow: bool,
    pub shadow_alpha: u8,
    /// Last resolved field light, shared by rendering and native color queries.
    pub light: Option<crate::effect::CharacterLight>,
    pub ambient_sound: Option<crate::AmbientSound>,
    pub enemy: Option<Enemy>,
    pub(crate) enemy_source: Option<crate::enemy_source::EnemySource>,
    pub(crate) emitter: Option<crate::emitter::Emitter>,
    pub ring_station: bool,
    pub attachment: Option<Attachment>,
    pub motion: Option<ActorMotion>,
    /// Native movement rate remains readable after a scripted destination ends.
    pub(crate) movement_speed: f32,
    pub autonomy: Option<crate::Autonomy>,
    pub scripted_animation: bool,
    pub idle_animation: u16,
}

pub const ACTOR_CONTACT_HEIGHT: f32 = 150.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorContact {
    None,
    Cylinder,
}
#[derive(Debug, Clone)]
pub struct Enemy {
    pub event: u16,
    pub behavior: u8,
    pub normal_speed: f32,
    pub alert_speed: f32,
    pub random_turns: u8,
    pub chase_on_sight: bool,
    pub sight_angle: f32,
    pub sight_distance: f32,
    /// Result of the field's most recent native sight query (property 49).
    pub alerted: bool,
    pub event_parameters: [i16; 2],
    /// Shared contact/ring/script timer (property 54); negatives pause indefinitely.
    pub pause_ticks: i16,
    pub reaction: crate::effect::StunEffect,
}
impl Enemy {
    pub fn stun_effect(&self) -> Option<crate::effect::StunEffect> {
        (self.pause_ticks != 0).then_some(self.reaction)
    }
}
impl Actor {
    pub fn model_resource(&self) -> u32 {
        resonance_content::appearance::costume_resource(self.resource, self.appearance.costume)
    }
    pub fn model_scale(&self) -> [f32; 3] {
        self.model_scale_percent().map(|scale| scale as f32 / 100.)
    }
    fn model_scale_percent(&self) -> [i32; 3] {
        if self.wings.is_some() {
            [100; 3]
        } else {
            self.rendered_scale.unwrap_or(self.scale_percent)
        }
    }
    pub fn tilt_degrees(&self) -> [f32; 2] {
        self.tilt.map(|angle| angle as f32)
    }
    pub fn ring_contact_enabled(&self) -> bool {
        !self.ring_contact_disabled
    }

    /// Model displacement does not move the actor's navigation or script origin.
    pub fn visual_position(&self) -> [f32; 3] {
        let mut position = self.position;
        if let Some(lift) = &self.visual_lift
            && lift.operation.is_pending()
        {
            position[2] += lift.height;
        }
        position
    }

    pub fn world_point(&self, point: [f32; 3]) -> [f32; 3] {
        let vector = self.local_vector(point);
        std::array::from_fn(|i| vector[i] + self.position[i])
    }

    fn local_vector(&self, point: [f32; 3]) -> [f32; 3] {
        let scale = self.model_scale_percent();
        let [x, y, z] = std::array::from_fn(|i| point[i] * scale[i] as f32 / 100.);
        let [tilt_x, tilt_y] = self.tilt_degrees();
        let (sx, cx) = tilt_x.to_radians().sin_cos();
        let (sy, cy) = tilt_y.to_radians().sin_cos();
        let (sz, cz) = self.heading.to_radians().sin_cos();
        let (y, z) = (cx * y - sx * z, sx * y + cx * z);
        let (x, z) = (cy * x + sy * z, -sy * x + cy * z);
        [cz * x - sz * y, sz * x + cz * y, z]
    }

    pub fn contains_solid(
        &self,
        point: [f32; 3],
        query: resonance_content::field::CollisionQuery,
    ) -> bool {
        self.solid_contact(point, point, query).is_some()
    }
    /// Clip a segment against every outward plane of each convex solid.
    pub fn solid_contact(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        query: resonance_content::field::CollisionQuery,
    ) -> Option<f32> {
        self.model_collision
            .as_ref()?
            .solids
            .iter()
            .filter_map(|group| {
                if !query.accepts(group.surface) || group.triangles.is_empty() {
                    return None;
                }
                let planes = self
                    .collision_triangles(group)
                    .map(crate::collision::Plane::triangle);
                crate::collision::segment_contact(planes, start, end)
            })
            .min_by(f32::total_cmp)
    }

    pub fn collision_triangles<'a>(
        &'a self,
        group: &'a resonance_content::field::CollisionGroup,
    ) -> impl Iterator<Item = [[f32; 3]; 3]> + 'a {
        group
            .triangles
            .iter()
            .map(|triangle| triangle.map(|i| self.collision_point(group.vertices[usize::from(i)])))
    }

    pub fn collision_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let points = self
            .model_collision
            .iter()
            .flat_map(|mesh| {
                if self.pushable && !mesh.floors.is_empty() {
                    &mesh.floors
                } else {
                    &mesh.solids
                }
            })
            .flat_map(|group| &group.vertices)
            .map(|p| self.collision_point(*p));
        let mut bounds = None;
        for point in points {
            let (low, high) = bounds.get_or_insert((point, point));
            for i in 0..3 {
                low[i] = low[i].min(point[i]);
                high[i] = high[i].max(point[i]);
            }
        }
        bounds.unwrap_or_else(|| {
            let [x, y, z] = self.position;
            (
                [x - self.radius, y - self.radius, z],
                [x + self.radius, y + self.radius, z + ACTOR_CONTACT_HEIGHT],
            )
        })
    }

    pub fn collision_point(&self, point: [f32; 3]) -> [f32; 3] {
        let point = self.world_point(point);
        self.collision_parent.map_or(point, |parent| {
            resonance_content::animation::transform_point(parent, point)
        })
    }

    pub(crate) fn local_matrix(&self) -> resonance_content::animation::Matrix {
        std::array::from_fn(|column| {
            if column == 3 {
                return [self.position[0], self.position[1], self.position[2], 1.];
            }
            let mut axis = [0.; 3];
            axis[column] = 1.;
            let point = self.local_vector(axis);
            [point[0], point[1], point[2], 0.]
        })
    }

    pub fn new(resource: u32, position: [f32; 3]) -> Self {
        Self {
            role: ActorRole::Ordinary,
            operation: None,
            instance: 0,
            authored_handle: None,
            resource,
            position,
            visual_lift: None,
            chain_impulses: BTreeMap::new(),
            visible: true,
            visible_from: 0,
            update_order: 0,
            interaction_anchor: false,
            depth_write: true,
            blend: None,
            animation: None,
            scenery_animations: BTreeMap::new(),
            scale_percent: [100; 3],
            rendered_scale: None,
            tilt: [0; 2],
            tint: [crate::effect::NEUTRAL_TINT; 3],
            opacity: 255,
            heading_lock: 0,
            interaction_label: 2,
            pushable: false,
            ring_contact_disabled: false,
            interaction_disabled: false,
            unlit: false,
            toon_lighting: None,
            draw_layer: 2,
            heading: 0.,
            target_heading: 0.,
            turn_speed: 5.,
            appearance: Appearance::default(),
            wings: None,
            retiring: false,
            cull_outside_view: true,
            culling_flags: [None; 2],
            animation_culled: false,
            grounded: true,
            collidable: true,
            contact: ActorContact::Cylinder,
            contact_event: false,
            model_collision: None,
            collision_parent: None,
            radius: 42.,
            path: Default::default(),
            casts_shadow: true,
            shadow_alpha: 64,
            light: None,
            ambient_sound: None,
            enemy: None,
            enemy_source: None,
            emitter: None,
            ring_station: false,
            attachment: None,
            motion: None,
            movement_speed: 0.,
            autonomy: None,
            scripted_animation: false,
            idle_animation: slot::IDLE,
        }
    }
    pub fn face(&mut self, heading: f32) {
        self.heading = heading.rem_euclid(360.);
        self.target_heading = self.heading;
    }
    pub fn movement_speed(&self) -> f32 {
        self.motion.as_ref().map_or_else(
            || {
                self.autonomy
                    .as_ref()
                    .map_or(self.movement_speed, |ai| ai.speed)
            },
            |motion| motion.speed,
        )
    }
    pub fn set_movement_speed(&mut self, speed: f32) {
        self.movement_speed = speed;
        if let Some(motion) = &mut self.motion {
            motion.speed = speed;
        }
        if let Some(ai) = &mut self.autonomy {
            ai.speed = speed;
        }
    }
    pub(crate) fn step_heading(&mut self, controlled: bool, moving: bool) {
        if self.wings.is_some() {
            return;
        }
        let speed = if controlled {
            20.
        } else {
            self.turn_speed
                * if moving && self.enemy.is_none() {
                    2.
                } else {
                    1.
                }
        };
        self.target_heading = self.target_heading.rem_euclid(360.);
        if speed > 0. {
            let delta = self.heading_delta();
            self.heading = if delta.abs() <= speed {
                self.target_heading
            } else {
                (self.heading + delta.signum() * speed).rem_euclid(360.)
            };
        }
    }
    fn heading_delta(&self) -> f32 {
        (self.target_heading - self.heading + 180.).rem_euclid(360.) - 180.
    }
    pub(crate) fn turn_direction(&self) -> f32 {
        let delta = self.heading_delta();
        if delta == 0. { 0. } else { delta.signum() }
    }
    pub(crate) fn step_motion(&mut self) {
        let speed = self.movement_speed();
        self.set_movement_speed(speed);
        let Some(motion) = &self.motion else {
            return;
        };
        let mut delta: [f32; 3] = std::array::from_fn(|i| motion.target[i] - self.position[i]);
        if self.grounded {
            delta[2] = 0.;
        }
        let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        if distance < 1. {
            self.motion = None;
            return;
        }
        self.target_heading = delta[0].atan2(-delta[1]).to_degrees().rem_euclid(360.);
        let fraction = (motion.speed / distance).min(1.);
        for (i, delta) in delta.into_iter().enumerate() {
            self.position[i] += delta * fraction;
        }
    }
}
#[derive(Debug, Clone)]
pub struct ActorMotion {
    pub target: [f32; 3],
    pub speed: f32,
}
#[derive(Debug, Clone)]
pub struct Attachment {
    pub actor: i32,
    pub bone: String,
}
#[derive(Debug, Clone, Default)]
pub struct Appearance {
    pub fixed_heading: Option<f32>,
    pub face: Face,
    pub eyes: Option<crate::EyeBlink>,
    pub mouth: Option<Face>,
    pub expression: u8,
    pub costume_frame: u8,
    pub costume: u8,
    pub model_hidden: bool,
    pub secondary_motion_disabled: bool,
    pub hidden_nodes: std::collections::BTreeSet<u16>,
    pub bone_adjustments: BTreeMap<u8, BoneAdjustment>,
}
#[derive(Debug, Clone, Copy, Default)]
pub enum Face {
    Disabled,
    #[default]
    Blink,
    Frame(u8),
}
#[derive(Debug, Clone)]
pub enum BoneTarget {
    Name(String),
    Index(u16),
}
#[derive(Debug, Clone)]
pub struct BoneAdjustment {
    pub bone: BoneTarget,
    /// Native node setters replace the local rotation; skeletal adjustments add to it.
    pub absolute_rotation: bool,
    /// Angles in model coordinates, after the native's integer half-angle conversion.
    pub angles: [f32; 3],
    pub from: [f32; 3],
    pub duration_ticks: u32,
    pub start_tick: u32,
    pub translation: Option<BoneTranslation>,
    pub scale: Option<BoneScale>,
}
/// Absolute local scale, retaining the bind-pose contribution until the tween ends.
/// The renderer supplies the model's bind scale; interrupted tweens stay continuous.
#[derive(Debug, Clone)]
pub struct BoneScale {
    from: [f32; 3],
    bind_weight: f32,
    to: [f32; 3],
    duration: u32,
    start: u32,
}
impl BoneScale {
    pub fn new(previous: Option<&Self>, to: [f32; 3], duration: u32, tick: u32) -> Self {
        Self {
            from: previous.map_or([0.; 3], |p| p.sample(tick, [0.; 3])),
            bind_weight: previous.map_or(1., |p| p.bind_weight * (1. - p.fraction(tick))),
            to,
            duration: duration.max(1),
            start: tick,
        }
    }
    fn fraction(&self, tick: u32) -> f32 {
        (tick.saturating_sub(self.start) + 1).min(self.duration) as f32 / self.duration as f32
    }
    pub fn sample(&self, tick: u32, bind_scale: [f32; 3]) -> [f32; 3] {
        let fraction = self.fraction(tick);
        std::array::from_fn(|i| {
            (self.from[i] + bind_scale[i] * self.bind_weight) * (1. - fraction)
                + self.to[i] * fraction
        })
    }
}
#[derive(Debug, Clone)]
pub struct BoneTranslation {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub duration_ticks: u32,
    pub start_tick: u32,
}
impl BoneAdjustment {
    pub fn translation(&self, tick: u32) -> [f32; 3] {
        self.translation.as_ref().map_or([0.; 3], |translation| {
            let fraction = (tick.saturating_sub(translation.start_tick) + 1)
                .min(translation.duration_ticks) as f32
                / translation.duration_ticks.max(1) as f32;
            std::array::from_fn(|i| {
                translation.from[i] + (translation.to[i] - translation.from[i]) * fraction
            })
        })
    }
    pub fn sample(&self, tick: u32) -> [f32; 3] {
        let fraction = (tick.saturating_sub(self.start_tick) + 1).min(self.duration_ticks) as f32
            / self.duration_ticks.max(1) as f32;
        std::array::from_fn(|i| self.from[i] + (self.angles[i] - self.from[i]) * fraction)
    }
}
#[derive(Debug, Clone)]
pub struct CameraTrack {
    pub resource: u32,
    pub start_tick: u32,
}
#[derive(Default)]
pub struct GameWorld {
    pub(crate) authored_actors: BTreeMap<i32, i32>,
    pub(crate) ring: crate::ring::Controller,
    pub(crate) fog_effects: BTreeMap<i32, crate::camera::FogEffect>,
    pub tick: u32,
    pub effect_tick: u32,
    /// The owning scene's map, also available to its nested skit scripts.
    pub current_field: Option<u32>,
    pub ring_scenery: resonance_content::field::RingScenery,
    /// Debug sessions are disabled for ordinary starts and New Game Plus.
    pub debug_session: bool,
    pub skit: Option<crate::skit::Scene>,
    pub skit_request: Option<crate::skit::Request>,
    pub menu_request: Option<crate::menu::Request>,
    pub actors: BTreeMap<i32, Actor>,
    pub(crate) duplicate_actors: BTreeMap<i32, i32>,
    pub(crate) automatic_wings: Option<(u64, u64)>,
    pub(crate) actor_order: Vec<i32>,
    pub(crate) next_actor_instance: u64,
    pub camera: Option<CameraTrack>,
    pub fade: Option<Fade>,
    pub scene_dissolve: Option<SceneDissolve>,
    pub next_transition_white: Option<bool>,
    pub overlays: BTreeMap<i32, Overlay>,
    pub effect_settings: BTreeMap<(i32, i32), [i32; 3]>,
    pub character_lights: BTreeMap<i32, crate::effect::CharacterLight>,
    pub texture_bindings: BTreeMap<i32, i32>,
    pub texture_animation_enabled: bool,
    /// Texture clock, held while texture animation is disabled.
    pub texture_animation_tick: u64,
    /// Running clock sampled by the last enabled field texture callback.
    pub texture_animation_effect_tick: u32,
    /// Two enlarged framebuffer copies, selected by their native depth test.
    /// Zero disables a pass; values are orthographic screen depths.
    pub screen_copy_depth: [f32; 2],
    pub dialogue: BTreeMap<u8, crate::dialogue::Dialogue>,
    pub choices: BTreeMap<u8, crate::dialogue::Choice>,
    pub party: Option<crate::party::Party>,
    pub field_transition: Option<FieldTransition>,
    pub world_transition: Option<WorldTransition>,
    pub(crate) field_exit: Option<crate::field_exit::DoorExit>,
    pub preload_field: Option<u32>,
    pub movie: Option<crate::dialogue::Movie>,
    /// Spoken dialogue duration is supplied by the game audio adapter.
    pub voice: Option<VoicePlayback>,
    pub voice_durations: std::sync::Arc<BTreeMap<u32, u32>>,
    pub field_camera: Option<crate::camera::CameraRig>,
    pub input_enabled: bool,
    pub menu_disabled: bool,
    pub input: crate::input::Input,
    /// Native event pause, independent of an authored task's input ownership.
    pub mapped_input_disabled: bool,
    pub controlled_actor: i32,
    /// Block retained by the player's grab action, independent of script targets.
    pub grabbed_block: Option<i32>,
    pub player_size: PlayerSize,
    pub event_flags: std::collections::BTreeSet<u16>,
    pub script_state: symphonia_script::authored::ScriptState,
    pub event_records: BTreeMap<u8, EventRecord>,
    /// Optional Unix time supplied by a replay; live events use the system clock.
    pub calendar_time: Option<i64>,
    pub triggers: Vec<Trigger>,
    pub save_points: Vec<SavePoint>,
    pub treasures: Vec<TreasureChest>,
    pub treasure_models: [Option<u32>; 2],
    /// Search distance for automatic scenery-door interactions; absent uses 250.
    pub door_interaction_radius: Option<f32>,
    pub audio_commands: Vec<AudioCommand>,
    pub rumble: Option<crate::rumble::Rumble>,
    pub(crate) ambient_voices: [Option<crate::ambient::Voice>; 2],
    pub voice_banks: [Option<u16>; 2],
    pub emotes: BTreeMap<i32, Emote>,
    pub damage_numbers: crate::field_damage::DamageNumbers,
    pub paralysis: Option<crate::effect::Paralysis>,
    pub billboards: BTreeMap<i32, crate::effect::BillboardEffect>,
    pub(crate) station_transfers: Vec<crate::effect::station::Transfer>,
    pub(crate) effect_changes: Vec<(i32, u32, crate::effect::property::Change)>,
    pub(crate) particles_before_update: i32,
    pub effect_palette: crate::effect::Palette,
    pub effect_textures: BTreeMap<u8, (u32, u8)>,
    pub model_particles: BTreeMap<i32, crate::model_particle::ModelParticle>,
    pub refractions: BTreeMap<i32, crate::effect::RefractionPulse>,
    pub random_state: u32,
    pub gameplay_random: crate::GameplayRandom,
    pub battle_request: Option<crate::battle::Request>,
    pub(crate) restore_battle_music: bool,
    /// Overworld bottles count movement updates; their scene owns that clock.
    pub external_encounter_clock: bool,
    pub(crate) loaded_resources: BTreeMap<i32, (crate::ResourceKind, u32)>,
    pub(crate) operations: crate::operation::OperationScope,
    pub(crate) next_particle: i32,
}

#[derive(Debug, Clone)]
pub struct VoicePlayback {
    pub resource: u32,
    pub end_tick: u32,
}

#[derive(Debug, Clone)]
pub struct SavePoint {
    pub actor: i32,
    pub position: [f32; 3],
    pub resource: u32,
    pub born: u32,
    pub active: bool,
    /// A sealed circle is inert until its persistent unlock flag is set.
    pub unlock_flag: Option<u16>,
    /// Vertical texture scale of the glow; the circle's geometry stays unchanged.
    pub glow_scale: f32,
}

impl SavePoint {
    pub fn is_open(&self, flags: &std::collections::BTreeSet<u16>) -> bool {
        self.unlock_flag.is_none_or(|flag| flags.contains(&flag))
    }
}

#[derive(Debug, Clone)]
pub struct FieldTransition {
    pub map: u32,
    pub position: [f32; 3],
    pub heading: f32,
    pub camera: Option<crate::camera::EntryCamera>,
    pub operation: crate::Operation,
}

/// Script field IDs >= 3000 enter the world service. Their second and fifth
/// arguments are a landmark and exit octant, rather than field coordinates.
#[derive(Debug, Clone)]
pub struct WorldTransition {
    pub location: u16,
    pub direction: i16,
    /// Numbered cinematics retire their caller and enter this destination afterward.
    pub following: Option<SceneDestination>,
    pub operation: crate::Operation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneDestination {
    pub map: u32,
    pub position: [f32; 3],
    pub heading: f32,
}

impl GameWorld {
    pub(crate) fn remove_actor(&mut self, id: i32) {
        self.actors.remove(&id);
        self.billboards.retain(|_, p| p.owner != Some(id));
        self.overlays.remove(&id);
        self.emotes.remove(&id);
    }

    /// Cinematic completion publishes another scene request. The source remains
    /// alive until the destination owner has prepared and accepted it.
    pub fn request_destination(&mut self, destination: SceneDestination) -> Result<(), String> {
        if self.field_transition.is_some()
            || self.world_transition.is_some()
            || self.field_exit.is_some()
        {
            return Err("scene transition is already pending".into());
        }
        if !destination
            .position
            .iter()
            .chain([&destination.heading])
            .all(|n| n.is_finite())
        {
            return Err("invalid scene destination".into());
        }
        if destination.map >= 3000 {
            let location = destination.position[0];
            if location.fract() != 0.
                || !(0. ..=337.).contains(&location)
                || destination.heading.fract() != 0.
                || destination.heading < f32::from(i16::MIN)
                || destination.heading > f32::from(i16::MAX)
            {
                return Err("invalid world destination".into());
            }
            self.request_world(location as u16, destination.heading as i16, None)?;
        } else {
            self.field_transition = Some(FieldTransition {
                map: destination.map,
                position: destination.position,
                heading: destination.heading.rem_euclid(360.),
                camera: None,
                operation: self.operations.begin()?,
            });
            self.input_enabled = false;
        }
        Ok(())
    }

    pub fn request_world(
        &mut self,
        location: u16,
        direction: i16,
        following: Option<SceneDestination>,
    ) -> Result<crate::Operation, String> {
        if self.field_transition.is_some()
            || self.world_transition.is_some()
            || self.field_exit.is_some()
        {
            return Err("scene transition is already pending".into());
        }
        let cinematic = (513..=526).contains(&location);
        if !(location == 0
            || (1..=98).contains(&location)
            || (257..=337).contains(&location)
            || cinematic)
            || cinematic != following.is_some()
        {
            return Err("invalid world scene or cinematic continuation".into());
        }
        let operation = self.operations.begin()?;
        self.world_transition = Some(WorldTransition {
            location,
            direction,
            following,
            operation: operation.clone(),
        });
        self.input_enabled = false;
        Ok(operation)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventRecord {
    pub value: u8,
    pub extra: u8,
    pub tick: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// UTC seconds at the script write, independent of saved play time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<i64>,
}
#[derive(Debug, Clone)]
pub struct Emote {
    pub draw_order: usize,
    pub phase: u32,
    pub actor: i32,
    pub kind: crate::emote::Kind,
    pub offset: [f32; 3],
    pub start_tick: u32,
    pub duration: Option<u32>,
}
/// Music requests decoded at the native script boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicCommand {
    Play(u16),
    PlayJingle(u16),
    Stop,
    Suspend,
    Resume,
}

impl TryFrom<i16> for MusicCommand {
    type Error = &'static str;

    fn try_from(command: i16) -> Result<Self, Self::Error> {
        // Dispatch these wire values before ordinary track IDs.
        Ok(match command {
            -1 => Self::Stop,
            -2 => Self::Suspend,
            -3 => Self::Resume,
            97 => Self::PlayJingle(97),
            0.. => Self::Play(command as u16),
            _ => return Err("unknown native music command"),
        })
    }
}

#[derive(Debug, Clone)]
pub enum AudioCommand {
    SoundReverb(u8),
    Voice(u32),
    StopVoice,
    SelectBank(u8),
    StopSound(u16),
    SoundVolume {
        slot: u16,
        volume: u8,
    },
    SoundPan {
        slot: u16,
        pan: u8,
    },
    Music(MusicCommand),
    MusicVolume {
        volume: u8,
        duration_ticks: u32,
    },
    Sound {
        id: i16,
        pan: u8,
        volume: u8,
        slot: Option<u8>,
    },
    /// Restart a vehicle engine whenever its original sound program ends.
    /// StopSound or replacing the owning scene retires this lease.
    RepeatSound {
        id: i16,
        pan: u8,
        volume: u8,
        slot: u8,
    },
}
#[derive(Debug, Clone)]
pub struct Trigger {
    pub ring_barrier: bool,
    /// Transient native contact count, shared by player and projectile queries.
    pub activations: u16,
    pub key: u32,
    /// Automatic contact using registry 2, shared with confirmed triggers.
    pub automatic_event: bool,
    pub shape: TriggerShape,
    /// Vertical extent above the line, not a horizontal activation radius.
    pub height: f32,
    /// Confirmed trigger records contain an interaction indicator followed by
    /// destination/preload hints.
    pub transition: Option<[u32; 3]>,
    /// Touch-trigger action and destination/preload hints, independent of activation.
    pub touch_metadata: [u32; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i16)]
pub(crate) enum TriggerKind {
    Touch = 1,
    Automatic = 2,
    Confirmed = 3,
}
impl Trigger {
    pub(crate) fn registry_kind(&self) -> u32 {
        match self.kind() {
            TriggerKind::Confirmed => TriggerKind::Automatic as u32,
            kind => kind as u32,
        }
    }

    pub(crate) fn kind(&self) -> TriggerKind {
        if self.transition.is_some() {
            TriggerKind::Confirmed
        } else if self.automatic_event {
            TriggerKind::Automatic
        } else {
            TriggerKind::Touch
        }
    }
}
#[derive(Debug, Clone)]
pub enum TriggerShape {
    Circle { center: [f32; 3], radius: f32 },
    Line([[f32; 3]; 2]),
    Triangle([[f32; 3]; 3]),
    Quad([[f32; 3]; 4]),
}
#[derive(Debug, Clone)]
pub struct TreasureChest {
    pub actor: i32,
    pub flag: u16,
    pub reward: TreasureReward,
    pub kind: TreasureKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TreasureKind {
    UnknownId0 = 0,
    UnknownId1 = 1,
    UnknownId2 = 2,
    CustomModel0 = 3,
    CustomModel1 = 4,
}
impl TryFrom<i32> for TreasureKind {
    type Error = &'static str;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::UnknownId0),
            1 => Ok(Self::UnknownId1),
            2 => Ok(Self::UnknownId2),
            3 => Ok(Self::CustomModel0),
            4 => Ok(Self::CustomModel1),
            _ => Err("invalid treasure kind"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreasureReward {
    Item(u16),
    Gald(u16),
}
impl TreasureReward {
    pub fn from_source(value: u16) -> Self {
        match value.checked_sub(1024) {
            Some(amount) => Self::Gald(amount),
            None => Self::Item(value),
        }
    }
}
impl GameWorld {
    pub(crate) fn scene_actor_key(&self, script: i32) -> Result<i32, String> {
        if !self.actors.contains_key(&script) {
            return Ok(script);
        }
        self.unaddressable_actor_key()
    }

    pub(crate) fn unaddressable_actor_key(&self) -> Result<i32, String> {
        // Field services reserve the preceding 2048 negative IDs. Duplicates
        // keep separate render/animation instances without replacing the first.
        (i32::MIN + 2048..i32::MIN + 6144)
            .find(|id| !self.actors.contains_key(id))
            .ok_or_else(|| "scene actor instance limit exceeded".into())
    }

    pub(crate) fn despawn_scene_actors(&mut self, script: i32, resources: &crate::ResourceLibrary) {
        let mut ids = vec![script];
        self.duplicate_actors.retain(|&id, key| {
            if *key == script {
                ids.push(id);
                false
            } else {
                true
            }
        });
        for id in ids {
            let actor = self.actors.remove(&id);
            let clear_particles = actor
                .as_ref()
                .and_then(|a| a.emitter.as_ref())
                .is_none_or(crate::emitter::Emitter::clear_particles);
            for particle in self.billboards.values_mut() {
                if particle.owner != Some(id) {
                    continue;
                }
                let age = self.tick.saturating_sub(particle.born);
                if clear_particles {
                    // Retire submitted particles after their final presentation.
                    particle.lifetime = particle.lifetime.min(age + 2);
                } else if let crate::effect::OwnerTail::Fade(updates) = particle.owner_tail {
                    particle.rgba[3] = particle.alpha(self.tick) as u8;
                    particle.lifetime = particle.lifetime.min(age + updates);
                    particle.fade = crate::effect::Fade::Proportional;
                }
            }
            for particle in self.refractions.values_mut() {
                if particle.owner != Some(id) {
                    continue;
                }
                if clear_particles {
                    let age = self.tick.saturating_sub(particle.born);
                    particle.lifetime = particle.lifetime.min(age + 2);
                }
            }
            if let Some(mut actor) = actor
                && resources.model(actor.resource).is_some()
            {
                actor.retiring = true;
                actor.emitter = None;
                self.actors.insert(id, actor);
            }
            self.overlays.remove(&id);
            self.emotes.remove(&id);
        }
    }

    /// Controlled actor first, followed by stable simulation order.
    pub fn actor_order(&self) -> &[i32] {
        &self.actor_order
    }
    pub fn insert_actor(&mut self, id: i32, mut actor: Actor) {
        self.next_actor_instance += 1;
        actor.instance = self.next_actor_instance;
        actor.visible_from = self.tick + 1;
        actor.update_order = self.actors.get(&id).map_or_else(
            || {
                (0..=self.actors.len())
                    .find(|order| self.actors.values().all(|a| a.update_order != *order))
                    .unwrap()
            },
            |previous| previous.update_order,
        );
        actor.authored_handle = None;
        if !self.actor_order.contains(&id) {
            self.actor_order.push(id);
        }
        self.actors.insert(id, actor);
    }
    pub(crate) fn sync_actor_order(&mut self) {
        self.actor_order.retain(|id| self.actors.contains_key(id));
        for &id in self.actors.keys() {
            if !self.actor_order.contains(&id) {
                self.actor_order.push(id);
            }
        }
        self.actor_order
            .sort_by_key(|id| self.actors[id].update_order);
        if let Some(index) = self
            .actor_order
            .iter()
            .position(|id| *id == self.controlled_actor)
        {
            self.actor_order[..=index].rotate_right(1);
        }
    }
    pub fn random(&mut self) -> u32 {
        // Scene state owns the random seed, independent of rendering.
        random(&mut self.random_state)
    }
    pub fn blocked_by_movie(&self) -> bool {
        self.movie
            .as_ref()
            .is_some_and(|movie| movie.operation.is_pending())
    }
    pub fn brightness(&self) -> f32 {
        1. - self.fade.as_ref().map_or(255., |f| f.alpha(self.tick)) as u8 as f32 / 255.
    }
    pub fn actor_for_resource(&self, resource: u32) -> Option<&Actor> {
        self.actors.values().find(|a| a.resource == resource)
    }
}

pub(crate) fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
    (*state >> 16) & 0x7fff
}

#[derive(Debug, Clone)]
pub struct Fade {
    pub start_tick: u32,
    pub duration: u32,
    pub from: f32,
    pub to: f32,
    pub white: bool,
}

/// Native transition mode 4 freezes the preceding field image, then reveals
/// the live scene beneath it. Its first presentation retains full opacity.
#[derive(Debug, Clone)]
pub struct SceneDissolve {
    pub start_tick: u32,
    pub duration: u32,
}
impl SceneDissolve {
    pub fn alpha(&self, tick: u32) -> f32 {
        (255. - tick.saturating_sub(self.start_tick) as f32 * 256. / self.duration.max(1) as f32)
            .max(0.)
    }
}
impl Fade {
    pub(crate) fn new(start_tick: u32, duration: u32, from: f32, target: f32, white: bool) -> Self {
        // Fade-out wakes the overlay at opacity one while retaining the rate
        // calculated from its previous opacity toward 256 (display clips at 255).
        let initial = if target > from && from as i32 == 0 {
            1.
        } else {
            from
        };
        Self {
            start_tick,
            duration: if duration == 0 { 10 } else { duration },
            from: initial,
            to: target + (initial - from),
            white,
        }
    }
    /// Commands see the preceding presentation, or an earlier command this update.
    pub(crate) fn before_update(&self, tick: u32) -> f32 {
        if tick <= self.start_tick {
            self.from
        } else {
            self.alpha(tick - 1)
        }
    }
    pub fn alpha(&self, tick: u32) -> f32 {
        if self.duration == 0 {
            return self.to.clamp(0., 255.);
        }
        // The controller advances before drawing, including its creation update.
        let elapsed = tick
            .checked_sub(self.start_tick)
            .map_or(0, |n| n.saturating_add(1));
        (self.from
            + (self.to - self.from) * elapsed.min(self.duration) as f32 / self.duration as f32)
            .clamp(0., 255.)
    }
}
#[derive(Debug, Clone)]
pub struct Overlay {
    pub born: u32,
    pub size: [i32; 2],
    /// Tint and target alpha; `alpha` supplies the displayed alpha.
    pub rgba: [u8; 4],
    pub duration: u32,
    pub kind: OverlayKind,
}
#[derive(Debug, Clone)]
pub enum OverlayKind {
    Sprite(SpriteOverlay),
    LocationCaption { hold_ticks: u32 },
}
#[derive(Debug, Clone)]
pub struct SpriteOverlay {
    pub depth: i32,
    pub image: u8,
    pub scale: [f32; 3],
    /// Alpha units per game tick.
    pub alpha_step: f32,
    alpha: f32,
    drawn_alpha: u8,
}
impl SpriteOverlay {
    pub(crate) fn new(depth: i32, alpha: u8, duration: i32) -> Self {
        let immediate = matches!(duration, 0 | 1);
        Self {
            depth,
            image: 0,
            scale: [1.; 3],
            alpha_step: if immediate {
                0.
            } else {
                f32::from(alpha) / duration as f32
            },
            alpha: if immediate { f32::from(alpha) } else { 0. },
            drawn_alpha: if immediate { alpha } else { 0 },
        }
    }
    pub(crate) fn step(&mut self, target: u8) {
        self.drawn_alpha = self.alpha as u8;
        self.alpha += self.alpha_step;
        if self.alpha_step != 0. {
            if self.alpha >= f32::from(target) {
                self.alpha = f32::from(target);
                self.alpha_step = 0.;
            }
            if self.alpha < 0. {
                self.alpha = 0.;
                self.alpha_step = 0.;
            }
        }
    }
}
impl Overlay {
    pub fn alpha(&self, tick: u32) -> u8 {
        match &self.kind {
            OverlayKind::Sprite(sprite) => sprite.drawn_alpha,
            OverlayKind::LocationCaption { hold_ticks } => {
                let fade = tick.saturating_sub(self.born).saturating_sub(*hold_ticks);
                self.rgba[3].saturating_sub(fade.saturating_mul(4).min(255) as u8)
            }
        }
    }
}

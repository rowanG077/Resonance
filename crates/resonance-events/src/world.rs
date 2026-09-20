use crate::animation::{Animation, slot};
use std::collections::BTreeMap;
#[derive(Debug, Clone)]
pub struct Actor {
    /// Replacing an actor invalidates its retained presentation instance.
    pub instance: u64,
    /// Constructor pose, before subsequent script commands reposition the actor.
    pub creation: Option<ActorCreation>,
    pub resource: u32,
    pub position: [f32; 3],
    pub visible: bool,
    /// A non-rendered scene marker can still own a scenery interaction.
    pub interaction_anchor: bool,
    /// Read-only actors use a strict depth test; ordinary actors test and write
    /// depth, including equal-depth fragments. This is presentation state.
    pub depth_write: bool,
    pub animation: Option<Animation>,
    pub properties: BTreeMap<i32, i32>,
    pub heading: f32,
    pub target_heading: f32,
    pub turn_speed: f32,
    pub appearance: Appearance,
    pub cull_outside_view: bool,
    /// Last actor update's view test; animation and secondary motion share it.
    pub animation_culled: bool,
    pub grounded: bool,
    pub collidable: bool,
    pub casts_shadow: bool,
    pub attachment: Option<Attachment>,
    pub motion: Option<ActorMotion>,
    pub autonomy: Option<crate::Autonomy>,
    pub scripted_animation: bool,
    pub idle_animation: u16,
}
#[derive(Debug, Clone, Copy)]
pub struct ActorCreation {
    pub tick: u32,
    pub position: [f32; 3],
    pub heading: f32,
}
impl Actor {
    pub fn new(resource: u32, position: [f32; 3]) -> Self {
        Self {
            instance: 0,
            creation: None,
            resource,
            position,
            visible: true,
            interaction_anchor: false,
            depth_write: true,
            animation: None,
            properties: BTreeMap::new(),
            heading: 0.,
            target_heading: 0.,
            turn_speed: 5.,
            appearance: Appearance::default(),
            cull_outside_view: true,
            animation_culled: false,
            grounded: true,
            collidable: true,
            casts_shadow: true,
            attachment: None,
            motion: None,
            autonomy: None,
            scripted_animation: false,
            idle_animation: slot::IDLE,
        }
    }
    pub fn face(&mut self, heading: f32) {
        self.heading = heading.rem_euclid(360.);
        self.target_heading = self.heading;
    }
    pub(crate) fn step_heading(&mut self, controlled: bool, moving: bool) {
        // A new direction can cross a full turn. Choose the turn before wrapping
        // the target, then stop when the new heading reaches its angular sector.
        let direction = self.turn_direction();
        let speed = if controlled {
            20.
        } else {
            self.turn_speed * if moving { 2. } else { 1. }
        };
        self.target_heading = self.target_heading.trunc().rem_euclid(360.);
        if speed <= 0. {
            return;
        }
        let current = if self.heading < 0. {
            360. + self.heading
        } else {
            self.heading
        };
        let next = self.heading + direction * speed.min((self.target_heading - current).abs());
        let sectors = (360. / speed) as i32;
        let same_sector = sectors > 0
            && ((360. + next.trunc().rem_euclid(360.)) / speed) as i32 % sectors
                == ((360. + self.target_heading) / speed) as i32 % sectors;
        self.heading = if same_sector {
            self.target_heading
        } else {
            next
        };
    }
    pub(crate) fn turn_direction(&self) -> f32 {
        let delta = self.heading - self.target_heading;
        if delta == 0. {
            return 0.;
        }
        let direction = if delta.abs() >= 180. { 1. } else { -1. };
        if delta < 0. { -direction } else { direction }
    }
    pub(crate) fn step_motion(&mut self) {
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
    pub model_hidden: bool,
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
pub struct BoneAdjustment {
    pub bone: String,
    /// Angles in model coordinates, after the native's integer half-angle conversion.
    pub angles: [f32; 3],
    pub from: [f32; 3],
    pub duration_ticks: u32,
    pub start_tick: u32,
}
impl BoneAdjustment {
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
#[derive(Debug, Clone)]
pub struct Particle {
    pub kind: i32,
    pub handle: i32,
    pub born: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub size: f32,
    pub size_delta: f32,
    pub rgba: [f32; 4],
    pub alpha_delta: f32,
    pub flutter: Option<crate::effect::Flutter>,
}
impl Particle {
    pub fn alive(&self, tick: u32) -> bool {
        tick - self.born <= self.lifetime && self.alpha(tick) >= 0.
    }
    pub fn alpha(&self, tick: u32) -> f32 {
        let age = tick.saturating_sub(self.born);
        if self.flutter.is_some() && self.alpha_delta == 0. {
            // A zero fade rate selects the automatic fade over the final 32 ticks.
            let steps = age.saturating_sub(self.lifetime.saturating_sub(31));
            let maximum = (self.rgba[3] as u8).saturating_sub(1) / 8;
            self.rgba[3] - steps.min(u32::from(maximum)) as f32 * 8.
        } else {
            self.rgba[3] + self.alpha_delta * age as f32
        }
    }
}
#[derive(Default)]
pub struct GameWorld {
    pub tick: u32,
    pub skit: Option<crate::skit::Scene>,
    pub skit_request: Option<crate::skit::Request>,
    pub menu_request: Option<crate::menu::Request>,
    pub actors: BTreeMap<i32, Actor>,
    pub(crate) actor_order: Vec<i32>,
    pub(crate) next_actor_instance: u64,
    pub camera: Option<CameraTrack>,
    pub particles: Vec<Particle>,
    pub fade: Option<Fade>,
    pub overlays: BTreeMap<i32, Overlay>,
    pub effect_settings: BTreeMap<(i32, i32), [i32; 3]>,
    pub character_lights: BTreeMap<i32, crate::effect::CharacterLight>,
    pub render_settings: BTreeMap<i32, i32>,
    pub dialogue: BTreeMap<u8, crate::dialogue::Dialogue>,
    pub choices: BTreeMap<u8, crate::dialogue::Choice>,
    pub party: Option<crate::party::Party>,
    pub field_transition: Option<FieldTransition>,
    pub(crate) field_exit: Option<crate::field_exit::DoorExit>,
    pub preload_field: Option<u32>,
    pub movie: Option<crate::dialogue::Movie>,
    /// The original external-media service also owns spoken dialogue. The
    /// game audio adapter supplies its duration on the gameplay clock.
    pub voice: Option<VoicePlayback>,
    pub field_camera: Option<crate::camera::CameraRig>,
    pub input_enabled: bool,
    pub controlled_actor: i32,
    pub event_flags: std::collections::BTreeSet<u16>,
    pub event_records: BTreeMap<u8, EventRecord>,
    /// Optional Unix time supplied by a replay; live events use the system clock.
    pub calendar_time: Option<i64>,
    pub triggers: Vec<Trigger>,
    pub save_points: Vec<SavePoint>,
    /// Search distance for automatic scenery-door interactions; absent uses 250.
    pub door_interaction_radius: Option<f32>,
    pub audio_commands: Vec<AudioCommand>,
    pub emotes: BTreeMap<i32, Emote>,
    pub paralysis: Option<crate::effect::Paralysis>,
    pub billboards: BTreeMap<i32, crate::effect::BillboardEffect>,
    pub refractions: BTreeMap<i32, crate::effect::RefractionPulse>,
    pub random_state: u32,
    pub gameplay_random: crate::GameplayRandom,
    pub(crate) pending_animation_bindings: std::collections::BTreeSet<i32>,
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
    /// Vertical texture scale of the glow; the circle's geometry stays unchanged.
    pub glow_scale: f32,
}

#[derive(Debug, Clone)]
pub struct FieldTransition {
    pub map: u32,
    pub position: [f32; 3],
    pub heading: f32,
    pub camera: Option<crate::camera::EntryCamera>,
    pub operation: crate::Operation,
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
    pub actor: i32,
    pub kind: u16,
    /// Low five bits of the shared visual random draw at creation.
    pub phase: u8,
    pub offset: [f32; 3],
    pub start_tick: u32,
    pub duration: Option<u32>,
}
#[derive(Debug, Clone)]
pub enum AudioCommand {
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
    Music(i16),
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
}
#[derive(Debug, Clone)]
pub struct Trigger {
    pub key: u32,
    pub shape: TriggerShape,
    /// Vertical extent above the line, not a horizontal activation radius.
    pub height: f32,
    /// Confirmed trigger records contain an interaction indicator followed by
    /// destination/preload hints.
    pub transition: Option<[u32; 3]>,
    /// Touch-trigger action and destination/preload hints, independent of activation.
    pub touch_metadata: [u32; 3],
}
#[derive(Debug, Clone)]
pub enum TriggerShape {
    Line([[f32; 3]; 2]),
    Quad([[f32; 3]; 4]),
}
impl GameWorld {
    /// Controlled actor first, followed by other actors in creation order.
    pub fn actor_order(&self) -> &[i32] {
        &self.actor_order
    }
    pub fn insert_actor(&mut self, id: i32, mut actor: Actor) {
        self.next_actor_instance += 1;
        actor.instance = self.next_actor_instance;
        actor.creation = Some(ActorCreation {
            tick: self.tick,
            position: actor.position,
            heading: actor.appearance.fixed_heading.unwrap_or(actor.heading),
        });
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
            .is_some_and(|movie| movie.blocking && movie.operation.is_pending())
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

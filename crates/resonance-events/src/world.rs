use crate::animation::{Animation, slot};
use std::collections::BTreeMap;
#[derive(Debug, Clone)]
pub struct Actor {
    pub resource: u32,
    pub position: [f32; 3],
    pub visible: bool,
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
    pub grounded: bool,
    pub collidable: bool,
    pub casts_shadow: bool,
    pub attachment: Option<Attachment>,
    pub motion: Option<ActorMotion>,
    pub scripted_animation: bool,
    pub idle_animation: u16,
}
impl Actor {
    pub fn new(resource: u32, position: [f32; 3]) -> Self {
        Self {
            resource,
            position,
            visible: true,
            depth_write: true,
            animation: None,
            properties: BTreeMap::new(),
            heading: 0.,
            target_heading: 0.,
            turn_speed: 5.,
            appearance: Appearance::default(),
            cull_outside_view: true,
            grounded: true,
            collidable: true,
            casts_shadow: true,
            attachment: None,
            motion: None,
            scripted_animation: false,
            idle_animation: slot::IDLE,
        }
    }
    pub fn face(&mut self, heading: f32) {
        self.heading = heading.rem_euclid(360.);
        self.target_heading = self.heading;
    }
    pub(crate) fn step_heading(&mut self, controlled: bool) {
        // Settle model facing to whole degrees without quantizing the movement vector.
        self.target_heading = self.target_heading.trunc().rem_euclid(360.);
        let speed = if controlled {
            20.
        } else {
            self.turn_speed * if self.motion.is_some() { 2. } else { 1. }
        };
        if speed <= 0. || self.heading == self.target_heading {
            return;
        }
        let delta = (self.target_heading - self.heading + 180.).rem_euclid(360.) - 180.;
        let next = self.heading + self.turn_direction() * speed.min(delta.abs());
        // Stop within the target’s angular sector, preserving fractional turn speeds.
        let sectors = (360. / speed) as i32;
        let same_sector = sectors > 0
            && ((360. + next.trunc().rem_euclid(360.)) / speed) as i32 % sectors
                == ((360. + self.target_heading) / speed) as i32 % sectors;
        self.heading = if same_sector || delta.abs() <= speed {
            self.target_heading
        } else {
            next.trunc().rem_euclid(360.)
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
}
impl Particle {
    pub fn alive(&self, tick: u32) -> bool {
        tick - self.born <= self.lifetime && self.alpha(tick) >= 0.
    }
    pub fn alpha(&self, tick: u32) -> f32 {
        self.rgba[3] + self.alpha_delta * (tick - self.born) as f32
    }
}
#[derive(Default)]
pub struct GameWorld {
    pub tick: u32,
    pub actors: BTreeMap<i32, Actor>,
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
    pub triggers: Vec<Trigger>,
    pub audio_commands: Vec<AudioCommand>,
    pub emotes: BTreeMap<i32, Emote>,
    pub billboards: BTreeMap<i32, crate::effect::BillboardEffect>,
    pub random_state: u32,
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
pub struct FieldTransition {
    pub map: u32,
    pub position: [f32; 3],
    pub heading: f32,
    pub operation: crate::Operation,
}

#[derive(Debug, Clone)]
pub struct EventRecord {
    pub value: u8,
    pub extra: u8,
    pub tick: u32,
}
#[derive(Debug, Clone)]
pub struct Emote {
    pub actor: i32,
    pub kind: u16,
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
    pub segment: [[f32; 3]; 2],
    /// Vertical extent above the line, not a horizontal activation radius.
    pub height: f32,
    /// Confirmed trigger records contain an interaction indicator followed by
    /// destination/preload hints.
    pub transition: Option<[u32; 3]>,
}
impl GameWorld {
    pub fn random(&mut self) -> u32 {
        // Scene state owns the random seed, independent of rendering.
        self.random_state = self
            .random_state
            .wrapping_mul(0x41c64e6d)
            .wrapping_add(0x3039);
        (self.random_state >> 16) & 0x7fff
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

#[derive(Debug, Clone)]
pub struct Fade {
    pub start_tick: u32,
    pub duration: u32,
    pub from: f32,
    pub to: f32,
    pub white: bool,
}
impl Fade {
    pub fn alpha(&self, tick: u32) -> f32 {
        self.from
            + (self.to - self.from) * (tick - self.start_tick).min(self.duration) as f32
                / self.duration as f32
    }
}
#[derive(Debug, Clone)]
pub struct Overlay {
    pub size: [i32; 2],
    pub angle: i32,
    pub rgba: [u8; 4],
    pub duration: u32,
    pub mode: i32,
}

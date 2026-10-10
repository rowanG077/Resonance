//! Scene-owned model playback with animation curves shared by the field renderer.
mod player;
pub use player::{CommonPose, ModelRequest, Models, Pose};
mod effect;
pub use effect::{EffectModelDefinition, EffectModelFrame, PreparedEffectModel};
use resonance_content::animation::pose;
mod secondary;
mod weapon;
use crate::{Actor, ActorId};
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Matrix, Motion, Skeleton};
use std::{collections::BTreeMap, sync::Arc};
pub use weapon::{WeaponDefinition, WeaponFrame, WeaponLayerDefinition, WeaponPlayback};

impl Models {
    /// Copies the current primary track without sampling or advancing it.
    /// This can differ from ModelFrame's retained drawing pose. Missing models
    /// have no observation; reading never binds an idle or consumes randomness.
    pub fn main_motion_observation(&self, actor: ActorId) -> Option<MainMotionObservation> {
        let model = self.models.get(actor.index())?.as_ref()?;
        let clock = &model.animation.clock;
        Some(MainMotionObservation {
            clip: model.clip,
            frame: clock.frame,
            end: clock.end,
            loop_start: clock.loop_start,
            rate: clock.rate,
            repeat: clock.repeat,
            stopped: clock.stopped,
            finished: clock.finished,
        })
    }
}

/// A read-only copy of the live selected main track, separate from ModelFrame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainMotionObservation {
    pub clip: u16,
    pub frame: f32,
    pub end: f32,
    pub loop_start: f32,
    pub rate: f32,
    pub repeat: bool,
    pub stopped: bool,
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub struct ModelDefinition {
    pub tint: [u8; 4],
    /// Corpse artwork fades when it has no persistent defeated pose.
    pub fade_on_defeat: bool,
    pub resource: u32,
    /// Skeleton and clips are validated together by the content loader.
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub secondary_motion: Vec<resonance_content::secondary_motion::Chain>,
    pub initial: Playback,
    /// Optional poses selected from native actor state.
    pub reactions: ReactionMotions,
    /// Ordinary idle and optional party low-health idle.
    pub idle_motions: [Option<u16>; 2],
    pub idle_expression: [u8; 4],
    pub blink: Option<resonance_content::battle_profile::Blink>,
    pub weapons: Vec<Arc<WeaponDefinition>>,
    pub shadow: Option<ShadowDefinition>,
    /// Observe root translation independently of drawn-root movement.
    pub suppress_root_translation: [bool; 3],
}

/// Common actor poses resolved by scene playback.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReactionMotions {
    pub jump: Option<u16>,
    pub backstep: Option<u16>,
    pub taunt: Option<u16>,
    pub returning: Option<u16>,
    pub stopping: Option<u16>,
    pub chant: Option<u16>,
    pub cast: Option<u16>,
    pub hurt: [Option<u16>; 2],
    pub guard: [Option<u16>; 2],
    pub stunned: Option<u16>,
    pub rising: Option<u16>,
    pub falling: Option<u16>,
    pub down: Option<u16>,
    pub get_up: Option<u16>,
    pub defeated: Option<u16>,
    pub breakfall: Option<u16>,
    pub airborne: Option<u16>,
    pub landing: Option<u16>,
}

impl ModelDefinition {
    /// Validate the selected pose, secondary chains and attachment slots.
    pub fn validate_bindings(&self) -> Result<()> {
        for chain in &self.secondary_motion {
            chain.validate(self.skeleton.bones.len())?;
        }
        ensure!(
            self.reactions
                .hurt
                .into_iter()
                .chain(self.reactions.guard)
                .chain([
                    self.reactions.jump,
                    self.reactions.backstep,
                    self.reactions.taunt,
                    self.reactions.stunned,
                    self.reactions.rising,
                    self.reactions.falling,
                    self.reactions.down,
                    self.reactions.get_up,
                    self.reactions.defeated,
                    self.reactions.returning,
                    self.reactions.stopping,
                    self.reactions.chant,
                    self.reactions.cast,
                    self.reactions.breakfall,
                    self.reactions.airborne,
                    self.reactions.landing,
                ])
                .chain(self.idle_motions)
                .flatten()
                .all(|clip| self.motions.contains_key(&clip)),
            "missing battle reaction motion"
        );
        let slots: std::collections::BTreeSet<_> = self.weapons.iter().map(|w| w.slot).collect();
        ensure!(
            self.shadow.is_none_or(|s| s.scale.is_finite()),
            "invalid actor shadow scale"
        );
        ensure!(
            slots.len() == self.weapons.len(),
            "duplicate weapon instance slot"
        );
        let play = self.initial;
        let motion = self
            .motions
            .get(&play.clip)
            .context("missing initial battle motion")?;
        Clock::new(play, motion.duration_frames, 0)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ShadowDefinition {
    pub scale: f32,
    pub color: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorShadowFrame {
    pub position: [f32; 3],
    pub radius: f32,
    pub color: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionBinding {
    pub model: u32,
    pub clip: u16,
}

/// Values are native clip frames, independent of action age and simulation time.
#[derive(Debug, Clone, Copy)]
pub struct Playback {
    pub clip: u16,
    pub frame: f32,
    pub rate: f32,
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ModelMaterial {
    #[default]
    Normal,
    RedChannel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelFrame {
    pub actor: ActorId,
    pub visible: bool,
    /// Opaque bodies write depth; translucent bodies do not.
    pub depth_write: bool,
    pub tint: [u8; 4],
    pub material: ModelMaterial,
    /// Expression atlas frame indices for prepared profile channels.
    pub texture_layers: [u8; 4],
    /// Model light position; None uses the prepared stage light.
    pub light: Option<[f32; 3]>,
    pub shadow: Option<ActorShadowFrame>,
    pub resource: u32,
    pub clip: u16,
    pub frame: f32,
    pub blend_weight: f32,
    pub root_translation: [f32; 3],
    pub world: Matrix,
    pub bones: Arc<Vec<Matrix>>,
}

#[derive(Debug, Clone)]
struct Clock {
    frame: f32,
    end: f32,
    loop_start: f32,
    rate: f32,
    repeat: bool,
    stopped: bool,
    finished: bool,
    blend: u8,
    blend_age: u8,
}

impl Clock {
    fn new(play: Playback, end: f32, blend: u8) -> Result<Self> {
        ensure!(
            // At 30 clip frames/second: at most two minutes, with 0.02–8x playback.
            // Zero holds a pose. These bounds keep ordinary f32 addition advancing.
            end > 0.
                && end <= 30. * 120.
                && (0. ..=end).contains(&play.frame)
                && (play.rate == 0. || (0.01..=4.).contains(&play.rate.abs())),
            "invalid battle animation interval or rate"
        );
        Ok(Self {
            frame: play.frame,
            end,
            loop_start: play.frame,
            rate: play.rate,
            repeat: play.repeat,
            stopped: false,
            finished: false,
            blend,
            blend_age: 0,
        })
    }

    fn blending(&self) -> bool {
        self.blend_age < self.blend
    }

    fn step(&mut self) -> f32 {
        let weight = if self.blending() {
            self.blend_age += 1;
            f32::from(self.blend_age) / f32::from(self.blend)
        } else {
            1.
        };
        if !self.stopped {
            let frame = self.frame + self.rate;
            let lower = if self.repeat { self.loop_start } else { 0. };
            let end = self.end;
            self.frame = if frame < (if self.rate < 0. { lower } else { 0. }) || frame > end {
                self.finished = true;
                if self.repeat && lower < end {
                    lower + (frame - lower).rem_euclid(end - lower)
                } else {
                    self.stopped = !self.repeat;
                    frame.clamp(lower, end)
                }
            } else {
                frame
            };
        }
        weight
    }
}

/// A model's animation clock and retained local pose, independent of its owner.
#[derive(Debug, Clone)]
struct AnimatedPose {
    clock: Clock,
    /// Pose visible when this playback replaced its predecessor.
    blend_from: Vec<pose::BonePose>,
    local: Vec<pose::BonePose>,
    pending_sample: bool,
}

impl AnimatedPose {
    fn new(skeleton: &Skeleton, motion: &Motion, play: Playback) -> Result<Self> {
        let mut clock = Clock::new(play, motion.duration_frames, 0)?;
        // Initial phase is not the loop origin (entry randomization supplies it).
        clock.loop_start = 0.;
        let local = pose::sample(skeleton, motion, play.frame)?;
        Ok(Self {
            clock,
            blend_from: local.clone(),
            local,
            pending_sample: false,
        })
    }

    fn start(&mut self, clock: Clock) {
        self.blend_from.clone_from(&self.local);
        self.clock = clock;
        self.pending_sample = true;
    }

    fn advance(&mut self, skeleton: &Skeleton, motion: &Motion) -> Result<f32> {
        let blending = self.clock.blending();
        let weight = self.clock.step();
        self.sample(skeleton, motion, weight, blending)?;
        Ok(weight)
    }

    fn sample_pending(&mut self, skeleton: &Skeleton, motion: &Motion) -> Result<Option<f32>> {
        if !self.pending_sample {
            return Ok(None);
        }
        let blending = self.clock.blending();
        let weight = if blending {
            f32::from(self.clock.blend_age) / f32::from(self.clock.blend)
        } else {
            1.
        };
        self.sample(skeleton, motion, weight, blending)?;
        Ok(Some(weight))
    }

    fn sample(
        &mut self,
        skeleton: &Skeleton,
        motion: &Motion,
        weight: f32,
        blending: bool,
    ) -> Result<()> {
        let mut sampled = pose::sample(skeleton, motion, self.clock.frame)?;
        for (to, previous) in sampled.iter_mut().zip(&self.local) {
            to.retain_unwritten(*previous);
        }
        self.local = sampled;
        if blending {
            for (to, from) in self.local.iter_mut().zip(&self.blend_from) {
                *to = from.mix(*to, weight);
            }
        }
        self.pending_sample = false;
        Ok(())
    }

    fn pose(
        &self,
        skeleton: &Skeleton,
        suppress_root: [bool; 3],
    ) -> Result<(resonance_content::animation::Pose, [f32; 3])> {
        let root = self.local[0].translation();
        let matrices = self
            .local
            .iter()
            .enumerate()
            .map(|(index, &pose)| {
                let mut pose = pose;
                if index == 0 {
                    pose.suppress_translation(suppress_root);
                }
                pose.matrix()
            })
            .collect();
        Ok((skeleton.pose(matrices)?, root))
    }
}

// A short closing/opening cycle every four seconds; stagger actors by a third second.
const BLINK_PERIOD_TICKS: u32 = 240;
const BLINK_FRAME_TICKS: u32 = 2;
const BLINK_STAGGER_TICKS: u32 = 20;

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub definition: Arc<ModelDefinition>,
    clip: u16,
    blink_ticks: u32,
    animation: AnimatedPose,
    secondary: Vec<resonance_content::secondary_motion::Simulation>,
    /// Sampled body pose used by attached artwork.
    pub shown: ModelFrame,
    weapons: Vec<weapon::Weapon>,
    in_results: bool,
}

/// Prepared clock writes for one immediate actor operation. Geometry and poses
/// remain with their existing model; no mutable battle state is copied.
pub(crate) struct PreparedPlay {
    clip: u16,
    clock: Clock,
    weapons: Vec<weapon::PreparedPlay>,
}

impl Model {
    /// Shadows follow the native body footprint, independently of animation.
    pub(crate) fn sample_shadow(&mut self, actor: &Actor) {
        self.shown.shadow = self
            .definition
            .shadow
            .filter(|_| actor.body_top() > 0.)
            .map(|shadow| {
                let altitude = (1. - actor.position[1] / 800.).max(0.25);
                ActorShadowFrame {
                    position: [actor.position[0], 1.1, actor.position[2]],
                    radius: (actor.body_radius() * shadow.scale * altitude).max(10.),
                    color: shadow.color,
                }
            });
    }

    pub fn new(definition: Arc<ModelDefinition>, actor_id: ActorId, actor: &Actor) -> Result<Self> {
        actor.body.validate()?;
        definition.validate_bindings()?;
        let play = definition.initial;
        let motion = &definition.motions[&play.clip];
        let animation = AnimatedPose::new(&definition.skeleton, motion, play)?;
        let mut model = Self {
            blink_ticks: (actor_id.index() as u32 * BLINK_STAGGER_TICKS) % BLINK_PERIOD_TICKS,
            weapons: Vec::new(),
            in_results: false,
            secondary: vec![Default::default(); definition.secondary_motion.len()],
            shown: ModelFrame {
                actor: actor_id,
                visible: true,
                depth_write: true,
                tint: definition.tint,
                material: ModelMaterial::Normal,

                texture_layers: definition.idle_expression,
                light: None,
                shadow: None,
                resource: definition.resource,
                clip: play.clip,
                frame: play.frame,
                blend_weight: 1.,
                root_translation: [0.; 3],
                world: world(actor),
                bones: Arc::new(Vec::new()),
            },
            definition,
            clip: play.clip,
            animation,
        };
        model.step(
            actor,
            actor.availability != crate::ActorAvailability::Petrified,
        )?;
        // Shadows use the native body footprint from the first frame.
        model.sample_shadow(actor);
        model.weapons = model
            .definition
            .weapons
            .iter()
            .map(|definition| weapon::Weapon::new(Arc::clone(definition), &model.shown))
            .collect::<Result<_>>()?;
        Ok(model)
    }

    pub fn play(
        &mut self,
        binding: MotionBinding,
        frame: f32,
        rate: f32,
        repeat: bool,
        blend: u8,
    ) -> Result<()> {
        let prepared = self.prepare_play(binding, frame, rate, repeat, blend)?;
        self.apply_play(prepared);
        Ok(())
    }

    pub(crate) fn prepare_play(
        &self,
        binding: MotionBinding,
        frame: f32,
        rate: f32,
        repeat: bool,
        blend: u8,
    ) -> Result<PreparedPlay> {
        let duration = self.duration(binding)?;
        let playback = Playback {
            clip: binding.clip,
            frame,
            rate,
            repeat,
        };
        Ok(PreparedPlay {
            clip: binding.clip,
            clock: Clock::new(playback, duration, blend)?,
            weapons: self
                .weapons
                .iter()
                .map(|weapon| weapon.prepare_play(playback, duration, blend))
                .collect::<Result<_>>()?,
        })
    }

    pub(crate) fn apply_play(&mut self, prepared: PreparedPlay) {
        self.clip = prepared.clip;
        self.animation.start(prepared.clock);
        for (weapon, prepared) in self.weapons.iter_mut().zip(prepared.weapons) {
            weapon.apply_play(prepared);
        }
    }

    pub fn hurt(&mut self, alternate: bool) -> Result<()> {
        let Some(clip) = self.definition.reactions.hurt[usize::from(alternate)] else {
            return Ok(());
        };
        self.play(
            MotionBinding {
                model: self.definition.resource,
                clip,
            },
            0.,
            0.5,
            false,
            4,
        )
    }

    pub fn duration(&self, binding: MotionBinding) -> Result<f32> {
        ensure!(
            binding.model == self.definition.resource,
            "motion belongs to another battle model"
        );
        Ok(self
            .definition
            .motions
            .get(&binding.clip)
            .context("unprepared battle motion")?
            .duration_frames)
    }

    pub fn is_playing(&self, binding: MotionBinding) -> Result<bool> {
        self.duration(binding)?;
        Ok(self.clip == binding.clip)
    }

    pub(crate) fn render_frame(&self, actor: &Actor) -> ModelFrame {
        let mut frame = self.shown.clone();
        let blink_start = BLINK_PERIOD_TICKS - 3 * BLINK_FRAME_TICKS;
        if actor.available()
            && self.blink_ticks >= blink_start
            && let Some(blink) = &self.definition.blink
            && let Some(expression) = frame.texture_layers.get_mut(usize::from(blink.channel))
            && !blink.excluded_expressions.contains(expression)
        {
            let stage = (self.blink_ticks - blink_start) / BLINK_FRAME_TICKS;
            *expression = match stage {
                1 => blink.frames[1],
                _ => blink.frames[0],
            };
        }
        frame
    }

    pub(crate) fn weapon_frames(&self) -> Vec<WeaponFrame> {
        self.weapons
            .iter()
            .flat_map(|weapon| weapon.frames().cloned())
            .collect()
    }

    pub(crate) fn weapon_visible(&mut self, slot: u8, visible: bool) -> Result<()> {
        let weapon = self
            .weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == slot)
            .context("unprepared weapon visibility slot")?;
        weapon.visible = visible;
        for frame in weapon.frames_mut() {
            frame.visible = self.shown.visible && visible;
        }
        Ok(())
    }

    pub(crate) fn detach_weapon(&mut self, slot: u8, world: Option<Matrix>) {
        if let Some(weapon) = self.weapons.iter_mut().find(|weapon| weapon.slot() == slot) {
            weapon.detach(world);
        }
    }

    pub(crate) fn attach_weapons(&mut self) {
        for weapon in &mut self.weapons {
            weapon.detach(None);
        }
    }

    /// Stage child ownership while the body track stays live.
    pub(crate) fn prepare_weapons(
        &self,
        definitions: &[Arc<WeaponDefinition>],
    ) -> Result<Vec<weapon::Weapon>> {
        let slots: std::collections::BTreeSet<_> =
            definitions.iter().map(|weapon| weapon.slot).collect();
        ensure!(
            slots.len() == definitions.len(),
            "duplicate weapon instance slot"
        );
        let body = Playback {
            clip: self.clip,
            frame: self.animation.clock.frame,
            rate: if self.animation.clock.stopped {
                0.
            } else {
                self.animation.clock.rate
            },
            repeat: self.animation.clock.repeat,
        };
        let duration = self.definition.motions[&self.clip].duration_frames;
        definitions
            .iter()
            .map(|definition| {
                let previous = self
                    .weapons
                    .iter()
                    .find(|weapon| weapon.slot() == definition.slot);
                weapon::Weapon::rebind(
                    Arc::clone(definition),
                    &self.shown,
                    previous,
                    body,
                    duration,
                )
            })
            .collect()
    }

    pub(crate) fn install_weapons(&mut self, weapons: Vec<weapon::Weapon>) {
        self.weapons = weapons;
    }

    pub fn set_loop_start(&mut self, frame: f32) -> Result<()> {
        ensure!(
            frame.is_finite() && (0. ..=self.animation.clock.end).contains(&frame),
            "invalid battle animation loop start"
        );
        self.animation.clock.loop_start = frame;
        Ok(())
    }

    /// Select ongoing feedback from completed native state. Discrete hits and actions
    /// still start their own poses; selecting a state pose never restarts its clock.
    /// Ongoing poses play at 30 frames/second and blend over eight simulation updates.
    pub(crate) fn settle(&mut self, actor: &Actor, activity: crate::Activity) -> Result<()> {
        use crate::Activity;
        if matches!(
            actor.availability,
            crate::ActorAvailability::Absent | crate::ActorAvailability::Petrified
        ) || actor.time_stop != 0
        {
            return Ok(());
        }
        let grounded = !actor.airborne();
        let reaction = self.definition.reactions;
        let (clip, repeat) = match activity {
            Activity::Defeated => (
                if grounded {
                    reaction.defeated.or(reaction.down)
                } else {
                    reaction.falling.or(reaction.hurt[0])
                },
                false,
            ),
            Activity::Guarding => (
                reaction.guard[usize::from(!grounded && !actor.movement.flying)],
                false,
            ),
            Activity::Stunned if grounded => (reaction.stunned, true),
            Activity::KnockedDown if grounded => (reaction.down, false),
            Activity::GettingUp => (reaction.get_up, false),
            Activity::Hurt
                if actor.hit_stop == 0
                    && actor.reaction.recoil.delay == 0
                    && actor.reaction.recoil.kind.launching() =>
            {
                (
                    if actor.movement.vertical > 0. {
                        reaction.rising
                    } else {
                        reaction.falling
                    },
                    false,
                )
            }
            Activity::Idle | Activity::Recovering
                if actor.hit_stop == 0
                    && !actor.needs_landing()
                    && match activity {
                        Activity::Idle => {
                            actor.movement.locomotion == crate::Locomotion::Idle
                                && actor.movement.forward == 0.
                        }
                        _ => self.animation.clock.finished && !self.animation.clock.repeat,
                    } =>
            {
                self.shown.texture_layers = self.definition.idle_expression;
                let injured = actor.side == crate::Side::Party
                    && i64::from(actor.hp) * 4 < i64::from(actor.equipment.max_hp);
                (
                    self.definition.idle_motions[usize::from(injured)]
                        .or(self.definition.idle_motions[0]),
                    true,
                )
            }
            _ => return Ok(()),
        };
        if let Some(clip) = clip
            && (self.clip != clip || self.animation.clock.repeat != repeat)
        {
            self.play(
                MotionBinding {
                    model: self.definition.resource,
                    clip,
                },
                0.,
                0.5,
                repeat,
                8,
            )?;
        }
        Ok(())
    }

    pub fn step(&mut self, actor: &Actor, advance: bool) -> Result<()> {
        if advance && actor.available() {
            self.blink_ticks = (self.blink_ticks + 1) % BLINK_PERIOD_TICKS;
        }
        let motion = &self.definition.motions[&self.clip];
        let weight = if advance {
            Some(self.animation.advance(&self.definition.skeleton, motion)?)
        } else {
            self.animation
                .sample_pending(&self.definition.skeleton, motion)?
        };
        if let Some(weight) = weight {
            self.shown.clip = self.clip;
            self.shown.frame = self.animation.clock.frame;
            self.shown.blend_weight = weight;
        }
        self.present(actor, advance)
    }

    pub(crate) fn sample_tint(&mut self, tint: [u8; 4]) {
        self.shown.tint = tint;
        for weapon in &mut self.weapons {
            for frame in weapon.frames_mut() {
                frame.tint = tint;
            }
        }
    }

    fn present(&mut self, actor: &Actor, advance: bool) -> Result<()> {
        let (mut pose, root) = self.animation.pose(
            &self.definition.skeleton,
            self.definition.suppress_root_translation,
        )?;
        let placement = secondary::Placement::actor(actor);
        let world = placement.world;
        secondary::apply(
            &self.definition.secondary_motion,
            &mut self.secondary,
            &mut pose.global,
            placement,
            advance,
        )?;
        self.shown.root_translation = root;
        self.shown.world = world;
        self.shown.bones = Arc::new(pose.global);
        for weapon in &mut self.weapons {
            weapon.step(&self.shown, advance)?;
        }
        Ok(())
    }
}

fn world(actor: &Actor) -> Matrix {
    let (sin, cos) = actor.heading.to_radians().sin_cos();
    let scale = actor.body.scale;
    // Model Z-up -> battle Y-up, then actor yaw and placement.
    [
        [cos * scale, 0., -sin * scale, 0.],
        [-sin * scale, 0., -cos * scale, 0.],
        [0., scale, 0., 0.],
        [actor.position[0], actor.position[1], actor.position[2], 1.],
    ]
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod player_tests;

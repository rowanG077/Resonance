//! Scene-owned model playback with animation curves shared by the field renderer.
mod player;
pub(crate) use player::UpdatePhase;
pub use player::{CommonPose, ModelRequest, Models, Pose};
mod effect;
mod independent;
pub use effect::{EffectModelDefinition, EffectModelFrame, PreparedEffectModel};
use resonance_content::animation::pose;
mod secondary;
mod weapon;
use crate::{Actor, ActorId};
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Matrix, Motion, Skeleton};
use std::{collections::BTreeMap, sync::Arc};
pub use weapon::{
    WeaponDefinition, WeaponFrame, WeaponLayerDefinition, WeaponPlayback, WeaponTextureAnimation,
};

impl Models {
    pub(crate) fn body_placements(&self) -> Vec<Option<Matrix>> {
        self.models
            .iter()
            .map(|model| model.as_ref().map(|model| model.shown.world))
            .collect()
    }

    /// Separation combines the completed pose with its placement before movement.
    pub(crate) fn body_collision_points(
        &self,
        actor: ActorId,
        placement: Matrix,
    ) -> Option<Vec<([f32; 3], f32)>> {
        let model = self.models.get(actor.index())?.as_ref()?;
        (!model.definition.body_spheres.is_empty())
            .then(|| model.body_points_at(placement).collect())
    }

    pub(crate) fn body_gap(
        &self,
        first_id: ActorId,
        first: &Actor,
        second_id: ActorId,
        second: &Actor,
    ) -> Option<f32> {
        let first_model = self.models.get(first_id.index())?.as_ref()?;
        let second_model = self.models.get(second_id.index())?.as_ref()?;
        if first_model.definition.body_spheres.is_empty()
            || second_model.definition.body_spheres.is_empty()
        {
            return None;
        }
        let mut gap = f32::INFINITY;
        for (first_center, first_radius) in first_model.body_points(first) {
            for (second_center, second_radius) in second_model.body_points(second) {
                gap = gap.min(
                    (first_center[0] - second_center[0]).hypot(first_center[2] - second_center[2])
                        - first_radius
                        - second_radius,
                );
            }
        }
        Some(if gap < 0. { 0.01 } else { gap })
    }

    /// Prepared artwork can be shared by a scene event without borrowing combat playback.
    pub fn definition(&self, actor: ActorId) -> Option<&Arc<ModelDefinition>> {
        self.models
            .get(actor.index())?
            .as_ref()
            .map(|model| &model.definition)
    }

    pub(crate) fn reception_points(
        &self,
        id: ActorId,
        actor: &Actor,
        placement: Option<Matrix>,
    ) -> Option<impl Iterator<Item = crate::geometry::HurtPoint> + '_> {
        let model = self.models.get(id.index())?.as_ref()?;
        let spheres = model.definition.reception_spheres.as_ref()?;
        let world = placement.unwrap_or_else(|| world(actor));
        let scale = actor.body.scale;
        Some(spheres.iter().map(move |sphere| {
            let bone = model.shown.bones[usize::from(sphere.bone)];
            crate::geometry::HurtPoint {
                center: resonance_content::animation::transform_point(
                    world,
                    [bone[3][0], bone[3][1], bone[3][2]],
                ),
                radius: sphere.radius / scale,
            }
        }))
    }

    pub(crate) fn anchor(&self, actor: ActorId, anchor: crate::MeleeAnchor) -> Result<[f32; 3]> {
        let model = self
            .models
            .get(actor.index())
            .and_then(Option::as_ref)
            .context("missing melee model")?;
        let (world, bones) = if anchor.resource == model.definition.resource {
            (&model.shown.world, &model.shown.bones)
        } else {
            let frame = model
                .weapons
                .iter()
                .flat_map(|weapon| weapon.frames())
                .find(|frame| frame.resource == anchor.resource)
                .context("missing melee weapon")?;
            (&frame.world, &frame.bones)
        };
        let bone = bones
            .get(usize::from(anchor.bone))
            .context("missing melee bone")?;
        let matrix = resonance_content::animation::multiply(*world, *bone);
        Ok([matrix[3][0], matrix[3][1], matrix[3][2]])
    }
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
    /// Authored body spheres, bound to this skeleton during preparation.
    pub body_spheres: Vec<BodySphere>,
    /// Authored reception markers. None retains the collider for unprepared bodies.
    pub reception_spheres: Option<Vec<BodySphere>>,
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

#[derive(Debug, Clone, Copy)]
pub struct BodySphere {
    pub bone: u16,
    /// Prepared battle-space radius; model scale applies to the bone position.
    pub radius: f32,
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
    /// Simulation updates held while blending into a rising hit reaction.
    pub rising_blend: u8,
    pub falling: Option<u16>,
    /// Simulation updates held while blending into a falling hit reaction.
    pub falling_blend: u8,
    pub down: Option<u16>,
    pub get_up: Option<u16>,
    pub defeated: Option<u16>,
    pub breakfall: Option<u16>,
    pub airborne: Option<u16>,
    pub landing: Option<u16>,
}

impl ModelDefinition {
    /// Bind authored contacts once; runtime contacts sample these exact bones.
    pub fn contact_anchors(
        &self,
        attachments: &resonance_content::battle_action::ContactAttachments,
    ) -> Result<Vec<crate::MeleeAnchor>> {
        use resonance_content::battle_action::ContactAttachments;
        let groups = match attachments {
            ContactAttachments::BodyBone(bone) => {
                ensure!(
                    usize::from(*bone) < self.skeleton.bones.len(),
                    "missing melee body bone {bone}"
                );
                return Ok(vec![crate::MeleeAnchor {
                    resource: self.resource,
                    bone: *bone,
                }]);
            }
            ContactAttachments::Groups(groups) => groups,
            ContactAttachments::BodyCenter {} => {
                anyhow::bail!("body-centered contact has no model attachment")
            }
        };
        let mut anchors = Vec::new();
        for &group in groups {
            let selected = self.contact_group_anchors(group);
            ensure!(
                !selected.is_empty(),
                "missing melee attachment group {group}"
            );
            anchors.extend(selected);
        }
        Ok(anchors)
    }

    /// Find the body's and carried weapons' markers for one contact group.
    pub fn contact_group_anchors(&self, group: u8) -> Vec<crate::MeleeAnchor> {
        let mut anchors = Vec::new();
        let mut collect = |resource, skeleton: &Skeleton, digit: usize, primary_group: u8| {
            for (bone, source) in skeleton.bones.iter().enumerate() {
                let name = source.name.as_bytes();
                if name.get(..2).is_some_and(|s| s.eq_ignore_ascii_case(b"at"))
                    && name
                        .get(digit)
                        .and_then(|c| c.checked_sub(b'0'))
                        .map(|marker| if marker == 0 { primary_group } else { marker })
                        == Some(group)
                {
                    anchors.push(crate::MeleeAnchor {
                        resource,
                        bone: bone as u16,
                    });
                }
            }
        };
        collect(self.resource, &self.skeleton, 3, 0);
        for weapon in &self.weapons {
            // Contact markers belong to the primary layer, not its outline.
            // Its primary marker group belongs to the equipped weapon slot.
            let primary = &weapon.layers[0];
            collect(primary.resource, &primary.skeleton, 2, weapon.slot);
        }
        anchors
    }
    /// Validate the selected pose, secondary chains and attachment slots.
    pub fn validate_bindings(&self) -> Result<()> {
        ensure!(
            self.body_spheres
                .iter()
                .chain(self.reception_spheres.iter().flatten())
                .all(|sphere| {
                    usize::from(sphere.bone) < self.skeleton.bones.len()
                        && sphere.radius.is_finite()
                        && sphere.radius >= 0.
                }),
            "invalid battle body sphere"
        );
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

impl crate::Battle {
    pub(crate) fn body_gap(&self, first: ActorId, second: ActorId) -> f32 {
        let first_actor = &self.actors[first.index()];
        let second_actor = &self.actors[second.index()];
        self.models
            .as_ref()
            .and_then(|models| models.body_gap(first, first_actor, second, second_actor))
            .unwrap_or_else(|| crate::control::capsule_gap(first_actor, second_actor))
    }

    pub fn set_models(&mut self, models: Models) {
        self.models = Some(models);
    }

    pub fn models(&self) -> Option<&Models> {
        self.models.as_ref()
    }

    /// Contact sampling and drawing share one player and one advance per update.
    pub fn advance_models(
        &mut self,
        frame: &mut crate::BattleFrame,
        clock: crate::BattleClock,
        settle: bool,
    ) -> Result<()> {
        let release_actor = if clock == crate::BattleClock::SpellTransition {
            self.volleys
                .values()
                .find_map(|volley| volley.advancing_cast_pose(self))
        } else {
            None
        };
        if let Some(models) = &mut self.models {
            models.advance_with_release(frame, clock, settle, release_actor)?;
        }
        for model in &mut frame.models {
            model.visible &= !self.capture_hidden(model.actor);
        }
        for weapon in &mut frame.weapons {
            weapon.visible &= !self.capture_hidden(weapon.owner)
                && (weapon.slot >= 2 || !self.cast_hides_primary_weapons(weapon.owner));
        }
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
    /// Authored scale applied to the rendered body's animated footprint.
    pub scale: f32,
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
        if self.blending() {
            self.blend_age += 1;
            return f32::from(self.blend_age) / (f32::from(self.blend) + 1.);
        }
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
        1.
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
        // A newly selected clip presents its start frame before its clock runs.
        if let Some(weight) = self.sample_pending(skeleton, motion)? {
            return Ok(weight);
        }
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
        if blending {
            for (index, (to, from)) in sampled.iter_mut().zip(&self.blend_from).enumerate() {
                *to = if motion
                    .tracks
                    .iter()
                    .any(|track| usize::from(track.bone) == index)
                {
                    pose::BonePose::complete(from.value().mix(to.value(), weight))
                } else {
                    self.local[index]
                };
            }
        } else {
            for (to, previous) in sampled.iter_mut().zip(&self.local) {
                to.retain_unwritten(*previous);
            }
        }
        self.local = sampled;
        self.pending_sample = false;
        Ok(())
    }

    fn pose(
        &self,
        skeleton: &Skeleton,
        suppress_root: [bool; 3],
    ) -> Result<(resonance_content::animation::Pose, [f32; 3])> {
        Self::pose_from(&self.local, skeleton, suppress_root)
    }

    fn pose_from(
        local: &[pose::BonePose],
        skeleton: &Skeleton,
        suppress_root: [bool; 3],
    ) -> Result<(resonance_content::animation::Pose, [f32; 3])> {
        let root = local[0].translation();
        let matrices: Vec<_> = local
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
        Ok((
            skeleton.pose_with_matrices(
                skeleton.bones.iter().map(|bone| bone.bind).collect(),
                &matrices,
            )?,
            root,
        ))
    }
}

// A short closing/opening cycle every four seconds; stagger actors by a third second.
const BLINK_PERIOD_TICKS: u32 = 240;
const BLINK_FRAME_TICKS: u32 = 2;
const BLINK_STAGGER_TICKS: u32 = 20;

#[derive(Debug, Clone)]
struct PosePart {
    clip: u16,
    bones: Vec<u16>,
    animation: AnimatedPose,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub definition: Arc<ModelDefinition>,
    clip: u16,
    blink_ticks: u32,
    animation: AnimatedPose,
    parts: Vec<PosePart>,
    bone_scales: Vec<(u16, [f32; 3])>,
    secondary: Vec<resonance_content::secondary_motion::Simulation>,
    /// Sampled body pose used by attached artwork.
    pub shown: ModelFrame,
    weapons: Vec<weapon::Weapon>,
    in_results: bool,
    tint_flash: Option<[u8; 3]>,
}

/// Prepared clock writes for one immediate actor operation. Geometry and poses
/// remain with their existing model; no mutable battle state is copied.
pub(crate) struct PreparedPlay {
    clip: u16,
    clock: Clock,
    weapons: Vec<weapon::PreparedPlay>,
}

impl Model {
    fn body_points(&self, actor: &Actor) -> impl Iterator<Item = ([f32; 3], f32)> + '_ {
        self.body_points_at(world(actor))
    }

    fn body_points_at(&self, world: Matrix) -> impl Iterator<Item = ([f32; 3], f32)> + '_ {
        self.definition.body_spheres.iter().map(move |sphere| {
            let bone = self.shown.bones[usize::from(sphere.bone)];
            (
                resonance_content::animation::transform_point(
                    world,
                    [bone[3][0], bone[3][1], bone[3][2]],
                ),
                sphere.radius,
            )
        })
    }

    /// Retain shadow appearance; the renderer supplies the drawn body's footprint.
    pub(crate) fn sample_shadow(&mut self, actor: &Actor) {
        self.shown.shadow = self
            .definition
            .shadow
            .filter(|_| actor.body_top() > 0.)
            .map(|shadow| {
                let altitude = (1. - actor.position[1] / 800.).max(0.25);
                ActorShadowFrame {
                    position: [actor.position[0], 1.1, actor.position[2]],
                    scale: shadow.scale * altitude,
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
            tint_flash: None,
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
            parts: Vec::new(),
            bone_scales: Vec::new(),
        };
        model.step(
            actor,
            actor.availability != crate::ActorAvailability::Petrified,
        )?;
        // Shadow appearance is available with the first drawn pose.
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
        if !self.parts.is_empty() {
            self.animation.local = self.composed_local();
            self.parts.clear();
        }
        self.clip = prepared.clip;
        self.animation.start(prepared.clock);
        for (weapon, prepared) in self.weapons.iter_mut().zip(prepared.weapons) {
            weapon.apply_play(prepared);
        }
    }

    fn play_part(&mut self, binding: MotionBinding, pose: Pose, bones: Vec<u16>) -> Result<()> {
        ensure!(
            !bones.is_empty()
                && bones.windows(2).all(|pair| pair[0] < pair[1])
                && bones
                    .iter()
                    .all(|&bone| usize::from(bone) < self.definition.skeleton.bones.len()),
            "invalid partial-pose bone mask"
        );
        let duration = self.duration(binding)?;
        let end = pose.end.unwrap_or(duration);
        ensure!(end <= duration, "partial-pose end exceeds motion");
        let mut clock = Clock::new(
            Playback {
                clip: binding.clip,
                frame: pose.frame,
                rate: pose.rate,
                repeat: pose.repeat,
            },
            end,
            pose.blend,
        )?;
        clock.loop_start = pose.loop_start.max(pose.frame);
        ensure!(
            clock.loop_start <= end,
            "partial-pose loop start exceeds end"
        );
        let local = self.composed_local();
        let part = PosePart {
            clip: binding.clip,
            bones,
            animation: AnimatedPose {
                clock,
                blend_from: local.clone(),
                local,
                pending_sample: true,
            },
        };
        if let Some(previous) = self
            .parts
            .iter_mut()
            .find(|previous| previous.bones == part.bones)
        {
            *previous = part;
        } else {
            self.parts.push(part);
        }
        Ok(())
    }

    fn composed_local(&self) -> Vec<pose::BonePose> {
        let mut local = self.animation.local.clone();
        // Earlier prepared parts have precedence on overlapping bones.
        for part in self.parts.iter().rev() {
            for &bone in &part.bones {
                local[usize::from(bone)] = part.animation.local[usize::from(bone)];
            }
        }
        for &(bone, scale) in &self.bone_scales {
            let value = &mut local[usize::from(bone)];
            *value = pose::BonePose::complete(value.value().with_scale(scale));
        }
        local
    }

    pub(crate) fn set_bone_scales(&mut self, scales: Vec<(u16, [f32; 3])>) -> Result<()> {
        ensure!(
            scales.iter().all(|(bone, scale)| {
                usize::from(*bone) < self.definition.skeleton.bones.len()
                    && scale.iter().all(|value| value.is_finite() && *value >= 0.)
            }),
            "invalid persistent bone scale"
        );
        self.bone_scales = scales;
        Ok(())
    }

    pub(crate) fn weapon_animation_rate(&mut self, slot: u8, rate: f32) -> Result<()> {
        self.weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == slot)
            .context("unprepared carried animation slot")?
            .set_animation_rate(rate)
    }

    pub(crate) fn play_carried_pose(
        &mut self,
        slot: u8,
        clip: u16,
        rate: f32,
        repeat: bool,
    ) -> Result<()> {
        self.weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == slot)
            .context("unprepared carried animation slot")?
            .play_carried_pose(clip, rate, repeat)
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
        self.prepare_weapons_from(definitions, &self.weapons)
    }

    fn prepare_weapons_from(
        &self,
        definitions: &[Arc<WeaponDefinition>],
        previous: &[weapon::Weapon],
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
                let previous = previous
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

    pub fn set_rate(&mut self, rate: f32) -> Result<()> {
        ensure!(
            rate == 0. || (0.01..=4.).contains(&rate.abs()),
            "invalid battle animation rate"
        );
        self.animation.clock.rate = rate;
        Ok(())
    }

    pub fn set_end(&mut self, end: f32) -> Result<()> {
        let clock = &mut self.animation.clock;
        ensure!(
            end.is_finite() && end >= clock.frame && end <= clock.end,
            "invalid battle animation end"
        );
        clock.end = end;
        Ok(())
    }

    /// Select ongoing feedback from completed native state. Discrete hits and actions
    /// still start their own poses; selecting a state pose never restarts its clock.
    /// Ongoing poses play at 30 frames/second; launch reactions use prepared blends.
    pub(crate) fn settle(
        &mut self,
        actor: &Actor,
        activity: crate::Activity,
        advance_outgoing: &mut bool,
    ) -> Result<()> {
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
        let mut blend = 8;
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
                        blend = reaction.rising_blend;
                        reaction.rising
                    } else {
                        blend = reaction.falling_blend;
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
                        _ => {
                            reaction.cast != Some(self.clip)
                                && self.animation.clock.finished
                                && !self.animation.clock.repeat
                        }
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
            self.advance_outgoing(advance_outgoing)?;
            self.play(
                MotionBinding {
                    model: self.definition.resource,
                    clip,
                },
                0.,
                0.5,
                repeat,
                blend,
            )?;
        }
        Ok(())
    }

    pub fn step(&mut self, actor: &Actor, advance: bool) -> Result<()> {
        self.step_with_clocks(actor, advance, advance)
    }

    pub(crate) fn step_with_clocks(
        &mut self,
        actor: &Actor,
        advance: bool,
        carried_advance: bool,
    ) -> Result<()> {
        self.step_with_phases(actor, advance, advance, carried_advance)
    }

    fn step_with_phases(
        &mut self,
        actor: &Actor,
        advance_pose: bool,
        advance_presentation: bool,
        carried_advance: bool,
    ) -> Result<()> {
        if advance_presentation && actor.available() {
            self.blink_ticks = (self.blink_ticks + 1) % BLINK_PERIOD_TICKS;
        }
        self.sample_pose(advance_pose)?;
        self.present(actor, advance_presentation, carried_advance)
    }

    fn sample_for_geometry(&mut self, actor: &Actor, advance: bool) -> Result<()> {
        // Secondary chains retain their history until final placement is known.
        self.sample_pose(advance)?;
        self.present_body(actor, None)
    }

    fn sample_pose(&mut self, advance: bool) -> Result<()> {
        self.sample_main(advance)?;
        for part in &mut self.parts {
            let motion = &self.definition.motions[&part.clip];
            if advance {
                part.animation.advance(&self.definition.skeleton, motion)?;
            } else {
                part.animation
                    .sample_pending(&self.definition.skeleton, motion)?;
            }
        }
        Ok(())
    }

    /// A replacement captures this update's outgoing pose before its start sample.
    /// The caller shares one allowance across requests and respects paused clocks.
    fn advance_outgoing(&mut self, advance: &mut bool) -> Result<()> {
        if std::mem::take(advance) {
            self.sample_main(true)?;
        }
        Ok(())
    }

    fn sample_main(&mut self, advance: bool) -> Result<()> {
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
        Ok(())
    }

    pub(crate) fn sample_tint(&mut self, tint: [u8; 4]) {
        self.shown.tint = tint;
        for weapon in &mut self.weapons {
            for frame in weapon.frames_mut() {
                frame.tint = tint;
            }
        }
    }

    fn present(&mut self, actor: &Actor, advance: bool, carried_advance: bool) -> Result<()> {
        self.present_body(actor, Some(advance))?;
        for weapon in &mut self.weapons {
            weapon.step_with_clocks(&self.shown, advance, carried_advance)?;
        }
        Ok(())
    }

    fn present_body(&mut self, actor: &Actor, secondary_advance: Option<bool>) -> Result<()> {
        let (mut pose, root) = if self.parts.is_empty() && self.bone_scales.is_empty() {
            self.animation.pose(
                &self.definition.skeleton,
                self.definition.suppress_root_translation,
            )?
        } else {
            AnimatedPose::pose_from(
                &self.composed_local(),
                &self.definition.skeleton,
                self.definition.suppress_root_translation,
            )?
        };
        let mut placement = secondary::Placement::actor(actor);
        for (axis, offset) in actor.reaction.hit_shake_offset().into_iter().enumerate() {
            placement.world[3][axis] += offset;
        }
        let world = placement.world;
        if let Some(advance) = secondary_advance {
            secondary::apply(
                &self.definition.secondary_motion,
                &mut self.secondary,
                &mut pose.global,
                placement,
                advance,
            )?;
        }
        self.shown.root_translation = root;
        self.shown.world = world;
        self.shown.bones = Arc::new(pose.global);
        Ok(())
    }
}

/// A pose whose clock belongs to a scene event rather than combat.
#[derive(Debug, Clone, Copy)]
pub struct ModelSample {
    pub clip: u16,
    pub frame: f32,
    pub blend_weight: f32,
    pub restart: bool,
    pub rate: f32,
    pub repeat: bool,
    pub blend_ticks: u8,
}

/// Independent presentation playback using the same poses and attachments as combat.
/// Sampling never changes the live battle's actor or animation state.
pub struct ModelSampler(Model);
impl ModelSampler {
    pub fn new(definition: Arc<ModelDefinition>, id: ActorId, actor: &Actor) -> Result<Self> {
        Ok(Self(Model::new(definition, id, actor)?))
    }

    pub fn sample(
        &mut self,
        actor: &Actor,
        sample: ModelSample,
        advance: bool,
    ) -> Result<(ModelFrame, Vec<WeaponFrame>)> {
        let model = &mut self.0;
        let motion = model
            .definition
            .motions
            .get(&sample.clip)
            .context("missing scene actor motion")?;
        ensure!(
            sample.frame.is_finite()
                && (0. ..=motion.duration_frames).contains(&sample.frame)
                && (0. ..=1.).contains(&sample.blend_weight),
            "invalid scene actor sample"
        );
        if sample.restart {
            model.play(
                MotionBinding {
                    model: model.definition.resource,
                    clip: sample.clip,
                },
                sample.frame,
                sample.rate,
                sample.repeat,
                sample.blend_ticks,
            )?;
        }
        let motion = &model.definition.motions[&sample.clip];
        model.clip = sample.clip;
        model.animation.clock.frame = sample.frame;
        model.animation.sample(
            &model.definition.skeleton,
            motion,
            sample.blend_weight,
            sample.blend_weight < 1.,
        )?;
        model.shown.clip = sample.clip;
        model.shown.frame = sample.frame;
        model.shown.blend_weight = sample.blend_weight;
        model.present(actor, advance, advance)?;
        model.sample_shadow(actor);
        Ok((model.shown.clone(), model.weapon_frames()))
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

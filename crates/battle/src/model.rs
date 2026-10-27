//! Scene-owned model playback with animation curves shared by the field renderer.
mod player;
pub(crate) use player::UpdatePhase;
pub use player::{CommonPose, ModelRequest, Models, Pose};
mod effect;
pub use effect::{EffectModelDefinition, EffectModelFrame, PreparedEffectModel};
use resonance_content::animation::pose;
mod secondary;
mod weapon;
use crate::{Actor, ActorId, ResourceId};
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Matrix, Motion, Skeleton};
use std::{collections::BTreeMap, sync::Arc};
pub use weapon::{
    WeaponDefinition, WeaponFrame, WeaponLayerDefinition, WeaponPlayback, WeaponSlot,
    WeaponTextureAnimation,
};

impl Models {
    pub(crate) fn motion_duration(&self, actor: ActorId, clip: u16) -> Option<f32> {
        let model = self.models.get(actor.index())?.as_ref()?;
        model
            .definition
            .motion(model.resolve_clip(clip)?)
            .ok()
            .map(|motion| motion.duration_frames)
    }

    pub(crate) fn release_hosted_body(
        &mut self,
        actor: ActorId,
        hurt: [Option<MotionBinding>; 2],
    ) -> Result<()> {
        let model = self
            .models
            .get_mut(actor.index())
            .and_then(Option::as_mut)
            .context("released participant has no model")?;
        for binding in hurt.into_iter().flatten() {
            model.duration(binding)?;
        }
        model.released_hurt = Some(hurt.map(|binding| binding.map(|binding| binding.clip)));
        model.world_override = None;
        Ok(())
    }

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

    fn approach_gap(
        &self,
        first_id: ActorId,
        first: &Actor,
        second_id: ActorId,
        second: &Actor,
    ) -> Option<f32> {
        if !self.approach_geometry_active {
            return self.body_gap(first_id, first, second_id, second);
        }
        let first_model = self.models.get(first_id.index())?.as_ref()?;
        let second_model = self.models.get(second_id.index())?.as_ref()?;
        if first_model.definition.body_spheres.is_empty()
            || second_model.definition.body_spheres.is_empty()
        {
            return None;
        }
        let mut gap = f32::INFINITY;
        for (&a, sphere) in self.approach_centers[first_id.index()]
            .iter()
            .zip(&first_model.definition.body_spheres)
        {
            for (&b, other) in self.approach_centers[second_id.index()]
                .iter()
                .zip(&second_model.definition.body_spheres)
            {
                gap = gap.min((a[0] - b[0]).hypot(a[2] - b[2]) - sphere.radius - other.radius);
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
        self.anchor_at(actor, anchor, None)
    }

    pub(crate) fn anchor_at(
        &self,
        actor: ActorId,
        anchor: crate::MeleeAnchor,
        body_placement: Option<Matrix>,
    ) -> Result<[f32; 3]> {
        let model = self
            .models
            .get(actor.index())
            .and_then(Option::as_ref)
            .context("missing melee model")?;
        let (world, bones, bone) = if anchor.resource == model.definition.resource {
            (
                body_placement.unwrap_or(model.shown.world),
                &model.shown.bones,
                usize::from(anchor.bone),
            )
        } else {
            let original = model
                .definition
                .weapons
                .iter()
                .find(|weapon| weapon.primary_resource() == anchor.resource);
            // A spare can move into another equipped slot. Prepared contact
            // markers retain that slot while its geometry and bone order change.
            let replacement = original.and_then(|original| {
                model.weapons.iter().find(|weapon| {
                    weapon.slot() == original.slot
                        && weapon.definition.primary_resource() != anchor.resource
                })
            });
            let (resource, bone) = if let Some(replacement) = replacement {
                let original_bones = &original.unwrap().layers[0].skeleton.bones;
                let selected = original_bones
                    .get(usize::from(anchor.bone))
                    .context("missing replaced melee bone")?;
                let replacement_bones = &replacement.definition.layers[0].skeleton.bones;
                let marker_group = |name: &str| {
                    let name = name.as_bytes();
                    if name
                        .get(..2)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"at"))
                    {
                        name.get(2).copied().filter(u8::is_ascii_digit)
                    } else {
                        None
                    }
                };
                let bone = if let Some(group) = marker_group(&selected.name) {
                    // Contact groups enumerate markers in skeleton order; their names
                    // may differ, or repeat, across equipped weapons.
                    let ordinal = original_bones[..usize::from(anchor.bone)]
                        .iter()
                        .filter(|bone| marker_group(&bone.name) == Some(group))
                        .count();
                    replacement_bones
                        .iter()
                        .enumerate()
                        .filter(|(_, bone)| marker_group(&bone.name) == Some(group))
                        .nth(ordinal)
                        .map(|(bone, _)| bone)
                } else {
                    replacement_bones
                        .iter()
                        .position(|bone| bone.name == selected.name)
                }
                .context("replacement weapon has no matching melee bone")?;
                (replacement.definition.primary_resource(), bone)
            } else {
                (anchor.resource, usize::from(anchor.bone))
            };
            let (weapon, frame) = model
                .weapons
                .iter()
                .find_map(|weapon| {
                    weapon
                        .frames()
                        .find(|frame| frame.resource == resource)
                        .map(|frame| (weapon, frame))
                })
                .context("missing melee weapon")?;
            let world = body_placement
                .filter(|_| weapon.enabled)
                .map_or(frame.world, |placement| {
                    weapon.world_at(&model.shown, placement)
                });
            (world, &frame.bones, bone)
        };
        let bone = bones.get(bone).context("missing melee bone")?;
        let matrix = resonance_content::animation::multiply(world, *bone);
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

    /// Read active subtree tracks without advancing or sampling them.
    #[cfg(feature = "test-support")]
    pub fn partial_motion_observations(
        &self,
        actor: ActorId,
    ) -> impl Iterator<Item = (u16, u16, f32)> + '_ {
        self.models
            .get(actor.index())
            .and_then(Option::as_ref)
            .into_iter()
            .flat_map(|model| model.parts.iter())
            .map(|part| (part.bones[0], part.clip, part.animation.clock.frame))
    }

    #[cfg(test)]
    pub(crate) fn motion_blending(&self, actor: ActorId) -> bool {
        self.models
            .get(actor.index())
            .and_then(Option::as_ref)
            .is_some_and(|model| model.animation.clock.blending())
    }

    /// Locomotion clocks use readiness after this update's pose visit.
    pub(crate) fn motion_ready_this_update(&self, actor: ActorId) -> bool {
        self.models
            .get(actor.index())
            .and_then(Option::as_ref)
            .is_none_or(|model| {
                let clock = &model.animation.clock;
                clock.blend_age.saturating_add(1) >= clock.blend
            })
    }

    pub(crate) fn down_pose_ready(&self, actor: ActorId) -> bool {
        self.models
            .get(actor.index())
            .and_then(Option::as_ref)
            .is_none_or(|model| {
                model.definition.reactions.down == Some(model.clip)
                    && !model.animation.clock.blending()
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
    pub resource: ResourceId,
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
    pub active_expression: [u8; 4],
    /// Story-selected eyes take precedence over expressions and blinking.
    pub forced_eye_frame: Option<u8>,
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
    pub walk: Option<u16>,
    pub run: Option<u16>,
    pub jump: Option<u16>,
    pub backstep: Option<u16>,
    pub taunt: Option<u16>,
    pub returning: Option<u16>,
    pub stopping: Option<u16>,
    pub chant: Option<u16>,
    pub chant_once: bool,
    pub chant_rate: f32,
    pub chant_loop_start: f32,
    pub cast: Option<u16>,
    pub cast_recovery: Option<u16>,
    pub hurt: [Option<u16>; 2],
    pub guard: [Option<u16>; 2],
    pub stunned: Option<u16>,
    pub rising: Option<u16>,
    /// Simulation updates held while blending into a rising hit reaction.
    pub rising_blend: u8,
    pub falling: Option<u16>,
    /// Simulation updates held while blending into a falling hit reaction.
    pub falling_blend: u8,
    pub stagger_down: Option<u16>,
    pub stagger_get_up: Option<u16>,
    pub down: Option<u16>,
    pub get_up: Option<u16>,
    pub defeated: Option<u16>,
    pub breakfall: Option<u16>,
    pub airborne: Option<u16>,
    pub landing: Option<u16>,
}

impl ModelDefinition {
    fn motion(&self, clip: u16) -> Result<&Motion> {
        self.motions.get(&clip).context("unprepared battle motion")
    }

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
        self.contact_group_anchors_with_weapons(group, &self.weapons)
    }

    /// Resolve the same markers for a prepared replacement weapon group.
    pub fn contact_group_anchors_with_weapons(
        &self,
        group: u8,
        weapons: &[Arc<WeaponDefinition>],
    ) -> Vec<crate::MeleeAnchor> {
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
        for weapon in weapons {
            let Some(slot) = weapon.slot else {
                continue;
            };
            // Contact markers belong to the primary layer, not its outline.
            // Its primary marker group belongs to the equipped weapon slot.
            let primary = &weapon.layers[0];
            collect(primary.resource, &primary.skeleton, 2, slot.0);
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
                    self.reactions.walk,
                    self.reactions.run,
                    self.reactions.jump,
                    self.reactions.backstep,
                    self.reactions.taunt,
                    self.reactions.stunned,
                    self.reactions.rising,
                    self.reactions.falling,
                    self.reactions.stagger_down,
                    self.reactions.stagger_get_up,
                    self.reactions.down,
                    self.reactions.get_up,
                    self.reactions.defeated,
                    self.reactions.returning,
                    self.reactions.stopping,
                    self.reactions.chant,
                    self.reactions.cast,
                    self.reactions.cast_recovery,
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

    pub(crate) fn approach_body_gap(&self, first: ActorId, second: ActorId) -> f32 {
        let first_actor = &self.actors[first.index()];
        let second_actor = &self.actors[second.index()];
        self.models
            .as_ref()
            .and_then(|models| models.approach_gap(first, first_actor, second, second_actor))
            .unwrap_or_else(|| crate::control::capsule_gap(first_actor, second_actor))
    }

    pub fn set_models(&mut self, mut models: Models) {
        for (index, setup) in self.prepared.actor_setup.iter().enumerate() {
            if matches!(
                setup
                    .enemy_decision
                    .as_ref()
                    .and_then(|decision| decision.behavior.as_ref()),
                Some(crate::EnemyBehavior::ActionWeapons {
                    idle_group: None,
                    ..
                })
            ) && let Some(model) = models.models[index].as_mut()
            {
                model.primary_weapons_enabled(false);
            }
        }
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
            models.advance_with_release(
                frame,
                clock,
                settle,
                release_actor,
                &self.prepared.hosted_participants,
            )?;
        }
        for model in &mut frame.models {
            model.visible &= !self.capture_hidden(model.actor);
        }
        for weapon in &mut frame.weapons {
            weapon.visible &= !self.capture_hidden(weapon.owner)
                && (!weapon.slot.is_some_and(WeaponSlot::is_primary)
                    || !self.cast_hides_primary_weapons(weapon.owner));
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
    pub model: ResourceId,
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
    /// Defeat fades stop writing depth; authored body opacity does not.
    pub depth_write: bool,
    pub tint: [u8; 4],
    pub material: ModelMaterial,
    /// Expression atlas frame indices for prepared profile channels.
    pub texture_layers: [u8; 4],
    /// Elapsed body presentation updates, independent of the current skeletal clip.
    pub texture_tick: u64,
    /// Model light position; None uses the prepared stage light.
    pub light: Option<[f32; 3]>,
    pub shadow: Option<ActorShadowFrame>,
    pub resource: ResourceId,
    pub clip: u16,
    pub frame: f32,
    pub blend_weight: f32,
    pub root_translation: [f32; 3],
    pub world: Matrix,
    /// Placement sampled by following particles before the completed body is drawn.
    pub particle_world: Matrix,
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
    advance_during_blend: bool,
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
            advance_during_blend: false,
        })
    }

    fn blending(&self) -> bool {
        self.blend > 1 && self.blend_age < self.blend
    }

    fn step(&mut self) -> f32 {
        let weight = if self.blending() {
            self.blend_age += 1;
            let weight = f32::from(self.blend_age) / (f32::from(self.blend) + 1.);
            if !self.advance_during_blend {
                return weight;
            }
            weight
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
    /// Last unblended local pose; interrupted blends retain the same baseline.
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
        self.clock = clock;
        self.pending_sample = true;
    }

    fn advance(&mut self, skeleton: &Skeleton, motion: &Motion) -> Result<f32> {
        // Replacements retain the sample already produced by this update.
        if let Some(weight) = self.sample_pending(skeleton, motion)? {
            return Ok(weight);
        }
        let blending = self.clock.blending();
        let weight = self.clock.step();
        self.sample(skeleton, motion, weight, blending)?;
        Ok(weight)
    }

    fn sample_pending(&mut self, skeleton: &Skeleton, motion: &Motion) -> Result<Option<f32>> {
        if !std::mem::take(&mut self.pending_sample) {
            return Ok(None);
        }
        let blending = self.clock.blending();
        let weight = if blending {
            f32::from(self.clock.blend_age) / f32::from(self.clock.blend)
        } else {
            1.
        };
        // A one-update transition explicitly replaces this update's sample.
        if self.clock.blend == 1 {
            self.sample(skeleton, motion, 1., false)?;
        }
        Ok(Some(weight))
    }

    fn sample(
        &mut self,
        skeleton: &Skeleton,
        motion: &Motion,
        weight: f32,
        blending: bool,
    ) -> Result<()> {
        // A replacement keeps the outgoing pose until its blend advances.
        if blending && weight == 0. {
            self.pending_sample = false;
            return Ok(());
        }
        let sampled = (0..skeleton.bones.len())
            .map(|bone| {
                self.sampled_bone(skeleton, motion, bone, self.clock.frame, weight, blending)
            })
            .collect::<Result<Vec<_>>>()?;
        if !blending {
            self.blend_from.clone_from(&sampled);
        }
        self.local = sampled;
        self.pending_sample = false;
        Ok(())
    }

    fn sampled_bone(
        &self,
        skeleton: &Skeleton,
        motion: &Motion,
        bone: usize,
        frame: f32,
        weight: f32,
        blending: bool,
    ) -> Result<pose::BonePose> {
        let track = motion
            .tracks
            .iter()
            .find(|track| usize::from(track.bone) == bone);
        let mut sampled = pose::sample_bone(skeleton.bones[bone].bind, track, frame)?;
        if blending {
            sampled = if track.is_some() {
                pose::BonePose::complete(self.blend_from[bone].value().mix(sampled.value(), weight))
            } else {
                self.local[bone]
            };
        } else {
            sampled.retain_unwritten(self.local[bone]);
        }
        Ok(sampled)
    }

    fn projected_bone(
        &self,
        skeleton: &Skeleton,
        motion: &Motion,
        bone: usize,
        advance: bool,
    ) -> Result<pose::BonePose> {
        if self.pending_sample {
            return if self.clock.blend == 1 {
                self.sampled_bone(skeleton, motion, bone, self.clock.frame, 1., false)
            } else {
                Ok(self.local[bone])
            };
        }
        if !advance {
            return Ok(self.local[bone]);
        }
        let mut clock = self.clock.clone();
        let blending = clock.blending();
        let weight = clock.step();
        if weight == 0. {
            return Ok(self.local[bone]);
        }
        self.sampled_bone(skeleton, motion, bone, clock.frame, weight, blending)
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
    bound_clip: u16,
    bones: Vec<u16>,
    animation: AnimatedPose,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub definition: Arc<ModelDefinition>,
    clip: u16,
    bound_clip: u16,
    motion_aliases: BTreeMap<u16, Option<u16>>,
    blink_ticks: u32,
    animation: AnimatedPose,
    parts: Vec<PosePart>,
    bone_scales: Vec<(u16, [f32; 3])>,
    secondary: Vec<resonance_content::secondary_motion::Simulation>,
    /// Sampled body pose used by attached artwork.
    pub shown: ModelFrame,
    weapons: Vec<weapon::Weapon>,
    in_results: bool,
    #[cfg(feature = "test-support")]
    fixture_defeated_hidden: bool,
    tint_flash: Option<[u8; 3]>,
    defeat_alpha: Option<u8>,
    preserve_pose_update: Option<u64>,
    world_override: Option<Matrix>,
    released_hurt: Option<[Option<u16>; 2]>,
    idle_pose_delay: Option<u8>,
}

/// Prepared clock writes for one immediate actor operation. Geometry and poses
/// remain with their existing model; no mutable battle state is copied.
pub(crate) struct PreparedPlay {
    clip: u16,
    bound_clip: u16,
    clock: Clock,
    weapons: Vec<weapon::PreparedPlay>,
}

impl Model {
    fn projected_bone(&self, bone: u16, advance: bool) -> Result<Matrix> {
        let index = usize::from(bone);
        let (animation, clip) = self
            .parts
            .iter()
            .find(|part| part.bones.contains(&bone))
            .map_or((&self.animation, self.bound_clip), |part| {
                (&part.animation, part.bound_clip)
            });
        let mut local = animation.projected_bone(
            &self.definition.skeleton,
            self.definition.motion(clip)?,
            index,
            advance,
        )?;
        if let Some((_, scale)) = self.bone_scales.iter().rev().find(|(id, _)| *id == bone) {
            local = pose::BonePose::complete(local.value().with_scale(*scale));
        }
        if bone == 0 {
            local.suppress_translation(self.definition.suppress_root_translation);
        }
        let local = local.matrix();
        Ok(
            if let Some(parent) = self.definition.skeleton.bones[index].parent {
                resonance_content::animation::multiply(self.projected_bone(parent, advance)?, local)
            } else {
                local
            },
        )
    }

    fn body_points(&self, actor: &Actor) -> impl Iterator<Item = ([f32; 3], f32)> + '_ {
        self.body_points_at(self.world_override.unwrap_or_else(|| world(actor)))
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
            #[cfg(feature = "test-support")]
            fixture_defeated_hidden: false,
            tint_flash: None,
            defeat_alpha: None,
            preserve_pose_update: None,
            world_override: None,
            released_hurt: None,
            idle_pose_delay: Some(0),
            secondary: vec![Default::default(); definition.secondary_motion.len()],
            shown: ModelFrame {
                actor: actor_id,
                visible: true,
                depth_write: true,
                tint: definition.tint,
                material: ModelMaterial::Normal,

                texture_layers: definition.idle_expression,
                texture_tick: 0,
                light: None,
                shadow: None,
                resource: definition.resource,
                clip: play.clip,
                frame: play.frame,
                blend_weight: 1.,
                root_translation: [0.; 3],
                world: world(actor),
                particle_world: world(actor),
                bones: Arc::new(Vec::new()),
            },
            definition,
            clip: play.clip,
            bound_clip: play.clip,
            motion_aliases: BTreeMap::new(),
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
            .map(|definition| {
                let mut component = weapon::Weapon::new(Arc::clone(definition), &model.shown)?;
                if definition.is_body() {
                    component.step(
                        &model.shown,
                        actor.availability != crate::ActorAvailability::Petrified,
                    )?;
                }
                Ok(component)
            })
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
        if self.motion_disabled(binding.clip) {
            return Ok(());
        }
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
            bound_clip: self
                .resolve_clip(binding.clip)
                .context("empty battle motion")?,
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
            Self::compose_parts(&self.parts, &mut self.animation.blend_from, true);
            self.parts.clear();
        }
        self.clip = prepared.clip;
        self.bound_clip = prepared.bound_clip;
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
        clock.advance_during_blend = true;
        ensure!(
            clock.loop_start <= end,
            "partial-pose loop start exceeds end"
        );
        let local = self.composed_local();
        let mut blend_from = self.animation.blend_from.clone();
        Self::compose_parts(&self.parts, &mut blend_from, true);
        let part = PosePart {
            clip: binding.clip,
            bound_clip: self
                .resolve_clip(binding.clip)
                .context("empty partial motion")?,
            bones,
            animation: AnimatedPose {
                clock,
                blend_from,
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
        Self::compose_parts(&self.parts, &mut local, false);
        for &(bone, scale) in &self.bone_scales {
            let value = &mut local[usize::from(bone)];
            *value = pose::BonePose::complete(value.value().with_scale(scale));
        }
        local
    }

    fn compose_parts(parts: &[PosePart], local: &mut [pose::BonePose], retained: bool) {
        // Earlier prepared parts have precedence on overlapping bones.
        for part in parts.iter().rev() {
            let pose = if retained {
                &part.animation.blend_from
            } else {
                &part.animation.local
            };
            for &bone in &part.bones {
                local[usize::from(bone)] = pose[usize::from(bone)];
            }
        }
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

    pub(crate) fn weapon_animation_rate(&mut self, slot: WeaponSlot, rate: f32) -> Result<()> {
        self.weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == Some(slot))
            .context("unprepared carried animation slot")?
            .set_animation_rate(rate)
    }

    pub(crate) fn play_carried_pose(
        &mut self,
        slot: WeaponSlot,
        clip: u16,
        rate: f32,
        repeat: bool,
    ) -> Result<()> {
        self.weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == Some(slot))
            .context("unprepared carried animation slot")?
            .play_carried_pose(clip, rate, repeat)
    }

    fn hurt_clip(&self, alternate: bool) -> Option<u16> {
        self.released_hurt.unwrap_or(self.definition.reactions.hurt)[usize::from(alternate)]
            .filter(|&clip| !self.motion_disabled(clip))
    }

    fn resolve_clip(&self, clip: u16) -> Option<u16> {
        self.motion_aliases
            .get(&clip)
            .copied()
            .unwrap_or(Some(clip))
    }

    fn set_motion_aliases(&mut self, aliases: BTreeMap<u16, Option<u16>>) {
        if let Some(clip) = aliases.get(&self.clip).copied().unwrap_or(Some(self.clip)) {
            self.bound_clip = clip;
        }
        for part in &mut self.parts {
            if let Some(clip) = aliases.get(&part.clip).copied().unwrap_or(Some(part.clip)) {
                part.bound_clip = clip;
            }
        }
        self.motion_aliases = aliases;
    }

    fn motion_disabled(&self, clip: u16) -> bool {
        self.motion_aliases.get(&clip) == Some(&None)
    }

    pub fn hurt(&mut self, alternate: bool) -> Result<()> {
        let Some(clip) = self.hurt_clip(alternate) else {
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
            .motion(
                self.resolve_clip(binding.clip)
                    .context("empty battle motion")?,
            )?
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
        if let Some(eyes) = self.definition.forced_eye_frame {
            frame.texture_layers[0] = eyes;
        }
        frame
    }

    pub(crate) fn weapon_frames(&self) -> Vec<WeaponFrame> {
        self.weapons
            .iter()
            .flat_map(|weapon| weapon.frames().cloned())
            .collect()
    }

    pub(crate) fn weapon_visible(&mut self, slot: Option<WeaponSlot>, visible: bool) -> Result<()> {
        let weapon = self
            .weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == slot)
            .context("unprepared weapon visibility slot")?;
        weapon.visible = visible;
        let visible = self.shown.visible && visible && weapon.enabled;
        for frame in weapon.frames_mut() {
            frame.visible = visible;
        }
        Ok(())
    }

    pub(crate) fn move_weapon(
        &mut self,
        source: WeaponSlot,
        destination: WeaponSlot,
    ) -> Result<()> {
        ensure!(
            source != destination,
            "weapon replacement uses the same slot"
        );
        let from = self
            .weapons
            .iter()
            .find(|weapon| weapon.slot() == Some(source))
            .context("unprepared replacement weapon")?;
        let to = self
            .weapons
            .iter()
            .find(|weapon| weapon.slot() == Some(destination))
            .context("unprepared replaced weapon slot")?;
        let mut definition = (*from.definition).clone();
        definition.slot = Some(destination);
        definition.attachment = to.definition.attachment;
        let mut replacement = self.prepare_weapons(&[Arc::new(definition)])?.remove(0);
        replacement.visible = true;
        for frame in replacement.frames_mut() {
            frame.visible = self.shown.visible;
        }
        self.weapons
            .retain(|weapon| weapon.slot() != Some(source) && weapon.slot() != Some(destination));
        self.weapons.push(replacement);
        Ok(())
    }

    pub(crate) fn detach_weapon(&mut self, slot: WeaponSlot, world: Option<Matrix>) {
        if let Some(weapon) = self
            .weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == Some(slot))
        {
            weapon.detach(world);
        }
    }

    pub(crate) fn attach_weapons(&mut self) {
        for weapon in &mut self.weapons {
            if !weapon.dropped {
                weapon.detach(None);
            }
        }
    }

    pub(crate) fn drop_weapon(&mut self, slot: WeaponSlot, world: Matrix) -> Result<()> {
        let weapon = self
            .weapons
            .iter_mut()
            .find(|weapon| weapon.slot() == Some(slot))
            .context("unprepared dropped weapon slot")?;
        weapon.detach(Some(world));
        weapon.dropped = true;
        Ok(())
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
        let duration = self.definition.motion(self.bound_clip)?.duration_frames;
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

    pub(crate) fn primary_weapons_visible(&mut self, visible: bool) {
        for weapon in &mut self.weapons {
            if weapon.slot().is_some_and(WeaponSlot::is_primary) {
                weapon.visible = visible;
                let visible = visible && self.shown.visible && weapon.enabled;
                for frame in weapon.frames_mut() {
                    frame.visible = visible;
                }
            }
        }
    }

    pub(crate) fn primary_weapons_enabled(&mut self, enabled: bool) {
        for weapon in &mut self.weapons {
            if weapon.slot().is_some_and(WeaponSlot::is_primary) {
                weapon.enabled = enabled;
                let visible = enabled && weapon.visible && self.shown.visible;
                for frame in weapon.frames_mut() {
                    frame.visible = visible;
                }
            }
        }
    }

    pub fn set_loop_start(&mut self, frame: f32) -> Result<()> {
        ensure!(
            frame.is_finite()
                && (0. ..=self.animation.clock.end).contains(&frame)
                && self
                    .weapons
                    .iter_mut()
                    .flat_map(weapon::Weapon::body_clocks)
                    .all(|clock| (0. ..=clock.end).contains(&frame)),
            "invalid battle animation loop start"
        );
        self.animation.clock.loop_start = frame;
        for clock in self
            .weapons
            .iter_mut()
            .flat_map(weapon::Weapon::body_clocks)
        {
            clock.loop_start = frame;
        }
        Ok(())
    }

    /// Resume a bound idle pose without replacing its current sample or phase.
    fn loop_idle(&mut self) {
        for clock in std::iter::once(&mut self.animation.clock).chain(
            self.weapons
                .iter_mut()
                .flat_map(weapon::Weapon::body_clocks),
        ) {
            clock.loop_start = 0.;
            clock.repeat = true;
            clock.stopped = false;
        }
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
        // Playback intervals may span more than one cycle of their motion curves.
        for clock in std::iter::once(&mut self.animation.clock).chain(
            self.weapons
                .iter_mut()
                .flat_map(weapon::Weapon::body_clocks),
        ) {
            Clock::new(
                Playback {
                    clip: self.clip,
                    frame: clock.frame,
                    rate: clock.rate,
                    repeat: clock.repeat,
                },
                end,
                clock.blend,
            )?;
        }
        self.animation.clock.end = end;
        for clock in self
            .weapons
            .iter_mut()
            .flat_map(weapon::Weapon::body_clocks)
        {
            clock.end = end;
        }
        Ok(())
    }

    fn idle_clip(&self, actor: &Actor) -> Option<u16> {
        // Keep the bound rest cycle until it completes before changing its pose.
        if self.definition.idle_motions.contains(&Some(self.clip)) && !self.animation.clock.finished
        {
            return Some(self.clip);
        }
        let ready = !matches!(
            actor.control,
            crate::Control::Manual | crate::Control::SemiAuto
        ) || self.idle_pose_delay == Some(0);
        let injured = actor.side == crate::Side::Party
            && ready
            && i64::from(actor.hp) * 4 < i64::from(actor.equipment.max_hp);
        self.definition.idle_motions[usize::from(injured)].or(self.definition.idle_motions[0])
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
        let initial_loop = self.definition.initial.repeat
            && self.animation.clock.repeat
            && self.clip == self.definition.initial.clip;
        let reaction = self.definition.reactions;
        let mut blend = 8;
        let (clip, repeat) = match activity {
            Activity::Defeated => {
                blend = if self.definition.fade_on_defeat || !grounded {
                    4
                } else {
                    12
                };
                (
                    if self.definition.fade_on_defeat || grounded {
                        reaction
                            .defeated
                            .or(reaction.down)
                            .or(reaction.stagger_down)
                    } else {
                        reaction.hurt[0]
                    },
                    false,
                )
            }
            Activity::Guarding => (
                reaction.guard[usize::from(!grounded && !actor.movement.flying)],
                false,
            ),
            Activity::KnockedDown if grounded && actor.reaction.recoil.kind.contact() => {
                let kind = actor.reaction.recoil.kind;
                if kind.contact() && !kind.launching() {
                    // A grounded knockdown keeps the hit pose through its final sample.
                    let mut clock = self.animation.clock.clone();
                    if *advance_outgoing && !self.animation.pending_sample {
                        clock.step();
                    }
                    if !clock.finished {
                        return Ok(());
                    }
                }
                blend = if kind == crate::RecoilKind::Down {
                    12
                } else {
                    4
                };
                (reaction.down.or(reaction.stagger_down), false)
            }
            Activity::GettingUp => {
                blend = 4;
                (
                    if actor.reaction.recoil.kind.contact() {
                        reaction.get_up
                    } else {
                        reaction.stagger_get_up
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
                                && ((self.animation.clock.finished && !initial_loop)
                                    || self.definition.idle_motions.contains(&Some(self.clip))
                                    || [reaction.walk, reaction.run].contains(&Some(self.clip)))
                        }
                        _ => {
                            actor.side == crate::Side::Party
                                && reaction.cast != Some(self.clip)
                                && self.animation.clock.finished
                                && !self.animation.clock.repeat
                        }
                    } =>
            {
                self.shown.texture_layers = self.definition.idle_expression;
                (self.idle_clip(actor), true)
            }
            _ => return Ok(()),
        };
        let clip = clip.filter(|&clip| !self.motion_disabled(clip));
        if repeat
            && matches!(activity, Activity::Idle | Activity::Recovering)
            && clip == Some(self.clip)
        {
            self.loop_idle();
        } else if let Some(clip) = clip
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
        if advance_presentation {
            self.shown.texture_tick += 1;
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
            let motion = self.definition.motion(part.bound_clip)?;
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
        let motion = self.definition.motion(self.bound_clip)?;
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
        if let Some(world) = self.world_override {
            placement.world = world;
        } else {
            for (axis, offset) in actor.reaction.hit_shake_offset().into_iter().enumerate() {
                placement.world[3][axis] += offset;
            }
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
#[derive(Clone)]
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
        let motion = model.definition.motion(
            model
                .resolve_clip(sample.clip)
                .context("empty scene motion")?,
        )?;
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
        let motion = model.definition.motion(
            model
                .resolve_clip(sample.clip)
                .context("empty scene motion")?,
        )?;
        model.clip = sample.clip;
        model.bound_clip = model
            .resolve_clip(sample.clip)
            .context("empty scene motion")?;
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

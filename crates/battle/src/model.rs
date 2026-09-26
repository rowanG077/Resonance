//! Simulation-owned model playback. Native controller arithmetic: DOL 8006D2E0;
//! battle bindings: REL 2C05C. Curves remain shared with the field renderer.
mod effect;
pub use effect::{
    EffectModelDefinition, EffectModelFrame, EffectMotionBinding, PreparedEffectModel,
};
mod pose;
mod secondary;
mod weapon;
use crate::{Actor, ActorId};
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Matrix, Motion, Skeleton};
use std::{collections::BTreeMap, sync::Arc};
pub use weapon::{WeaponDefinition, WeaponFrame, WeaponPlayback};

/// A resolved point on the body or a rigidly attached weapon. Loading composes
/// a static weapon bone into `offset`; gameplay never resolves original names.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub bone: u16,
    pub offset: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct ModelDefinition {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub secondary_motion: Vec<resonance_content::secondary_motion::Chain>,
    pub initial: Playback,
    /// Prepared ordinary and alternate hurt clips. Absent native clips or a
    /// profile's no-body-motion flag leave the current playback in place.
    pub hurt_motions: [Option<u16>; 2],
    /// Ordinary idle and optional party low-health idle (2B404).
    pub idle_motions: [Option<u16>; 2],
    /// Ground and airborne guard clips; repeated blocks retain the current clip.
    pub guard_motions: [Option<u16>; 2],
    pub stun: Option<crate::StunBinding>,
    pub knockdown: Option<crate::KnockdownBinding>,
    pub anchors: Vec<Anchor>,
    pub weapons: Vec<Arc<WeaponDefinition>>,
    /// Bone bindings correspond to the actor's prepared hurt points, in order.
    pub hurt_bones: Vec<u16>,
    pub approach_bones: Vec<u16>,
    pub target_bones: Vec<u16>,
    pub shadow: Option<ShadowDefinition>,
    /// Profile target-point bone plus an actor-space offset. None uses twice
    /// the scaled profile center without heading rotation (1FF48).
    pub target_marker: Option<Anchor>,
    /// Original root translation can be observed without moving the drawn root.
    pub suppress_root_translation: [bool; 3],
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

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct ModelFrame {
    pub actor: ActorId,
    pub visible: bool,
    pub tint: [u8; 4],
    /// Original expression atlas amounts. Only prepared profile channels draw.
    pub texture_layers: [u8; 4],
    /// Source model light position; None retains the prepared stage light.
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
    start: f32,
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
            play.frame.is_finite() && (0. ..=end).contains(&play.frame) && play.rate.is_finite(),
            "invalid battle animation interval or rate"
        );
        Ok(Self {
            frame: play.frame,
            start: play.frame,
            end,
            loop_start: play.frame,
            rate: play.rate,
            repeat: play.repeat,
            stopped: false,
            finished: false,
            blend: if blend > 1 { blend } else { 0 },
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
            self.frame += self.rate;
            // Native reverse wrapping tests zero, not the playback start.
            if self.frame < 0. {
                self.frame = if self.repeat {
                    self.frame + self.end
                } else {
                    self.start
                };
                self.finished = true;
            }
            if self.frame > self.end {
                self.frame = if self.repeat {
                    let frame = (self.frame - self.end) + self.loop_start;
                    if frame > self.end {
                        self.loop_start
                    } else {
                        frame
                    }
                } else {
                    self.end
                };
                self.finished = true;
            }
            if self.finished && !self.repeat {
                self.stopped = true;
            }
        }
        1.
    }
}

/// A model's animation clock and retained local pose, independent of its owner.
#[derive(Debug, Clone)]
struct AnimatedPose {
    clock: Clock,
    /// Cross-fades use the last completed, unblended local pose.
    completed: Vec<pose::BonePose>,
    local: Vec<pose::BonePose>,
}

impl AnimatedPose {
    fn new(skeleton: &Skeleton, motion: &Motion, play: Playback) -> Result<Self> {
        let mut clock = Clock::new(play, motion.duration_frames, 0)?;
        // Initial phase is not the loop origin (entry randomization supplies it).
        clock.start = 0.;
        clock.loop_start = 0.;
        let local = pose::sample(skeleton, motion, play.frame)?;
        Ok(Self {
            clock,
            completed: local.clone(),
            local,
        })
    }

    fn advance(&mut self, skeleton: &Skeleton, motion: &Motion) -> Result<f32> {
        let blending = self.clock.blending();
        let weight = self.clock.step();
        ensure!(
            self.clock.frame.is_finite(),
            "battle animation clock overflow"
        );
        self.local = pose::sample(skeleton, motion, self.clock.frame)?;
        if blending {
            for ((to, from), rest) in self
                .local
                .iter_mut()
                .zip(&self.completed)
                .zip(&skeleton.bones)
            {
                *to = from.mix(*to, rest, weight);
            }
        } else {
            self.completed.clone_from(&self.local);
        }
        Ok(weight)
    }

    fn pose(
        &self,
        skeleton: &Skeleton,
        suppress_root: [bool; 3],
    ) -> Result<(resonance_content::animation::Pose, [f32; 3])> {
        let mut sampled = self.local.clone();
        let root = sampled[0].translation();
        sampled[0].suppress_translation(suppress_root);
        let matrices: Vec<_> = sampled.iter().map(|p| p.matrix()).collect();
        let locals = sampled.iter().map(|p| p.transform()).collect();
        Ok((skeleton.pose_with_matrices(locals, &matrices)?, root))
    }
}

/// Placement and animation have distinct pause gates in 5200C/52668.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementUpdate {
    Actor,
    TransitionOwner,
    Held,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub definition: Arc<ModelDefinition>,
    clip: u16,
    animation: AnimatedPose,
    secondary: Vec<resonance_content::secondary_motion::Simulation>,
    pub shown: ModelFrame,
    /// Original model yaw is sampled before actor callbacks (52668).
    sampled_heading: f32,
    placement: secondary::Placement,
    shadow_extent: Option<[f32; 2]>,
    weapons: Vec<weapon::Weapon>,
}

impl Model {
    /// Group one uses the new actor root height with the preceding sampled bounds.
    pub(crate) fn sample_shadow(&mut self, actor: &Actor) {
        self.shown.shadow =
            self.definition
                .shadow
                .zip(self.shadow_extent)
                .map(|(shadow, extent)| {
                    let factor = (1. - actor.position[1] / 800.).max(0.25);
                    let radius =
                        (shadow.scale * ((extent[0] * factor + extent[1] * factor) / 1.5)).max(10.);
                    ActorShadowFrame {
                        position: [
                            actor.body.target_center[0],
                            1.1,
                            actor.body.target_center[2],
                        ],
                        radius,
                        color: shadow.color,
                    }
                });
    }

    pub(crate) fn target_marker_position(&self, actor: &Actor) -> [f32; 3] {
        if let Some(anchor) = self.definition.target_marker {
            let position = self.bone_position(anchor.bone);
            let offset =
                crate::geometry::rotate(anchor.offset.map(|v| v * actor.body.scale), actor.heading);
            std::array::from_fn(|i| position[i] + offset[i])
        } else {
            crate::markers::initial_position(actor)
        }
    }

    pub fn new(
        definition: Arc<ModelDefinition>,
        actor_id: ActorId,
        actor: &mut Actor,
    ) -> Result<Self> {
        definition.skeleton.validate()?;
        for motion in definition.motions.values() {
            motion.validate(&definition.skeleton)?;
        }
        for chain in &definition.secondary_motion {
            chain.validate(definition.skeleton.bones.len())?;
        }
        ensure!(
            definition
                .hurt_motions
                .into_iter()
                .chain(definition.guard_motions)
                .chain(definition.idle_motions)
                .flatten()
                .all(|clip| definition.motions.contains_key(&clip)),
            "missing battle reaction motion"
        );
        if let Some(down) = definition.knockdown {
            ensure!(
                std::iter::once(down.down_motion)
                    .chain(down.recovery_motion)
                    .all(|clip| definition.motions.contains_key(&clip)),
                "missing knockdown motion"
            );
        }
        let bones = definition.skeleton.bones.len();
        if let Some(stun) = &definition.stun {
            stun.particle.validate()?;
            ensure!(
                usize::from(stun.head) < bones
                    && stun.offset.iter().all(|v| v.is_finite())
                    && [stun.loop_motion, stun.down_motion, stun.recovery_motion]
                        .iter()
                        .all(|clip| definition.motions.contains_key(clip)),
                "invalid stun model binding"
            );
        }
        ensure!(
            definition.anchors.len()
                + definition
                    .weapons
                    .iter()
                    .map(|w| w.anchors.len())
                    .sum::<usize>()
                <= 256
                && definition
                    .anchors
                    .iter()
                    .all(|a| usize::from(a.bone) < bones && a.offset.iter().all(|v| v.is_finite()))
                && definition.hurt_bones.len() == actor.body.points.len()
                && definition.approach_bones.len() == actor.body.approach_points.len()
                && definition
                    .hurt_bones
                    .iter()
                    .chain(&definition.approach_bones)
                    .all(|&b| usize::from(b) < bones),
            "invalid battle model attachment binding"
        );
        ensure!(
            definition
                .target_bones
                .iter()
                .all(|&b| usize::from(b) < bones),
            "invalid battle model attachment binding"
        );
        let slots: std::collections::BTreeSet<_> =
            definition.weapons.iter().map(|w| w.slot).collect();
        ensure!(
            definition.shadow.is_none_or(|s| s.scale.is_finite()),
            "invalid actor shadow scale"
        );
        ensure!(
            definition.target_marker.is_none_or(
                |a| usize::from(a.bone) < bones && a.offset.iter().all(|v| v.is_finite())
            ),
            "invalid target marker anchor"
        );
        ensure!(
            slots.len() == definition.weapons.len(),
            "duplicate weapon instance slot"
        );
        let play = definition.initial;
        let motion = definition
            .motions
            .get(&play.clip)
            .context("missing initial battle motion")?;
        let animation = AnimatedPose::new(&definition.skeleton, motion, play)?;
        let mut model = Self {
            sampled_heading: actor.heading,
            placement: secondary::Placement::actor(actor),
            shadow_extent: None,
            weapons: Vec::new(),
            secondary: vec![Default::default(); definition.secondary_motion.len()],
            shown: ModelFrame {
                actor: actor_id,
                visible: true,
                tint: actor.body.tint,
                texture_layers: [0; 4],
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
        // 52AA8 calls 52668(mode 1) before the first ordinary battle visit.
        // Advance animation and compose once, including one secondary visit.
        model.step(actor, true, PlacementUpdate::Actor)?;
        model.weapons = model
            .definition
            .weapons
            .iter()
            .map(|definition| weapon::Weapon::new(Arc::clone(definition), &model.shown))
            .collect::<Result<_>>()?;
        for weapon in &model.weapons {
            actor.body.anchors.extend(weapon.anchors(&model.shown));
        }
        actor.body.validate()?;
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
        let duration = self.duration(binding)?;
        self.animation.clock = Clock::new(
            Playback {
                clip: binding.clip,
                frame,
                rate,
                repeat,
            },
            duration,
            blend,
        )?;
        self.clip = binding.clip;
        for weapon in &mut self.weapons {
            weapon.play(
                Playback {
                    clip: binding.clip,
                    frame,
                    rate,
                    repeat,
                },
                blend,
            )?;
        }
        Ok(())
    }

    pub fn hurt(&mut self, alternate: bool) -> Result<()> {
        let Some(clip) = self.definition.hurt_motions[usize::from(alternate)] else {
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

    pub fn guard(&mut self, airborne: bool) -> Result<()> {
        let Some(clip) = self.definition.guard_motions[usize::from(airborne)] else {
            return Ok(());
        };
        if self.clip != clip {
            self.play(
                MotionBinding {
                    model: self.definition.resource,
                    clip,
                },
                0.,
                0.5,
                false,
                4,
            )?;
        }
        Ok(())
    }

    pub(crate) fn knockdown_motion(&mut self, getting_up: bool) -> Result<bool> {
        let binding = self
            .definition
            .knockdown
            .context("missing knockdown resources")?;
        let Some(clip) = (if getting_up {
            binding.recovery_motion
        } else {
            Some(binding.down_motion)
        }) else {
            return Ok(false);
        };
        if self.clip != clip {
            self.play(
                MotionBinding {
                    model: self.definition.resource,
                    clip,
                },
                0.,
                0.5,
                false,
                if getting_up { 4 } else { 12 },
            )?;
        }
        Ok(true)
    }

    pub(crate) fn bone_position(&self, bone: u16) -> [f32; 3] {
        use resonance_content::animation::transform_point;
        transform_point(
            self.shown.world,
            transform_point(self.shown.bones[usize::from(bone)], [0.; 3]),
        )
    }

    pub(crate) fn stun_motion(&mut self, remaining: i16, grounded: bool) -> Result<()> {
        let binding = self
            .definition
            .stun
            .as_ref()
            .context("missing stun motion binding")?;
        let requested = if remaining == 0 && self.clip == binding.down_motion {
            Some((binding.recovery_motion, 4, false))
        } else if remaining > 0 && grounded && self.clip != binding.loop_motion {
            Some((binding.loop_motion, 8, true))
        } else {
            None
        };
        if let Some((clip, blend, repeat)) = requested {
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

    pub(crate) fn weapon_frames(&self) -> Vec<WeaponFrame> {
        self.weapons
            .iter()
            .map(|weapon| weapon.shown.clone())
            .collect()
    }

    pub(crate) fn weapon_visible(&mut self, slot: u8, visible: bool) -> Result<()> {
        let weapon = self
            .weapons
            .iter_mut()
            .find(|weapon| weapon.shown.slot == slot)
            .context("unprepared weapon visibility slot")?;
        weapon.visible = visible;
        weapon.shown.visible = self.shown.visible && visible;
        Ok(())
    }

    pub(crate) fn weapon_attachment(&self, slot: u8) -> Result<[f32; 3]> {
        Ok(self
            .weapons
            .iter()
            .find(|weapon| weapon.shown.slot == slot)
            .context("unprepared detached weapon slot")?
            .attachment(&self.shown))
    }

    pub(crate) fn detach_weapon(
        &mut self,
        slot: u8,
        world: Option<Matrix>,
        actor: &mut Actor,
    ) -> Result<()> {
        self.weapons
            .iter_mut()
            .find(|weapon| weapon.shown.slot == slot)
            .context("unprepared detached weapon slot")?
            .detach(world);
        actor.body.anchors.truncate(self.definition.anchors.len());
        for weapon in &self.weapons {
            actor.body.anchors.extend(weapon.anchors(&self.shown));
        }
        Ok(())
    }

    pub(crate) fn anchor_attached(&self, anchor: u16) -> bool {
        let mut start = self.definition.anchors.len();
        for (weapon, definition) in self.weapons.iter().zip(&self.definition.weapons) {
            let end = start + definition.anchors.len();
            if (start..end).contains(&usize::from(anchor)) {
                return weapon.attached();
            }
            start = end;
        }
        true
    }

    pub(crate) fn attach_weapons(&mut self, actor: &mut Actor) {
        actor.body.anchors.truncate(self.definition.anchors.len());
        for weapon in &mut self.weapons {
            weapon.detach(None);
            actor.body.anchors.extend(weapon.anchors(&self.shown));
        }
    }

    /// Diagnostic retirement also tolerates a missing slot: there is then no
    /// detached model to restore, but the surviving anchors still need a rebuild.
    pub(crate) fn attach_weapon(&mut self, slot: u8, actor: &mut Actor) {
        if let Some(weapon) = self
            .weapons
            .iter_mut()
            .find(|weapon| weapon.shown.slot == slot)
        {
            weapon.detach(None);
        }
        actor.body.anchors.truncate(self.definition.anchors.len());
        for weapon in &self.weapons {
            actor.body.anchors.extend(weapon.anchors(&self.shown));
        }
    }

    pub fn set_loop_start(&mut self, frame: f32) -> Result<()> {
        ensure!(
            frame.is_finite() && (0. ..=self.animation.clock.end).contains(&frame),
            "invalid battle animation loop start"
        );
        self.animation.clock.loop_start = frame;
        Ok(())
    }

    pub fn blending(&self) -> bool {
        self.animation.clock.blending()
    }
    pub fn finished(&self) -> bool {
        self.animation.clock.finished
    }

    pub fn step(
        &mut self,
        actor: &mut Actor,
        advance: bool,
        placement: PlacementUpdate,
    ) -> Result<()> {
        // DOL 8006D2E0 mode 2 retains the sampled pose, even if a callback has
        // replaced its motion binding. World composition still runs.
        if advance {
            let weight = self.animation.advance(
                &self.definition.skeleton,
                &self.definition.motions[&self.clip],
            )?;
            self.shown.clip = self.clip;
            self.shown.frame = self.animation.clock.frame;
            self.shown.blend_weight = weight;
        }
        self.present(actor, advance, advance, placement)
    }

    /// 5200C samples body appearance before 155CC copies it to carried models.
    pub(crate) fn sample_tint(&mut self, tint: [u8; 4]) {
        self.shown.tint = tint;
        for weapon in &mut self.weapons {
            weapon.shown.tint = tint;
        }
    }

    pub(crate) fn sampled_heading(&self) -> f32 {
        self.sampled_heading
    }

    /// 40C8 initializes bodies before formation placement and target facing.
    /// Recompose the already advanced 52AA8 cursor at that initial placement;
    /// the first ordinary visit moves it to the actor and lets chain recovery
    /// handle the displacement. Generic sampled-pose construction is unchanged.
    pub(crate) fn initialize_entry_placement(&mut self, actor: &mut Actor) -> Result<()> {
        self.secondary.fill_with(Default::default);
        self.placement = secondary::Placement::initial();
        self.sampled_heading = 0.;
        self.present(actor, true, false, PlacementUpdate::Held)
    }

    fn present(
        &mut self,
        actor: &mut Actor,
        advance_secondary: bool,
        advance_weapons: bool,
        update: PlacementUpdate,
    ) -> Result<()> {
        let (mut pose, root) = self.animation.pose(
            &self.definition.skeleton,
            self.definition.suppress_root_translation,
        )?;
        if update != PlacementUpdate::Held {
            let previous_position = self.placement.world[3];
            self.placement = secondary::Placement::actor(actor);
            self.sampled_heading = actor.heading;
            if update == PlacementUpdate::TransitionOwner {
                self.placement.world[3] = previous_position;
            }
            // 52668 shares 106C with the portrait bounce. The odd countdown
            // scales the recoil vector without normalizing or rotating it.
            let countdown = actor.hud.portrait_bounce as i16;
            let product = (i32::from(countdown) << 1) * i32::from(countdown & 1);
            if product != 0 {
                let direction = if crate::distance::length(actor.reaction.direction) >= 0.1 {
                    actor.reaction.direction
                } else {
                    [1., 0., 0.]
                };
                let offset = direction.map(|value| value * (product as f32 * 2.));
                for (axis, value) in offset.into_iter().enumerate() {
                    self.placement.world[3][axis] = actor.position[axis] + value;
                }
            }
        }
        let placement = self.placement;
        let world = placement.world;
        secondary::apply(
            &self.definition.secondary_motion,
            &self.definition.motions[&self.clip],
            &mut self.secondary,
            &mut pose.global,
            placement,
            advance_secondary,
            actor.body.jitter.take_acceleration(),
        )?;
        let point = |a: Anchor| -> Result<[f32; 3]> {
            Ok(resonance_content::animation::transform_point(
                world,
                pose.point(a.bone, a.offset)?,
            ))
        };
        actor.body.anchors = self
            .definition
            .anchors
            .iter()
            .map(|&a| point(a))
            .collect::<Result<_>>()?;
        for (hurt, &bone) in actor
            .body
            .points
            .iter_mut()
            .zip(&self.definition.hurt_bones)
        {
            hurt.center = point(Anchor {
                bone,
                offset: [0.; 3],
            })?;
        }
        for (volume, &bone) in actor
            .body
            .approach_points
            .iter_mut()
            .zip(&self.definition.approach_bones)
        {
            volume.center = point(Anchor {
                bone,
                offset: [0.; 3],
            })?;
        }
        // 1B1DC visits admitted bone origins in source index order, excluding
        // points strictly below 0.1, and uses 4DC44's subtract/scale/add midpoint.
        let mut maximum = [-10000_f32; 3];
        let mut minimum = [10000_f32; 3];
        let mut found = false;
        for &bone in &self.definition.target_bones {
            let position = point(Anchor {
                bone,
                offset: [0.; 3],
            })?;
            if position[1] < 0.1 {
                continue;
            }
            found = true;
            for axis in 0..3 {
                maximum[axis] = maximum[axis].max(position[axis]);
                minimum[axis] = minimum[axis].min(position[axis]);
            }
        }
        actor.body.target_center =
            std::array::from_fn(|i| minimum[i] + (maximum[i] - minimum[i]) * 0.5);
        // 31C88's bounds-found bit gates7A90C independently of death visibility.
        self.shadow_extent = found.then_some([maximum[0] - minimum[0], maximum[2] - minimum[2]]);
        self.sample_shadow(actor);
        actor.body.validate()?;
        self.shown.root_translation = root;
        self.shown.tint = actor.body.tint;
        self.shown.world = world;
        self.shown.bones = Arc::new(pose.global);
        for weapon in &mut self.weapons {
            weapon.step(&self.shown, advance_weapons)?;
            actor.body.anchors.extend(weapon.anchors(&self.shown));
        }
        actor.body.validate()?;
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
mod casting_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod stun_tests;

#[cfg(test)]
mod scene_tests;

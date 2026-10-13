//! Consume native pose requests without giving playback any authority over combat.
use super::{Model, ModelDefinition, MotionBinding, WeaponDefinition, weapon::Weapon};
use crate::BattleClock;
use crate::{Activity, Actor, ActorAvailability, ActorId, BattleFrame};

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use resonance_content::diagnostics::Diagnostics;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

// A third of a second at 60 updates/second.
const DEFEAT_FADE_TICKS: u8 = 20;

/// Clip frames and blend duration are visual tuning, independent of action timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub frame: f32,
    pub end: Option<f32>,
    pub rate: f32,
    pub repeat: bool,
    pub blend: u8,
    pub loop_start: f32,
    /// False keeps an already selected clip running.
    pub restart: bool,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            frame: 0.,
            end: None,
            rate: 0.5,
            repeat: false,
            blend: 4,
            loop_start: 0.,
            restart: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonPose {
    Jump,
    Backstep,
    Taunt,
    Breakfall,
    Falling,
    Landing,
    Returning,
    Stopping,
    Chant,
    Cast,
    SustainedCast,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelRequest {
    Play {
        actor: ActorId,
        motion: MotionBinding,
        pose: Pose,
    },
    PartialPose {
        actor: ActorId,
        motion: MotionBinding,
        pose: Pose,
        bones: Vec<u16>,
    },
    PlayKeepingParts {
        actor: ActorId,
        motion: MotionBinding,
        pose: Pose,
    },
    BoneScales {
        actor: ActorId,
        scales: Vec<(u16, [f32; 3])>,
    },
    CarriedPose {
        actor: ActorId,
        slot: u8,
        clip: u16,
        rate: f32,
        repeat: bool,
    },
    WeaponAnimationRate {
        actor: ActorId,
        slot: u8,
        rate: f32,
    },
    AnimationRate {
        actor: ActorId,
        rate: f32,
    },
    #[cfg(feature = "test-support")]
    LoopStart {
        actor: ActorId,
        frame: f32,
    },
    Hurt {
        actor: ActorId,
        alternate: bool,
    },
    Common {
        actor: ActorId,
        pose: CommonPose,
    },
    Expression {
        actor: ActorId,
        layers: [u8; 4],
    },
    Tint {
        actor: ActorId,
        color: [u8; 3],
    },
    Weapon {
        actor: ActorId,
        slot: u8,
        visible: bool,
    },
    PrimaryWeapons {
        actor: ActorId,
        visible: bool,
    },
    /// Enter result presentation and restore or hide this actor’s artwork.
    Result {
        actor: ActorId,
        visible: bool,
    },
    Equip {
        actor: ActorId,
        items: [u16; 2],
    },
}

/// Geometry samples once before separation; presentation finishes at final placement.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdatePhase {
    Complete,
    PoseOnly,
    FinishPose,
}

impl ModelRequest {
    fn changes_body_pose(&self) -> bool {
        #[cfg(feature = "test-support")]
        if matches!(self, Self::LoopStart { .. }) {
            return true;
        }
        matches!(
            self,
            Self::Play { .. }
                | Self::PlayKeepingParts { .. }
                | Self::PartialPose { .. }
                | Self::BoneScales { .. }
                | Self::AnimationRate { .. }
                | Self::Hurt { .. }
                | Self::Common { .. }
        )
    }

    fn actor(&self) -> ActorId {
        match *self {
            Self::Play { actor, .. }
            | Self::PartialPose { actor, .. }
            | Self::PlayKeepingParts { actor, .. }
            | Self::BoneScales { actor, .. }
            | Self::CarriedPose { actor, .. }
            | Self::WeaponAnimationRate { actor, .. }
            | Self::AnimationRate { actor, .. }
            | Self::Hurt { actor, .. }
            | Self::Common { actor, .. }
            | Self::Expression { actor, .. }
            | Self::Tint { actor, .. }
            | Self::Weapon { actor, .. }
            | Self::PrimaryWeapons { actor, .. }
            | Self::Result { actor, .. }
            | Self::Equip { actor, .. } => actor,
            #[cfg(feature = "test-support")]
            Self::LoopStart { actor, .. } => actor,
        }
    }
}

impl crate::Battle {
    pub(crate) fn request_pose(
        &mut self,
        actor: ActorId,
        motion: Option<MotionBinding>,
        pose: Pose,
    ) {
        if let Some(motion) = motion {
            self.model_requests.push(ModelRequest::Play {
                actor,
                motion,
                pose,
            });
        }
    }
}

pub struct Models {
    pub(super) models: Vec<Option<Model>>,
    equipment: BTreeMap<(ActorId, u16), Vec<Arc<WeaponDefinition>>>,
    independent: Vec<super::independent::IndependentModel>,
    diagnostics: Diagnostics,
    advanced_update: Option<u64>,
    hit_stopped: Vec<bool>,
    weapon_overrides: BTreeMap<ActorId, SavedWeapons>,
}

struct SavedWeapons {
    action: Option<crate::ActionId>,
    ordinary: Vec<Weapon>,
    overridden: BTreeSet<u8>,
}

impl Models {
    pub fn new(
        actors: &[Actor],
        definitions: Vec<Option<Arc<ModelDefinition>>>,
        equipment: BTreeMap<(ActorId, u16), Vec<Arc<WeaponDefinition>>>,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        ensure!(
            actors.len() == definitions.len(),
            "battle model/actor count differs"
        );
        let models = definitions
            .into_iter()
            .zip(actors)
            .enumerate()
            .map(|(index, (definition, actor))| {
                definition
                    .map(|definition| {
                        diagnostics.attempt(
                            "battle body",
                            Model::new(definition, ActorId(index as u8), actor),
                        )
                    })
                    .transpose()
                    .map(Option::flatten)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            models,
            equipment,
            independent: Vec::new(),
            diagnostics,
            advanced_update: None,
            hit_stopped: vec![false; actors.len()],
            weapon_overrides: BTreeMap::new(),
        })
    }

    /// Keep a separately animated scene model until this encounter is released.
    pub fn add_independent(&mut self, model: super::PreparedEffectModel) -> Result<()> {
        self.independent
            .push(super::independent::IndependentModel::new(model)?);
        Ok(())
    }

    pub(crate) fn begin_unison_weapons(&mut self) {
        for (index, model) in self.models.iter().enumerate() {
            if let Some(model) = model {
                self.weapon_overrides
                    .entry(ActorId(index as u8))
                    .or_insert_with(|| SavedWeapons {
                        action: None,
                        ordinary: model.weapons.clone(),
                        overridden: BTreeSet::new(),
                    });
            }
        }
    }

    /// A temporary scene weapon is restored when its owning action stops attacking.
    pub(crate) fn override_action_weapons(
        &mut self,
        actor: ActorId,
        action: crate::ActionId,
        definitions: &[Arc<WeaponDefinition>],
    ) -> Result<()> {
        let model = self
            .models
            .get(actor.index())
            .and_then(Option::as_ref)
            .context("temporary weapon owner has no model")?;
        let saved = self
            .weapon_overrides
            .entry(actor)
            .or_insert_with(|| SavedWeapons {
                action: Some(action),
                ordinary: model.weapons.clone(),
                overridden: BTreeSet::new(),
            });
        ensure!(
            saved.action == Some(action),
            "temporary weapon already has another owner"
        );
        self.override_weapons(actor, definitions)
    }

    pub(crate) fn restore_action_weapons(&mut self, actor: ActorId, action: crate::ActionId) {
        if self
            .weapon_overrides
            .get(&actor)
            .is_some_and(|saved| saved.action == Some(action))
        {
            let saved = self.weapon_overrides.remove(&actor).unwrap();
            if let Some(model) = self.models.get_mut(actor.index()).and_then(Option::as_mut) {
                model.install_weapons(saved.ordinary);
            }
        }
    }

    pub(crate) fn override_unison_weapons(
        &mut self,
        actor: ActorId,
        definitions: &[Arc<WeaponDefinition>],
    ) -> Result<()> {
        self.override_weapons(actor, definitions)
    }

    fn override_weapons(
        &mut self,
        actor: ActorId,
        definitions: &[Arc<WeaponDefinition>],
    ) -> Result<()> {
        if definitions.is_empty() {
            return Ok(());
        }
        let model = self
            .models
            .get_mut(actor.index())
            .and_then(Option::as_mut)
            .context("temporary weapon owner has no model")?;
        let saved = self
            .weapon_overrides
            .get_mut(&actor)
            .context("temporary weapons have not been saved")?;
        let mut replacements = model.prepare_weapons(definitions)?;
        for weapon in &mut replacements {
            weapon.visible = true;
            for frame in weapon.frames_mut() {
                frame.visible = model.shown.visible;
            }
        }
        let slots: BTreeSet<_> = definitions.iter().map(|weapon| weapon.slot).collect();
        model
            .weapons
            .retain(|weapon| !slots.contains(&weapon.slot()));
        model.weapons.extend(replacements);
        model.weapons.sort_by_key(Weapon::slot);
        saved.overridden.extend(slots);
        Ok(())
    }

    pub(crate) fn restore_unison_weapons(&mut self, actor: ActorId) {
        if let Some(saved) = self.weapon_overrides.get_mut(&actor) {
            if let Some(model) = self.models.get_mut(actor.index()).and_then(Option::as_mut) {
                model.install_weapons(saved.ordinary.clone());
            }
            saved.overridden.clear();
        }
    }

    pub(crate) fn end_unison_weapons(&mut self) {
        self.weapon_overrides.retain(|actor, saved| {
            if saved.action.is_some() {
                return true;
            }
            if let Some(model) = self.models.get_mut(actor.index()).and_then(Option::as_mut) {
                model.install_weapons(saved.ordinary.clone());
            }
            false
        });
    }

    /// Requests are consumed once. Drawing always uses the completed native positions.
    pub fn advance(
        &mut self,
        frame: &mut BattleFrame,
        clock: BattleClock,
        settle: bool,
    ) -> Result<()> {
        self.advance_with_release(frame, clock, settle, None)
    }

    pub(crate) fn advance_with_release(
        &mut self,
        frame: &mut BattleFrame,
        clock: BattleClock,
        settle: bool,
        release_actor: Option<ActorId>,
    ) -> Result<()> {
        self.advance_phase(frame, clock, settle, release_actor, UpdatePhase::Complete)
    }

    fn advance_phase(
        &mut self,
        frame: &mut BattleFrame,
        clock: BattleClock,
        settle: bool,
        release_actor: Option<ActorId>,
        phase: UpdatePhase,
    ) -> Result<()> {
        let pose_only = phase == UpdatePhase::PoseOnly;
        if !pose_only {
            self.weapon_overrides.retain(|actor, saved| {
                let Some(action) = saved.action else {
                    return true;
                };
                if frame
                    .actions
                    .iter()
                    .any(|&(id, owner, _)| id == action && owner == *actor)
                    && frame
                        .actors
                        .get(actor.index())
                        .is_some_and(|actor| actor.activity == Activity::Action)
                {
                    return true;
                }
                if let Some(model) = self.models.get_mut(actor.index()).and_then(Option::as_mut) {
                    model.install_weapons(saved.ordinary.clone());
                }
                false
            });
        }
        let already_advanced = self.advanced_update == Some(frame.update);
        if !pose_only {
            self.advanced_update = Some(frame.update);
        }
        let mut requests = std::mem::take(&mut frame.model_requests);
        let independent_advance = !already_advanced
            && matches!(clock, BattleClock::Running | BattleClock::SummonTransition);
        if !pose_only {
            frame.independent_models = self
                .independent
                .iter_mut()
                .map(|model| model.step(independent_advance))
                .collect::<Result<_>>()
                .context("independent battle model playback")?;
        }
        frame.models.clear();
        frame.weapons.clear();
        for (index, actor) in frame.actors.iter().enumerate().take(self.models.len()) {
            let ticking = !already_advanced
                && (clock == BattleClock::Running
                    || clock == BattleClock::SummonTransition
                    || clock == BattleClock::Rescue(ActorId(index as u8)))
                && frame
                    .unison
                    .as_ref()
                    .is_none_or(|scene| scene.animates(ActorId(index as u8)));
            let paused = frame.paused_actors.contains(&ActorId(index as u8));
            let cinematic = frame
                .summon
                .as_ref()
                .is_some_and(|scene| scene.owner.is_some());
            // Playback resumes one update after the actor leaves hit-stop.
            let resuming = self.hit_stopped[index];
            if ticking && !pose_only {
                self.hit_stopped[index] = !cinematic && paused && actor.time_stop == 0;
            }
            let advance = ticking
                && (cinematic || !paused && !resuming)
                && actor.availability != crate::ActorAvailability::Petrified;
            let advance_pose = advance && phase != UpdatePhase::FinishPose;
            let mut advance_outgoing = advance_pose;
            for request in requests.extract_if(.., |request| {
                request.actor().index() == index && (!pose_only || request.changes_body_pose())
            }) {
                let result = self.apply(request, &frame.actors, &mut advance_outgoing);
                self.diagnostics.attempt("battle model request", result)?;
            }
            let slot = &mut self.models[index];
            let Some(model) = slot else { continue };
            let result = (|| {
                let settle = settle
                    && (actor.activity != Activity::Recovering
                        || frame
                            .actions
                            .iter()
                            .any(|(_, owner, _)| owner.index() == index));
                if !model.in_results && (settle || actor.availability == ActorAvailability::Dead) {
                    model.settle(actor, actor.activity, &mut advance_outgoing)?;
                }
                if pose_only {
                    return model.sample_for_geometry(actor, advance_pose);
                }
                model.attach_weapons();
                for flight in frame
                    .weapon_flights
                    .iter()
                    .filter(|flight| flight.owner.index() == index)
                {
                    let world = Mat4::from_rotation_translation(
                        Quat::from_rotation_arc(
                            Vec3::NEG_Y,
                            Vec3::from_array(flight.direction).normalize_or(Vec3::Z),
                        ),
                        Vec3::from_array(flight.position),
                    )
                    .to_cols_array_2d();
                    model.detach_weapon(flight.slot, Some(world));
                }
                if !already_advanced
                    && clock == BattleClock::SpellTransition
                    && release_actor == Some(ActorId(index as u8))
                    && model.definition.reactions.cast == Some(model.clip)
                    && !paused
                    && !resuming
                    && actor.time_stop == 0
                {
                    model.sample_main(true)?;
                }
                model.step_with_phases(actor, advance_pose, advance, ticking)?;
                let mut tint = model.definition.tint;
                if let Some(flash) = &mut model.tint_flash {
                    for (channel, target) in flash.iter_mut().zip(tint) {
                        if ticking {
                            *channel = match (*channel).cmp(&target) {
                                std::cmp::Ordering::Less => *channel + 1,
                                std::cmp::Ordering::Greater => *channel - 1,
                                std::cmp::Ordering::Equal => *channel,
                            };
                        }
                    }
                    tint[..3].copy_from_slice(flash);
                    if flash.as_slice() == &model.definition.tint[..3] {
                        model.tint_flash = None;
                    }
                }
                model.shown.light = if model.in_results {
                    tint = [64, 64, 64, 255];
                    Some(
                        (Vec3::from_array(actor.position) + Vec3::new(150., 300., 200.)).to_array(),
                    )
                } else if matches!(actor.activity, Activity::Casting { .. }) {
                    Some(actor.position)
                } else {
                    None
                };
                if !model.in_results
                    && model.definition.fade_on_defeat
                    && actor.availability == ActorAvailability::Dead
                {
                    tint[3] = if advance {
                        model.shown.tint[3].saturating_sub(u8::MAX.div_ceil(DEFEAT_FADE_TICKS))
                    } else {
                        model.shown.tint[3]
                    };
                }
                model.sample_tint(tint);
                model.shown.depth_write = tint[3] == u8::MAX;
                model.sample_shadow(actor);
                Ok(())
            })();
            if self
                .diagnostics
                .attempt("battle model playback", result)?
                .is_none()
            {
                *slot = None;
                continue;
            }
            let visible = actor.availability != crate::ActorAvailability::Absent
                && !(actor.side == crate::Side::Party
                    && frame
                        .enemy_presentation
                        .as_ref()
                        .is_some_and(|scene| scene.hide_party))
                && model.shown.tint[3] != 0
                && frame
                    .summon
                    .as_ref()
                    .and_then(|scene| scene.owner)
                    .is_none_or(|owner| owner.index() == index)
                && frame
                    .unison
                    .as_ref()
                    .is_none_or(|scene| !scene.hidden.contains(&ActorId(index as u8)));
            let mut body = model.render_frame(actor);
            body.visible &= visible;
            frame.models.push(body);
            frame
                .weapons
                .extend(model.weapon_frames().into_iter().map(|mut weapon| {
                    weapon.visible &= visible;
                    weapon
                }));
        }
        if pose_only {
            frame.model_requests = requests;
        }
        Ok(())
    }

    pub(crate) fn advance_for_contacts(
        &mut self,
        frame: &mut BattleFrame,
        clock: BattleClock,
        settle: bool,
        phase: UpdatePhase,
    ) -> Result<()> {
        self.advance_phase(frame, clock, settle, None, phase)
    }

    pub fn equipment_for_items(
        &self,
        actor: ActorId,
        items: [u16; 2],
    ) -> Vec<Arc<WeaponDefinition>> {
        items
            .into_iter()
            .filter_map(|item| self.equipment.get(&(actor, item)))
            .flatten()
            .cloned()
            .collect()
    }

    fn apply(
        &mut self,
        request: ModelRequest,
        actors: &[crate::ActorFrame],
        advance_outgoing: &mut bool,
    ) -> Result<()> {
        use ModelRequest::*;
        let actor = request.actor();
        let Some(model) = self.models.get_mut(actor.index()).and_then(Option::as_mut) else {
            return Ok(());
        };
        match request {
            Play { motion, pose, .. } => {
                if pose.restart || !model.is_playing(motion)? {
                    let prepared = model.prepare_play(
                        motion,
                        pose.frame,
                        pose.rate,
                        pose.repeat,
                        pose.blend,
                    )?;
                    model.advance_outgoing(advance_outgoing)?;
                    model.apply_play(prepared);
                    if let Some(end) = pose.end {
                        model.set_end(end)?;
                    }
                    model.set_loop_start(pose.loop_start.max(pose.frame))?;
                }
            }
            PartialPose {
                motion,
                pose,
                bones,
                ..
            } => model.play_part(motion, pose, bones)?,
            PlayKeepingParts { motion, pose, .. } => {
                let prepared =
                    model.prepare_play(motion, pose.frame, pose.rate, pose.repeat, pose.blend)?;
                model.advance_outgoing(advance_outgoing)?;
                let parts = std::mem::take(&mut model.parts);
                model.apply_play(prepared);
                model.parts = parts;
                if let Some(end) = pose.end {
                    model.set_end(end)?;
                }
                model.set_loop_start(pose.loop_start.max(pose.frame))?;
            }
            BoneScales { scales, .. } => model.set_bone_scales(scales)?,
            CarriedPose {
                slot,
                clip,
                rate,
                repeat,
                ..
            } => model.play_carried_pose(slot, clip, rate, repeat)?,
            WeaponAnimationRate { slot, rate, .. } => model.weapon_animation_rate(slot, rate)?,
            AnimationRate { rate, .. } => model.set_rate(rate)?,
            #[cfg(feature = "test-support")]
            LoopStart { frame, .. } => model.set_loop_start(frame)?,
            Hurt { alternate, .. } => {
                if model.definition.reactions.hurt[usize::from(alternate)].is_some() {
                    model.advance_outgoing(advance_outgoing)?;
                }
                model.hurt(alternate)?;
            }
            Common { pose, .. } => {
                let motions = model.definition.reactions;
                let clip = match pose {
                    CommonPose::Jump => motions.jump,
                    CommonPose::Backstep => motions.backstep,
                    CommonPose::Taunt => motions.taunt,
                    CommonPose::Breakfall => motions.breakfall,
                    CommonPose::Falling => motions.airborne,
                    CommonPose::Landing => motions.landing,
                    CommonPose::Returning => motions.returning,
                    CommonPose::Stopping => motions.stopping,
                    CommonPose::Chant => motions.chant,
                    CommonPose::Cast | CommonPose::SustainedCast => motions.cast,
                };
                if let Some(clip) = clip
                    && (!matches!(pose, CommonPose::Falling | CommonPose::Landing)
                        || model.clip != clip)
                {
                    let prepared = model.prepare_play(
                        MotionBinding {
                            model: model.definition.resource,
                            clip,
                        },
                        0.,
                        if pose == CommonPose::Returning {
                            actors[actor.index()]
                                .motion_rate(0.5, actors[actor.index()].conditions.effective())
                        } else {
                            0.5
                        },
                        matches!(
                            pose,
                            CommonPose::Returning | CommonPose::Chant | CommonPose::SustainedCast
                        ),
                        match pose {
                            CommonPose::Taunt => 10,
                            CommonPose::Backstep | CommonPose::Chant => 8,
                            _ => 4,
                        },
                    )?;
                    model.advance_outgoing(advance_outgoing)?;
                    model.apply_play(prepared);
                    if pose == CommonPose::SustainedCast {
                        // A one-frame loop retains the endpoint without replaying its gesture.
                        // Ordinary idle selection and hit reactions still replace this pose.
                        model.set_loop_start(model.animation.clock.end)?;
                    }
                }
            }
            Expression { layers, .. } => model.shown.texture_layers = layers,
            Tint { color, .. } => model.tint_flash = Some(color),
            Weapon { slot, visible, .. } => model.weapon_visible(slot, visible)?,
            PrimaryWeapons { visible, .. } => {
                for weapon in &mut model.weapons {
                    if weapon.slot() < 2 {
                        weapon.visible = visible;
                    }
                }
            }
            Result { visible, .. } => {
                model.in_results = true;
                model.shown.visible = visible;
                for weapon in &mut model.weapons {
                    weapon.visible = visible;
                }
            }
            Equip { items, .. } => {
                let definitions = items
                    .into_iter()
                    .filter_map(|item| self.equipment.get(&(actor, item)))
                    .flatten()
                    .cloned()
                    .collect::<Vec<_>>();
                let saved = self.weapon_overrides.get_mut(&actor);
                let prepared = model.prepare_weapons_from(
                    &definitions,
                    saved
                        .as_ref()
                        .map_or(&model.weapons, |saved| &saved.ordinary),
                );
                let (mut weapons, error) = match prepared {
                    Ok(weapons) => (weapons, None),
                    Err(error) => (Vec::new(), Some(error)),
                };
                if let Some(saved) = saved {
                    saved.ordinary = weapons.clone();
                    weapons.retain(|weapon| !saved.overridden.contains(&weapon.slot()));
                    weapons.extend(
                        model
                            .weapons
                            .iter()
                            .filter(|weapon| saved.overridden.contains(&weapon.slot()))
                            .cloned(),
                    );
                    weapons.sort_by_key(|weapon| weapon.slot());
                }
                model.install_weapons(weapons);
                if let Some(error) = error {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

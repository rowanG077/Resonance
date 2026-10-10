//! Consume native pose requests without giving playback any authority over combat.
use super::{Model, ModelDefinition, MotionBinding, WeaponDefinition};
use crate::BattleClock;
use crate::{Activity, Actor, ActorAvailability, ActorId, BattleFrame};

use anyhow::{Result, ensure};
use glam::{Mat4, Quat, Vec3};
use resonance_content::diagnostics::Diagnostics;
use std::{collections::BTreeMap, sync::Arc};

// A third of a second at 60 updates/second.
const DEFEAT_FADE_TICKS: u8 = 20;

/// Clip frames and blend duration are visual tuning, independent of action timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub frame: f32,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelRequest {
    Play {
        actor: ActorId,
        motion: MotionBinding,
        pose: Pose,
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
    diagnostics: Diagnostics,
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
            diagnostics,
        })
    }

    /// Requests are consumed once. Drawing always uses the completed native positions.
    pub fn advance(
        &mut self,
        frame: &mut BattleFrame,
        clock: BattleClock,
        settle: bool,
    ) -> Result<()> {
        for request in std::mem::take(&mut frame.model_requests) {
            let result = self.apply(request, &frame.actors);
            self.diagnostics.attempt("battle model request", result)?;
        }
        frame.models.clear();
        frame.weapons.clear();
        for (index, (slot, actor)) in self.models.iter_mut().zip(&frame.actors).enumerate() {
            let Some(model) = slot else { continue };
            let advance = (clock == BattleClock::Running
                || clock == BattleClock::Rescue(ActorId(index as u8)))
                && actor.time_stop == 0
                && actor.hit_stop == 0
                && actor.availability != crate::ActorAvailability::Petrified;
            let result = (|| {
                if !model.in_results && (settle || actor.availability == ActorAvailability::Dead) {
                    model.settle(actor, actor.activity)?;
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
                model.step(actor, advance)?;
                let mut tint = model.definition.tint;
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
            let visible =
                actor.availability != crate::ActorAvailability::Absent && model.shown.tint[3] != 0;
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
        Ok(())
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

    fn apply(&mut self, request: ModelRequest, actors: &[crate::ActorFrame]) -> Result<()> {
        use ModelRequest::*;
        let actor = match request {
            Play { actor, .. }
            | Hurt { actor, .. }
            | Common { actor, .. }
            | Expression { actor, .. }
            | Weapon { actor, .. }
            | PrimaryWeapons { actor, .. }
            | Result { actor, .. }
            | Equip { actor, .. } => actor,
        };
        let Some(model) = self.models.get_mut(actor.index()).and_then(Option::as_mut) else {
            return Ok(());
        };
        match request {
            Play { motion, pose, .. } => {
                if pose.restart || !model.is_playing(motion)? {
                    model.play(motion, pose.frame, pose.rate, pose.repeat, pose.blend)?;
                    model.set_loop_start(pose.loop_start.max(pose.frame))?;
                }
            }
            Hurt { alternate, .. } => model.hurt(alternate)?,
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
                    CommonPose::Cast => motions.cast,
                };
                if let Some(clip) = clip
                    && (!matches!(pose, CommonPose::Falling | CommonPose::Landing)
                        || model.clip != clip)
                {
                    model.play(
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
                        matches!(pose, CommonPose::Returning | CommonPose::Chant),
                        match pose {
                            CommonPose::Taunt => 10,
                            CommonPose::Backstep | CommonPose::Chant => 8,
                            _ => 4,
                        },
                    )?;
                }
            }
            Expression { layers, .. } => model.shown.texture_layers = layers,
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
                match model.prepare_weapons(&definitions) {
                    Ok(weapons) => model.install_weapons(weapons),
                    Err(error) => {
                        model.install_weapons(Vec::new());
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
}

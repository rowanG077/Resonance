//! Animated angel wings and Colette's automatic flight accessory.
use crate::{Actor, Animation, Attachment, GameWorld, ResourceLibrary, animation::slot};
use anyhow::{Context, Result};
use resonance_content::{
    animation::{Matrix, multiply, transform_point},
    field::COLETTE_WINGS_RESOURCE,
};

pub(crate) const AUTOMATIC_WINGS: i32 = 90021;
const ECHO_INTERVAL: u32 = 8;
const ECHO_LIFETIME: u32 = 2 * ECHO_INTERVAL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WingStyle {
    Layered,
    Echo,
}

#[derive(Debug, Clone)]
pub struct Wings {
    style: WingStyle,
    attachment: Option<RetainedAttachment>,
    echoes: Vec<WingEcho>,
}

pub struct WingLayer<'a> {
    pub alpha: u8,
    pub pose_delay: f32,
    pub uv_offset: f32,
    pub echo: Option<&'a WingEcho>,
    pub visible: bool,
}

impl Wings {
    pub const LAYERS: u8 = 3;

    pub fn layer(&self, pass: u8, tick: u32) -> WingLayer<'_> {
        let base = pass == Self::LAYERS - 1;
        let echo = (!base)
            .then(|| self.echoes.get(usize::from(pass)))
            .flatten();
        let layered = self.style == WingStyle::Layered;
        WingLayer {
            alpha: if layered {
                [127, 63, 255][usize::from(pass)]
            } else {
                255
            },
            pose_delay: if layered && !base {
                2. * f32::from(pass + 1)
            } else if echo.is_some() {
                -1.
            } else {
                0.
            },
            uv_offset: if layered {
                (tick % 128) as f32 / 128.
            } else {
                0.
            },
            visible: layered || base || echo.is_some(),
            echo,
        }
    }
}

impl Actor {
    pub fn set_wings(&mut self, style: WingStyle) {
        self.wings = Some(Wings {
            style,
            attachment: None,
            echoes: Vec::new(),
        });
        self.collidable = false;
        self.contact = crate::ActorContact::None;
        self.grounded = false;
        self.casts_shadow = false;
        self.depth_write = false;
        self.cull_outside_view = false;
        self.scripted_animation = true;
        self.blend = Some(crate::effect::Blend::Additive);
        if let Some(animation) = &mut self.animation {
            animation.blend_ticks = 2;
        }
    }
}

#[derive(Debug, Clone)]
pub struct WingEcho {
    born: u32,
    pub position: [f32; 3],
    pub angles: [f32; 3],
    pub attachment: Option<Attachment>,
}

impl WingEcho {
    pub fn scale(&self, tick: u32) -> [f32; 3] {
        let age = tick.saturating_sub(self.born) as f32;
        [1. + 0.02 * age, 1. + 0.02 * age, 1. + 0.05 * age]
    }

    pub fn rgba(&self, tick: u32) -> [u8; 4] {
        let age = tick.saturating_sub(self.born).min(ECHO_LIFETIME);
        let tint = 64 - 4 * age;
        let alpha = if age == 0 {
            255
        } else {
            (32 * age.min(ECHO_LIFETIME - age)).min(255)
        };
        [tint as u8, tint as u8, 64, alpha as u8]
    }
}

#[derive(Debug, Clone)]
struct RetainedAttachment {
    attachment: Attachment,
    parent: Matrix,
}

impl GameWorld {
    pub(crate) fn step_wings(&mut self, resources: &ResourceLibrary, enabled: bool) -> Result<()> {
        for actor in self.actors.values_mut() {
            if actor
                .wings
                .as_ref()
                .is_none_or(|w| w.style != WingStyle::Echo)
            {
                continue;
            }
            let position = actor.visual_position();
            let angles = [
                actor.tilt_degrees()[0],
                actor.tilt_degrees()[1],
                actor.heading,
            ];
            let wings = actor.wings.as_mut().unwrap();
            wings
                .echoes
                .retain(|echo| self.tick.saturating_sub(echo.born) < ECHO_LIFETIME);
            if self.effect_tick.is_multiple_of(ECHO_INTERVAL)
                && actor.visible
                && !actor.appearance.model_hidden
            {
                wings.echoes.push(WingEcho {
                    born: self.tick,
                    position,
                    angles,
                    attachment: actor.attachment.clone(),
                });
            }
        }
        let parent = self.actors.get(&2).map(|a| a.instance);
        if let Some((instance, owner)) = self.automatic_wings {
            let current = self
                .actors
                .get(&AUTOMATIC_WINGS)
                .is_some_and(|a| a.instance == instance);
            if !current || !enabled || parent != Some(owner) {
                if current {
                    self.despawn_scene_actors(AUTOMATIC_WINGS);
                }
                self.automatic_wings = None;
            }
        }
        if enabled
            && let Some(parent) = parent
            && !self.actors.contains_key(&AUTOMATIC_WINGS)
        {
            let model = resources
                .model(COLETTE_WINGS_RESOURCE)
                .context("Colette flight wings are not cooked")?;
            let clip = model
                .clips
                .get(&slot::IDLE)
                .context("Colette wing motion is missing")?;
            let mut actor = Actor::new(COLETTE_WINGS_RESOURCE, [0.; 3]);
            actor.set_wings(WingStyle::Layered);
            actor.tilt = [-180, -90];
            actor.attachment = Some(Attachment {
                actor: 2,
                bone: "Bone_sebone03".into(),
            });
            let mut animation = Animation::new(
                COLETTE_WINGS_RESOURCE,
                slot::IDLE,
                clip.duration_ticks,
                self.tick,
            );
            // Flight wings advance at 0.2% of the normal animation speed.
            animation.rate = 0.002;
            actor.animation = Some(animation);
            self.insert_actor(AUTOMATIC_WINGS, actor);
            self.automatic_wings = Some((self.actors[&AUTOMATIC_WINGS].instance, parent));
            return Ok(());
        }
        let layered: Vec<_> = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                actor
                    .wings
                    .as_ref()
                    .is_some_and(|w| w.style == WingStyle::Layered)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in layered {
            let actor = &self.actors[&id];
            if !actor.visible || actor.appearance.model_hidden {
                continue;
            }
            let (root, frame) = if let Some(attachment) = &actor.attachment {
                let parent = if self.actors.contains_key(&attachment.actor) {
                    self.attachment_parent(resources, attachment)?
                } else {
                    // Keep attached wings in place while their owner is temporarily absent.
                    actor
                        .wings
                        .as_ref()
                        .unwrap()
                        .attachment
                        .as_ref()
                        .filter(|frame| {
                            frame.attachment.actor == attachment.actor
                                && frame.attachment.bone == attachment.bone
                        })
                        .context("wing attachment owner is missing before its first pose")?
                        .parent
                };
                let frame = RetainedAttachment {
                    attachment: attachment.clone(),
                    parent,
                };
                (multiply(parent, actor.local_matrix()), Some(frame))
            } else {
                (actor.local_matrix(), None)
            };
            let actor = self.actors.get_mut(&id).unwrap();
            actor.wings.as_mut().unwrap().attachment = frame;
            if self.effect_tick & 3 != 0 {
                continue;
            }
            let model = resources
                .model(actor.resource)
                .context("wing model is missing")?;
            let points: Result<Vec<_>> = model
                .names
                .iter()
                .map(|name| {
                    Ok(transform_point(
                        multiply(root, resources.bone_matrix(actor, name, self.tick)?),
                        [0.; 3],
                    ))
                })
                .collect();
            for point in points? {
                let position = point.map(|v| v + (self.random() & 31) as f32 - 15.);
                let size = 2. + (self.random() & 3) as f32;
                let fall = -((self.random() & 7) as f32) / 16.;
                self.emit_billboard(crate::effect::BillboardEffect {
                    field_lighting: true,
                    recipe: resonance_content::effect::WING_SPARK_SPRITE,
                    born: self.tick,
                    lifetime: 21,
                    position,
                    velocity: [0., 0., fall],
                    angular_velocity: [0., 0., -6.],
                    size: [size; 2],
                    rgba: [255; 4],
                    fade: crate::effect::Fade::tail(21),
                    blend: Some(crate::effect::Blend::Alpha),
                    ..Default::default()
                })
                .map_err(anyhow::Error::msg)?;
            }
        }
        Ok(())
    }
}

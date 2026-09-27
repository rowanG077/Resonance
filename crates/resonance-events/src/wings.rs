//! Colette's flight accessory and scripted wings.
use crate::{Actor, Animation, Attachment, GameWorld, ResourceLibrary, animation::slot};
use anyhow::{Context, Result};
use resonance_content::{
    animation::{Matrix, multiply, transform_point},
    field::COLETTE_WINGS_RESOURCE,
};

pub const COLETTE_WINGS_ACTOR: i32 = 90021;
const WINGS: i32 = COLETTE_WINGS_ACTOR;

pub(crate) struct RetainedAttachment {
    instance: u64,
    attachment: Attachment,
    parent: Matrix,
}

impl GameWorld {
    pub(crate) fn step_colette_wings(
        &mut self,
        resources: &ResourceLibrary,
        enabled: bool,
    ) -> Result<()> {
        let parent = self.actors.get(&2).map(|a| a.instance);
        if let Some((instance, owner)) = self.automatic_wings {
            let current = self
                .actors
                .get(&WINGS)
                .is_some_and(|a| a.instance == instance);
            if !current || !enabled || parent != Some(owner) {
                if current {
                    self.despawn_scene_actors(WINGS);
                }
                self.automatic_wings = None;
            }
        }
        if enabled
            && let Some(parent) = parent
            && !self.actors.contains_key(&WINGS)
        {
            let model = resources
                .model(COLETTE_WINGS_RESOURCE)
                .context("Colette flight wings are not cooked")?;
            let clip = model
                .clips
                .get(&slot::IDLE)
                .context("Colette wing motion is missing")?;
            let mut actor = Actor::new(COLETTE_WINGS_RESOURCE, [0.; 3]);
            actor.collidable = false;
            actor.contact = crate::ActorContact::None;
            actor.grounded = false;
            actor.casts_shadow = false;
            actor.depth_write = false;
            actor.cull_outside_view = false;
            actor.scripted_animation = true;
            actor.properties.extend([(35, -180), (36, -90)]);
            actor.blend = Some(crate::model_particle::Blend::Additive);
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
            // Native track times are 30-Hz frames; the callback advances .001.
            animation.rate = 0.002;
            actor.animation = Some(animation);
            self.insert_actor(WINGS, actor);
            self.automatic_wings = Some((self.actors[&WINGS].instance, parent));
            return Ok(());
        }
        let Some(actor) = self.actors.get(&WINGS) else {
            self.wing_attachment = None;
            return Ok(());
        };
        if !actor.visible || actor.appearance.model_hidden {
            return Ok(());
        }
        let root = if let Some(attachment) = &actor.attachment {
            let parent = if self.actors.contains_key(&attachment.actor) {
                self.attachment_parent(resources, attachment)?
            } else {
                // Native attachment matrices survive the owner's temporary
                // removal. Triet recreates Colette while these wings remain.
                self.wing_attachment
                    .as_ref()
                    .filter(|frame| {
                        frame.instance == actor.instance
                            && frame.attachment.actor == attachment.actor
                            && frame.attachment.bone == attachment.bone
                    })
                    .context("wing attachment owner is missing before its first pose")?
                    .parent
            };
            self.wing_attachment = Some(RetainedAttachment {
                instance: actor.instance,
                attachment: attachment.clone(),
                parent,
            });
            multiply(parent, actor.local_matrix())
        } else {
            self.wing_attachment = None;
            actor.local_matrix()
        };
        if self.effect_tick & 3 != 0 {
            return Ok(());
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
                blend_mode: Some(0),
                ..Default::default()
            })
            .map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

//! Colette's automatic flight accessory, fn_8001A6FC / fn_800191F0.
use crate::{Actor, Animation, Attachment, GameWorld, ResourceLibrary, animation::slot};
use anyhow::{Context, Result};
use resonance_content::{
    animation::{multiply, transform_point},
    field::COLETTE_WINGS_RESOURCE,
};

const WINGS: i32 = 90021;

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
        if self.automatic_wings.is_none() || self.effect_tick & 3 != 0 {
            return Ok(());
        }
        let actor = &self.actors[&WINGS];
        let root = self.attachment_root(resources, WINGS)?;
        let model = resources.model(COLETTE_WINGS_RESOURCE).unwrap();
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
                operation: None,
                owner: None,
                field_lighting: true,
                field_fog: true,
                recipe: resonance_content::effect::WING_SPARK_SPRITE,
                orientation: crate::effect::SpriteOrientation::Camera,
                anchor: resonance_content::effect::VerticalAnchor::Center,
                palette: None,
                born: self.tick,
                lifetime: 21,
                position,
                velocity: [0., 0., fall],
                acceleration: None,
                controller: None,
                gravity: 0.,
                rotation: [0.; 3],
                angular_velocity: [0., 0., -6.],
                size: [size; 2],
                size_delta: 0.,
                rgba: [255; 4],
                fade: crate::effect::Fade::tail(21),
                blend_mode: Some(0),
            })
            .map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

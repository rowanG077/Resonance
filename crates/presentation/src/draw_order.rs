//! Authored ordering for ordinary transparent scene meshes.
use bevy::prelude::*;

/// Native drawing phases. Asset material order is local to its scene or instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Layer {
    Background,
    Scene(u32),
    FieldOverlay(u32),
    Shadows,
    Actor(usize, ActorLayer),
    Foreground(u8),
    Effects,
    ModelEffects(u32),
    EffectOverlay,
    Overlay,
}

impl Layer {
    pub(super) fn is_field_actor(self) -> bool {
        use resonance_content::field::DrawStage;
        matches!(self, Self::Scene(stage)
            if stage == DrawStage::Actors.offset() || stage == DrawStage::LateActors.offset())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ActorLayer {
    Behind,
    BackWeapon(u8, bool),
    Body,
    FrontWeapon(u8, bool),
    Front,
}

/// Layer, instance order, then material order within that instance.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DrawOrder(pub Layer, pub usize, pub u32);

pub(super) fn field_actor_depths(
    depths: &mut Vec<(i32, f32)>,
    world: &resonance_events::GameWorld,
    direction: Vec3,
    future: Option<(&resonance_events::GameWorld, f32)>,
    rendered_position: impl Fn(i32) -> Option<Vec3>,
) {
    depths.clear();
    depths.extend(world.actor_order().iter().filter_map(|&id| {
        // Attached actors have local coordinates and draw with their committed owner.
        let owner = std::iter::successors(Some(id), |id| {
            Some(world.field_model_actor(*id)?.attachment.as_ref()?.actor)
        })
        .take(world.actors.len() + 1)
        .last()?;
        let Some(actor) = world.field_model_actor(owner) else {
            // A cutscene can remove the owner first. The attachment keeps its
            // last rendered transform until its owner returns or it is removed.
            let child = world.field_model_actor(id)?;
            let position =
                rendered_position(id).unwrap_or_else(|| Vec3::from_array(child.visual_position()));
            return Some((id, position.dot(direction)));
        };
        let mut position = Vec3::from_array(actor.visual_position());
        if let Some((next, alpha)) = future
            && let Some(target) = next.field_model_actor(owner)
            && crate::field_view::prediction::continuous_pose(actor, target)
        {
            position = position.lerp(Vec3::from_array(target.visual_position()), alpha);
        }
        Some((id, position.dot(direction)))
    }));
    // Stable ties retain the simulation's submission order.
    depths.sort_by(|a, b| b.1.total_cmp(&a.1));
}

impl DrawOrder {
    pub(super) fn field_model(mut layer: Layer, instance: usize, material: u32) -> Self {
        use resonance_content::field::MODEL_DRAW_SPAN;
        // Actors are ordered back to front. Finish the whole model, including
        // its outline, before a nearer translucent actor can write depth.
        if layer.is_field_actor() {
            return Self(layer, instance, material);
        }
        // Scenery and effects retain their authored material passes.
        match &mut layer {
            Layer::Scene(stage) | Layer::FieldOverlay(stage) | Layer::ModelEffects(stage) => {
                *stage += material / MODEL_DRAW_SPAN * MODEL_DRAW_SPAN;
            }
            _ => unreachable!("field model layer"),
        }
        Self(layer, instance, material % MODEL_DRAW_SPAN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_actors_finish_body_and_outline_before_nearer_models() {
        use resonance_content::field::{DrawStage, MODEL_DRAW_SPAN};
        use resonance_events::{Actor, Attachment, GameWorld};
        let mut world = GameWorld::default();
        for (id, depth) in [(1, 1.), (2, 3.), (3, 100.)] {
            world.insert_actor(id, Actor::new(1, [0., depth, 0.]));
        }
        world.actors.get_mut(&3).unwrap().attachment = Some(Attachment {
            actor: 1,
            bone: "head".into(),
        });
        let mut next = GameWorld::default();
        next.actors = world.actors.clone();
        next.actors.get_mut(&1).unwrap().position[1] = 5.;
        let mut depths = Vec::new();
        for (alpha, order) in [(0.25, [2, 1, 3]), (0.5, [1, 2, 3]), (0.75, [1, 3, 2])] {
            field_actor_depths(&mut depths, &world, Vec3::Y, Some((&next, alpha)), |_| None);
            assert_eq!(depths.iter().map(|&(id, _)| id).collect::<Vec<_>>(), order);
        }
        // After its owner disappears, the child still renders at world Y = 1.
        world.actors.remove(&1);
        field_actor_depths(&mut depths, &world, Vec3::Y, None, |id| {
            assert_eq!(id, 3);
            Some(Vec3::Y)
        });
        assert_eq!(depths, [(2, 3.), (3, 1.)]);
        for stage in [DrawStage::Actors, DrawStage::LateActors] {
            let layer = Layer::Scene(stage.offset());
            let far_body = DrawOrder::field_model(layer, 0, 15);
            let far_outline = DrawOrder::field_model(layer, 0, MODEL_DRAW_SPAN);
            let near_body = DrawOrder::field_model(layer, 1, 0);
            let near_outline = DrawOrder::field_model(layer, 1, MODEL_DRAW_SPAN);
            assert!(far_body < far_outline);
            assert!(far_outline < near_body);
            assert!(near_body < near_outline);
            assert!(
                near_outline < DrawOrder(Layer::Scene(stage.offset() + 2 * MODEL_DRAW_SPAN), 0, 0)
            );
        }
        // Static scenery keeps its separate foreground pass.
        assert!(
            DrawOrder::field_model(Layer::Scene(0), 3, DrawStage::Foreground.offset() + 7)
                == DrawOrder(Layer::Scene(DrawStage::Foreground.offset()), 3, 7)
        );
    }
}

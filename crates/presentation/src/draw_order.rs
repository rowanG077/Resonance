//! Authored ordering for ordinary transparent scene meshes.
use bevy::{
    core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        sync_world::MainEntityHashMap,
    },
};

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
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DrawOrder(pub Layer, pub usize, pub u32);

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

#[derive(Resource, Default)]
struct VisibleOrders(MainEntityHashMap<DrawOrder>);

pub(super) struct DrawOrderPlugin;
impl Plugin for DrawOrderPlugin {
    fn build(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<VisibleOrders>()
                .add_systems(ExtractSchedule, extract)
                .add_systems(
                    Render,
                    apply
                        .after(RenderSystems::Queue)
                        .before(RenderSystems::PhaseSort),
                );
        }
    }
}

fn extract(
    mut orders: ResMut<VisibleOrders>,
    draws: Extract<Query<(Entity, &ViewVisibility, &DrawOrder)>>,
) {
    // Prepared pools can contain thousands of hidden draws. Publish only this
    // frame's visible orders, dropping entries as soon as a draw becomes hidden.
    orders.0.clear();
    orders.0.extend(
        draws.iter().filter_map(|(entity, visible, order)| {
            visible.get().then_some((entity.into(), *order))
        }),
    );
}

fn apply(mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>, orders: Res<VisibleOrders>) {
    let orders = &orders.0;
    for phase in phases.values_mut() {
        // Stable native keys determine transparent order, independent of ECS insertion.
        phase
            .items
            .sort_by_key(|_, item| orders.get(&item.main_entity()).copied());
        for (rank, item) in phase.items.values_mut().enumerate() {
            if orders.contains_key(&item.main_entity()) {
                // Feed the native order into Bevy's ascending depth sort. This changes
                // scheduling only; the material's real depth tests remain enabled.
                item.sorting_info = TransparentSortingInfo3d::Sorted {
                    mesh_center: Vec3::ZERO,
                    depth_bias: rank as f32,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        ecs::system::RunSystemOnce,
        render::{MainWorld, sync_world::MainEntity},
    };

    #[test]
    fn field_actors_finish_body_and_outline_before_nearer_models() {
        use resonance_content::field::{DrawStage, MODEL_DRAW_SPAN};
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

    #[test]
    fn unsynced_visible_meshes_publish_current_orders_and_remove_stale_draws() {
        let mut main = MainWorld::default();
        let field_order = DrawOrder(Layer::Scene(2), 0, 1);
        let actor_order = DrawOrder(Layer::Actor(0, ActorLayer::Body), 0, 0);
        // Ordinary mesh draws use main-world keys without a RenderEntity.
        let field = main.spawn((field_order, ViewVisibility::VISIBLE)).id();
        let actor = main.spawn((actor_order, ViewVisibility::HIDDEN)).id();
        let field_key = MainEntity::from(field);
        let actor_key = MainEntity::from(actor);
        let mut render = World::new();
        render.insert_resource(main);
        render.init_resource::<VisibleOrders>();
        render.run_system_once(extract).unwrap();
        assert_eq!(render.resource::<VisibleOrders>().0.len(), 1);
        assert!(render.resource::<VisibleOrders>().0[&field_key] == field_order);
        assert!(
            !render
                .resource::<VisibleOrders>()
                .0
                .contains_key(&actor_key)
        );

        let changed_order = DrawOrder(Layer::Actor(1, ActorLayer::Front), 3, 4);
        {
            let mut main = render.resource_mut::<MainWorld>();
            main.entity_mut(field).insert(ViewVisibility::HIDDEN);
            main.entity_mut(actor)
                .insert((ViewVisibility::VISIBLE, changed_order));
        }
        render.run_system_once(extract).unwrap();
        assert_eq!(render.resource::<VisibleOrders>().0.len(), 1);
        assert!(render.resource::<VisibleOrders>().0[&actor_key] == changed_order);
        assert!(
            !render
                .resource::<VisibleOrders>()
                .0
                .contains_key(&field_key)
        );

        render.resource_mut::<MainWorld>().despawn(actor);
        render.run_system_once(extract).unwrap();
        assert!(render.resource::<VisibleOrders>().0.is_empty());
    }
}

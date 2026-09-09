//! Authored ordering for ordinary transparent scene meshes.
use bevy::{
    core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        sync_world::MainEntity,
    },
};
use std::collections::HashMap;

#[derive(Component, Clone, Copy, ExtractComponent)]
pub(super) struct DrawOrder(pub u32);

pub(super) struct DrawOrderPlugin;
impl Plugin for DrawOrderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<DrawOrder>::default());
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                Render,
                apply
                    .after(RenderSystems::Queue)
                    .before(RenderSystems::PhaseSort),
            );
        }
    }
}

fn apply(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    orders: Query<(&MainEntity, &DrawOrder)>,
) {
    let orders: HashMap<_, _> = orders
        .iter()
        .map(|(entity, order)| (*entity, order.0))
        .collect();
    for phase in phases.values_mut() {
        for item in phase.items.values_mut() {
            if let Some(order) = orders.get(&item.main_entity()) {
                // A common center makes this sort solely by the cooked order,
                // regardless of camera movement or deforming mesh bounds. This
                // changes scheduling only; actual depth tests remain enabled.
                item.sorting_info = TransparentSortingInfo3d::Sorted {
                    mesh_center: Vec3::ZERO,
                    depth_bias: *order as f32,
                };
            }
        }
    }
}

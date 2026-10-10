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

/// Native drawing phases. Asset material order is local to its scene or instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Layer {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ActorLayer {
    Behind,
    BackWeapon(u8, bool),
    Body,
    FrontWeapon(u8, bool),
    Front,
}

/// Layer, instance order, then material order within that instance.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ExtractComponent)]
pub(super) struct DrawOrder(pub Layer, pub usize, pub u32);

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
        .map(|(entity, order)| (*entity, *order))
        .collect();
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

//! Serial hierarchy propagation for our small, frequently animated scenes.
//!
//! Similar transform stalls are tracked in Bevy issue #19411:
//! https://github.com/bevyengine/bevy/issues/19411
use bevy::{
    app::ValidateParentHasComponentPlugin,
    prelude::*,
    transform::{
        TransformSystems,
        components::TransformTreeChanged,
        systems::{StaticTransformOptimizations, mark_dirty_trees, sync_simple_transforms},
    },
};

pub(super) struct SerialTransformPlugin;

impl Plugin for SerialTransformPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ValidateParentHasComponentPlugin::<GlobalTransform>::default())
            .init_resource::<StaticTransformOptimizations>()
            .add_systems(
                PostStartup,
                (mark_dirty_trees, propagate, sync_simple_transforms)
                    .chain()
                    .in_set(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                (mark_dirty_trees, propagate, sync_simple_transforms)
                    .chain()
                    .in_set(TransformSystems::Propagate),
            );
    }
}

// Entity, expected parent, parent's world transform and its change flag.
type Pending = (Entity, Entity, GlobalTransform, bool);

/// Bevy's parallel propagation busy-waits on a shared queue. If a worker is
/// descheduled while holding its lock, the main and render threads can spend
/// tens of milliseconds spinning. Walk our modest hierarchies on the caller
/// instead, retaining dirty-subtree skips and precise global change detection.
fn propagate(
    mut roots: Query<
        (
            Entity,
            &Transform,
            &mut GlobalTransform,
            &Children,
            Ref<TransformTreeChanged>,
        ),
        Without<ChildOf>,
    >,
    mut nodes: Query<
        (
            &Transform,
            &mut GlobalTransform,
            Option<&Children>,
            &ChildOf,
            Ref<TransformTreeChanged>,
        ),
        With<ChildOf>,
    >,
    optimizations: Res<StaticTransformOptimizations>,
    mut pending: Local<Vec<Pending>>,
) {
    for (root, local, mut global, children, tree) in &mut roots {
        if optimizations.is_enabled() && !tree.is_changed() {
            continue;
        }
        global.set_if_neq(GlobalTransform::from(*local));
        pending.extend(
            children
                .iter()
                .map(|child| (child, root, *global, global.is_changed())),
        );

        while let Some((entity, parent, parent_global, parent_changed)) = pending.pop() {
            let Ok((local, mut global, children, child_of, tree)) = nodes.get_mut(entity) else {
                continue;
            };
            assert_eq!(
                child_of.parent(),
                parent,
                "inconsistent transform hierarchy"
            );
            if optimizations.is_enabled() && !tree.is_changed() && !parent_changed {
                continue;
            }
            global.set_if_neq(parent_global.mul_transform(*local));
            if let Some(children) = children {
                pending.extend(
                    children
                        .iter()
                        .map(|child| (child, entity, *global, global.is_changed())),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;

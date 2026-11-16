//! Serial hierarchy propagation for our small, frequently animated scenes.
//!
//! Similar transform stalls are tracked in Bevy issue #19411:
//! https://github.com/bevyengine/bevy/issues/19411
use crate::sparse_animation::affine::{self, Locals};
use bevy::{
    app::ValidateParentHasComponentPlugin,
    prelude::*,
    transform::{
        TransformSystems, components::TransformTreeChanged, systems::StaticTransformOptimizations,
    },
};

pub(crate) struct SerialTransformPlugin;

#[derive(Resource, Default, Clone, Copy, serde::Serialize)]
pub(crate) struct Stats {
    #[serde(skip)]
    pub enabled: bool,
    roots: usize,
    nodes: usize,
    evaluations: usize,
}

impl Plugin for SerialTransformPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ValidateParentHasComponentPlugin::<GlobalTransform>::default())
            .init_resource::<StaticTransformOptimizations>()
            .init_resource::<Stats>()
            .add_systems(
                PostStartup,
                (affine::mark_current, mark_dirty_trees, propagate)
                    .chain()
                    .in_set(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                (affine::mark_current, mark_dirty_trees, propagate)
                    .chain()
                    .in_set(TransformSystems::Propagate),
            );
    }
}

type DirtyTransform = Or<(Changed<Transform>, Changed<ChildOf>, Added<GlobalTransform>)>;

// Dirty marking is also serial: these hierarchies are small enough that
// worker handoff and shared queues cost more than the ancestor walk.
fn mark_dirty_trees(
    changed: Query<Entity, DirtyTransform>,
    mut orphaned: RemovedComponents<ChildOf>,
    mut trees: Query<&mut TransformTreeChanged>,
    parents: Query<&ChildOf>,
    optimizations: Res<StaticTransformOptimizations>,
) {
    if !optimizations.is_enabled() {
        return;
    }
    for entity in changed.iter().chain(orphaned.read()) {
        let mut next = entity;
        while let Ok(mut tree) = trees.get_mut(next) {
            if tree.is_changed() && !tree.is_added() {
                break;
            }
            tree.set_changed();
            let Ok(parent) = parents.get(next) else { break };
            next = parent.parent();
        }
    }
}

// Entity, expected parent, parent's world transform and its change flag.
type Pending = (Entity, Entity, GlobalTransform, Vec3, bool);

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
            Option<&Children>,
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
    affine: Option<Res<Locals>>,
    mut pending: Local<Vec<Pending>>,
    mut stats: ResMut<Stats>,
) {
    let _timer = crate::performance::smoothing_profile::Timer::start(3);
    let enabled = stats.enabled;
    *stats = Stats {
        enabled,
        ..default()
    };
    let affine = affine.filter(|affine| !affine.is_empty());
    let parts = |entity, local, parent, offset| {
        affine.as_ref().map_or_else(
            || (parent * GlobalTransform::from(local), offset, false),
            |affine| affine.propagation_parts(entity, local, parent, offset),
        )
    };
    let translated = |pose: GlobalTransform, offset: Vec3| {
        let mut matrix = pose.affine();
        matrix.translation += bevy::math::Vec3A::from(offset);
        GlobalTransform::from(matrix)
    };
    for (root, local, mut global, children, tree) in &mut roots {
        if enabled {
            stats.roots += 1;
        }
        if optimizations.is_enabled() && !tree.is_changed() {
            continue;
        }
        let (base, offset, authored) = parts(root, *local, GlobalTransform::IDENTITY, Vec3::ZERO);
        if enabled {
            stats.evaluations += 1;
        }
        global.set_if_neq(translated(base, offset));
        if let Some(children) = children {
            pending.extend(
                children
                    .iter()
                    .map(|child| (child, root, base, offset, global.is_changed() || authored)),
            );
        }

        while let Some((entity, parent, parent_base, parent_offset, parent_changed)) = pending.pop()
        {
            if enabled {
                stats.nodes += 1;
            }
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
            let (base, offset, authored) = parts(entity, *local, parent_base, parent_offset);
            if enabled {
                stats.evaluations += 1;
            }
            global.set_if_neq(translated(base, offset));
            if let Some(children) = children {
                // A translation boundary can cancel a base movement at this
                // node while descendants select their own offset. Carry the
                // base change even when the composed world matrix is equal.
                let changed = global.is_changed() || authored || parent_changed;
                pending.extend(
                    children
                        .iter()
                        .map(|child| (child, entity, base, offset, changed)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;

//! Keep warmed assets resident without running visibility and skin bounds on
//! inactive worlds. Entity identities, hierarchy and GPU assets remain stable.
use crate::materials::WorldSurface;
use bevy::prelude::*;

pub(super) type Dormant = crate::scene::residency::Dormant<WorldSurface>;

#[derive(Resource, Default, Clone, Copy, serde::Serialize)]
pub(crate) struct Stats {
    pub skipped_publications: usize,
    sleeping_roots: usize,
    sleeping_meshes: usize,
    parks: u64,
    wakes: u64,
}
#[derive(Component)]
struct Managed;
#[derive(Component)]
struct Sleeping {
    dormant: Dormant,
    meshes: usize,
}

type InstanceVisibility = (
    Entity,
    &'static super::Instance,
    &'static Visibility,
    Has<Sleeping>,
);
type VisibilityChange = Or<(Changed<Visibility>, Without<Managed>)>;

pub(super) struct HiddenState {
    instances: QueryState<InstanceVisibility, VisibilityChange>,
    sleeping: QueryState<&'static Sleeping, With<super::Instance>>,
    changes: Vec<(Entity, bool, bool)>,
}
impl FromWorld for HiddenState {
    fn from_world(world: &mut World) -> Self {
        Self {
            instances: world.query_filtered(),
            sleeping: world.query_filtered(),
            changes: Vec::new(),
        }
    }
}

/// Keep invisible variants out of skin/bounds/visibility queries, without
/// discarding their entities, authored hierarchy or resident GPU resources.
pub(super) fn sync_hidden(world: &mut World, mut state: Local<HiddenState>) {
    if world.contains_resource::<super::warm::Draws>() || !world.contains_resource::<super::Art>() {
        return;
    }
    sync_hidden_inner(world, &mut state);
}
fn sync_hidden_inner(world: &mut World, state: &mut HiddenState) {
    state.changes.clear();
    state.changes.extend(
        state
            .instances
            .iter(world)
            .filter(|(_, instance, _, _)| instance.prepared)
            .map(|(entity, _, visibility, sleeping)| {
                (entity, *visibility == Visibility::Hidden, sleeping)
            }),
    );
    let mut parks = 0;
    let mut wakes = 0;
    for (root, hidden, sleeping) in state.changes.drain(..) {
        if hidden && !sleeping {
            let dormant = Dormant::suspend(world, std::iter::once(root));
            let meshes = dormant.mesh_count();
            world.entity_mut(root).insert(Sleeping { dormant, meshes });
            parks += 1;
        } else if !hidden && sleeping {
            world
                .entity_mut(root)
                .take::<Sleeping>()
                .unwrap()
                .dormant
                .resume(world);
            wakes += 1;
        }
        world.entity_mut(root).insert(Managed);
    }
    let (roots, meshes) = state
        .sleeping
        .iter(world)
        .fold((0, 0), |(roots, meshes), sleeping| {
            (roots + 1, meshes + sleeping.meshes)
        });
    let mut stats = world.resource_mut::<Stats>();
    stats.sleeping_roots = roots;
    stats.sleeping_meshes = meshes;
    stats.parks += parks;
    stats.wakes += wakes;
}

/// While a battle presents, no overworld system
/// runs and every world root is hidden. The roots' descendants (about 20k
/// entities) are then disabled, which takes them out of every default query:
/// transform and visibility propagation, change scans, extraction and the
/// frame compiler stop walking them. They are re-enabled before anything in
/// the overworld can run again. Roots stay enabled so world queries see them.
#[derive(Resource, Default)]
pub(super) struct Suspension(bool);
#[derive(Component)]
struct Suspended(Vec<Entity>);

fn battle_presenting(world: &World) -> bool {
    world
        .get_resource::<crate::battle::Owner>()
        .is_some_and(|owner| owner.presenting())
}

pub(super) fn suspend(world: &mut World) {
    if battle_presenting(world) {
        suspend_roots(world);
    }
}
fn suspend_roots(world: &mut World) {
    let mut roots = world
        .query_filtered::<(Entity, &Visibility), (With<super::WorldRoot>, Without<Suspended>)>();
    let roots: Vec<Entity> = roots
        .iter(world)
        .filter(|(_, visibility)| **visibility == Visibility::Hidden)
        .map(|(entity, _)| entity)
        .collect();
    if roots.is_empty() {
        return;
    }
    for root in roots {
        let mut pending: Vec<Entity> = world
            .get::<Children>(root)
            .map(|children| children.iter().collect())
            .unwrap_or_default();
        let mut descendants = Vec::with_capacity(pending.len());
        while let Some(entity) = pending.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                pending.extend(children.iter());
            }
            descendants.push(entity);
        }
        for &entity in &descendants {
            world
                .entity_mut(entity)
                .insert(bevy::ecs::entity_disabling::Disabled);
        }
        world.entity_mut(root).insert(Suspended(descendants));
    }
    world.resource_mut::<Suspension>().0 = true;
}

/// Ordered before every overworld system; does nothing while the battle presents.
pub(super) fn resume(world: &mut World) {
    if world.get_resource::<Suspension>().is_some_and(|s| s.0) && !battle_presenting(world) {
        resume_roots(world);
    }
}
fn resume_roots(world: &mut World) {
    let mut roots = world.query_filtered::<Entity, With<Suspended>>();
    let roots: Vec<Entity> = roots.iter(world).collect();
    for root in roots {
        let Suspended(descendants) = world.entity_mut(root).take::<Suspended>().unwrap();
        for entity in descendants {
            if let Ok(mut entity) = world.get_entity_mut(entity) {
                entity.remove::<bevy::ecs::entity_disabling::Disabled>();
            }
        }
    }
    world.resource_mut::<Suspension>().0 = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::MeshMaterial3d;
    use bevy::{camera::visibility::DynamicSkinnedMeshBounds, mesh::skinning::SkinnedMesh};

    #[test]
    fn suspended_world_leaves_queries_until_resumed_with_roots_and_hierarchy_intact() {
        let mut world = World::new();
        world.init_resource::<Suspension>();
        let hidden = world
            .spawn((
                super::super::WorldRoot,
                Transform::IDENTITY,
                Visibility::Hidden,
            ))
            .id();
        let shown = world
            .spawn((
                super::super::WorldRoot,
                Transform::IDENTITY,
                Visibility::Inherited,
            ))
            .id();
        let bone = world.spawn((Transform::IDENTITY, ChildOf(hidden))).id();
        let leaf = world.spawn((Transform::IDENTITY, ChildOf(bone))).id();
        let visible_child = world.spawn((Transform::IDENTITY, ChildOf(shown))).id();
        let mut transforms = world.query::<(Entity, &Transform)>();
        assert_eq!(transforms.iter(&world).count(), 5);

        suspend_roots(&mut world);
        assert!(world.resource::<Suspension>().0);
        let live: Vec<Entity> = transforms.iter(&world).map(|(e, _)| e).collect();
        assert_eq!(live.len(), 3);
        assert!(live.contains(&hidden) && live.contains(&shown) && live.contains(&visible_child));
        assert_eq!(world.get::<ChildOf>(leaf).map(ChildOf::parent), Some(bone));
        // A second pass leaves suspended roots alone.
        suspend_roots(&mut world);
        assert_eq!(world.get::<Suspended>(hidden).map(|s| s.0.len()), Some(2));

        resume_roots(&mut world);
        assert!(!world.resource::<Suspension>().0);
        assert_eq!(transforms.iter(&world).count(), 5);
        assert!(world.get::<Suspended>(hidden).is_none());
        assert!(
            world
                .get::<bevy::ecs::entity_disabling::Disabled>(leaf)
                .is_none()
        );
    }

    #[test]
    fn invisible_variant_wakes_with_its_current_pose_and_original_gpu_assets() {
        let mut world = World::new();
        world.init_resource::<Stats>();
        let mut meshes = Assets::<Mesh>::default();
        let mesh = meshes.add(Mesh::from(Rectangle::new(1., 1.)));
        let mut materials = Assets::<WorldSurface>::default();
        let material = materials.add(WorldSurface::default());
        let mut bindposes = Assets::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>::default();
        let inverse_bindposes = bindposes.add(
            bevy::mesh::skinning::SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]),
        );
        let root = world
            .spawn((
                super::super::WorldRoot,
                Transform::IDENTITY,
                Visibility::Hidden,
            ))
            .id();
        let bone = world
            .spawn((Transform::from_xyz(1., 2., 3.), ChildOf(root)))
            .id();
        let draw = world
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                SkinnedMesh {
                    inverse_bindposes: inverse_bindposes.clone(),
                    joints: vec![bone],
                },
                DynamicSkinnedMeshBounds,
                Visibility::Inherited,
                ViewVisibility::default(),
                ChildOf(bone),
            ))
            .id();
        world.entity_mut(root).insert(super::super::Instance {
            model: super::super::Model::Actor(1),
            part: 0,
            landmark: None,
            enemy: None,
            flight: None,
            wake: None,
            barrier: None,
            prepared: true,
            materials: vec![material.clone()],
            draws: vec![(draw, 0)],
            motion_draws: vec![],
            binding: crate::sparse_animation::Binding(vec![(bone, Transform::IDENTITY)]),
            clip: Some((0, 0)),
            render: Default::default(),
        });
        let mut hidden = HiddenState::from_world(&mut world);
        sync_hidden_inner(&mut world, &mut hidden);
        assert_eq!(world.resource::<Stats>().sleeping_meshes, 1);
        assert!(world.get::<Sleeping>(root).is_some());
        assert!(world.get::<SkinnedMesh>(draw).is_none());
        world.clear_trackers();
        sync_hidden_inner(&mut world, &mut hidden);
        assert_eq!(
            world.resource::<Stats>().parks,
            1,
            "unchanged hidden variants must not park repeatedly"
        );
        // A whole-world retirement must not steal the sleeping variant's assets.
        let inactive = Dormant::suspend(&mut world, std::iter::once(root));
        let instance = world
            .entity_mut(root)
            .take::<super::super::Instance>()
            .unwrap();
        inactive.resume(&mut world);
        world.entity_mut(root).insert(instance);
        assert!(world.get::<Mesh3d>(draw).is_none());
        for epoch in 1..=3 {
            world.clear_trackers();
            // The visibility owner publishes current state before waking.
            let transform = Transform::from_xyz(epoch as f32 * 30., 4., 8.);
            *world.get_mut::<Transform>(root).unwrap() = transform;
            *world.get_mut::<Transform>(bone).unwrap() = Transform::from_xyz(1., epoch as f32, 3.);
            *world.get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
            sync_hidden_inner(&mut world, &mut hidden);
            assert!(world.get::<Sleeping>(root).is_none());
            assert_eq!(*world.get::<Transform>(root).unwrap(), transform);
            assert_eq!(
                world.get::<Transform>(bone).unwrap().translation.y,
                epoch as f32
            );
            assert_eq!(world.get::<Mesh3d>(draw).unwrap().0.id(), mesh.id());
            assert_eq!(
                world
                    .get::<MeshMaterial3d<WorldSurface>>(draw)
                    .unwrap()
                    .0
                    .id(),
                material.id()
            );
            assert_eq!(world.get::<SkinnedMesh>(draw).unwrap().joints, [bone]);
            assert_eq!(
                world
                    .get::<SkinnedMesh>(draw)
                    .unwrap()
                    .inverse_bindposes
                    .id(),
                inverse_bindposes.id()
            );
            assert!(world.get::<ViewVisibility>(draw).is_some());
            assert_eq!(world.get::<ChildOf>(draw).unwrap().parent(), bone);
            world.clear_trackers();
            *world.get_mut::<Visibility>(root).unwrap() = Visibility::Hidden;
            sync_hidden_inner(&mut world, &mut hidden);
            assert_eq!(world.resource::<Stats>().sleeping_meshes, 1);
            assert!(world.get::<Mesh3d>(draw).is_none());
        }
        assert_eq!(world.resource::<Stats>().parks, 4);
        assert_eq!(world.resource::<Stats>().wakes, 3);
        assert!(meshes.contains(mesh.id()));
        assert!(materials.contains(material.id()));
        assert!(bindposes.contains(inverse_bindposes.id()));
    }
}

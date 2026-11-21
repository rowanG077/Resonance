//! Retain render components while a scene's entities stay outside mesh queries.
//! Bones and transforms stay queryable so hidden accessories can animate and
//! wake with their current pose. Whole-world battle suspension has a separate
//! owner because it deliberately stops those updates too.
use crate::native::MeshMaterial3d;
use bevy::{mesh::skinning::SkinnedMesh, prelude::*};

pub(crate) struct Dormant<M: Asset>(Vec<Node<M>>);

struct Node<M: Asset> {
    entity: Entity,
    mesh: Option<Mesh3d>,
    material: Option<MeshMaterial3d<M>>,
    skin: Option<SkinnedMesh>,
    visible: bool,
}

impl<M: Asset> Default for Dormant<M> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<M: Asset> Dormant<M> {
    pub fn suspend(world: &mut World, roots: impl Iterator<Item = Entity>) -> Self {
        let mut pending: Vec<_> = roots.collect();
        let mut nodes = Vec::new();
        while let Some(entity) = pending.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                pending.extend(children.iter());
            }
            let Ok(mut node) = world.get_entity_mut(entity) else {
                continue;
            };
            let mesh = node.take::<Mesh3d>();
            let material = node.take::<MeshMaterial3d<M>>();
            let skin = node.take::<SkinnedMesh>();
            let visible = node.take::<ViewVisibility>().is_some();
            if mesh.is_some() || material.is_some() || skin.is_some() || visible {
                nodes.push(Node {
                    entity,
                    mesh,
                    material,
                    skin,
                    visible,
                });
            }
        }
        Self(nodes)
    }

    pub fn resume(self, world: &mut World) {
        for node in self.0 {
            let Ok(mut entity) = world.get_entity_mut(node.entity) else {
                continue;
            };
            if let Some(mesh) = node.mesh {
                entity.insert(mesh);
            }
            if let Some(material) = node.material {
                entity.insert(material);
            }
            if let Some(skin) = node.skin {
                entity.insert(skin);
            }
            if node.visible {
                entity.insert(ViewVisibility::default());
            }
        }
    }

    pub fn mesh_count(&self) -> usize {
        self.0.iter().filter(|node| node.mesh.is_some()).count()
    }

    #[cfg(test)]
    pub fn assert_suspended(&self, world: &World) {
        for node in &self.0 {
            assert!(world.get::<Mesh3d>(node.entity).is_none());
            assert!(world.get::<MeshMaterial3d<M>>(node.entity).is_none());
            assert!(world.get::<SkinnedMesh>(node.entity).is_none());
            assert!(world.get::<ViewVisibility>(node.entity).is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::WorldSurface;
    use bevy::camera::visibility::DynamicSkinnedMeshBounds;

    #[test]
    fn inactive_graph_leaves_render_queries_and_resumes_with_the_same_assets_and_entities() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        ));
        let mut materials = Assets::<WorldSurface>::default();
        let material = materials.add(WorldSurface::default());
        let mut bindposes = Assets::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>::default();
        let inverse_bindposes = bindposes.add(
            bevy::mesh::skinning::SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]),
        );
        let root = world.spawn((Transform::IDENTITY, Visibility::Hidden)).id();
        let bone = world
            .spawn((
                Transform::from_xyz(1., 2., 3.),
                Visibility::Inherited,
                ChildOf(root),
            ))
            .id();
        let draw = world
            .spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material.clone()),
                SkinnedMesh {
                    inverse_bindposes: inverse_bindposes.clone(),
                    joints: vec![bone],
                },
                DynamicSkinnedMeshBounds,
                Visibility::Hidden,
                ChildOf(bone),
            ))
            .id();
        let active = world
            .spawn((
                Mesh3d(handle.clone()),
                DynamicSkinnedMeshBounds,
                Visibility::Inherited,
            ))
            .id();
        let count_bounds = |world: &mut World| {
            world
                .query_filtered::<Entity, (With<Mesh3d>, With<DynamicSkinnedMeshBounds>)>()
                .iter(world)
                .count()
        };
        assert_eq!(count_bounds(&mut world), 2);
        for _ in 0..3 {
            let dormant = Dormant::<WorldSurface>::suspend(&mut world, std::iter::once(root));
            assert!(world.get::<Mesh3d>(draw).is_none());
            assert_eq!(count_bounds(&mut world), 1);
            assert_eq!(world.query::<&SkinnedMesh>().iter(&world).count(), 0);
            for entity in [root, bone, draw] {
                assert!(world.get::<ViewVisibility>(entity).is_none());
            }
            assert!(world.get::<Mesh3d>(active).is_some());
            assert!(world.get::<MeshMaterial3d<WorldSurface>>(draw).is_none());
            assert_eq!(world.get::<ChildOf>(draw).unwrap().parent(), bone);
            assert_eq!(
                world.get::<Transform>(bone).unwrap().translation,
                Vec3::new(1., 2., 3.)
            );
            assert!(meshes.contains(handle.id()) && materials.contains(material.id()));
            dormant.resume(&mut world);
            assert_eq!(*world.get::<Visibility>(draw).unwrap(), Visibility::Hidden);
            assert_eq!(count_bounds(&mut world), 2);
            assert_eq!(world.get::<Mesh3d>(draw).unwrap().0.id(), handle.id());
            assert_eq!(
                world
                    .get::<MeshMaterial3d<WorldSurface>>(draw)
                    .unwrap()
                    .0
                    .id(),
                material.id()
            );
            let skin = world.get::<SkinnedMesh>(draw).unwrap();
            assert_eq!(skin.joints, [bone]);
            assert_eq!(skin.inverse_bindposes.id(), inverse_bindposes.id());
            assert!(bindposes.contains(inverse_bindposes.id()));
            for entity in [root, bone, draw] {
                assert!(world.get::<ViewVisibility>(entity).is_some());
            }
        }
        let dormant = Dormant::<WorldSurface>::suspend(&mut world, std::iter::once(root));
        world.despawn(root);
        dormant.resume(&mut world);
        assert!(!world.entities().contains(draw));
    }
}

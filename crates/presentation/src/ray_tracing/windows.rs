//! Make the classroom's actual glass panes emit skylight into the room.
use super::{ActorPart, State, mesh};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    solari::prelude::RaytracingMesh3d,
};
use resonance_content::field::SCENERY_RESOURCE_BASE;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Cache {
    copies: HashMap<Entity, Copy>,
    material: Option<Handle<StandardMaterial>>,
}
struct Copy {
    entity: Entity,
    mesh: Handle<Mesh>,
    transform: Mat4,
}

#[allow(clippy::too_many_arguments)] // Authored pane geometry and its ray-only lighting copies.
pub(super) fn sync(
    mut commands: Commands,
    state: Res<State>,
    roots: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    sources: Query<(&Mesh3d, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Cache>,
) {
    let mut seen = HashSet::new();
    if state.active && state.supported {
        let material = cache
            .material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    emissive: LinearRgba::rgb(22000., 24000., 28000.),
                    ..default()
                })
            })
            .clone();
        for (root, part) in &roots {
            if part.resource != SCENERY_RESOURCE_BASE || !part.prepared {
                continue;
            }
            for entity in children.iter_descendants(root) {
                let Ok((source, global)) = sources.get(entity) else {
                    continue;
                };
                // These are the two authored glass groups, each with two
                // windows. Keep their bevelled tops and gaps between panes.
                let window = std::iter::once(entity)
                    .chain(parents.iter_ancestors(entity))
                    .find_map(|ancestor| match names.get(ancestor).ok()?.as_str() {
                        "glass01" => Some(("side wall", Vec3::NEG_X)),
                        "hide04_a" => Some(("back wall", Vec3::Y)),
                        _ => None,
                    });
                let Some((wall, inward)) = window else {
                    continue;
                };
                seen.insert(entity);
                let transform = global.to_matrix();
                if cache
                    .copies
                    .get(&entity)
                    .is_some_and(|copy| copy.transform == transform)
                {
                    continue;
                }
                let Some(source) = meshes.get(&source.0) else {
                    continue;
                };
                let Some(panes) = light_mesh(source, global, inward) else {
                    continue;
                };
                if let Some(copy) = cache.copies.get_mut(&entity) {
                    *meshes.get_mut(&copy.mesh).unwrap() = panes;
                    copy.transform = transform;
                } else {
                    let mesh = meshes.add(panes);
                    let light = commands
                        .spawn((
                            Name::new(format!("Classroom skylight: {wall} windows")),
                            RaytracingMesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::IDENTITY,
                        ))
                        .id();
                    cache.copies.insert(
                        entity,
                        Copy {
                            entity: light,
                            mesh,
                            transform,
                        },
                    );
                    info!("Classroom skylight bound to the model's {wall} window panes");
                }
            }
        }
    }
    // Cutscene wall hiding must not turn off the physical windows' light.
    // Retire these sources only when their room is removed or Solari is off.
    cache.copies.retain(|source, copy| {
        if seen.contains(source) {
            true
        } else {
            commands.entity(copy.entity).despawn();
            meshes.remove(copy.mesh.id());
            false
        }
    });
}

fn light_mesh(source: &Mesh, global: &GlobalTransform, inward: Vec3) -> Option<Mesh> {
    let positions: Vec<[f32; 3]> = source
        .attribute(Mesh::ATTRIBUTE_POSITION)?
        .as_float3()?
        .iter()
        // Glass sits in the wall recess. Bring emission just inside the trim
        // so opaque authored wall polygons cannot seal off its illumination.
        .map(|p| (global.transform_point(Vec3::from_array(*p)) + inward * 14.).to_array())
        .collect();
    // The glTF loader expands unnormaled glass into unindexed flat triangles.
    let mut indices: Vec<u32> = source.indices().map_or_else(
        || (0..positions.len() as u32).collect(),
        |indices| indices.iter().map(|i| i as u32).collect(),
    );
    for triangle in indices.chunks_exact_mut(3) {
        let [a, b, c] = std::array::from_fn(|i| Vec3::from_array(positions[triangle[i] as usize]));
        if (b - a).cross(c - a).dot(inward) < 0. {
            triangle.swap(1, 2);
        }
    }
    let mut panes = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    panes.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    panes.insert_indices(Indices::U32(indices));
    panes.compute_smooth_normals();
    mesh::prepare(&panes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unindexed_glass_from_the_gltf_loader_still_emits_light() {
        let mut source = Mesh::from(Rectangle::new(100., 200.));
        source.duplicate_vertices();
        assert!(source.indices().is_none());
        let light = light_mesh(&source, &GlobalTransform::IDENTITY, Vec3::Z).unwrap();
        assert_eq!(
            light.indices().unwrap().iter().collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(light.count_vertices(), 6);
    }

    #[test]
    fn panes_on_both_walls_emit_inward_without_changing_the_glass() {
        let mut source = Mesh::from(Rectangle::new(100., 200.));
        source.enable_raytracing = false;
        for (position, rotation, inward) in [
            (
                Vec3::new(370., -300., 180.),
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                Vec3::NEG_X,
            ),
            (
                Vec3::new(140., -730., 180.),
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                Vec3::Y,
            ),
        ] {
            let global = GlobalTransform::from(
                Transform::from_translation(position).with_rotation(rotation),
            );
            let light = light_mesh(&source, &global, inward).unwrap();
            let positions = light
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            assert_eq!(positions.len(), source.count_vertices());
            for p in positions {
                assert!(((Vec3::from_array(*p) - position).dot(inward) - 14.).abs() < 0.001);
            }
            let indices: Vec<_> = light.indices().unwrap().iter().collect();
            for triangle in indices.chunks_exact(3) {
                let [a, b, c] = std::array::from_fn(|i| Vec3::from_array(positions[triangle[i]]));
                assert!((b - a).cross(c - a).normalize().dot(inward) > 0.999);
            }
            assert!(light.enable_raytracing);
        }
        assert!(!source.enable_raytracing);
        assert!(
            source
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap()
                .iter()
                .all(|p| p[2] == 0.)
        );
    }
}

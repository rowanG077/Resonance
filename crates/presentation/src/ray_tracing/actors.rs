//! Pose-matched opaque characters that cast AND receive Solari lighting.
//! Retain authored cutouts, outlines and effects on their original path.
use super::{ActorPart, State, TitleSurface, mesh};
use bevy::{
    camera::visibility::NoFrustumCulling,
    material::OpaqueRendererMethod,
    math::Affine2,
    mesh::{
        VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::*,
    solari::prelude::RaytracingMesh3d,
};
use resonance_content::field::SCENERY_RESOURCE_BASE;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Cache {
    copies: HashMap<Entity, Copy>,
    images: HashMap<AssetId<Image>, Handle<Image>>,
}
struct Copy {
    entity: Entity,
    mesh: Handle<Mesh>,
    raster_mesh: Handle<Mesh>,
    pose: Vec<Mat4>,
    original: Handle<TitleSurface>,
    material: Handle<StandardMaterial>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn sync(
    mut commands: Commands,
    state: Res<State>,
    roots: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    sources: Query<(
        &Mesh3d,
        Option<&MeshMaterial3d<TitleSurface>>,
        &InheritedVisibility,
        Option<&SkinnedMesh>,
    )>,
    transforms: Query<&GlobalTransform>,
    bindposes: Res<Assets<SkinnedMeshInverseBindposes>>,
    surfaces: Res<Assets<TitleSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: Local<Cache>,
) {
    let was_empty = cache.copies.is_empty();
    let mut seen = HashSet::new();
    if state.active && state.supported {
        for (root, part) in &roots {
            if part.resource >= SCENERY_RESOURCE_BASE || !part.prepared {
                continue;
            }
            for source_entity in children.iter_descendants(root) {
                let Ok((source, binding, visibility, skin)) = sources.get(source_entity) else {
                    continue;
                };
                let Some(original) = binding
                    .map(|b| b.0.clone())
                    .or_else(|| cache.copies.get(&source_entity).map(|c| c.original.clone()))
                else {
                    continue;
                };
                let Some(surface) = surfaces.get(&original) else {
                    continue;
                };
                if !visibility.get()
                    || surface.blend
                    || surface.additive
                    || surface.constant_color
                    || surface.multiply.is_some()
                    || !surface.depth_write
                {
                    continue;
                }
                let pose: Option<Vec<Mat4>> = if let Some(skin) = skin {
                    bindposes.get(&skin.inverse_bindposes).and_then(|bind| {
                        (bind.len() == skin.joints.len())
                            .then(|| {
                                skin.joints
                                    .iter()
                                    .zip(bind.iter())
                                    .map(|(joint, inverse)| {
                                        transforms
                                            .get(*joint)
                                            .ok()
                                            .map(|global| global.to_matrix() * *inverse)
                                    })
                                    .collect()
                            })
                            .flatten()
                    })
                } else {
                    transforms
                        .get(source_entity)
                        .ok()
                        .map(|global| vec![global.to_matrix()])
                };
                let Some(pose) = pose else {
                    continue;
                };
                let texture = if let Some(source) = &surface.color {
                    if let Some(image) = cache.images.get(&source.id()) {
                        Some(image.clone())
                    } else {
                        let Some(image) = images.get(source) else {
                            continue;
                        };
                        let mut image = image.clone();
                        image.texture_descriptor.format =
                            image.texture_descriptor.format.add_srgb_suffix();
                        let image = images.add(image);
                        cache.images.insert(source.id(), image.clone());
                        Some(image)
                    }
                } else {
                    None
                };
                seen.insert(source_entity);
                if let Some(copy) = cache.copies.get(&source_entity) {
                    let material = materials.get(&copy.material).unwrap();
                    let color = Color::srgb(surface.tint.x, surface.tint.y, surface.tint.z);
                    let uv = Affine2::from_scale_angle_translation(
                        surface.uv_scales.xy(),
                        0.,
                        surface.uv_offsets.xy(),
                    );
                    if material.base_color != color
                        || material.uv_transform != uv
                        || material.base_color_texture != texture
                    {
                        let mut material = materials.get_mut(&copy.material).unwrap();
                        material.base_color = color;
                        material.uv_transform = uv;
                        material.base_color_texture = texture.clone();
                    }
                }
                if cache
                    .copies
                    .get(&source_entity)
                    .is_some_and(|copy| copy.pose == pose)
                {
                    continue;
                }
                let Some(source) = meshes.get(&source.0) else {
                    continue;
                };
                let Some(posed) = posed_mesh(source, skin.is_some(), &pose) else {
                    continue;
                };
                let mut raster = posed.clone();
                // Solari's acceleration layout cannot contain vertex colors,
                // but several authored props use them as their base color.
                // Keep a separate raster mesh to populate the correct albedo.
                raster.enable_raytracing = false;
                if let Some(colors) = source.attribute(Mesh::ATTRIBUTE_COLOR) {
                    raster.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors.clone());
                }
                if let Some(copy) = cache.copies.get_mut(&source_entity) {
                    *meshes.get_mut(&copy.mesh).unwrap() = posed;
                    *meshes.get_mut(&copy.raster_mesh).unwrap() = raster;
                    copy.pose = pose;
                } else {
                    let mesh = meshes.add(posed);
                    let raster_mesh = meshes.add(raster);
                    let material = materials.add(StandardMaterial {
                        base_color: Color::srgb(surface.tint.x, surface.tint.y, surface.tint.z),
                        base_color_texture: texture,
                        uv_transform: Affine2::from_scale_angle_translation(
                            surface.uv_scales.xy(),
                            0.,
                            surface.uv_offsets.xy(),
                        ),
                        // The character atlases are painted toon art with
                        // highlights already in the texture. A generic PBR
                        // sheen makes skin, hair and cloth all look varnished.
                        // Keep diffuse lighting/occlusion with a matte response;
                        // rigid props retain their separate material settings.
                        perceptual_roughness: if surface.toon_ramp.is_some() {
                            1.
                        } else {
                            0.85
                        },
                        reflectance: if surface.toon_ramp.is_some() {
                            0.
                        } else {
                            0.25
                        },
                        opaque_render_method: OpaqueRendererMethod::Deferred,
                        double_sided: surface.cull == resonance_content::CullFace::None,
                        cull_mode: match surface.cull {
                            resonance_content::CullFace::Back => {
                                Some(bevy::render::render_resource::Face::Back)
                            }
                            resonance_content::CullFace::Front => {
                                Some(bevy::render::render_resource::Face::Front)
                            }
                            resonance_content::CullFace::None => None,
                        },
                        ..default()
                    });
                    let entity = commands
                        .spawn((
                            Mesh3d(raster_mesh.clone()),
                            RaytracingMesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::IDENTITY,
                            // Vertices are already posed in world space and
                            // move beyond the first pose's cached bounds.
                            NoFrustumCulling,
                        ))
                        .id();
                    commands
                        .entity(source_entity)
                        .remove::<MeshMaterial3d<TitleSurface>>();
                    cache.copies.insert(
                        source_entity,
                        Copy {
                            entity,
                            mesh,
                            raster_mesh,
                            pose,
                            original,
                            material,
                        },
                    );
                }
            }
        }
    }
    cache.copies.retain(|source, copy| {
        if seen.contains(source) {
            true
        } else {
            commands.entity(copy.entity).despawn();
            meshes.remove(copy.mesh.id());
            meshes.remove(copy.raster_mesh.id());
            materials.remove(copy.material.id());
            if let Ok(mut source) = commands.get_entity(*source) {
                source.insert(MeshMaterial3d(copy.original.clone()));
            }
            false
        }
    });
    if !state.active || !state.supported {
        cache.images.clear();
    }
    if was_empty && !cache.copies.is_empty() {
        info!(
            "Classroom: {} opaque character/prop meshes cast and receive ray-traced lighting",
            cache.copies.len()
        );
    }
}

fn posed_mesh(source: &Mesh, skinned: bool, pose: &[Mat4]) -> Option<Mesh> {
    let positions = source.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
    let transforms: Vec<Mat4> = if skinned {
        let VertexAttributeValues::Uint16x4(joints) =
            source.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)?
        else {
            return None;
        };
        let VertexAttributeValues::Float32x4(weights) =
            source.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)?
        else {
            return None;
        };
        if joints.len() != positions.len() || weights.len() != positions.len() {
            return None;
        }
        joints
            .iter()
            .zip(weights)
            .map(|(joints, weights)| {
                let mut world = Mat4::ZERO;
                for (&joint, &weight) in joints.iter().zip(weights) {
                    if weight != 0. {
                        world += *pose.get(usize::from(joint))? * weight;
                    }
                }
                Some(world)
            })
            .collect::<Option<_>>()?
    } else {
        vec![*pose.first()?; positions.len()]
    };
    let positions: Vec<[f32; 3]> = positions
        .iter()
        .zip(&transforms)
        .map(|(p, transform)| transform.transform_point3(Vec3::from_array(*p)).to_array())
        .collect();
    let mut posed = source.clone();
    posed.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    // Match the authored vertex shader's inverse-transpose skinning. Rebuilding
    // normals from split UV triangles makes the HD faces look faceted.
    if !skinned {
        // Rigid props include authored toon normals that point into their own
        // triangles. They self-occlude in PBR; use their geometric normals.
        posed.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
    } else if let Some(normals) = source
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(|n| n.as_float3())
    {
        if normals.len() != transforms.len() {
            return None;
        }
        let normals: Vec<[f32; 3]> = normals
            .iter()
            .zip(&transforms)
            .map(|(n, transform)| {
                transform
                    .inverse()
                    .transpose()
                    .transform_vector3(Vec3::from_array(*n))
                    .normalize_or_zero()
                    .to_array()
            })
            .collect();
        posed.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    }
    mesh::prepare(&posed).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rigid_props_replace_inward_toon_normals_without_changing_the_source() {
        let mut source = Mesh::from(Rectangle::new(1., 1.));
        source.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., -1.]; 4]);
        let posed = posed_mesh(&source, false, &[Mat4::IDENTITY]).unwrap();
        assert!(
            posed
                .attribute(Mesh::ATTRIBUTE_NORMAL)
                .unwrap()
                .as_float3()
                .unwrap()
                .iter()
                .all(|n| n[2] > 0.999)
        );
        assert!(
            source
                .attribute(Mesh::ATTRIBUTE_NORMAL)
                .unwrap()
                .as_float3()
                .unwrap()
                .iter()
                .all(|n| n[2] == -1.)
        );
    }

    #[test]
    fn skinned_normals_follow_joint_rotation_and_nonuniform_scale() {
        let mut source = Mesh::from(Rectangle::new(1., 1.));
        let normal = Vec3::new(1., 0., 1.).normalize();
        source.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal.to_array(); 4]);
        source.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0]; 4]),
        );
        source.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1., 0., 0., 0.]; 4]);
        let rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let pose =
            Mat4::from_scale_rotation_translation(Vec3::new(2., 1., 1.), rotation, Vec3::Y * 5.);
        let posed = posed_mesh(&source, true, &[pose]).unwrap();
        let expected = rotation * Vec3::new(0.5, 0., 1.).normalize();
        for n in posed
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
            .as_float3()
            .unwrap()
        {
            assert!(Vec3::from_array(*n).distance(expected) < 0.0001);
        }
    }

    #[test]
    fn shadow_copy_uses_weighted_world_pose_without_mutating_raster_mesh() {
        let mut source = Mesh::from(Rectangle::new(1., 1.));
        source.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0, 1, 0, 0]; 4]),
        );
        source.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[0.25, 0.75, 0., 0.]; 4]);
        let original = source
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap()[0];
        let posed = posed_mesh(
            &source,
            true,
            &[Mat4::IDENTITY, Mat4::from_translation(Vec3::Z * 4.)],
        )
        .unwrap();
        let position = posed
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap()[0];
        assert_eq!(
            Vec3::from_array(position),
            Vec3::from_array(original) + Vec3::Z * 3.
        );
        assert!(source.contains_attribute(Mesh::ATTRIBUTE_JOINT_INDEX));
        assert!(!posed.contains_attribute(Mesh::ATTRIBUTE_JOINT_INDEX));
    }
}

//! Kendama strings use the same camera-facing segments as battle rendering.
use super::*;
use crate::{sparse_animation::affine::Helper, weapon_geometry::Vertices};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    mut commands: Commands,
    state: State,
    cache: Res<EquipmentCache>,
    mut roots: Query<(Entity, &mut Instance)>,
    mut transforms: ParamSet<(Helper, ResMut<Locals>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut orders: Query<&mut DrawOrder>,
    mut visibility: Query<&mut Visibility, Without<Instance>>,
) {
    let world = &state.get().events.world;
    let Some(camera) = &world.field_camera else {
        return;
    };
    let view = crate::field_view::camera_transform(camera);
    let mut draws = Vec::new();
    {
        let helper = transforms.p0();
        for (entity, mut root) in &mut roots {
            if !root.prepared {
                continue;
            }
            let layer = &cache.equipment[&root.item].layers[root.layer];
            if layer.preview.scene.outline_color.is_some() {
                continue;
            }
            let names = &layer.art.spec.bone_names;
            if !names.iter().any(|n| n == "SP_himo01") {
                continue;
            }
            let Some(binding) = &root.binding else {
                continue;
            };
            let Some(chain) = binding.0.get(..10) else {
                continue;
            };
            let mut data = Vertices::default();
            let mut visible = false;
            let distance = helper
                .compute_global_transform(entity)
                .map(|m| m.translation().distance(view.translation))
                .unwrap_or_default();
            let width = crate::weapon_geometry::string_width(distance);
            // The authored SP_himo chain starts at the kendama body. The ball
            // is not a separate tenth segment in the original battle renderer.
            for pair in chain.windows(2) {
                if let (Ok(a), Ok(b)) = (
                    helper.compute_global_transform(pair[0].0),
                    helper.compute_global_transform(pair[1].0),
                ) {
                    data.line(
                        a.translation(),
                        b.translation(),
                        view.translation,
                        view.rotation,
                        width,
                        camera.fov_degrees().to_radians(),
                    );
                }
            }
            if let Some(actor) = world.actors.get(&root.actor) {
                visible = !layer.art.spec.material_nodes.iter().flatten().any(|&n| {
                    (names[usize::from(n)].starts_with("SP_himo")
                        || names[usize::from(n)] == "obj_gen000_tama")
                        && actor
                            .appearance
                            .hidden_nodes
                            .contains(&root.choreography.as_ref().unwrap().bones[usize::from(n)])
                });
            }
            let source = root
                .materials
                .first()
                .and_then(|h| surfaces.get(h))
                .cloned()
                .unwrap_or_default();
            let surface = TitleSurface {
                color: None,
                sampling: None,
                multiply: None,
                blend: Some(Blend::Alpha),
                depth_equal: true,
                cull: resonance_content::CullFace::None,
                toon_ramp: None,
                vertex_color: true,
                constant_color: false,
                ..source
            };
            let order = root
                .geometry
                .first()
                .and_then(|(_, e)| orders.get(*e).ok())
                .copied();
            let mesh = data.mesh();
            let draw = if let Some((draw, handle, material)) = &root.rope {
                if let Some(mut current) = meshes.get_mut(handle) {
                    *current = mesh;
                }
                if let Some(mut current) = surfaces.get_mut(material) {
                    *current = surface;
                }
                *draw
            } else {
                let handle = meshes.add(mesh);
                let material = surfaces.add(surface);
                let draw = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        crate::field_view::ActorGeometry(root.actor),
                        MeshMaterial3d(material.clone()),
                        ChildOf(entity),
                        Transform::default(),
                        Visibility::Inherited,
                        bevy::camera::visibility::NoFrustumCulling,
                    ))
                    .id();
                root.rope = Some((draw, handle, material));
                draw
            };
            if let Some(order) = order {
                if let Ok(mut current) = orders.get_mut(draw) {
                    *current = order;
                } else {
                    commands.entity(draw).insert(order);
                }
            }
            let next = if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if let Ok(mut current) = visibility.get_mut(draw) {
                *current = next;
            } else {
                commands.entity(draw).insert(next);
            }
            draws.push(draw);
        }
    }
    let mut affine = transforms.p1();
    for draw in draws {
        affine.set_world(draw, GlobalTransform::IDENTITY);
    }
}

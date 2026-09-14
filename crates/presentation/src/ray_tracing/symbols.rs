//! Composite head symbols with the ungraded dialogue overlay. Their authored
//! world-space quads retain their screen position, size, UVs and opacity.
use super::State;
use crate::{FieldCamera, FieldOverlayCamera, field_effects::HeadSymbol, materials::TitleSurface};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Cache(HashMap<Entity, Copy>);
struct Copy {
    mesh: Handle<Mesh>,
    material: Handle<ColorMaterial>,
    original: Handle<TitleSurface>,
}

#[allow(clippy::too_many_arguments)] // Two view transforms, authored art and overlay copies.
pub(super) fn sync(
    mut commands: Commands,
    state: Res<State>,
    display: Res<crate::display::Display>,
    cameras: Query<(&Projection, &GlobalTransform), With<FieldCamera>>,
    overlays: Query<&GlobalTransform, With<FieldOverlayCamera>>,
    symbols: Query<(Entity, &Mesh3d, &GlobalTransform, &HeadSymbol)>,
    surfaces: Res<Assets<TitleSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut cache: Local<Cache>,
) {
    let mut seen = HashSet::new();
    if state.active
        && let Ok((projection, camera)) = cameras.single()
        && let Ok(overlay) = overlays.single()
    {
        let clip_from_world = projection.get_clip_from_view() * camera.to_matrix().inverse();
        for (entity, source, transform, symbol) in &symbols {
            let Some(source) = meshes.get(&source.0) else {
                continue;
            };
            let Some(projected) = project(
                source,
                clip_from_world * transform.to_matrix(),
                transform.to_matrix().inverse() * overlay.to_matrix(),
                display.0.ui_size(),
            ) else {
                continue;
            };
            if let Some(copy) = cache.0.get(&entity) {
                *meshes.get_mut(&copy.mesh).unwrap() = projected;
            } else {
                let original = symbol.material.clone();
                let Some(surface) = surfaces.get(&original) else {
                    continue;
                };
                let mesh = meshes.add(projected);
                // These atlases contain encoded artwork, just like the UI.
                // The LDR overlay samples them directly without a scene grade.
                let material = materials.add(ColorMaterial {
                    texture: surface.color.clone(),
                    ..default()
                });
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<TitleSurface>>()
                    .insert((Mesh2d(mesh.clone()), MeshMaterial2d(material.clone())));
                cache.0.insert(
                    entity,
                    Copy {
                        mesh,
                        material,
                        original,
                    },
                );
            }
            seen.insert(entity);
        }
    }
    cache.0.retain(|entity, copy| {
        if seen.contains(entity) {
            return true;
        }
        if symbols.get(*entity).is_ok() {
            commands
                .entity(*entity)
                .remove::<(Mesh2d, MeshMaterial2d<ColorMaterial>)>()
                .insert(MeshMaterial3d(copy.original.clone()));
        }
        meshes.remove(copy.mesh.id());
        materials.remove(copy.material.id());
        false
    });
}

fn project(
    source: &Mesh,
    clip_from_local: Mat4,
    local_from_overlay: Mat4,
    canvas: Vec2,
) -> Option<Mesh> {
    let positions = source.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
    let positions: Option<Vec<[f32; 3]>> = positions
        .iter()
        .map(|p| {
            let clip = clip_from_local * Vec3::from_array(*p).extend(1.);
            if clip.w <= 0. || !clip.is_finite() {
                return None;
            }
            let screen = clip.truncate().truncate() / clip.w * canvas * 0.5;
            Some(
                local_from_overlay
                    .transform_point3(screen.extend(0.))
                    .to_array(),
            )
        })
        .collect();
    let mut projected = source.clone();
    projected.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions?);
    projected.enable_raytracing = false;
    Some(projected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::CameraProjection;

    #[test]
    fn toggling_symbols_restores_the_original_material_and_releases_copies() {
        let mut app = App::new();
        app.insert_resource(State {
            enabled: true,
            supported: true,
            software: false,
            active: true,
            pathtracing: false,
        })
        .insert_resource(crate::display::Display(crate::Resolution::default()))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<TitleSurface>>()
        .init_resource::<Assets<ColorMaterial>>()
        .add_systems(Update, sync);
        app.world_mut().spawn((
            FieldCamera,
            Projection::Orthographic(OrthographicProjection::default_2d()),
            GlobalTransform::IDENTITY,
        ));
        app.world_mut()
            .spawn((FieldOverlayCamera, GlobalTransform::IDENTITY));
        let original = app
            .world_mut()
            .resource_mut::<Assets<TitleSurface>>()
            .add(TitleSurface::default());
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(20., 30.));
        let symbol = app
            .world_mut()
            .spawn((
                HeadSymbol {
                    material: original.clone(),
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(original.clone()),
                GlobalTransform::IDENTITY,
            ))
            .id();
        for _ in 0..2 {
            app.world_mut().resource_mut::<State>().active = true;
            app.update();
            assert!(
                app.world()
                    .get::<MeshMaterial3d<TitleSurface>>(symbol)
                    .is_none()
            );
            assert!(
                app.world()
                    .get::<MeshMaterial2d<ColorMaterial>>(symbol)
                    .is_some()
            );
            assert_eq!(app.world().get::<Mesh3d>(symbol).unwrap().0, mesh);
            app.world_mut().resource_mut::<State>().active = false;
            app.update();
            assert!(app.world().get::<Mesh2d>(symbol).is_none());
            assert!(
                app.world()
                    .get::<MeshMaterial2d<ColorMaterial>>(symbol)
                    .is_none()
            );
            assert_eq!(
                app.world()
                    .get::<MeshMaterial3d<TitleSurface>>(symbol)
                    .unwrap()
                    .0,
                original
            );
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
            assert!(app.world().resource::<Assets<ColorMaterial>>().is_empty());
        }
    }

    #[test]
    fn projecting_head_symbols_preserves_screen_alignment_and_artwork() {
        let source = Mesh::from(Rectangle::new(30., 40.));
        let camera = Transform::from_xyz(0., -400., 200.).looking_at(Vec3::ZERO, Vec3::Z);
        let symbol =
            Transform::from_translation(Vec3::new(35., 0., 20.)).with_rotation(camera.rotation);
        for canvas in [Vec2::new(640., 480.), Vec2::new(480. * 16. / 9., 480.)] {
            let projection = crate::camera::TitleProjection(PerspectiveProjection {
                aspect_ratio: canvas.x / canvas.y,
                near: 100.,
                ..default()
            });
            let clip_from_local =
                projection.get_clip_from_view() * camera.to_matrix().inverse() * symbol.to_matrix();
            let overlay = crate::camera::overlay_alignment();
            let result = project(
                &source,
                clip_from_local,
                symbol.to_matrix().inverse() * overlay.to_matrix(),
                canvas,
            )
            .unwrap();
            let original = source
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            let projected = result
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            for (before, after) in original.iter().zip(projected) {
                let expected = clip_from_local.project_point3(Vec3::from_array(*before));
                let actual = overlay
                    .to_matrix()
                    .inverse()
                    .transform_point3(symbol.transform_point(Vec3::from_array(*after)));
                assert!((actual.truncate() * 2. / canvas - expected.truncate()).length() < 0.00001);
            }
            assert_eq!(
                result.attribute(Mesh::ATTRIBUTE_UV_0),
                source.attribute(Mesh::ATTRIBUTE_UV_0)
            );
            assert_eq!(result.indices(), source.indices());
            assert!(!result.enable_raytracing);
        }
    }
}

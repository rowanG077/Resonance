//! Warm recipes before any draw exists, then check their first real rendering.
use super::*;
use crate::{
    gpu_prepare,
    materials::{
        GeometryData, GeometryMesh, GeometryVertex, TitleSurface, WorldRecords, WorldSurface,
    },
    native::{DistanceFog, MeshMaterial2d, MeshMaterial3d, Prepare},
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, ScalingMode, visibility::NoFrustumCulling},
    mesh::{
        VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
};
use resonance_content::menu_data::AntiAliasing;

#[test]
#[ignore = "requires a GPU; verifies warmup against first real draws and pixels"]
fn prepared_recipes_render_without_first_use_pipeline_creation() -> anyhow::Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins());
    let target = super::tests::image(&mut app, 32);
    crate::test_support::native_scene(&mut app, target.clone());
    gpu_prepare::install(&mut app);
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1., 1.));
    let mut skinned = Mesh::from(Rectangle::new(1., 1.));
    let count = skinned.count_vertices();
    skinned.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(vec![[0; 4]; count]),
    );
    skinned.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1., 0., 0., 0.]; count]);
    let skinned = app.world_mut().resource_mut::<Assets<Mesh>>().add(skinned);
    let streamed = app.world().resource::<GeometryMesh>().0.clone();
    let title = TitleSurface {
        constant_color: true,
        tint: Vec4::new(1., 0., 0., 1.),
        ..default()
    };
    let world = WorldSurface {
        constant_color: true,
        tint: Vec4::new(0., 1., 0., 1.),
        ..default()
    };
    let preparation = gpu_prepare::Shared::new(app.world());
    {
        let mut report = preparation.0.lock().unwrap();
        for view in [gpu_prepare::View::Field, gpu_prepare::View::Battle] {
            for samples in [1, 4] {
                for (mesh, streamed) in [(&mesh, false), (&skinned, false), (&streamed, true)] {
                    report.prepare(view, mesh.id(), &title, streamed, Some(samples));
                }
                report.prepare(view, mesh.id(), &world, false, Some(samples));
                report.prepare(view, mesh.id(), Prepare::Backdrop, false, Some(samples));
            }
        }
        report.armed = true;
    }
    crate::test_support::finish_app(&mut app)?;
    crate::test_support::render_until(
        &mut app,
        "recipes did not warm without draw entities",
        |_| {
            let report = preparation.0.lock().unwrap();
            assert!(report.error.is_none(), "{:?}", report.error);
            report.completed.load(std::sync::atomic::Ordering::Acquire)
        },
    )?;
    let warmed = preparation.0.lock().unwrap().pipelines.clone();

    // Real material assets are only created after preparation completes. Pipeline
    // readiness must not depend on placeholder asset lifetime or asset events.
    let title = app
        .world_mut()
        .resource_mut::<Assets<TitleSurface>>()
        .add(title);
    let world = app
        .world_mut()
        .resource_scope(|world_mut, mut records: Mut<WorldRecords>| {
            records.add(world, &mut world_mut.resource_mut::<Assets<WorldSurface>>())
        });
    let captured = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        ));
    let backdrop = app
        .world_mut()
        .resource_mut::<Assets<crate::menu_backdrop::Material>>()
        .add(
            crate::menu_backdrop::Material::visible(captured)
                .with_cover(Vec4::new(1., 0., 0., 0.5)),
        );
    let inverse_bindposes = app
        .world_mut()
        .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
        .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]));
    let joint = app.world_mut().spawn(Transform::IDENTITY).id();
    let projection = Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::Fixed {
            width: 2.,
            height: 2.,
        },
        ..OrthographicProjection::default_3d()
    });
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: Color::BLACK.into(),
                ..default()
            },
            projection.clone(),
            RenderTarget::Image(target.clone().into()),
            Transform::from_xyz(0., 0., 4.).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    let overlay = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                is_active: false,
                clear_color: Color::BLACK.into(),
                ..default()
            },
            crate::anti_aliasing::SceneMsaa,
            projection,
            RenderTarget::Image(target.clone().into()),
            Transform::from_xyz(0., 0., 4.).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    for mode in [AntiAliasing::Off, AntiAliasing::Msaa4] {
        app.world_mut()
            .resource_mut::<crate::anti_aliasing::AntiAliasingSettings>()
            .0 = mode;
        for fog in [false, true] {
            if fog {
                app.world_mut().entity_mut(camera).insert(DistanceFog {
                    start: 100.,
                    end: 200.,
                    ..default()
                });
            } else {
                app.world_mut().entity_mut(camera).remove::<DistanceFog>();
            }
            for kind in 0..5 {
                app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = kind != 4;
                app.world_mut()
                    .get_mut::<Camera>(overlay)
                    .unwrap()
                    .is_active = kind == 4;
                let mut draw = app
                    .world_mut()
                    .spawn((Transform::IDENTITY, NoFrustumCulling));
                match kind {
                    0 => {
                        draw.insert((Mesh3d(mesh.clone()), MeshMaterial3d(title.clone())));
                    }
                    1 => {
                        draw.insert((
                            Mesh3d(skinned.clone()),
                            MeshMaterial3d(title.clone()),
                            SkinnedMesh {
                                inverse_bindposes: inverse_bindposes.clone(),
                                joints: vec![joint],
                            },
                        ));
                    }
                    2 => {
                        let geometry = GeometryData::default();
                        geometry.replace(
                            [[-0.5, -0.5, 0., 1.], [0.5, -0.5, 0., 1.], [0., 0.5, 0., 1.]]
                                .into_iter()
                                .map(|position| GeometryVertex {
                                    position,
                                    normal: [0., 0., 1., 0.],
                                    uv: [0.; 4],
                                    color: [1.; 4],
                                }),
                        );
                        draw.insert((
                            Mesh3d(streamed.clone()),
                            MeshMaterial3d(title.clone()),
                            geometry,
                        ));
                    }
                    3 => {
                        draw.insert((Mesh3d(mesh.clone()), MeshMaterial3d(world.clone())));
                    }
                    _ => {
                        draw.insert((Mesh2d(mesh.clone()), MeshMaterial2d(backdrop.clone())));
                    }
                }
                let entity = draw.id();
                crate::test_support::render_until(
                    &mut app,
                    "first use did not reach the renderer",
                    |app| {
                        let render = app.sub_app(RenderApp).world();
                        render.resource::<Drawn>().draws.iter().any(|(draw, pipeline)| {
                        if *draw != entity.into() { return false; }
                        assert!(warmed.contains(pipeline), "{mode:?}, fog={fog}, kind={kind}: first use compiled {pipeline:?}");
                        render.resource::<PipelineCache>().get_render_pipeline(*pipeline).is_some()
                    })
                    },
                )?;
                let pixels = crate::test_support::capture_image(&mut app, target.clone())?
                    .try_into_dynamic()?
                    .to_rgba8();
                let expected = match kind {
                    3 => [0, 255, 0, 255],
                    4 => [255, 128, 128, 255],
                    _ => [255, 0, 0, 255],
                };
                assert_eq!(
                    pixels.get_pixel(16, 16).0,
                    expected,
                    "{mode:?}, fog={fog}, kind={kind}"
                );
                assert_eq!(pixels.get_pixel(0, 0).0, [0, 0, 0, 255]);
                app.world_mut().despawn(entity);
            }
        }
    }
    Ok(())
}

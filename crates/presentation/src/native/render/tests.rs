use super::*;
use crate::{menu_backdrop, native::MeshMaterial2d};
use bevy::{asset::RenderAssetUsages, camera::RenderTarget};

fn wait(app: &mut App, message: &str, ready: impl Fn(&App) -> bool) {
    crate::test_support::render_until(app, message, ready).unwrap();
}

fn renderer(app: &App) -> &Renderer {
    app.sub_app(RenderApp).world().resource::<Renderer>()
}

fn frame_state(app: &App) -> &FrameState {
    app.sub_app(RenderApp).world().resource::<FrameState>()
}

pub(super) fn image(app: &mut App, size: u32) -> Handle<Image> {
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            size,
            size,
            TextureFormat::Bgra8Unorm,
            None,
        ))
}

#[test]
#[ignore = "requires a GPU; checks indexed strips and cached index-format variants"]
fn strip_meshes_render_with_both_index_formats_and_without_indices() {
    use bevy::{camera::visibility::NoFrustumCulling, mesh::Indices};
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins());
    let scene = image(&mut app, 64);
    crate::test_support::native_scene(&mut app, scene.clone());
    let material = app
        .world_mut()
        .resource_mut::<Assets<crate::TitleSurface>>()
        .add(crate::TitleSurface {
            tint: Vec4::ONE,
            constant_color: true,
            ..default()
        });
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(2., 2.));
    app.world_mut().spawn((
        Mesh3d(mesh.clone()),
        crate::native::MeshMaterial3d(material),
        Transform::IDENTITY,
        NoFrustumCulling,
    ));
    app.world_mut().spawn((
        Camera3d::default(),
        Msaa::Off,
        RenderTarget::Image(scene.clone().into()),
        Transform::from_xyz(0., 0., 4.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    crate::test_support::finish_app(&mut app).unwrap();

    for (topology, indices) in [
        (
            PrimitiveTopology::TriangleList,
            Some(Indices::U16(vec![0, 1, 2, 2, 1, 3])),
        ),
        (
            PrimitiveTopology::TriangleStrip,
            Some(Indices::U16(vec![0, 1, 2, 3])),
        ),
        (
            PrimitiveTopology::TriangleStrip,
            Some(Indices::U32(vec![0, 1, 2, 3])),
        ),
        (PrimitiveTopology::TriangleStrip, None),
        (
            PrimitiveTopology::LineStrip,
            Some(Indices::U16(vec![0, 1, 2, 3])),
        ),
        (
            PrimitiveTopology::LineStrip,
            Some(Indices::U32(vec![0, 1, 2, 3])),
        ),
        (PrimitiveTopology::LineStrip, None),
    ] {
        let index_format = indices.as_ref().map(|indices| match indices {
            Indices::U16(_) => IndexFormat::Uint16,
            Indices::U32(_) => IndexFormat::Uint32,
        });
        let mut geometry = Mesh::new(topology, RenderAssetUsages::default())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[-1., -1., 0.], [1., -1., 0.], [-1., 1., 0.], [1., 1., 0.]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_0,
                vec![[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
            );
        if let Some(indices) = indices {
            geometry.insert_indices(indices);
        }
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .insert(mesh.id(), geometry)
            .unwrap();
        wait(&mut app, "strip pipeline not ready", |app| {
            let world = app.sub_app(RenderApp).world();
            world
                .resource::<RenderAssets<RenderMesh>>()
                .get(mesh.id())
                .is_some_and(|mesh| {
                    mesh.primitive_topology() == topology && mesh.index_format() == index_format
                })
                && frame_state(app).resolved.iter().flatten().any(|draw| {
                    world
                        .resource::<PipelineCache>()
                        .get_render_pipeline(draw.pipeline)
                        .is_some()
                })
        });
        let pixels = crate::test_support::capture_image(&mut app, scene.clone())
            .unwrap()
            .try_into_dynamic()
            .unwrap()
            .to_rgba8();
        assert!(
            pixels
                .pixels()
                .any(|pixel| pixel.0[..3].iter().all(|&channel| channel > 240)),
            "{topology:?}/{index_format:?} drew no white pixels"
        );
    }
}

#[test]
#[ignore = "requires a GPU; checks texture ownership across scene and display changes"]
fn caches_release_retired_images_and_obsolete_attachments() {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins());
    crate::native::init_material::<menu_backdrop::Material>(&mut app);
    let scene = image(&mut app, 64);
    let offscreen = image(&mut app, 32);
    crate::test_support::native_scene(&mut app, scene.clone());
    crate::test_support::finish_app(&mut app).unwrap();
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            crate::anti_aliasing::SceneMsaa,
            Msaa::Sample4,
            RenderTarget::Image(scene.clone().into()),
        ))
        .id();
    let preview = app
        .world_mut()
        .spawn((
            Camera2d,
            Msaa::Off,
            crate::native::Offscreen,
            RenderTarget::Image(offscreen.into()),
        ))
        .id();
    let source = image(&mut app, 16);
    let old = source.id();
    let material = app
        .world_mut()
        .resource_mut::<Assets<menu_backdrop::Material>>()
        .add(menu_backdrop::Material::visible(source));
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(20., 20.));
    let quad = app
        .world_mut()
        .spawn((Mesh2d(mesh), MeshMaterial2d(material.clone())))
        .id();
    let old_key = Material::Backdrop { source: old };
    wait(&mut app, "backdrop not prepared", |app| {
        renderer(app).materials.contains_key(&old_key)
    });
    assert_eq!(renderer(&app).targets.len(), 2);
    let preview_key = (32, 32, 1, TextureFormat::Bgra8Unorm, true);
    let preview_depth = renderer(&app).targets[&preview_key]
        .depth
        .as_ref()
        .unwrap()
        .id();
    let group = renderer(&app).materials[&old_key].group.id();

    // Hidden but live assets remain warm. An image leaving the main world alone
    // is not retirement: render-only assets deliberately do exactly that.
    app.world_mut().entity_mut(quad).insert(Visibility::Hidden);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(renderer(&app).materials[&old_key].group.id(), group);
    let mut render_only = Image::new_target_texture(16, 16, TextureFormat::Bgra8Unorm, None);
    render_only.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let source = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(render_only);
    let current = source.id();
    *app.world_mut()
        .resource_mut::<Assets<menu_backdrop::Material>>()
        .get_mut(&material)
        .unwrap() = menu_backdrop::Material::visible(source);
    wait(&mut app, "retired backdrop still cached", |app| {
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderAssets<GpuImage>>()
            .get(old)
            .is_none()
            && !renderer(app).materials.contains_key(&old_key)
    });
    app.world_mut().entity_mut(quad).insert(Visibility::Visible);
    let current_key = Material::Backdrop { source: current };
    wait(&mut app, "render-only backdrop not prepared", |app| {
        renderer(app).materials.contains_key(&current_key)
    });
    let group = renderer(&app).materials[&current_key].group.id();
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .remove(current);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(renderer(&app).materials[&current_key].group.id(), group);

    // Resolution and sample-count changes replace only the obsolete attachment;
    // the concurrently used offscreen depth image keeps its identity.
    for (size, samples) in [(128, 4), (128, 1), (64, 4)] {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&scene)
            .unwrap()
            .resize(Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            });
        app.world_mut()
            .resource_mut::<crate::anti_aliasing::AntiAliasingSettings>()
            .0 = if samples == 4 {
            resonance_content::menu_data::AntiAliasing::Msaa4
        } else {
            resonance_content::menu_data::AntiAliasing::Off
        };
        let key = (size, size, samples, TextureFormat::Bgra8Unorm, true);
        wait(&mut app, "obsolete render attachments retained", |app| {
            renderer(app).targets.len() == 2 && renderer(app).targets.contains_key(&key)
        });
        assert_eq!(
            renderer(&app).targets[&preview_key]
                .depth
                .as_ref()
                .unwrap()
                .id(),
            preview_depth
        );
    }

    // No draws must release the last frame's cloned bind groups as well.
    app.world_mut().despawn(quad);
    app.world_mut().despawn(camera);
    app.world_mut().despawn(preview);
    drop(material);
    wait(&mut app, "empty frame retains GPU resources", |app| {
        let renderer = renderer(app);
        let frame = frame_state(app);
        frame.frame.passes.is_empty()
            && renderer.targets.is_empty()
            && renderer.materials.is_empty()
            && frame.groups.is_empty()
            && frame.resolved.is_empty()
    });
}

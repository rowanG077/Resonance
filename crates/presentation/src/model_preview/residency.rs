//! A closed catalogue keeps its assets and poses, outside render/skin queries.
use super::*;

pub(super) type Dormant = crate::scene::residency::Dormant<Surface>;

pub(super) fn sync(world: &mut World) {
    world.resource_scope(|world, mut viewer: Mut<Viewer>| {
        let active = world
            .get::<Camera>(viewer.camera)
            .is_some_and(|camera| camera.is_active);
        if active {
            if let Some(dormant) = viewer.dormant.take() {
                dormant.resume(world);
            }
        } else if viewer.dormant.is_none()
            && viewer.status == PreviewStatus::Ready
            && !viewer.parts.is_empty()
        {
            let roots: Vec<_> = viewer
                .parts
                .iter()
                .filter(|part| part.spec.attached_to.is_none())
                .filter_map(|part| part.root)
                .collect();
            viewer.dormant = Some(Dormant::suspend(world, roots.into_iter()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::RunSystemOnce, mesh::skinning::SkinnedMesh};
    #[test]
    fn only_ready_closed_previews_park_and_reopening_restores_before_pose_update() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Composite>>();
        world.init_resource::<crate::scene::SampledImages>();
        world.run_system_once(setup).unwrap();
        let root = world
            .spawn((
                Mesh3d::default(),
                MeshMaterial3d::<Surface>::default(),
                Transform::IDENTITY,
                Visibility::Inherited,
            ))
            .id();
        world.resource_mut::<Viewer>().parts.push(Part {
            spec: serde_json::from_value(serde_json::json!({"mesh":"preview.glb"})).unwrap(),
            gltf: Handle::default(),
            textures: vec![],
            toon: None,
            root: Some(root),
            materials: vec![],
            clips: vec![],
            motions: vec![],
            clip: None,
            binding: default(),
            bones: BTreeMap::new(),
            ready: true,
        });
        sync(&mut world);
        assert!(
            world.get::<Mesh3d>(root).is_some(),
            "incomplete GPU preparation must retain its live graph"
        );
        world.resource_mut::<Viewer>().status = PreviewStatus::Ready;
        sync(&mut world);
        assert!(world.get::<Mesh3d>(root).is_none());
        let camera = world.resource::<Viewer>().camera;
        world.get_mut::<Camera>(camera).unwrap().is_active = true;
        sync(&mut world);
        assert!(world.get::<Mesh3d>(root).is_some());
        assert!(world.get::<MeshMaterial3d<Surface>>(root).is_some());
        assert!(world.resource::<Viewer>().dormant.is_none());
    }
    #[test]
    #[ignore = "requires a GPU; checks cached preview skin restoration without a window or game assets"]
    fn parked_preview_gpu_restores_current_bones_and_pixels() -> Result<()> {
        use bevy::{mesh::VertexAttributeValues, render::render_resource::TextureFormat};
        let mut app = App::new();
        app.add_plugins(crate::test_support::render_plugins_with_features(
            wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ))
        .add_plugins(crate::materials::install_surface_material);
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    128,
                    64,
                    TextureFormat::Bgra8Unorm,
                    None,
                ));
        crate::test_support::native_scene(&mut app, target.clone());
        crate::test_support::finish_app(&mut app)?;
        app.update();
        app.world_mut().spawn((
            Camera3d::default(),
            Camera {
                clear_color: Color::BLACK.into(),
                ..default()
            },
            Msaa::Sample2,
            RenderTarget::Image(target.clone().into()),
            Transform::from_xyz(0., 0., 4.).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        let mut mesh = Mesh::from(Rectangle::new(1., 1.));
        let count = mesh.count_vertices();
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0; 4]; count]),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1., 0., 0., 0.]; count]);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let material = app
            .world_mut()
            .resource_mut::<Assets<Surface>>()
            .add(Surface {
                constant_color: true,
                tint: Vec4::new(1., 0., 0., 1.),
                ..default()
            });
        let bindposes = app
            .world_mut()
            .resource_mut::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
            .add(bevy::mesh::skinning::SkinnedMeshInverseBindposes::from(
                vec![Mat4::IDENTITY],
            ));
        let root = app
            .world_mut()
            .spawn((Transform::IDENTITY, Visibility::Inherited))
            .id();
        let bone = app
            .world_mut()
            .spawn((Transform::from_xyz(-0.8, 0., 0.), ChildOf(root)))
            .id();
        let draw = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                ChildOf(root),
                SkinnedMesh {
                    inverse_bindposes: bindposes.clone(),
                    joints: vec![bone],
                },
                NoFrustumCulling,
            ))
            .id();
        let capture = |app: &mut App| -> Result<Image> {
            for _ in 0..20 {
                app.update();
                crate::playthrough::check_exit(app)?;
            }
            crate::test_support::capture_image(app, target.clone())
        };
        let first = capture(&mut app)?.try_into_dynamic()?.to_rgba8();
        assert_eq!(first.get_pixel(48, 32).0, [255, 0, 0, 255]);
        assert_eq!(first.get_pixel(80, 32).0, [0, 0, 0, 255]);
        let dormant = Dormant::suspend(app.world_mut(), std::iter::once(root));
        assert!(app.world().get::<SkinnedMesh>(draw).is_none());
        app.world_mut()
            .get_mut::<Transform>(bone)
            .unwrap()
            .translation
            .x = 0.8;
        let parked = capture(&mut app)?.try_into_dynamic()?.to_rgba8();
        assert!(parked.pixels().all(|pixel| pixel.0 == [0, 0, 0, 255]));
        dormant.resume(app.world_mut());
        let resumed = capture(&mut app)?.try_into_dynamic()?.to_rgba8();
        assert_eq!(resumed.get_pixel(48, 32).0, [0, 0, 0, 255]);
        assert_eq!(resumed.get_pixel(80, 32).0, [255, 0, 0, 255]);
        assert_eq!(app.world().get::<Mesh3d>(draw).unwrap().0, mesh);
        assert_eq!(
            app.world().get::<MeshMaterial3d<Surface>>(draw).unwrap().0,
            material
        );
        assert_eq!(
            app.world()
                .get::<SkinnedMesh>(draw)
                .unwrap()
                .inverse_bindposes,
            bindposes
        );
        Ok(())
    }
}

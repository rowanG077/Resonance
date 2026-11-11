use super::*;
use bevy::{
    gltf::{Gltf, GltfMesh, GltfPlugin},
    world_serialization::WorldSerializationPlugin,
};
use std::{
    collections::BTreeSet,
    path::Path,
    time::{Duration, Instant},
};

fn app(root: &Path, configure: impl FnOnce(&mut App)) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: root.to_string_lossy().into_owned(),
            ..default()
        },
        WorldSerializationPlugin,
        bevy::image::ImagePlugin::default(),
        bevy::mesh::MeshPlugin,
        GltfPlugin::default(),
    ))
    .init_asset::<StandardMaterial>();
    configure(&mut app);
    app.finish();
    app.cleanup();
    app
}

fn load(app: &mut App, path: &str) -> Result<Handle<Gltf>> {
    let server = app.world().resource::<AssetServer>().clone();
    let original: Handle<Gltf> = server.load(path.to_owned());
    let since = Instant::now();
    while !server.is_loaded_with_dependencies(original.id()) {
        app.update();
        for id in [original.id().untyped()] {
            if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(id) {
                anyhow::bail!("{path}: {error}");
            }
        }
        ensure!(
            since.elapsed() < Duration::from_secs(30),
            "{path}: loading timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(original)
}

fn compare(app: &mut App, path: &str) -> Result<()> {
    let original = load(app, path)?;
    let world = app.world();
    let original = world.resource::<Assets<Gltf>>().get(&original).unwrap();
    let scene = world
        .resource::<Assets<WorldAsset>>()
        .get(&original.scenes[0])
        .unwrap();
    let model = prepared(scene)?;
    let source = &scene.world;
    assert_eq!(
        source
            .try_query::<&Transform>()
            .unwrap()
            .iter(source)
            .count(),
        model.nodes.len(),
        "{path}"
    );
    assert_eq!(
        model
            .nodes
            .iter()
            .filter(|node| node.parent.is_none())
            .count(),
        1,
        "{path}"
    );
    let meshes = world.resource::<Assets<GltfMesh>>();
    assert_eq!(model.meshes.len(), original.meshes.len(), "{path}");
    for handle in &original.meshes {
        let mesh = meshes.get(handle).unwrap();
        assert_eq!(
            model.meshes[mesh.index],
            mesh.primitives
                .iter()
                .map(|primitive| primitive.mesh.clone())
                .collect::<Vec<_>>(),
            "{path}: GPU geometry was duplicated"
        );
    }
    for (index, node) in model.nodes.iter().enumerate() {
        assert_eq!(
            source.get::<Name>(node.source),
            node.name.as_ref(),
            "{path}"
        );
        assert_eq!(
            source.get::<Transform>(node.source),
            Some(&node.transform),
            "{path}"
        );
        assert_eq!(
            source.get::<ChildOf>(node.source).map(|p| p.parent()),
            node.parent.map(|i| model.nodes[i].source),
            "{path}"
        );
        assert_eq!(
            source.get::<Mesh3d>(node.source).map(|m| m.0.id()),
            node.draw.as_ref().map(|draw| draw.mesh.id()),
            "{path}"
        );
        if let Some(draw) = &node.draw {
            assert_eq!(
                source.get::<crate::materials::MaterialSlot>(node.source),
                Some(&draw.slot),
                "{path}"
            );
            let skin = source.get::<SkinnedMesh>(node.source);
            assert_eq!(skin.is_some(), draw.skin.is_some(), "{path}");
            if let Some(prepared) = &draw.skin {
                let skin = skin.unwrap();
                assert_eq!(prepared.inverse_bindposes, skin.inverse_bindposes, "{path}");
                assert_eq!(
                    prepared
                        .joints
                        .iter()
                        .map(|&i| model.nodes[i].source)
                        .collect::<Vec<_>>(),
                    skin.joints,
                    "{path}"
                );
            }
        }
        if let Some(parent) = node.parent {
            assert_ne!(parent, index, "{path}");
        }
    }

    Ok(())
}

#[test]
fn prepared_model_preserves_skin_palette_and_matches_stock_vertices() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut nodes: Vec<_> = (0..137).map(|index| serde_json::json!({"name":format!("bone{index}"),"extras":{"resonance_bone":index}})).collect();
    nodes[0]["children"] = serde_json::json!((1..138).collect::<Vec<_>>());
    nodes.push(serde_json::json!({"name":"draw","mesh":0,"skin":0}));
    let mut binary: Vec<u8> = [0f32, 0., 0., 1., 0., 0., 0., 1., 0.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    binary.extend(
        [0u16, 136, 1, 2]
            .repeat(3)
            .into_iter()
            .flat_map(u16::to_le_bytes),
    );
    binary.extend(
        [0.5f32, 0.25, 0.125, 0.125]
            .repeat(3)
            .into_iter()
            .flat_map(f32::to_le_bytes),
    );
    binary.extend(
        [0f32, 0., 1.]
            .repeat(3)
            .into_iter()
            .flat_map(f32::to_le_bytes),
    );
    for uv in [[0f32, 0., 1., 0., 0., 1.], [0.25, 0.5, 0.75, 0.5, 0.25, 1.]] {
        binary.extend(uv.into_iter().flat_map(f32::to_le_bytes));
    }
    binary.extend([255u8, 128, 0, 255, 0, 255, 64, 128, 0, 32, 255, 64]);
    binary.extend([0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes));
    let json = serde_json::json!({
        "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":nodes,
        "skins":[{"joints":(0..137).collect::<Vec<_>>()}],"materials":[{}],
        "buffers":[{"byteLength":binary.len()}],
        "bufferViews":[{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":24},
            {"buffer":0,"byteOffset":60,"byteLength":48},{"buffer":0,"byteOffset":108,"byteLength":36},
            {"buffer":0,"byteOffset":144,"byteLength":24},{"buffer":0,"byteOffset":168,"byteLength":24},
            {"buffer":0,"byteOffset":192,"byteLength":12},{"buffer":0,"byteOffset":204,"byteLength":6}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
            {"bufferView":1,"componentType":5123,"count":3,"type":"VEC4"},{"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"},
            {"bufferView":3,"componentType":5126,"count":3,"type":"VEC3"},
            {"bufferView":4,"componentType":5126,"count":3,"type":"VEC2"},{"bufferView":5,"componentType":5126,"count":3,"type":"VEC2"},
            {"bufferView":6,"componentType":5121,"normalized":true,"count":3,"type":"VEC4"},
            {"bufferView":7,"componentType":5123,"count":3,"type":"SCALAR"}],
        "meshes":[{"primitives":[
            {"attributes":{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2},"material":0},
            {"attributes":{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2,"NORMAL":3,"TEXCOORD_0":4,"TEXCOORD_1":5,"COLOR_0":6},"indices":7,"material":0}
        ]}]
    });
    std::fs::write(
        root.path().join("body.glb"),
        resonance_asset_writer::gltf::pack_glb(&json, &binary)?,
    )?;
    // A separate app leaves Bevy's glTF loader unmodified for the reference.
    let mut stock = app(root.path(), |_| {});
    let original = load(&mut stock, "body.glb")?;
    let mut app = app(root.path(), crate::materials::install);
    compare(&mut app, "body.glb")?;
    let gltfs = app.world().resource::<Assets<Gltf>>();
    let (_, gltf) = gltfs.iter().next().unwrap();
    let model = prepared(
        app.world()
            .resource::<Assets<WorldAsset>>()
            .get(&gltf.scenes[0])
            .unwrap(),
    )?;
    assert_eq!(
        model
            .nodes
            .iter()
            .filter_map(|node| node.draw.as_ref()?.skin.as_ref())
            .next()
            .unwrap()
            .joints
            .len(),
        137
    );
    assert_eq!(model.bones.len(), 137);
    let stock_world = stock.world();
    let original = stock_world
        .resource::<Assets<Gltf>>()
        .get(&original)
        .unwrap();
    let original = stock_world
        .resource::<Assets<GltfMesh>>()
        .get(&original.meshes[0])
        .unwrap();
    assert_eq!(original.primitives.len(), model.meshes[0].len());
    let stock_meshes = stock_world.resource::<Assets<Mesh>>();
    let meshes = app.world().resource::<Assets<Mesh>>();
    for (expected, actual) in original.primitives.iter().zip(&model.meshes[0]) {
        let expected = stock_meshes.get(&expected.mesh).unwrap();
        let actual = meshes.get(actual).unwrap();
        assert_eq!(actual.primitive_topology(), expected.primitive_topology());
        assert_eq!(actual.indices(), expected.indices());
        assert_eq!(actual.count_vertices(), expected.count_vertices());
        // Canonical padding may add channels, but every authored channel and
        // Bevy's generated fallback normals must retain their original values.
        for (attribute, values) in expected.attributes() {
            assert_eq!(
                actual.attribute(attribute.id),
                Some(values),
                "{}",
                attribute.name
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; compares prepared model geometry with the existing loader without a GPU"]
fn installed_models_match_gltf_assets() -> Result<()> {
    use resonance_content::{diagnostics::Diagnostics, library::Library};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
    let root = Path::new(&root);
    let library = Library::open(root, &[])?;
    let mut snapshot = library.snapshot();
    let mut paths = BTreeSet::new();
    for id in [332, 340, 341, 376, 520, 374, 464] {
        let (_, field) =
            resonance_content::field::document::capture_scene(&mut snapshot, id, || false)?;
        paths.extend(
            field
                .parts
                .iter()
                .chain(field.actors.iter().flat_map(|actor| &actor.parts))
                .map(|part| part.mesh.clone()),
        );
    }
    for id in 1..=9 {
        let (_, party) = resonance_content::battle_model::document::capture_party(
            &mut snapshot,
            id,
            0,
            &Diagnostics::new(true),
            &|| false,
        )?;
        paths.extend(
            party
                .parts
                .iter()
                .chain(&party.wings)
                .map(|part| part.mesh.clone()),
        );
    }
    let mut app = app(root, crate::materials::install);
    for path in &paths {
        compare(&mut app, path)?;
    }
    println!(
        "Prepared model equivalence: {} distinct field/default-character meshes",
        paths.len()
    );
    Ok(())
}

fn fixture() -> (WorldAsset, Mat4, Mat4) {
    let mut world = World::new();
    let root = world.spawn(Transform::from_xyz(12., 3., -4.)).id();
    let first = world
        .spawn((
            Transform::from_xyz(1., 2., 3.),
            ChildOf(root),
            bevy::gltf::GltfExtras {
                value: "{\"resonance_bone\":0}".into(),
            },
        ))
        .id();
    let suffix = Transform::from_xyz(4., 5., 6.)
        .with_scale(Vec3::new(-2., 3., 1.))
        .with_rotation(Quat::from_rotation_z(0.3));
    let wrapper = world.spawn((suffix, ChildOf(first))).id();
    let second = world
        .spawn((
            Transform::from_xyz(7., 8., 9.),
            ChildOf(wrapper),
            bevy::gltf::GltfExtras {
                value: "{\"resonance_bone\":1}".into(),
            },
        ))
        .id();
    let local = Transform::from_xyz(10., 11., 12.).with_rotation(Quat::from_rotation_x(0.5));
    world.spawn((
        local,
        ChildOf(second),
        Mesh3d::default(),
        crate::materials::MaterialSlot(0),
        SkinnedMesh {
            inverse_bindposes: Handle::default(),
            joints: vec![second, first],
        },
    ));
    world.spawn((
        local,
        ChildOf(wrapper),
        Mesh3d::default(),
        crate::materials::MaterialSlot(1),
    ));
    // Neither this branch nor non-joint wrappers need live entities.
    world.spawn((Transform::from_xyz(99., 0., 0.), ChildOf(root)));
    (
        WorldAsset::new(world),
        local.to_matrix(),
        suffix.to_matrix() * local.to_matrix(),
    )
}

#[test]
fn prepared_layout_preserves_affine_suffixes_and_validates_ancestry() -> Result<()> {
    let (mut scene, local, nested) = fixture();
    let root = scene
        .world
        .try_query::<(Entity, &Transform)>()
        .unwrap()
        .iter(&scene.world)
        .find(|(entity, _)| scene.world.get::<ChildOf>(*entity).is_none())
        .unwrap()
        .0;
    let capture = |scene: &WorldAsset| Data::capture(&scene.world, root, vec![vec![]; 2]);
    let data = capture(&scene)?;
    data.check_bones(2)?;
    assert!(data.check_bones(1).is_err());
    assert!(data.check_bones(3).is_err());
    data.check_bones(0)?;
    assert_eq!(
        data.render.len(),
        4,
        "only draws and skin joints need live battle entities"
    );
    for &index in &data.render {
        let node = &data.nodes[index];
        let Some(draw) = &node.draw else { continue };
        if draw.slot.0 == 0 {
            assert_eq!(node.bone, Some(1));
            assert!(node.suffix.abs_diff_eq(local, 0.00001));
            let skin = draw.skin.as_ref().unwrap();
            assert_eq!(
                skin.joints
                    .iter()
                    .map(|&index| data.nodes[index].bone)
                    .collect::<Vec<_>>(),
                vec![Some(1), Some(0)]
            );
            assert!(
                skin.joints
                    .iter()
                    .all(|&index| data.nodes[index].suffix == Mat4::IDENTITY)
            );
        } else {
            assert_eq!(node.bone, Some(0));
            assert!(node.suffix.abs_diff_eq(nested, 0.00001));
            assert!(draw.skin.is_none());
        }
    }
    let data = Arc::new(data);
    let weak = Arc::downgrade(&data);
    scene.world.entity_mut(root).insert(Model(Ok(data.clone())));
    assert!(Arc::ptr_eq(&data, &prepared(&scene)?));
    drop(data);
    assert!(weak.upgrade().is_some(), "scene owns its layout");
    let entity = scene
        .world
        .query_filtered::<Entity, With<crate::materials::MaterialSlot>>()
        .iter(&scene.world)
        .next()
        .unwrap();
    let parent = scene.world.spawn(Transform::default()).id();
    scene.world.entity_mut(entity).insert(ChildOf(parent));
    scene.world.entity_mut(parent).insert(ChildOf(entity));
    assert!(capture(&scene).is_err(), "cyclic ancestry accepted");
    drop(scene);
    assert!(
        weak.upgrade().is_none(),
        "no global layout owner may retain a retired scene"
    );
    let (mut scene, _, _) = fixture();
    scene.world.spawn((
        Transform::default(),
        bevy::gltf::GltfExtras {
            value: "{\"resonance_bone\":0}".into(),
        },
    ));
    assert!(capture(&scene).is_err(), "duplicate cooked bone accepted");
    let mut world = World::new();
    let root = world
        .spawn((Transform::default(), crate::materials::MaterialSlot(0)))
        .id();
    assert!(
        Data::capture(&world, root, vec![vec![]]).is_err(),
        "material slot without mesh accepted"
    );
    world.entity_mut(root).insert(Mesh3d::default());
    assert_eq!(Data::capture(&world, root, vec![vec![]])?.render.len(), 1);
    Ok(())
}

#[test]
fn prepared_scene_is_available_in_its_birth_update() {
    use crate::materials::MaterialSlot;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.init_resource::<Assets<WorldAsset>>()
        .register_type::<Transform>()
        .register_type::<GlobalTransform>()
        .register_type::<ChildOf>()
        .register_type::<Children>()
        .register_type::<MaterialSlot>()
        .register_type::<Mesh3d>();
    let mut scene = World::new();
    let parent = scene.spawn(Transform::from_xyz(3., 0., 0.)).id();
    scene.spawn((
        ChildOf(parent),
        Transform::IDENTITY,
        MaterialSlot(0),
        Mesh3d::default(),
    ));
    let data = Data::capture(&scene, parent, vec![vec![]]).unwrap();
    scene.entity_mut(parent).insert(Model(Ok(Arc::new(data))));
    let scene = app
        .world_mut()
        .resource_mut::<Assets<WorldAsset>>()
        .add(WorldAsset::new(scene));
    let root = app.world_mut().spawn_empty().id();
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            instantiate(&mut commands, root, scene.clone());
        })
        .unwrap();
    let parent = app.world().get::<Children>(root).unwrap()[0];
    let mesh = app.world().get::<Children>(parent).unwrap()[0];
    assert_eq!(
        app.world().get::<MaterialSlot>(mesh),
        Some(&MaterialSlot(0))
    );
    assert_eq!(
        app.world().get::<Transform>(parent).unwrap().translation.x,
        3.
    );
    app.world_mut().despawn(root);
    assert!(app.world().get_entity(mesh).is_err());
}

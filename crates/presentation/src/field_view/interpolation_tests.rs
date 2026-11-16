//! Exercise the production material system with the Salvation wing bindings.
use super::*;
use crate::native::MeshMaterial3d;
use bevy::ecs::{change_detection::Tick, system::RunSystemOnce};
use resonance_events::{Actor, SavePoint, WingStyle, effect::CharacterLight};

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; no window or audio device"]
fn material_endpoints_follow_lights_wings_and_save_glow_without_advancing_the_scene() -> Result<()>
{
    let root = PathBuf::from(std::env::var_os("RESONANCE_TEST_ASSETS").context("cooked assets")?);
    let package =
        crate::new_game::FieldPackage::prepare(&root, 535, &mut Default::default(), || false)?;
    let mut manifest = package.assets.clone();
    let script: Vec<_> = [4u16, 0, 0, 0, 0x20ff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    manifest.parts.clear();
    manifest.doors.clear();
    manifest.ground = vec![resonance_content::field::CollisionGroup {
        surface: 0,
        vertices: vec![
            [-1000., -1000., -100.],
            [1000., -1000., -100.],
            [0., 1000., -100.],
        ],
        triangles: vec![[0, 1, 2]],
    }];
    let mut field = FieldSession::new(&script, vec![], &manifest)?;
    field.events.world = Default::default();
    // Predict the empty scene first; explicit endpoints isolate rendering from
    // gameplay scripts, physics and the wing particle emitter.
    let mut storage = resonance_game::field::PredictionBuffer::new()?;
    let mut next = storage.predict(&field, Default::default())?;
    let next_world = &mut next.state.as_mut().unwrap().events.world;
    *next_world = Default::default();
    for (id, resource) in [(1, 131428), (2, 131430), (3, 131428)] {
        let mut actor = Actor::new(resource, [0.; 3]);
        if id != 3 {
            actor.set_wings(WingStyle::Layered);
        }
        actor.light = Some(CharacterLight {
            position: LightPosition::Relative([10., 20., 30.]),
            shade: [40; 3],
            bright: [64; 3],
            ..Default::default()
        });
        field.events.world.insert_actor(id, actor);
        let mut actor = field.events.world.actors[&id].clone();
        actor.position = [8., 0., 0.];
        if id == 3 {
            actor.shadow_alpha = 128;
            actor.scale_percent[0] = -100;
        }
        actor.light.as_mut().unwrap().shade = [48; 3];
        next_world.insert_actor(id, actor);
        next_world.actors.get_mut(&id).unwrap().instance = field.events.world.actors[&id].instance;
    }
    // The caster stays at the same height while its supporting platform rises.
    // This catches accidentally querying the committed collision mesh twice.
    let mut platform = Actor::new(131428, [0.; 3]);
    platform.casts_shadow = false;
    platform.model_collision = Some(std::sync::Arc::new(
        resonance_content::field::ModelCollision {
            floors: vec![resonance_content::field::CollisionGroup {
                surface: 0,
                vertices: vec![[-100., -100., 0.], [100., -100., 0.], [0., 100., 0.]],
                triangles: vec![[0, 1, 2]],
            }],
            ..Default::default()
        },
    ));
    field.events.world.insert_actor(4, platform.clone());
    platform.position[2] = 10.;
    next_world.insert_actor(4, platform);
    field.events.world.effect_tick = 127;
    next_world.effect_tick = 128;
    field.events.world.tick = 127;
    next_world.tick = 128;
    let point = SavePoint {
        actor: 3,
        position: [0.; 3],
        resource: 0,
        born: 0,
        active: true,
        unlock_flag: None,
        glow_scale: 0.2,
    };
    field.events.world.save_points.push(point.clone());
    next_world.save_points.push(SavePoint {
        glow_scale: 0.4,
        ..point
    });
    let mut prediction = Prediction::default();
    prediction.reserve()?;
    prediction.predict(
        prediction::Key::Field(Tick::new(1), Default::default()),
        || Ok(prediction::Endpoint::Field(next)),
    );

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: root.to_string_lossy().into_owned(),
            ..Default::default()
        },
    ))
    .init_asset::<Image>()
    .init_resource::<Applied>()
    .init_resource::<crate::sparse_animation::affine::Locals>();
    let server = app.world().resource::<AssetServer>();
    // This fixture exercises material publication without spawning geometry,
    // but each prepared actor still needs its model layout and instance.
    let layout = std::sync::Arc::new(crate::model_asset::Data {
        nodes: vec![],
        meshes: vec![],
        bones: vec![],
        render: vec![],
        named: Default::default(),
    });
    let mut art = prepared_test_art(&manifest, server);
    for resource in [131428, 131430] {
        let mut spec = manifest
            .actors
            .iter()
            .find(|a| a.resource == resource)
            .unwrap()
            .parts[0]
            .clone();
        spec.appearance = None;
        spec.materials[0].cull = resonance_content::CullFace::Back;
        art.models.insert(
            resource,
            vec![Part {
                spec,
                has_eye_material: false,
                gltf: Handle::default(),
                layout: Some(layout.clone()),
                scene: Handle::default(),
                clips: vec![],
                materials: vec![],
                motions: vec![],
            }],
        );
    }
    let mut meshes = Assets::<Mesh>::default();
    let mut surfaces = Assets::<TitleSurface>::default();
    let effects = crate::field_effects::Artwork::load(
        &manifest,
        server,
        &mut meshes,
        &mut surfaces,
        &package.files,
        None,
        None,
    )?;
    let mut bindings = Vec::new();
    for (&id, actor) in &field.events.world.actors {
        if id == 4 {
            continue;
        }
        let spec = &art.models[&actor.resource][0].spec;
        let handles: Vec<_> = spec
            .materials
            .iter()
            .map(|_| surfaces.add(TitleSurface::default()))
            .collect();
        bindings.push((id, handles.clone()));
        let mut part = ActorPart::new(id, actor, 0, 2, handles);
        part.prepared = true;
        let entity = app
            .world_mut()
            .spawn((
                part,
                Transform::IDENTITY,
                Visibility::Inherited,
                crate::model_asset::Instance {
                    data: layout.clone(),
                    nodes: vec![],
                },
            ))
            .id();
        app.world_mut()
            .get_mut::<ActorPart>(entity)
            .unwrap()
            .shadow_anchor = Some(entity);
    }
    app.insert_resource(art)
        .insert_resource(effects)
        .insert_resource(meshes)
        .insert_resource(surfaces)
        .insert_resource(Session(field))
        .insert_resource(prediction);
    app.world_mut().run_system_once(shadows::sync).unwrap();
    let mut draw = Schedule::default();
    draw.add_systems(
        (
            pose,
            crate::field_animation::prepare_prediction,
            crate::field_animation::present::<false>,
            crate::field_pose::finish_models,
            shadows::pose,
        )
            .chain(),
    );
    for alpha in [0., 0.25, 0.5, 0.75, 1., 0.5, 0.] {
        app.world_mut().resource_mut::<Prediction>().alpha = alpha;
        draw.run(app.world_mut());
        let mut shadows = app
            .world_mut()
            .query::<(&Transform, &MeshMaterial3d<TitleSurface>)>();
        let (shadow, material) = shadows.single(app.world())?;
        let height_offset = app.world().resource::<Art>().shadows.spec.height_offset;
        assert!(
            (shadow.translation.z - (height_offset + 10. * alpha)).abs() < 0.00001,
            "fraction {alpha}: shadow={shadow:?}; current floor={:?}; predicted floor={:?}",
            app.world()
                .resource::<Session>()
                .0
                .collision()
                .ground_surface([0.; 3]),
            app.world()
                .resource::<Prediction>()
                .field()
                .unwrap()
                .collision()
                .ground_surface([8., 0., 0.])
        );
        assert!(
            (app.world()
                .resource::<Assets<TitleSurface>>()
                .get(&material.0)
                .unwrap()
                .tint
                .w
                - (64. + 64. * alpha) / 255.)
                .abs()
                < 0.00001
        );
        let surfaces = app.world().resource::<Assets<TitleSurface>>();
        use resonance_content::CullFace;
        assert_eq!(
            surfaces.get(&bindings[2].1[0]).unwrap().cull,
            if alpha > 0.5 {
                CullFace::Front
            } else {
                CullFace::Back
            }
        );
        for (id, handles) in &bindings {
            for handle in handles {
                let surface = surfaces.get(handle).unwrap();
                assert_eq!(surface.field_light.x, 10. + 8. * alpha);
                assert!((surface.shade_colors[0].x - (41. + 8. * alpha) / 255.).abs() < 0.00001);
                match id {
                    1 => assert_eq!(surface.uv_offsets.x, (127. + alpha) / 128.),
                    2 => assert_eq!(surface.uv_offsets.z, (127. + alpha) / 128.),
                    3 => assert!((surface.uv_scales.w - (0.2 + 0.2 * alpha)).abs() < 0.00001),
                    _ => unreachable!(),
                }
            }
        }
        let field = &app.world().resource::<Session>().0;
        assert_eq!(field.events.world.effect_tick, 127);
        assert_eq!(field.events.world.actors[&1].position, [0.; 3]);
        assert_eq!(field.events.world.save_points[0].glow_scale, 0.2);
    }
    // Replacing an actor must not smear its old material toward the new one.
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .events
        .world
        .actors
        .get_mut(&1)
        .unwrap()
        .instance += 100;
    app.world_mut().resource_mut::<Prediction>().alpha = 0.5;
    app.world_mut().run_system_once(pose).unwrap();
    assert_eq!(
        app.world()
            .resource::<Assets<TitleSurface>>()
            .get(&bindings[0].1[0])
            .unwrap()
            .field_light
            .x,
        10.
    );
    Ok(())
}

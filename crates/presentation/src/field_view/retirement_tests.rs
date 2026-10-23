use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
#[ignore = "requires locally cooked field 7; no devices"]
fn battle_return_removes_actor_rendering_before_resuming_animation() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
        PathBuf::from,
    );
    let manifest: FieldAssets = serde_json::from_slice(&fs::read(root.join("fields/map-7.json"))?)?;
    let mut field = FieldSession::new(&fs::read(root.join(&manifest.script))?, vec![], &manifest)?;
    field.events.world.actors.clear();
    for id in [1, 2] {
        let mut actor = resonance_events::Actor::new(0, [0.; 3]);
        actor.casts_shadow = true;
        field.events.world.insert_actor(id, actor);
    }

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: root.to_string_lossy().into_owned(),
            ..Default::default()
        },
    ))
    .register_asset_loader(bevy::image::ImageLoader::new(
        bevy::image::CompressedImageFormats::NONE,
    ))
    .init_asset::<Image>()
    .init_resource::<Assets<Mesh>>()
    .init_resource::<Assets<TitleSurface>>()
    .init_resource::<Assets<crate::sparse_animation::Clip>>()
    .init_resource::<crate::sparse_animation::affine::Locals>()
    .init_resource::<crate::field_pose::Authored>()
    .init_resource::<Applied>()
    .init_resource::<crate::loading::Resident>()
    .insert_resource(Session(field));
    let mut art = prepared_test_art(&manifest, app.world().resource::<AssetServer>());
    let mut roots = Vec::new();
    for id in [1, 2] {
        let entity = app
            .world_mut()
            .spawn(ActorPart {
                actor: id,
                instance: 0,
                resource: 0,
                part: 0,
                pass: 0,
                materials: vec![],
                prepared: true,
                disabled: false,
                instantiated: true,
                geometry: vec![],
                active_clip: None,
                shadow_anchor: None,
            })
            .id();
        art.instances.insert(id, vec![entity]);
        roots.push(entity);
    }
    let bone = app
        .world_mut()
        .spawn((Transform::IDENTITY, ChildOf(roots[0])))
        .id();
    app.world_mut()
        .entity_mut(roots[0])
        .insert(crate::field_animation::Rig::new(vec![(
            bone,
            Transform::IDENTITY,
        )]));
    app.insert_resource(art);
    app.world_mut().run_system_once(shadows::sync).unwrap();
    app.world()
        .resource::<crate::loading::Resident>()
        .battle
        .store(true, std::sync::atomic::Ordering::Release);

    // Victory commits after scene preparation, then reopens field presentation
    // in the same frame, before another field simulation update can run.
    app.add_systems(
        Update,
        (|mut session: ResMut<Session>, resident: Res<crate::loading::Resident>| {
            session.0.events.world.actors.remove(&1);
            resident
                .battle
                .store(false, std::sync::atomic::Ordering::Release);
        })
        .after(FieldPreparation),
    );
    app.add_systems(
        PostUpdate,
        sample_poses()
            .before(crate::field_animation::blend)
            .run_if(crate::battle::field_presenting),
    );
    app.add_systems(
        PostUpdate,
        crate::field_animation::blend
            .after(crate::field_animation::sample)
            .run_if(crate::battle::field_presenting),
    );
    app.update();

    assert!(app.world().get_entity(roots[0]).is_err());
    assert!(app.world().get_entity(bone).is_err());
    assert!(app.world().get_entity(roots[1]).is_ok());
    assert_eq!(
        app.world()
            .resource::<Art>()
            .instances
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(
        app.world_mut()
            .query::<&shadows::Shadow>()
            .iter(app.world())
            .count(),
        1
    );
    Ok(())
}

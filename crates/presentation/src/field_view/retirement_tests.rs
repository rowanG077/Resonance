use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; exercises failed costume loading without a GPU"]
fn failed_costume_loading_recovers_once_instead_of_resetting_readiness() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
    let mut package = crate::new_game::FieldPackage::prepare_with_diagnostics(
        Path::new(&root),
        332,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::default(),
    )?;
    let checkpoint = crate::test_support::field_checkpoint(&package.files)?;
    let mut costume = package
        .assets
        .actors
        .iter()
        .find(|actor| actor.resource == 1)
        .context("default character missing")?
        .clone();
    let resource = resonance_content::appearance::costume_resource(1, 99);
    costume.resource = resource;
    for part in &mut costume.parts {
        part.mesh = "missing-costume.glb".into();
    }
    package.assets.actors.push(costume);
    let mut session =
        crate::new_game::Session::load_prepared(Arc::new(package), Some(checkpoint), None)?;
    session.events_mut().world.actors.clear();
    session
        .events_mut()
        .world
        .insert_actor(1, resonance_events::Actor::new(resource, [0.; 3]));

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<WorldAsset>()
        .init_asset::<crate::sparse_animation::Clip>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<crate::scene::SampledImages>()
        .init_resource::<crate::loading::Resident>()
        .insert_resource(crate::diagnostics::Diagnostics(
            resonance_content::diagnostics::Diagnostics::default(),
        ));
    let mut art = prepared_test_art(
        session.field_assets(),
        app.world().resource::<AssetServer>(),
    );
    art.unlit = true;
    art.disabled_shadows = true;
    app.insert_resource(art)
        .insert_resource(session)
        .add_systems(Update, (load_costumes, prepare).chain());
    app.finish();
    app.cleanup();
    let since = Instant::now();
    loop {
        app.update();
        let art = app.world().resource::<Art>();
        if art.ready && art.failed_models.contains(&resource) {
            break;
        }
        ensure!(since.elapsed().as_secs() < 5, "costume load did not fail");
        std::thread::yield_now();
    }
    app.world()
        .resource::<crate::loading::Resident>()
        .active
        .store(true, std::sync::atomic::Ordering::Release);
    for _ in 0..3 {
        app.update();
    }
    let art = app.world().resource::<Art>();
    assert!(art.ready && !art.models.contains_key(&resource));
    assert!(
        app.world()
            .resource::<crate::loading::Resident>()
            .active
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let diagnostics = app
        .world()
        .resource::<crate::diagnostics::Diagnostics>()
        .0
        .entries();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].occurrences, 1);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked field 7; no devices"]
fn battle_return_removes_actor_rendering_before_resuming_animation() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
        PathBuf::from,
    );
    let library = resonance_content::library::Library::open(&root, &[])?;
    let (_, manifest) =
        resonance_content::field::document::capture_scene(&mut library.snapshot(), 7, || false)?;
    let mut field = FieldSession::new(&fs::read(root.join(&manifest.script))?, vec![], &manifest)?;
    field.events.world.actors.clear();
    for id in [1, 2] {
        let mut actor = resonance_events::Actor::new(0, [0.; 3]);
        actor.casts_shadow = true;
        field.events.world.insert_actor(id, actor);
    }

    let mut app = App::new();
    crate::texture::decoded_files(&mut app, &root);
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

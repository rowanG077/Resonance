use super::scenario::Scenario;
use super::*;

#[test]
#[ignore = "requires installed assets and RESONANCE_TEST_LOAD_WORLD/FIELD save copies; run serially"]
fn save_installation_renders_overworld_and_field_replacements() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let assets = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
    let field = std::env::var_os("RESONANCE_TEST_LOAD_FIELD").context("field save required")?;
    let overworld = std::env::var_os("RESONANCE_TEST_LOAD_WORLD").context("world save required")?;
    let mut options = crate::test_support::run_options(std::path::PathBuf::from(assets));
    options.saves.load = Some(overworld.clone().into());
    let directory = tempfile::tempdir()?;
    options.saves.directory = Some(directory.path().to_owned());
    let captures = std::env::var_os("RESONANCE_SAVE_SEQUENCE_CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| directory.path().join("frames"));
    if let Some(mods) = std::env::var_os("RESONANCE_TEST_MOD_ASSETS") {
        options.mods.push(mods.into());
    }
    let mut scenario = Scenario::boot_with_options(options, Default::default(), |_| {})?;
    scenario
        .app
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
    scenario.wait("initial world save did not render", |app| {
        super::scenario::scene_ready(app)
            && !app
                .world()
                .contains_resource::<crate::new_game::scene_fade::Fade>()
    })?;
    for (index, path) in [&field, &overworld, &field, &field].into_iter().enumerate() {
        let options = scenario.app.world().resource::<crate::RunOptions>();
        let pending = crate::loading::Pending::start(
            options.assets.clone(),
            options.script_root.clone(),
            Some(std::fs::read(path)?),
            None,
            scenario.app.world().resource::<crate::loading::Resident>(),
        )?;
        let candidate = loop {
            if let Some(candidate) = pending.poll()? {
                break candidate?;
            }
            scenario.update()?;
        };
        crate::saves::install_loaded(scenario.app.world_mut(), candidate);
        let since = std::time::Instant::now();
        loop {
            scenario.update()?;
            if super::scenario::scene_ready(&mut scenario.app)
                && !scenario
                    .app
                    .world()
                    .contains_resource::<crate::new_game::scene_fade::Fade>()
            {
                break;
            }
            if since.elapsed().as_secs() >= 15 {
                let world = scenario.app.world_mut();
                let ready = crate::new_game::scene_ready(world);
                let active = world
                    .resource::<crate::loading::Resident>()
                    .active
                    .load(std::sync::atomic::Ordering::Acquire);
                let cameras: Vec<_> = world
                    .query::<(Entity, &Camera)>()
                    .iter(world)
                    .map(|(id, c)| (id, c.is_active))
                    .collect();
                anyhow::bail!(
                    "save replacement stuck: scene_ready={ready}, resident={active}, cameras={cameras:?}"
                );
            }
        }
        scenario
            .app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::ZERO,
            ));
        let pixels = Arc::new(AtomicUsize::new(usize::MAX));
        let result = pixels.clone();
        let path = captures.join(format!("{index}.png"));
        let framebuffer = scenario
            .app
            .world()
            .resource::<crate::Framebuffer>()
            .0
            .clone();
        scenario
            .app
            .world_mut()
            .spawn(bevy::render::view::screenshot::Screenshot(framebuffer))
            .observe(
                move |event: On<bevy::render::view::screenshot::ScreenshotCaptured>| {
                    let image = event.image.clone().try_into_dynamic().unwrap().to_rgb8();
                    crate::screenshot::write(&event.image, &path, None).unwrap();
                    result.store(
                        image
                            .pixels()
                            .filter(|pixel| pixel.0.iter().any(|&channel| channel > 16))
                            .count(),
                        Ordering::Release,
                    );
                },
            );
        scenario.wait("save image readback timed out", |_| {
            pixels.load(Ordering::Acquire) != usize::MAX
        })?;
        assert!(
            pixels.load(Ordering::Acquire) > 1000,
            "save replacement {index} rendered black"
        );
        scenario
            .app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and a GPU; run serially"]
fn save_loading_and_cached_world_entry_reveal_only_ready_scenes() -> Result<()> {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    let mut scenario = Scenario::boot(Default::default(), |_| {})?;
    // Exercise a visible field, including a return to already-resident world graphics.
    let output = std::env::var_os("RESONANCE_WORLD_FADE_CAPTURE").map(std::path::PathBuf::from);
    if let Some(path) = &output {
        std::fs::create_dir_all(path)?;
    }
    let capture = |app: &mut App, name: &str| -> Result<()> {
        let Some(output) = &output else { return Ok(()) };
        let failed = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicU32::new(0));
        crate::new_game_capture::screenshot_held(
            app,
            output.join(name),
            failed.clone(),
            written.clone(),
        )?;
        anyhow::ensure!(
            !failed.load(Ordering::Acquire) && written.load(Ordering::Acquire) == 1,
            "fade readback failed"
        );
        Ok(())
    };
    let brightness = |app: &App| {
        app.world()
            .resource::<Assets<crate::materials::TitleOutput>>()
            .iter()
            .next()
            .unwrap()
            .1
            .brightness
            .x
    };
    scenario.request_scene(332)?;
    scenario.wait(
        "saved field did not become ready",
        super::scenario::scene_ready,
    )?;
    assert!(
        scenario
            .app
            .world()
            .contains_resource::<crate::new_game::scene_fade::Fade>()
    );
    scenario.wait("save fade did not begin", |app| brightness(app) > 0.)?;
    for _ in 0..6 {
        scenario.update()?;
    }
    assert!(
        brightness(&scenario.app) < 1.,
        "loading a save skipped its fade in"
    );
    capture(&mut scenario.app, "load-fade-in.png")?;
    // Depart during the saved field's fade in as well as after a cached return.
    for visit in 0..2 {
        let events = scenario
            .app
            .world_mut()
            .resource_mut::<crate::new_game::Session>()
            .into_inner()
            .events_mut();
        events.world.fade = Some(resonance_events::Fade {
            start_tick: events.tick(),
            duration: 0,
            from: 0.,
            to: 0.,
            white: false,
        });
        scenario
            .app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
        scenario.update()?;
        scenario
            .app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::ZERO,
            ));
        capture(&mut scenario.app, &format!("{visit}-field.png"))?;
        let previous_brightness = brightness(&scenario.app);
        scenario.request_scene(3000)?;
        scenario.update()?;
        let initial_brightness = brightness(&scenario.app);
        assert!(
            (initial_brightness - previous_brightness).abs() <= 1. / 15. + 0.001,
            "replacing the load fade caused a brightness jump"
        );
        assert!(crate::new_game::scene_fade::outgoing(scenario.app.world()));
        for _ in 0..7 {
            scenario.update()?;
        }
        assert!(
            scenario
                .app
                .world()
                .resource::<crate::new_game::Session>()
                .is_field(),
            "cached destination skipped the outgoing fade"
        );
        assert!((brightness(&scenario.app) - initial_brightness * 8. / 15.).abs() < 0.001);
        capture(&mut scenario.app, &format!("{visit}-fade-out.png"))?;
        scenario.wait("world artwork did not become ready", |app| {
            app.world()
                .resource::<crate::new_game::Session>()
                .overworld()
                .is_some()
                && super::scenario::scene_ready(app)
        })?;
        scenario.wait("world fade did not begin", |app| brightness(app) > 0.)?;
        for _ in 0..6 {
            scenario.update()?;
        }
        assert!(
            brightness(&scenario.app) < 1.,
            "ready destination skipped its incoming fade"
        );
        capture(&mut scenario.app, &format!("{visit}-fade-in.png"))?;
        scenario.wait("world fade did not finish", |app| {
            !app.world()
                .contains_resource::<crate::new_game::scene_fade::Fade>()
        })?;
        assert_eq!(brightness(&scenario.app), 1.);
        capture(&mut scenario.app, &format!("{visit}-world.png"))?;
        if visit == 0 {
            scenario.enter(332)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and a native GPU; run serially"]
fn failed_world_ui_image_reports_selected_error_without_hanging() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?,
    );
    let overlay = tempfile::tempdir()?;
    let image = "ui/menu/sylvarant-map.ktx2";
    std::fs::create_dir_all(overlay.path().join("ui/menu"))?;
    std::fs::write(overlay.path().join(image), b"invalid unused map image")?;
    let mut options = crate::test_support::run_options(root);
    options.mods.push(overlay.path().to_owned());
    options.paranoid = false;
    let mut scenario = Scenario::boot_with_options(options, Default::default(), |_| {})?;
    scenario.enter(332)?;
    assert!(
        scenario.enter(3000).is_err(),
        "failed world UI must not stay loading"
    );
    let error = scenario
        .app
        .world()
        .resource::<crate::diagnostics::FatalErrors>()
        .finish(Ok(()))
        .unwrap_err();
    assert!(
        format!("{error:#}").contains(image),
        "original loader error lost: {error:#}"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and a GPU; run serially"]
fn unused_battle_gpu_failure_does_not_block_world_admission() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
    let work = tempfile::tempdir()?;
    let overlay = work.path().join("mod");
    let image = resonance_content::model_preview::document::TOON_RAMP;
    let path = overlay.join(image);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, b"invalid unused battle toon texture")?;
    let mut options = crate::test_support::run_options(std::path::PathBuf::from(root));
    options.mods.push(overlay);
    options.paranoid = false;
    let save = work.path().join("world.json");
    crate::saves::prepare_overworld_test_fixture(&options.assets, &options.mods, None, &save)?;
    options.saves.load = Some(save);
    options.saves.directory = Some(work.path().join("saves"));
    let mut strict = crate::test_support::run_options(options.assets.clone());
    strict.mods = options.mods.clone();
    strict.saves.load = options.saves.load.clone();
    strict.saves.directory = options.saves.directory.clone();
    let mut scenario = Scenario::boot_with_options(options, Default::default(), |_| {})?;
    scenario.wait(
        "healthy world did not become ready",
        super::scenario::scene_ready,
    )?;
    let world = scenario.app.world();
    assert!(
        world
            .resource::<crate::new_game::Session>()
            .overworld()
            .is_some()
    );
    assert_eq!(
        world.resource::<crate::battle_view::ResidentModels>().len(),
        0
    );
    assert_eq!(
        world
            .resource::<crate::battle_view::ResidentBindings>()
            .len(),
        0
    );
    assert!(
        world
            .resource::<crate::diagnostics::Diagnostics>()
            .0
            .entries()
            .iter()
            .any(|entry| entry.scope == "startup battle graphics" && entry.message.contains(image))
    );
    drop(scenario);
    let error = Scenario::boot_with_options(strict, Default::default(), |_| {})
        .err()
        .context("strict startup must preserve the image error")?;
    assert!(format!("{error:#}").contains(image), "{error:#}");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and a native GPU; run serially"]
fn unused_world_gpu_failure_does_not_block_field_admission() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
    let overlay = tempfile::tempdir()?;
    let path = overlay
        .path()
        .join("world/skies/normal/textures/part-0-texture-0.ktx2");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, b"invalid unused world texture")?;
    let mut options = crate::test_support::run_options(std::path::PathBuf::from(root));
    options.mods.push(overlay.path().to_owned());
    options.paranoid = false;
    let mut scenario = Scenario::boot_with_options(options, Default::default(), |_| {})?;
    // CPU capture succeeded; only GPU preparation rejected this texture.
    assert!(
        scenario
            .app
            .world()
            .resource::<crate::loading::Resident>()
            .common
            .get()
            .unwrap()
            .world
            .is_some()
    );
    assert!(
        scenario
            .app
            .world()
            .resource::<crate::diagnostics::Diagnostics>()
            .0
            .entries()
            .iter()
            .any(|entry| entry.scope == "startup world")
    );
    scenario.enter(332)?;
    assert!(
        scenario.enter(3000).is_err(),
        "selected world errors must still surface"
    );
    Ok(())
}

#[test]
fn common_images_share_identity_and_retirement_keeps_only_resident_entities() {
    let resident = Resident::default();
    let mut images = Assets::<Image>::default();
    let first = resident.images(&mut images);
    assert_eq!(first, resident.images(&mut images));
    assert_eq!(images.len(), 2);
    let weak = resident.downgrade();
    let mut world = World::new();
    let common = world.spawn((Canvas, Visibility::Visible)).id();
    let transient = world.spawn(Visibility::Visible).id();
    let dialogue = DialogueOverlay {
        images: Vec::new(),
        surfaces: Vec::new(),
        layers: [common, transient]
            .into_iter()
            .enumerate()
            .map(|(index, entity)| {
                (
                    (0, index),
                    Layer {
                        entity,
                        mesh: Handle::default(),
                        material: Handle::default(),
                        uploaded: None,
                        visible: true,
                    },
                )
            })
            .collect(),
        head_heights: Default::default(),
        choice_trail: Default::default(),
    };
    dialogue.despawn(&mut world);
    assert_eq!(world.get::<Visibility>(common), Some(&Visibility::Hidden));
    assert!(world.get_entity(transient).is_err());
    drop(resident);
    assert!(weak.upgrade().is_none());
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and a native GPU; run serially"]
fn boot_and_field_reentry_reuse_uploaded_ui() -> Result<()> {
    use bevy::{
        pbr::PreparedMaterial,
        render::{Render, RenderApp, RenderSystems, erased_render_asset::ErasedRenderAssets},
    };
    #[derive(Default)]
    struct DelayedBinding {
        map: Option<u32>,
        material: Option<bevy::asset::UntypedAssetId>,
        entity: Option<Entity>,
        withheld: bool,
    }
    let delayed = Arc::new(Mutex::new(DelayedBinding::default()));
    let mut scenario = Scenario::boot(Default::default(), |app| {
        let selected = delayed.clone();
        app.add_systems(
            Update,
            (move |world: &mut World| {
                let mut delay = selected.lock().unwrap();
                if delay.map.is_some()
                    && world
                        .get_resource::<crate::field_view::Art>()
                        .is_some_and(|art| Some(art.map) == delay.map)
                    && delay.material.is_none()
                {
                    // Add a fresh binding to the retained scene. It must join
                    // admission even though no model pipeline needs rebuilding.
                    let material = world
                        .resource_mut::<Assets<crate::materials::TitleSurface>>()
                        .add(crate::materials::TitleSurface::default());
                    let mesh = world
                        .resource_mut::<Assets<Mesh>>()
                        .add(Rectangle::default());
                    delay.material = Some(material.id().untyped());
                    delay.entity = Some(
                        world
                            .spawn((
                                Mesh3d(mesh),
                                MeshMaterial3d(material),
                                Transform::default(),
                                Visibility::Visible,
                                bevy::camera::visibility::NoFrustumCulling,
                            ))
                            .id(),
                    );
                }
            })
            .after(crate::field_view::FieldPreparation),
        );
        let selected = delayed.clone();
        app.sub_app_mut(RenderApp).add_systems(Render, (
            move |mut materials: ResMut<ErasedRenderAssets<PreparedMaterial>>,
                  mut held: Local<Option<(bevy::asset::UntypedAssetId, PreparedMaterial)>>| {
                let mut delay = selected.lock().unwrap();
                if delay.map.is_some() {
                    if let Some(id) = delay.material
                        && let Some(material) = materials.remove(id)
                    {
                        *held = Some((id, material));
                        delay.withheld = true;
                    }
                } else if let Some((id, material)) = held.take() {
                    materials.insert(id, material);
                }
            }
        ).after(RenderSystems::PrepareAssets).before(RenderSystems::Queue));
    })?;
    let resident = scenario.app.world().resource::<Resident>().clone();
    let common = scenario
        .app
        .world()
        .resource::<crate::loading::Resident>()
        .common
        .get()
        .unwrap()
        .clone();
    let battle = common.battle.as_ref().context("startup battle missing")?;
    let shared_effects: Vec<_> = scenario
        .app
        .world()
        .resource::<crate::field_effects::Shared>()
        .bindings()
        .map(|(mesh, material)| (mesh.id(), material.id()))
        .collect();
    let shared_shadow = scenario
        .app
        .world()
        .resource::<crate::field_view::SharedShadows>()
        .binding();
    let entities: std::collections::HashSet<_> = resident.entities().into_iter().collect();
    assert!(!entities.is_empty());
    assert_eq!(
        entities.len(),
        resident.entities().len(),
        "UI canvases alias"
    );
    let subtitle = resident
        .0
        .lock()
        .unwrap()
        .field
        .as_ref()
        .unwrap()
        .subtitle_layer
        .as_ref()
        .unwrap()
        .entity;
    assert_eq!(
        scenario
            .app
            .world()
            .get::<bevy::camera::visibility::RenderLayers>(subtitle),
        Some(&bevy::camera::visibility::RenderLayers::layer(3))
    );
    let check = |app: &App| -> Result<()> {
        resident.check(app.world())?;
        crate::loading::graphics::check(app.world())?;
        assert_eq!(entities, resident.entities().into_iter().collect());
        Ok(())
    };
    check(&scenario.app)?;

    // One route checks field and world reentry through production admission.
    let mut fields = BTreeMap::new();
    let mut world_roots = None;
    let mut delayed_reentry = false;
    for map in [332, 340, 520, 332, 520, 3000, 332, 3000] {
        if map == 332 && fields.contains_key(&map) && !delayed_reentry {
            // Field reentry and same-map save reloads both create fresh bindings.
            for reload in [false, true] {
                *delayed.lock().unwrap() = DelayedBinding {
                    map: Some(map),
                    ..Default::default()
                };
                if reload {
                    let world = scenario.app.world_mut();
                    let package = world
                        .resource::<crate::new_game::Session>()
                        .field_package()
                        .clone();
                    let saved = crate::test_support::field_checkpoint(&package.files)?;
                    let candidate =
                        crate::new_game::Session::load_prepared(package, Some(saved), None)?;
                    crate::saves::install_loaded(world, candidate);
                } else {
                    scenario.request_scene(map)?;
                }
                scenario.wait("reentry did not reach the delayed material", |app| {
                    let (material, withheld) = {
                        let delay = delayed.lock().unwrap();
                        (delay.material, delay.withheld)
                    };
                    if material.is_some() {
                        assert!(
                            !app.world()
                                .resource::<crate::loading::Resident>()
                                .active
                                .load(std::sync::atomic::Ordering::Acquire),
                            "scene admitted before its new material (reload={reload})"
                        );
                    }
                    withheld
                })?;
                for _ in 0..3 {
                    scenario.update()?;
                    assert!(
                        !scenario
                            .app
                            .world()
                            .resource::<crate::loading::Resident>()
                            .active
                            .load(std::sync::atomic::Ordering::Acquire)
                    );
                }
                delayed.lock().unwrap().map = None;
                if reload {
                    scenario.wait("reloaded field did not become visible", |app| {
                        super::scenario::scene_ready(app)
                            && !app
                                .world()
                                .contains_resource::<crate::new_game::scene_fade::Fade>()
                    })?;
                } else {
                    scenario.wait_for_scene(map)?;
                }
                scenario
                    .app
                    .world_mut()
                    .despawn(delayed.lock().unwrap().entity.take().unwrap());
            }
            delayed_reentry = true;
        } else {
            scenario.enter(map)?;
        }
        check(&scenario.app)?;
        if map >= 3000 {
            let roots = crate::overworld::residency_roots(scenario.app.world_mut());
            assert!(!roots.is_empty());
            assert_eq!(world_roots.get_or_insert_with(|| roots.clone()), &roots);
            continue;
        }
        let session = scenario.app.world().resource::<crate::new_game::Session>();
        let effects = scenario
            .app
            .world()
            .resource::<crate::field_effects::Artwork>();
        let bindings: std::collections::HashSet<_> = effects
            .prepared_bindings()
            .map(|(mesh, material)| (mesh.id(), material.id()))
            .collect();
        assert!(
            shared_effects
                .iter()
                .all(|binding| bindings.contains(binding)),
            "field entry rebuilt default effect bindings"
        );
        assert_eq!(
            scenario
                .app
                .world()
                .resource::<crate::field_view::Art>()
                .shadow_binding(),
            Some(shared_shadow.clone())
        );
        let slot = Arc::downgrade(&session.field_package().graphics);
        if let Some(previous) = fields.insert(map, slot) {
            assert!(
                previous.upgrade().is_none(),
                "a departed field kept its graphics owner"
            );
        }
        assert!(Arc::ptr_eq(
            &session.field_package().services,
            &common.services
        ));
        assert!(
            scenario
                .app
                .world()
                .resource::<Artwork>()
                .common_layers()
                .all(|layer| entities.contains(&layer.entity))
        );
        let art = scenario.app.world().resource::<crate::field_view::Art>();
        for part in art.models.values().flatten() {
            let motions = part
                .spec
                .prepared_motion_snapshot()
                .context("field motions missing")?;
            assert!(part.clips.is_empty());
            assert_eq!(motions.len(), part.motions.len());
            assert!(
                motions
                    .iter()
                    .zip(&part.motions)
                    .all(|(a, b)| Arc::ptr_eq(a, b))
            );
        }
    }

    // A cinematic uses its own graph while the ordinary world stays resident.
    scenario.enter(332)?;
    scenario
        .app
        .world_mut()
        .resource_mut::<crate::new_game::Session>()
        .events_mut()
        .world
        .request_world(
            513,
            0,
            Some(resonance_events::SceneDestination {
                map: 3000,
                position: [0.; 3],
                heading: 0.,
            }),
        )
        .map_err(anyhow::Error::msg)?;
    scenario.wait_for_scene(3000)?;
    assert!(
        world_roots.as_ref().unwrap().iter().all(|&root| scenario
            .app
            .world()
            .get_entity(root)
            .is_ok())
    );
    scenario.enter(332)?;
    scenario.enter(3000)?;
    assert_eq!(
        world_roots.as_ref().unwrap(),
        &crate::overworld::residency_roots(scenario.app.world_mut())
    );

    // Reenter battle with a different costume; materials and effects still reuse
    // their fixed owners, while the selected alternate retires when deselected.
    let mut alternate = None;
    for costume in [0, 1] {
        if costume == 1 {
            for selected in [1, 0, 1] {
                scenario
                    .app
                    .world_mut()
                    .resource_mut::<crate::new_game::Session>()
                    .events_mut()
                    .world
                    .party
                    .as_mut()
                    .unwrap()
                    .members[0]
                    .costume = selected;
                scenario.update()?;
            }
            // Request battle while the selected costume is still being prepared.
            // Admission must await that job instead of capturing a second body.
            assert!(battle.body(1, 1).is_none());
        }
        // The battle-requesting update cannot run the ordinary field audio
        // adapter. Its outgoing cues must be admitted at the handoff instead.
        scenario
            .app
            .world_mut()
            .resource_mut::<crate::new_game::Session>()
            .events_mut()
            .world
            .audio_commands
            .push(resonance_events::AudioCommand::StopVoice);
        let request = scenario.battle()?;
        if costume == 1 {
            assert_eq!(crate::loading::party::retained(scenario.app.world()), 1);
            alternate = Some(Arc::downgrade(&battle.body(1, 1).unwrap()));
        }
        assert!(
            scenario
                .app
                .world()
                .resource::<crate::new_game::Session>()
                .events()
                .world
                .audio_commands
                .is_empty()
        );
        crate::battle_view::check_entry_frame(scenario.app.world_mut())?;
        crate::loading::startup::check_audio_reuse(scenario.app.world())?;
        crate::battle_view::startup::check_reuse(scenario.app.world())?;
        check(&scenario.app)?;
        scenario.return_from_battle(request)?;
        scenario
            .app
            .world()
            .resource::<crate::battle_view::ResidentModels>()
            .check_returned(
                scenario
                    .app
                    .world()
                    .resource::<Assets<crate::materials::TitleSurface>>(),
            )?;
        check(&scenario.app)?;
    }
    scenario
        .app
        .world_mut()
        .resource_mut::<crate::new_game::Session>()
        .events_mut()
        .world
        .party
        .as_mut()
        .unwrap()
        .members[0]
        .costume = 0;
    scenario.update()?;
    assert_eq!(crate::loading::party::retained(scenario.app.world()), 0);
    assert!(alternate.unwrap().upgrade().is_none());

    let weak = resident.downgrade();
    drop((resident, scenario));
    assert!(weak.upgrade().is_none(), "UI ownership cycle");
    Ok(())
}

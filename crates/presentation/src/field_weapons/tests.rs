use super::*;
use resonance_content::prepared::Files;
use std::time::Instant;

#[test]
fn carried_props_keep_their_identity_after_detaching() {
    use resonance_events::{Actor, Attachment, FieldWeapon, GameWorld};
    let mut world = GameWorld::default();
    world.insert_actor(99, Actor::new(9, [0.; 3]));
    let mut prop = Actor::new(12345, [0.; 3]);
    prop.attachment = Some(Attachment {
        actor: 99,
        bone: "kk00".into(),
    });
    let expected = Some(FieldWeapon {
        character: 9,
        slot: 0,
    });
    assert_eq!(carried(&prop, &world), expected);
    prop.field_weapon = carried(&prop, &world);
    prop.attachment = None;
    assert_eq!(carried(&prop, &world), expected);
}

#[test]
fn field_weapon_system_queries_are_compatible() {
    use bevy::ecs::system::{IntoSystem, System};
    let mut world = World::new();
    IntoSystem::into_system(prepare).initialize(&mut world);
    IntoSystem::into_system(instances).initialize(&mut world);
    IntoSystem::into_system(render).initialize(&mut world);
    IntoSystem::into_system(choreograph).initialize(&mut world);
    IntoSystem::into_system(dynamics).initialize(&mut world);
    IntoSystem::into_system(strings).initialize(&mut world);
    IntoSystem::into_system(publish).initialize(&mut world);
}

#[test]
fn texture_animation_preserves_signed_scroll_and_frame_order() -> Result<()> {
    let scroll = serde_json::from_value(serde_json::json!({
        "kind":"scroll", "texture":2, "step_milli_uv":[80,-12], "offset_milli_uv":[-128,4]
    }))?;
    assert_eq!(texture_offset(&scroll, 0), (2, [-0.128, 0.004]));
    assert_eq!(texture_offset(&scroll, 10), (2, [0.672, -0.116]));
    assert_eq!(texture_offset(&scroll, 1000), (2, [0.872, -0.996]));
    let atlas = serde_json::from_value(serde_json::json!({
        "kind":"atlas", "texture":1, "rows":4, "ticks_per_frame":3, "frames":[0,1,2,3,2,1]
    }))?;
    for (frame, row) in [0., 0.25, 0.5, 0.75, 0.5, 0.25, 0.].into_iter().enumerate() {
        assert_eq!(texture_offset(&atlas, frame as u32 * 3), (1, [0., row]));
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; CPU only"]
fn weapon_effect_layers_are_preserved() -> Result<()> {
    use resonance_content::{diagnostics::Diagnostics, library::Library};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let library = Library::open(std::path::Path::new(&root), &[])?;
    for item in [145, 155, 158, 174, 189, 204, 218, 241, 257, 273] {
        let model = capture(&library, item, &Diagnostics::new(true), &|| false)?;
        assert!(
            model
                .layers
                .iter()
                .any(|(_, preview, texture)| { preview.animation.is_some() || texture.is_some() }),
            "item {item} lost its animation"
        );
        match item {
            145 => {
                assert_eq!(model.layers.len(), 4);
                assert_eq!(
                    model.layers.iter().filter(|(_, p, _)| p.additive).count(),
                    2
                );
            }
            155 => assert!(
                model
                    .layers
                    .iter()
                    .all(|(_, p, _)| !p.scene.secondary_motion.is_empty())
            ),
            158 => assert!(model.layers.iter().all(|(_, p, _)| p.animation.is_some())),
            _ => {}
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; CPU only"]
fn readiness_uses_current_equipment_after_a_swap() -> Result<()> {
    use resonance_content::{diagnostics::Diagnostics, library::Library, preparation};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let mut snapshot = Library::open(std::path::Path::new(&root), &[])?.snapshot();
    let diagnostics = Diagnostics::new(true);
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let data = resonance_content::session::SessionData::load(&files)?;
    let mut party = resonance_events::party::Party::new(&data, Default::default())?;
    for member in &mut party.members {
        member.equipment.fill(0);
    }
    party.members[0].equipment[0] = 135;
    let mut world = resonance_events::GameWorld::default();
    world.party = Some(party);
    let mut artwork = EquipmentCache::default();
    artwork.equipment.insert(
        135,
        Equipment {
            shield: false,
            layers: vec![],
            ready: true,
            started: Instant::now(),
        },
    );
    assert!(artwork.assets_ready(&world));
    world.party.as_mut().unwrap().members[0].equipment[0] = 145;
    assert!(
        !artwork.assets_ready(&world),
        "an uncached equipment swap must stop simulation"
    );
    Ok(())
}

#[test]
fn retiring_a_field_removes_weapon_scenes_and_strings() {
    let mut world = World::new();
    let owner = world.spawn_empty().id();
    let root = world
        .spawn(Instance {
            owner,
            actor: 120,
            item: 145,
            slot: 0,
            layer: 0,
            birth: 0,
            binding: None,
            camera_facing: vec![],
            materials: vec![],
            geometry: vec![],
            dynamics: None,
            choreography: None,
            prepared: false,
            sampled: false,
            active: false,
            rope: None,
        })
        .id();
    let string = world.spawn(ChildOf(root)).id();
    retire(&mut world);
    assert!(world.get_entity(root).is_err());
    assert!(world.get_entity(string).is_err());
    assert!(world.get_entity(owner).is_ok());
}

fn fixture(map: u32, id: i32) -> Result<(App, Entity, Entity, Entity)> {
    use resonance_content::{diagnostics::Diagnostics, library::Library, preparation};
    let path = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let library = Library::open(std::path::Path::new(&path), &[])?;
    let mut snapshot = library.snapshot();
    let (_, manifest) =
        resonance_content::field::document::capture_scene(&mut snapshot, map, || false)?;
    let diagnostics = Diagnostics::new(true);
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let data = resonance_content::session::SessionData::load(&files)?;
    let mut party = resonance_events::party::Party::new(&data, Default::default())?;
    for member in &mut party.members {
        member.equipment.fill(0);
    }
    party.members[0].equipment[0] = 155;
    let script: Vec<_> = [4u16, 0, 0, 0, 0x20ff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut field = resonance_game::field::FieldSession::new(&script, vec![], &manifest)?;
    field.events.world.actors.clear();
    field.events.world.party = Some(party);
    let mut actor = resonance_events::Actor::new(131355, [0.; 3]);
    actor.field_weapon = Some(resonance_events::FieldWeapon {
        character: 1,
        slot: 0,
    });
    field.events.world.insert_actor(id, actor);
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: path.to_string_lossy().into_owned(),
            ..default()
        },
    ))
    .register_asset_loader(bevy::image::ImageLoader::new(
        bevy::image::CompressedImageFormats::NONE,
    ))
    .init_asset::<Image>()
    .init_asset::<bevy::gltf::Gltf>()
    .init_asset::<crate::sparse_animation::Clip>()
    .init_resource::<Assets<TitleSurface>>()
    .init_resource::<crate::scene::SampledImages>()
    .init_resource::<EquipmentCache>();
    let art =
        crate::field_view::prepared_test_art(&manifest, app.world().resource::<AssetServer>());
    app.insert_resource(art);
    let generic = app
        .world_mut()
        .spawn((Visibility::Inherited, crate::field_view::ActorGeometry(id)))
        .id();
    let replacement = app
        .world_mut()
        .spawn((Visibility::Inherited, crate::field_view::ActorGeometry(id)))
        .id();
    let mut part = ActorPart::new(id, &field.events.world.actors[&id], 0, 0, vec![]);
    part.prepared = true;
    part.geometry = vec![(0, generic)];
    let owner = app.world_mut().spawn((part, Visibility::Inherited)).id();
    app.insert_resource(crate::field_view::Session(field));
    Ok((app, owner, generic, replacement))
}

fn published(owner: Entity, item: u16, layer: usize, active: bool) -> (Instance, Visibility) {
    (
        Instance {
            owner,
            actor: 120,
            item,
            slot: 0,
            layer,
            birth: 0,
            binding: None,
            camera_facing: vec![],
            materials: vec![],
            geometry: vec![],
            dynamics: None,
            choreography: None,
            prepared: true,
            sampled: true,
            active,
            rope: None,
        },
        Visibility::Inherited,
    )
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; CPU only"]
fn cold_swap_keeps_old_model_until_all_new_layers_are_ready() -> Result<()> {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, owner, generic, _) = fixture(7, 120)?;
    let old = app.world_mut().spawn(published(owner, 145, 0, true)).id();
    app.world_mut().run_system_once(instances).unwrap();
    app.world_mut().run_system_once(publish).unwrap();
    assert!(app.world().get::<Instance>(old).unwrap().active);
    assert_eq!(
        *app.world().get::<Visibility>(generic).unwrap(),
        Visibility::Hidden
    );

    let path = std::env::var_os("RESONANCE_TEST_ASSETS").unwrap();
    let library = resonance_content::library::Library::open(std::path::Path::new(&path), &[])?;
    let captured = capture(
        &library,
        155,
        &resonance_content::diagnostics::Diagnostics::new(true),
        &|| false,
    )?;
    let server = app.world().resource::<AssetServer>();
    let layers = captured
        .layers
        .into_iter()
        .map(|(part, preview, animation)| Layer {
            part,
            art: crate::field_view::Part::load(preview.scene.clone(), server, &Default::default()),
            preview,
            animation,
        })
        .collect();
    app.world_mut()
        .resource_mut::<EquipmentCache>()
        .equipment
        .insert(
            155,
            Equipment {
                shield: false,
                layers,
                ready: true,
                started: Instant::now(),
            },
        );
    let first = app.world_mut().spawn(published(owner, 155, 0, false)).id();
    *app.world_mut().get_mut::<Visibility>(generic).unwrap() = Visibility::Inherited;
    app.world_mut().run_system_once(publish).unwrap();
    assert!(app.world().get::<Instance>(old).unwrap().active);
    assert!(!app.world().get::<Instance>(first).unwrap().active);
    let second = app.world_mut().spawn(published(owner, 155, 1, false)).id();
    *app.world_mut().get_mut::<Visibility>(generic).unwrap() = Visibility::Inherited;
    app.world_mut().run_system_once(publish).unwrap();
    assert!(!app.world().get::<Instance>(old).unwrap().active);
    assert!(
        [first, second]
            .into_iter()
            .all(|e| app.world().get::<Instance>(e).unwrap().active)
    );
    app.world_mut().run_system_once(instances).unwrap();
    assert!(app.world().get_entity(old).is_err());
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; CPU only"]
fn reflection_only_policy_routes_replacements_without_unhiding_generic_meshes() -> Result<()> {
    use bevy::{camera::visibility::RenderLayers, ecs::system::RunSystemOnce};
    let (mut app, _, generic, replacement) = fixture(520, 999980)?;
    let string = app
        .world_mut()
        .spawn((
            Visibility::Inherited,
            crate::field_view::ActorGeometry(999980),
        ))
        .id();
    *app.world_mut().get_mut::<Visibility>(generic).unwrap() = Visibility::Hidden;
    app.world_mut()
        .run_system_once(crate::field_view::actor_layers)
        .unwrap();
    let layers = app.world().get::<RenderLayers>(generic).unwrap();
    assert!(!layers.intersects(&RenderLayers::layer(0)));
    for entity in [replacement, string] {
        assert_eq!(app.world().get::<RenderLayers>(entity), Some(layers));
    }
    assert_eq!(
        *app.world().get::<Visibility>(generic).unwrap(),
        Visibility::Hidden
    );
    Ok(())
}

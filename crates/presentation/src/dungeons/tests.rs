//! Device-free scene regressions using fresh progression fixtures.
use resonance_events::input::Button;
#[path = "crafting_tests.rs"]
mod crafting;
#[path = "exploration_tests.rs"]
mod exploration;
#[path = "fog_tests.rs"]
mod fog;
#[path = "iselia_tests.rs"]
mod iselia;
#[path = "movement_tests.rs"]
mod movement;
use super::{destinations::*, new_game};
use crate::field_test::{SCENE_TIMEOUT, Scene, dialogue_input};
use anyhow::{Context, Result};
use resonance_game::field::{FieldInput, FieldSession};
use std::{path::PathBuf, sync::Arc};

#[test]
fn location_menu_pages_and_shortcuts_select_visible_rows() {
    use super::{DESTINATIONS, Menu, PAGE_SIZE};
    let mut menu = Menu::default();
    menu.turn_page(true);
    assert_eq!(menu.selected, PAGE_SIZE);
    assert!(menu.select_row(2));
    assert_eq!(menu.selected, PAGE_SIZE + 2);
    menu.turn_page(false);
    assert_eq!(menu.selected, 2);
    menu.turn_page(false);
    assert_eq!(
        menu.page_start(),
        (DESTINATIONS.len() - 1) / PAGE_SIZE * PAGE_SIZE
    );
    assert!(!menu.select_row(PAGE_SIZE));
    let remaining = DESTINATIONS.len() - menu.page_start();
    assert!(menu.select_row(remaining - 1));
    assert!(!menu.select_row(remaining));
    menu.turn_page(true);
    assert_eq!(menu.page_start(), 0);

    use bevy::prelude::*;
    let mut world = World::new();
    menu.state = super::State::AwaitRelease;
    world.insert_resource(menu);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::KeyW);
    keys.press(KeyCode::Enter);
    keys.clear();
    world.insert_resource(keys);
    super::controls(&mut world);
    assert!(world.resource::<Menu>().blocked());
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Enter);
    super::controls(&mut world);
    assert!(!world.resource::<Menu>().blocked());
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn location_menu_checkpoints_are_playable() -> Result<()> {
    let root = assets_root()?;
    let mut cache = Default::default();
    let mut failures = Vec::new();
    for &destination in super::DESTINATIONS {
        let result = (|| -> Result<()> {
            let package =
                new_game::FieldPackage::prepare(&root, destination.map, &mut cache, || false)?;
            let entry = destination.entry(
                Arc::new(package.files.json("game/session-data.json")?),
                new_game::available_fields(&root)?,
            )?;
            let mut field = Scene::enter(&package, entry)?;
            field.replay_to_control(&root)?;
            anyhow::ensure!(
                field
                    .events
                    .world
                    .fade
                    .as_ref()
                    .is_none_or(|fade| fade.alpha(field.events.tick()) == 0.),
                "checkpoint returned control behind a fade"
            );
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error:#}", destination.name));
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn location_menu_end_checkpoints_reach_final_rooms_and_encounters() -> Result<()> {
    let root = assets_root()?;
    let mut cache = Default::default();
    for (destination, portal, final_room) in [
        (OSSA_END, [150., 0.], 350),
        (MARTEL_END, [0., 2775.], 309),
        (TRIET_END, [-2131., -154.], 221),
        (THODA_END, [-2555., -611.], 10),
        (BALACRUF_END, [11., 2000.], 510),
        (MANA_END, [-852., 2321.], 369),
        (BASE_END, [-1000., 950.], 276),
        (PALMACOSTA_END, [0., 875.], 207),
        (ASGARD_END, [0., 1060.], 217),
        (ISELIA_END, [2065., 6100.], 197),
        (REMOTE_ISLAND_END, [600., -2.], 232),
        (SALVATION_END, [0., 4860.], 535),
    ] {
        let map = destination.map;
        let package = new_game::FieldPackage::prepare(&root, map, &mut cache, || false)?;
        let entry = destination.entry(
            Arc::new(package.files.json("game/session-data.json")?),
            new_game::available_fields(&root)?,
        )?;
        let mut field = Scene::enter(&package, entry)?;
        field.advance_until(FieldSession::player_has_control)?;
        if map == 366 {
            const LIT_RECEIVER: u32 = (-1_179_642_i32) as u32;
            assert_eq!(
                field
                    .events
                    .world
                    .actors
                    .values()
                    .filter(|a| a.visible && a.resource == LIT_RECEIVER)
                    .count(),
                3,
                "all three receivers must be lit in the solved mirror room"
            );
        }
        anyhow::ensure!(
            field.events.world.field_transition.is_none(),
            "{} started inside its exit",
            destination.name
        );
        for tick in 0..SCENE_TIMEOUT {
            let player = field.actor(field.events.world.controlled_actor).position;
            let delta = [portal[0] - player[0], portal[1] - player[1]];
            let length = delta[0].hypot(delta[1]).max(4.);
            field.step(FieldInput {
                pressed_buttons: field
                    .walking(delta.map(|v| v / length))
                    .pressed_buttons
                    .with(Button::Ring, map == 206 && tick == 0)
                    .with(Button::Accept, tick % 2 == 0),
                held_buttons: field
                    .walking(delta.map(|v| v / length))
                    .held_buttons
                    .with(Button::Accept, true),
                ..field.walking(delta.map(|v| v / length))
            })?;
            if map == final_room && field.events.world.battle_request.is_some() {
                break;
            }
            skip_battle(&mut field)?;
            if field.events.world.field_transition.is_some() {
                break;
            }
        }
        if map == final_room {
            anyhow::ensure!(
                field.events.world.battle_request.is_some(),
                "{} cannot reach its final encounter; position={:?}, waits={:?}",
                destination.name,
                field.actor(field.events.world.controlled_actor).position,
                field.events.pending_operations()
            );
            continue;
        }
        anyhow::ensure!(
            field.events.world.field_transition.as_ref().map(|t| t.map) == Some(final_room),
            "{} cannot reach the final room; position={:?}, waits={:?}",
            destination.name,
            field.actor(field.events.world.controlled_actor).position,
            field.events.pending_operations()
        );
        if matches!(final_room, 221 | 10 | 510 | 369) {
            field.follow_transition(&root)?;
            field
                .until(dialogue_input(), |f| {
                    if let Some(movie) = &f.events.world.movie {
                        movie.operation.complete(None).map_err(anyhow::Error::msg)?;
                    }
                    Ok(f.events.world.battle_request.is_some())
                })
                .with_context(|| format!("{} final encounter", destination.name))?;
            skip_battle(&mut field)?;
            let mut saw_wings = false;
            field
                .advance_until(|field| {
                    saw_wings |= field.events.world.actors.values().any(|actor| {
                        actor.visible
                            && actor.wings.is_some()
                            && actor.attachment.as_ref().is_some_and(|a| a.actor == 2)
                    });
                    field.player_has_control()
                })
                .with_context(|| format!("{} seal scene", destination.name))?;
            assert!(
                saw_wings,
                "{} blessing never showed wings",
                destination.name
            );
            match final_room {
                221 => assert_eq!(field.story_progress()?, 1_303_000),
                10 => assert_eq!(mission(&field, 0xc4), 21_000),
                _ => {}
            }
            let (flag, angel) = match final_room {
                221 => (200, 1),
                10 => (201, 1),
                510 => (202, 101),
                369 => (203, 201),
                _ => unreachable!(),
            };
            assert!(field.events.world.event_flags.contains(&flag));
            assert_eq!(
                field
                    .events
                    .memory()
                    .read(0x4c, symphonia_script::Width::S32)?,
                angel
            );
        }
    }
    Ok(())
}

fn assets_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    ))
}
fn configured(
    destination: Destination,
    map: u32,
    configure: impl FnOnce(&mut resonance_game::field::FieldEntry) -> Result<()>,
) -> Result<Scene> {
    let root = assets_root()?;
    let package = new_game::FieldPackage::prepare(&root, map, &mut Default::default(), || false)?;
    let data = Arc::new(package.files.json("game/session-data.json")?);
    let mut entry = destination.entry(data, new_game::available_fields(&root)?)?;
    configure(&mut entry)?;
    Scene::enter(&package, entry)
}
fn enter(destination: Destination, map: u32, story: Option<i32>) -> Result<Scene> {
    configured(destination, map, |entry| {
        if let Some(story) = story {
            entry
                .persistent
                .memory
                .write(0x40, symphonia_script::Width::S32, story)?;
        }
        Ok(())
    })
}
fn mission(field: &FieldSession, address: u16) -> i32 {
    field
        .events
        .memory()
        .read(address, symphonia_script::Width::S32)
        .unwrap()
}
fn skip_battle(field: &mut FieldSession) -> Result<bool> {
    field
        .events
        .world
        .skip_battle_as_victory()
        .map_err(anyhow::Error::msg)
}
#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn martel_golem_battle_creates_a_pushable_block() -> Result<()> {
    let mut field = enter(MARTEL_START, 308, Some(107_000))?;
    field.advance_until(FieldSession::player_has_control)?;
    let golem = *field
        .events
        .world
        .actors
        .iter()
        .find(|(_, actor)| actor.enemy.as_ref().is_some_and(|enemy| enemy.event == 101))
        .context("golem contact event")?
        .0;
    assert!(field.events.contact_enemy(golem)?);
    field.advance_until(|field| field.events.world.battle_request.is_some())?;
    assert!(skip_battle(&mut field)?);
    field.advance_until(FieldSession::player_has_control)?;
    let block = field
        .events
        .world
        .actors
        .get(&5000)
        .context("defeated golem block")?;
    assert!(block.pushable);
    assert!(block.model_collision.is_some());
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn guard_entrance_event_can_pause_the_guards() -> Result<()> {
    let mut field = enter(BASE_ENTRANCE, 267, Some(1_105_000))?;
    field.advance_until(FieldSession::player_has_control)?;
    // Controlled approach to the exit, then ordinary movement through its
    // opening door and transition. This does not replay the cell escape.
    let player = field.events.world.controlled_actor;
    let actor = field.actor_mut(player);
    actor.position = [2252., 1000., 0.];
    actor.face(180.);
    field.until(
        FieldInput {
            direction: [0., 1.],
            ..Default::default()
        },
        |field| Ok(field.events.world.field_transition.is_some()),
    )?;
    field.follow_transition(&assets_root()?)?;
    assert_eq!(field.map_id, 268);
    field.advance_until(|field| {
        field
            .events
            .world
            .actors
            .get(&3004)
            .and_then(|actor| actor.enemy.as_ref())
            .is_some_and(|enemy| enemy.pause_ticks == -1)
            && field
                .events
                .world
                .billboards
                .values()
                .any(|effect| effect.recipe == 42)
    })?;
    assert_eq!(
        field.actor(3004).enemy.as_ref().unwrap().stun_effect(),
        Some(resonance_events::effect::StunEffect::Electric)
    );
    field.advance_until(|field| field.player_has_control() && mission(field, 0x40) == 1_105_100)?;
    let player = field.events.world.controlled_actor;
    let before = field.actor(player).position;
    field.ticks(
        10,
        FieldInput {
            direction: [0., -1.],
            ..Default::default()
        },
    )?;
    let after = field.actor(player).position;
    assert!(
        (after[0] - before[0]).hypot(after[1] - before[1]) > 20.,
        "player must be able to walk after the guards' scene"
    );
    field.until(FieldInput::default(), |field| {
        Ok(field.events.world.battle_request.is_some())
    })?;
    // The resumed patrol can reach Lloyd. The dungeon selector grants this
    // encounter as a victory, after which exploration must remain usable.
    assert!(skip_battle(&mut field)?);
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn sylvarant_electrified_drones_open_the_panel_door() -> Result<()> {
    use resonance_events::ring::{ElectricOrbKind, SorcerersRing};
    let mut field = enter(BASE_ENTRANCE, 268, Some(1_105_100))?;
    field.advance_until(FieldSession::player_has_control)?;
    field.party_mut().travel.sorcerers_ring =
        SorcerersRing::ElectricOrb(ElectricOrbKind::Sylvarant);
    assert!(!field.events.world.event_flags.contains(&154));
    assert!(field.events.world.actors.contains_key(&200));
    for (id, x) in [(8001, -1000.), (8002, 1000.)] {
        let drone = field.actor_mut(id);
        drone.position = [x, 900., 0.];
        drone.face(180.);
        let ai = drone.autonomy.as_mut().unwrap();
        ai.activity = resonance_events::Activity::Walk;
        ai.initialized = true;
        ai.remaining = 500;
        let player = field.events.world.controlled_actor;
        let actor = field.actor_mut(player);
        actor.position = [x, 1300., 0.];
        actor.face(0.);
        field.step(FieldInput {
            pressed_buttons: [Button::Ring].into(),
            ..Default::default()
        })?;
        field.ticks(60, FieldInput::default())?;
        let enemy = field.actor(id).enemy.as_ref().unwrap();
        assert!(enemy.pause_ticks > 0);
        assert_eq!(
            enemy.stun_effect(),
            Some(resonance_events::effect::StunEffect::Electric)
        );
        assert!(
            field
                .events
                .world
                .billboards
                .values()
                .any(|spark| spark.alpha(field.events.tick()) > 0.)
        );
        if id == 8001 {
            assert!(field.events.world.actors.contains_key(&401));
            assert!(!field.events.world.event_flags.contains(&154));
        }
    }
    assert!(
        field.events.world.event_flags.contains(&154),
        "both electrified panels must open the door"
    );
    let mut previous = [field.actor(8001).position, field.actor(8002).position];
    field.until(dialogue_input(), |field| {
        for (id, before) in [8001, 8002].into_iter().zip(&mut previous) {
            let actor = &field.events.world.actors[&id];
            let enemy = actor.enemy.as_ref().unwrap();
            let distance = (actor.position[0] - before[0]).hypot(actor.position[1] - before[1]);
            anyhow::ensure!(
                distance <= enemy.alert_speed.max(enemy.normal_speed) + 1.,
                "drone {id} jumped {distance} while opening the door"
            );
            *before = actor.position;
        }
        Ok(field.player_has_control() && !field.events.world.actors.contains_key(&200))
    })?;
    assert!(
        field
            .events
            .world
            .triggers
            .iter()
            .any(|trigger| trigger.key == 7010)
    );
    let player = field.events.world.controlled_actor;
    field.actor_mut(player).position = [-1000., 1500., 0.];
    for tick in 0..600 {
        field.step(FieldInput {
            pressed_buttons: field
                .walking([-1., 0.])
                .pressed_buttons
                .with(Button::Accept, tick % 2 == 0),
            held_buttons: field
                .walking([-1., 0.])
                .held_buttons
                .with(Button::Accept, true),
            ..field.walking([-1., 0.])
        })?;
        if field.events.world.field_transition.is_some() {
            break;
        }
    }
    assert_eq!(
        field
            .events
            .world
            .field_transition
            .as_ref()
            .context("walk through the opened panel door")?
            .map,
        269
    );
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn asgard_guard_alarm_starts_battle_and_resumes_the_room() -> Result<()> {
    let mut field = configured(ASGARD_RANCH, 210, |entry| {
        entry
            .persistent
            .memory
            .write(0xe0, symphonia_script::Width::S32, 3110)?;
        Ok(())
    })?;
    assert_eq!(
        field.replay(|f| Ok(f.player_has_control() && mission(f, 0xe0) == 3200))?,
        1
    );
    assert!(!field.events.world.actors.contains_key(&300));
    assert!(!field.events.world.actors.contains_key(&200));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn asgard_kvar_preparation_can_restore_a_party_with_empty_slots() -> Result<()> {
    let mut field = enter(ASGARD_RANCH, 213, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.trigger(6001, false)?);
    field.advance_until(|field| field.events.world.field_transition.is_some())?;
    field.follow_transition(&assets_root()?)?;
    assert_eq!(field.map_id, 214);
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.trigger(6001, true)?);
    field.advance_until(|field| field.events.world.field_transition.is_some())?;
    assert_eq!(mission(&field, 0xe0), 3100);
    assert_eq!(
        field.events.world.field_transition.as_ref().unwrap().map,
        211
    );
    assert!(!field.party().formation.is_empty());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn rheaird_rider_finishes_his_seated_pose_blend_while_attached() -> Result<()> {
    let mut field = enter(BASE_GENERATOR, 278, Some(2_404_000))?;
    field.events.world.event_flags.insert(190); // The generator is ready.
    field.advance_until(|field| {
        field.events.world.actors.get(&1).is_some_and(|actor| {
            actor
                .animation
                .as_ref()
                .is_some_and(|a| a.resource == 65582)
        })
    })?;
    field.ticks(60, FieldInput::default())?;
    let lloyd = &field.actor(1);
    assert_eq!(lloyd.position, [0.; 3]); // Local to the Rheaird's seat.
    assert_eq!(lloyd.attachment.as_ref().unwrap().actor, 210);
    assert!(
        !lloyd.animation_culled,
        "the visible rider was culled at his local origin"
    );
    assert_eq!(
        lloyd
            .animation
            .as_ref()
            .unwrap()
            .blend_weight(field.events.tick()),
        1.
    );
    field.actor_mut(210).position = [0., -1_000_000., 0.];
    field.step(FieldInput::default())?;
    assert!(field.actor(1).animation_culled);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn rheaird_crash_runs_the_debris_effect_and_restores_control() -> Result<()> {
    // Field 416's arrival branch compares story against 0x24B288.
    let mut field = enter(BASE_GENERATOR, 416, Some(2_405_000))?;
    field.advance_until(|field| {
        field
            .events
            .world
            .billboards
            .values()
            .any(|e| e.recipe == 52)
    })?;
    field
        .advance_until(|field| field.player_has_control() && mission(field, 0x40) == 10_001_000)?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Mana lamps and Thoda bridge; no devices"]
fn scenery_materials_animate_on_the_visible_objects() -> Result<()> {
    use resonance_content::field::{FieldAssets, SCENERY_RESOURCE_BASE, TextureMotion};
    let root = assets_root()?;
    for (destination, map, target, mission) in [
        (MANA_START, 362, 999998, None),
        (THODA_START, 6, 6000, Some((0xc4, 13_000))),
    ] {
        let assets: FieldAssets =
            serde_json::from_slice(&std::fs::read(root.join(format!("fields/map-{map}.json")))?)?;
        let mut field = configured(destination, map, |entry| {
            if let Some((address, value)) = mission {
                entry
                    .persistent
                    .memory
                    .write(address, symphonia_script::Width::S32, value)?;
            }
            Ok(())
        })?;
        field.advance_until(|field| {
            field.events.world.texture_animation_enabled
                && field.events.world.actors.contains_key(&target)
        })?;
        let world = &field.events.world;
        let actor = &world.actors[&target];
        assert!(actor.visible);
        let track = assets
            .texture_animations
            .iter()
            .find(|track| track.actor.resolve(&world.texture_bindings) == target)
            .context("visible object has no texture animation")?;
        let texture = track.texture.resolve(&world.texture_bindings);
        let parts = assets
            .parts
            .iter()
            .filter(|part| u32::from(part.resource) + SCENERY_RESOURCE_BASE == actor.resource)
            .chain(
                assets
                    .actors
                    .iter()
                    .filter(|model| model.resource == actor.resource)
                    .flat_map(|model| &model.parts),
            );
        assert!(
            parts
                .flat_map(|part| &part.materials)
                .flat_map(|material| [&material.color, &material.multiply])
                .flatten()
                .any(|binding| binding.texture as i32 == texture),
            "animation misses its material"
        );
        let offset = |field: &FieldSession| {
            track.offset(
                field.events.world.texture_animation_tick,
                field.events.world.texture_animation_effect_tick,
            )
        };
        let first = offset(&field);
        let period = match track.motion {
            TextureMotion::Atlas {
                frames, interval, ..
            } => Some(frames * interval),
            _ => None,
        };
        let mut changed = false;
        for _ in 0..period.unwrap_or(60) {
            field.step(FieldInput::default())?;
            changed |= offset(&field) != first;
        }
        assert!(changed, "visible material did not animate in map {map}");
        if period.is_some() {
            assert_eq!(offset(&field), first, "atlas did not repeat");
        }
    }
    Ok(())
}

fn mana_intro() -> Result<Scene> {
    let mut field = enter(MANA_START, 362, None)?;
    field.advance_until(|field| field.events.world.field_transition.is_some())?;
    assert_eq!(mission(&field, 0xcc), 11_000);
    field.follow_transition(&assets_root()?)?;
    assert_eq!(field.map_id, 364);
    field.replay_to_control(&assets_root()?)?;
    Ok(field)
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn mana_reunion_restores_both_groups_with_empty_slots() -> Result<()> {
    // Preserve the real intro's split-party flags, then grant solved puzzles
    // to isolate the reported reunion event rather than replay every block.
    let intro = mana_intro()?;
    let mut persistent = intro.events.persistent_state()?;
    persistent
        .memory
        .write(0xcc, symphonia_script::Width::S32, 13_500)?;
    let root = PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let package = new_game::FieldPackage::prepare(&root, 366, &mut Default::default(), || false)?;
    let mut field = Scene::enter(
        &package,
        resonance_game::field::FieldEntry {
            persistent,
            data: Some(Arc::new(package.files.json("game/session-data.json")?)),
            available_fields: new_game::available_fields(&root)?,
            position: [-400., 2600., 913.],
            ..Default::default()
        },
    )?;
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.trigger(1101, false)?);
    field.advance_until(|field| field.player_has_control() && mission(field, 0xcc) == 13_600)?;
    let party = field.party();
    for id in [1, 2, 3, 4, 9] {
        assert!(
            party.formation.contains(&id),
            "missing reunited member {id}"
        );
    }
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn palmacosta_teleport_lands_before_the_arrival_fade_reveals_the_player() -> Result<()> {
    let mut field = configured(PALMACOSTA_RANCH, 206, |entry| {
        // The south teleporter places Lloyd 82 units above the room floor.
        entry.position = [0., -722., 82.];
        entry.heading = 180.;
        entry
            .persistent
            .memory
            .write(0x12c, symphonia_script::Width::S32, 1)?;

        Ok(())
    })?;
    let mut visible = false;
    for _ in 0..300 {
        field.step(FieldInput::default())?;
        let world = &field.events.world;
        if world
            .fade
            .as_ref()
            .is_some_and(|fade| fade.alpha(world.tick) < 255.)
        {
            visible = true;
            assert!(
                world.actors[&world.controlled_actor].position[2].abs() < 0.01,
                "teleport arrival is still airborne when visible: {:?}",
                world.actors[&world.controlled_actor].position
            );
        }
    }
    assert!(visible && field.player_has_control());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn palmacosta_post_boss_exit_runs_the_ranch_destruction() -> Result<()> {
    let mut field = configured(PALMACOSTA_RANCH, 198, |entry| {
        // The defeated Magnius scene writes 6100 before its ChangeField to 198.
        // Keep the selector's story stage so this catches its skipped cutscene.
        entry
            .persistent
            .memory
            .write(0xb8, symphonia_script::Width::S32, 6100)?;
        entry.position = [-751., -5721., -51.];

        Ok(())
    })?;
    let mut visited = vec![field.map_id];
    let root = assets_root()?;
    field.replay(|scene| {
        if scene.events.world.field_transition.is_some() {
            scene.follow_transition(&root)?;
            visited.push(scene.map_id);
        }
        Ok(scene.events.world.world_transition.is_some())
    })?;
    assert_eq!(visited, [198, 207, 198]);
    assert_eq!(mission(&field, 0xb8), 8000);
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn iselia_enemies_cannot_enter_the_doorway_floor_region() -> Result<()> {
    use resonance_events::{Autonomy, Behavior};
    for (map, start_y, target_y, boundary) in
        [(194, 625., 900., 675.5), (196, 1950., 2280., 2099.74)]
    {
        let mut field = enter(ISELIA_RANCH, map, None)?;
        field.advance_until(FieldSession::player_has_control)?;
        let id = *field
            .events
            .world
            .actors
            .iter()
            .find(|(_, actor)| actor.enemy.is_some())
            .context("Iselia field enemy")?
            .0;
        let start = [224., start_y, 0.];
        let target = [224., target_y, 0.];
        let configure = |actor: &mut resonance_events::Actor| {
            // Probe floor access independently of body contact. Another guard
            // occupies this path in map 196 and now correctly blocks the probe.
            actor.collidable = false;
            actor.position = start;
            actor.face(180.);
            actor.motion = None;
            actor.autonomy = Some(Autonomy::new(Behavior::FollowPath, 4., start));
            actor.path.count = 1;
            actor.path.next = 0;
            actor.path.points[0] = target;
        };
        configure(field.actor_mut(id));
        field.ticks(150, FieldInput::default())?;
        let stopped = field.actor(id).position;
        assert!(
            stopped[1] > start[1] && stopped[1] < boundary,
            "map {map}: enemy crossed the authored doorway boundary: {stopped:?}"
        );
        let actor = field.actor_mut(id);
        actor.enemy = None;
        configure(actor);
        field.ticks(150, FieldInput::default())?;
        assert!(
            field.actor(id).position[1] > target_y - 10.,
            "ordinary actors must retain access to the doorway"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn iselia_damage_spheres_keep_moving() -> Result<()> {
    for (map, first, count) in [(194, 3021, 8), (196, 3301, 13)] {
        let mut field = enter(ISELIA_RANCH, map, None)?;
        field.advance_until(FieldSession::player_has_control)?;
        // Let reverse clips cross zero before measuring: they previously moved
        // once, then froze together at the first sample for the rest of play.
        field.ticks(1200, FieldInput::default())?;
        let mut bounds = vec![([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]); count];
        for _ in 0..1200 {
            field.step(FieldInput::default())?;
            for (i, (min, max)) in bounds.iter_mut().enumerate() {
                let actor = field.actor(first + i as i32);
                for axis in 0..2 {
                    min[axis] = min[axis].min(actor.position[axis]);
                    max[axis] = max[axis].max(actor.position[axis]);
                }
            }
        }
        for (i, (min, max)) in bounds.into_iter().enumerate() {
            assert!(
                (max[0] - min[0]).max(max[1] - min[1]) > 200.,
                "map {map} sphere {} stopped moving: {min:?}..{max:?}",
                first + i as i32
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn iselia_chest_notice_clears_a_declined_elevator_choice() -> Result<()> {
    let mut field = enter(ISELIA_RANCH, 194, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    let player = field.events.world.controlled_actor;
    // Controlled approaches isolate the native choice and treasure service without
    // claiming the intervening route through this multi-level room.
    let actor = field.actor_mut(player);
    actor.position = [1600., -500., 300.];
    actor.face(0.);
    actor.motion = None;
    assert!(field.events.trigger(6100, false)?);
    field.advance_until(|field| {
        field
            .events
            .world
            .choices
            .get(&0)
            .is_some_and(|c| c.operation.is_pending())
    })?;
    field.ticks(180, FieldInput::default())?;
    assert!(field.events.world.choices[&0].operation.is_pending());
    field.step(FieldInput {
        direction: [0., -1.],
        ..Default::default()
    })?;
    field.step(FieldInput::default())?;
    field.step(FieldInput {
        pressed_buttons: [Button::Accept].into(),
        ..Default::default()
    })?;
    field.advance_until(FieldSession::player_has_control)?;
    assert_eq!(field.events.world.choices[&0].selected_line, 1);
    assert!(!field.events.world.choices[&0].operation.is_pending());

    const REWARD: u16 = 390;
    let count = field.party().items.get(&REWARD).copied().unwrap_or(0);
    let actor = field.actor_mut(player);
    actor.position = [364., -852., 0.];
    actor.face(180.);
    actor.motion = None;
    field.step(FieldInput {
        pressed_buttons: [Button::Accept].into(),
        ..Default::default()
    })?;
    field.ticks(180, FieldInput::default())?;
    assert!(field.events.world.dialogue[&0].operation.is_pending());
    assert!(
        field.events.world.choices.is_empty(),
        "choice cursor leaked into the chest notice"
    );
    assert_eq!(field.party().items[&REWARD], count + 1);
    field.step(FieldInput {
        pressed_buttons: [Button::Accept].into(),
        ..Default::default()
    })?;
    field.ticks(30, FieldInput::default())?;
    assert!(field.player_has_control());
    assert!(field.party().travel.opened_treasures.contains(&157));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn iselia_exit_restores_the_party_and_finishes_the_scene() -> Result<()> {
    let mut field = enter(ISELIA_RANCH, 193, Some(20_307_000))?;
    field.replay(|f| Ok(f.events.world.field_transition.is_some()))?;
    assert_eq!(field.story_progress()?, 20_308_000);
    let party = field.party();
    assert_eq!(party.formation.len(), 8);
    assert!(party.formation.iter().all(|id| *id != 0));
    assert_eq!(party.field_leader, 1);
    field.follow_transition(&assets_root()?)?;
    let mut saw_movie = false;
    field.replay(|f| {
        saw_movie |= f.events.world.movie.is_some();
        Ok(saw_movie && (f.player_has_control() || f.events.world.field_transition.is_some()))
    })?;
    assert!(saw_movie);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_mimic_blocks_walking_like_an_ordinary_chest() -> Result<()> {
    let mut field = enter(TRIET_START, 219, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    // This chest is a scripted model actor, so its animation drives the lid.
    let chest = field.actor(8300).clone();
    assert_eq!(chest.position, [-4126., 1764., 15.]);
    assert!(chest.collidable);
    let player = field.events.world.controlled_actor;
    let radius = chest.radius + field.actor(player).radius;
    let start = [
        chest.position[0] + 160.,
        chest.position[1],
        chest.position[2],
    ];
    field.events.world.field_camera = None;
    for enabled in [true, false] {
        field.actor_mut(8300).collidable = enabled;
        field.actor_mut(player).position = start;
        field.ticks(
            80,
            FieldInput {
                direction: [-1., 0.],
                ..Default::default()
            },
        )?;
        let position = field.actor(player).position;
        if enabled {
            assert!(position[0] >= chest.position[0] + radius, "{position:?}");
            assert!(position[0] < start[0], "player never approached the chest");
        } else {
            assert!(position[0] < chest.position[0], "{position:?}");
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_memory_circle_replaces_sealed_geometry_when_unlocked() -> Result<()> {
    let mut field = enter(TRIET_START, 220, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    let point = &field.events.world.save_points[0];
    let id = point.actor;
    let flag = point.unlock_flag.context("sealed circle flag")?;
    let check = |field: &FieldSession, hidden: &str| {
        let actor = &field.events.world.actors[&id];
        let names = &field
            .events
            .resources()
            .model(actor.resource)
            .unwrap()
            .names;
        for (node, name) in names.iter().enumerate() {
            assert_eq!(
                actor.appearance.hidden_nodes.contains(&(node as u16)),
                name.starts_with(hidden),
                "{name} visibility with {hidden} hidden"
            );
        }
    };
    check(&field, "LIVE_");
    assert_eq!(field.actor(id).animation.as_ref().unwrap().rate, 0.);
    field.events.world.event_flags.insert(flag);
    field.step(FieldInput::default())?;
    check(&field, "HID_");
    assert!(field.actor(id).animation.as_ref().unwrap().rate > 0.);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn balacruf_light_column_stops_before_remiel_descends() -> Result<()> {
    let mut field = enter(BALACRUF_START, 510, None)?;
    let (mut saw_column, mut saw_remiel) = (false, false);
    field.replay(|field| {
        let world = &field.events.world;
        let column = world.model_particles.values().any(|p| p.resource == 68604);
        saw_column |= column;
        saw_remiel |= world
            .actors
            .get(&1000)
            .is_some_and(|actor| actor.visible && actor.position[2] < 1300.);
        if saw_remiel {
            assert!(!column, "Balacruf light column outlived its event");
        }
        Ok(saw_remiel && field.player_has_control())
    })?;
    assert!(saw_column);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn thoda_ring_shots_light_both_torches_and_fill_the_upper_cup() -> Result<()> {
    use resonance_events::ring::SorcerersRing;
    let mut field = enter(THODA_START, 9, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    for (target, flag, ring) in [
        (102, 150, SorcerersRing::Fire),
        (103, 151, SorcerersRing::Fire),
        (100, 152, SorcerersRing::Water),
    ] {
        assert!(!field.events.world.event_flags.contains(&flag));
        let center = field.actor(target).position;
        field.party_mut().travel.sorcerers_ring = ring;
        let player = field.events.world.controlled_actor;
        let actor = field.actor_mut(player);
        if target == 100 {
            actor.position = [center[0], center[1] - 300., 0.];
            // Approach from the front, through the cup's invisible talk marker.
            // A side-only shot misses that marker and cannot catch interception.
            field.ticks(30, field.walking([0., 1.]))?;
        } else {
            actor.position = [center[0] + 150., center[1], 0.];
            actor.face(270.);
        }
        field.step(FieldInput {
            pressed_buttons: [Button::Ring].into(),
            ..Default::default()
        })?;
        for _ in 0..600 {
            field.step(FieldInput::default())?;
            if field.events.world.event_flags.contains(&flag) && field.player_has_control() {
                break;
            }
        }
        assert!(
            field.events.world.event_flags.contains(&flag) && field.player_has_control(),
            "ring {ring:?} did not activate Thoda target {target}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Iselia fields; no devices"]
fn iselia_first_tutorial_reaches_its_battle_and_returns_control() -> Result<()> {
    let mut field = enter(MARTEL_START, 332, Some(2500))?;
    field.party_mut().formation = vec![1, 2, 3];
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.trigger(2002, false)?);
    // The field requests two tutorial encounters. Supply victory explicitly;
    // this checks the field continuation, not an implemented combat runner.
    for formation in [1, 2] {
        field.advance_until(|field| field.events.world.battle_request.is_some())?;
        let request = field.events.world.battle_request.as_ref().unwrap();
        assert_eq!(
            request.setup.encounter,
            resonance_events::battle::Encounter::Formation(formation)
        );
        skip_battle(&mut field)?;
    }
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Martel fields; no devices"]
fn martel_selector_starts_before_the_golem_scene_and_ring_pickup() -> Result<()> {
    let mut field = enter(MARTEL_START, 308, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    assert_eq!(field.story_progress()?, 104_000);
    let party = field.party();
    assert!(!party.items.contains_key(&resonance_events::ring::ITEM));
    assert_eq!(
        party.travel.sorcerers_ring,
        resonance_events::ring::SorcerersRing::Disabled
    );
    let ring = field
        .events
        .world
        .actors
        .get(&106)
        .context("ring on the altar")?;
    assert!(ring.visible && !ring.appearance.model_hidden);
    assert!(field.events.trigger(1014, false)?);
    field.advance_until(|field| field.events.world.battle_request.is_some())?;
    skip_battle(&mut field)?;
    field.advance_until(FieldSession::player_has_control)?;
    assert_eq!(field.story_progress()?, 105_000);
    assert!(field.events.world.event_flags.contains(&207));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Mana bridge assets; no devices"]
fn mana_enemies_respect_closed_bridge_barriers() -> Result<()> {
    let mut field = configured(MANA_START, 366, |entry| {
        entry
            .persistent
            .memory
            .write(0xcc, symphonia_script::Width::S32, 12_000)?;
        entry.position = [40., 0., 7.];
        Ok(())
    })?;
    field.advance_until(FieldSession::player_has_control)?;
    let enemy = *field
        .events
        .world
        .actors
        .iter()
        .find(|(_, a)| a.enemy.is_some())
        .context("bridge-room enemy")?
        .0;
    let start = [650., 605., 309.2];
    let target = [350., 605., 309.2];
    for open in [false, true] {
        if open {
            field.events.world.actors.remove(&800);
        }
        let actor = field.actor_mut(enemy);
        actor.position = start;
        actor.autonomy = None;
        actor.collidable = false;
        actor.motion = Some(resonance_events::ActorMotion { target, speed: 4. });
        field.ticks(100, FieldInput::default())?;
        let x = field.actor(enemy).position[0];
        if open {
            assert!(
                (x - target[0]).abs() < 1.,
                "open bridge stopped enemy at {x}"
            );
        } else {
            assert!(
                x < start[0] && x >= 595.,
                "closed barrier let enemy through to {x}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Triet town; no devices"]
fn triet_genis_returns_to_idle_after_getup() -> Result<()> {
    let mut field = configured(TRIET_START, 485, |entry| {
        entry
            .persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 1_306_000)?;
        entry.persistent.event_flags.insert(1749);
        Ok(())
    })?;
    let (mut getup, mut released) = (false, false);
    field.replay(|f| {
        if let Some(actor) = f.events.world.actors.get(&3) {
            getup |= actor
                .animation
                .as_ref()
                .is_some_and(|a| a.resource == 66057);
            released |= getup
                && !actor.scripted_animation
                && actor
                    .animation
                    .as_ref()
                    .is_some_and(|a| a.resource == 3 && a.blend_ticks > 0);
        }
        Ok(released && f.player_has_control())
    })?;
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn story_scenes_reach_their_next_stage() -> Result<()> {
    for (map, before, after, trigger) in [
        (347, 1_305_000, 1_402_000, Some(3003)),
        (276, 1_107_000, 1_108_000, None),
        (276, 2_401_000, 2_402_000, None),
        (535, 2_302_000, 2_303_000, None),
        (376, 10_101_000, 10_101_500, Some(1005)),
    ] {
        let mut field = configured(TRIET_START, map, |entry| {
            entry
                .persistent
                .memory
                .write(0x40, symphonia_script::Width::S32, before)?;
            entry.position = [0.; 3];
            Ok(())
        })?;
        if let Some(trigger) = trigger {
            field.advance_until(FieldSession::player_has_control)?;
            assert!(field.events.trigger(trigger, false)?);
        }
        field.until(FieldInput::default(), |f| {
            Ok(!f.player_has_control() || f.events.world.blocked_by_movie())
        })?;
        for tick in 0..SCENE_TIMEOUT {
            let choosing = field
                .events
                .world
                .choices
                .values()
                .any(|c| c.operation.is_pending());
            let done = if choosing {
                // Skipping leaves choices to the player; this route takes the first option.
                field.step(FieldInput {
                    pressed_buttons: dialogue_input()
                        .pressed_buttons
                        .with(Button::Accept, tick % 2 == 0),
                    ..dialogue_input()
                })?;
                false
            } else {
                field.skip_event_step()?
            };
            if done || field.events.world.field_transition.is_some() {
                break;
            }
            anyhow::ensure!(
                tick + 1 < SCENE_TIMEOUT,
                "scene {map}: cutscene skip timed out"
            );
        }
        assert!(
            field.story_progress()? >= after,
            "scene {map}: skipping stopped at story {:?}, choices={:?}, transition={:?}",
            field.story_progress(),
            field.events.world.choices,
            field.events.world.field_transition
        );
        if map == 276 && before == 1_107_000 {
            let party = field.events.world.party.as_ref().unwrap();
            assert_eq!(
                (party.members[1].ex_gems[0], party.members[1].ex_skills[0]),
                (1, 1)
            );
            assert_eq!(
                (party.members[8].ex_gems[0], party.members[8].ex_skills[0]),
                (1, 5)
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn first_fire_bridge_returns_control() -> Result<()> {
    let mut field = enter(TRIET_START, 220, Some(1_302_000))?;
    field.advance_until(FieldSession::player_has_control)?;
    let position = field.actor(5020).position;
    field.actor_mut(1).position = [position[0], position[1] + 250., position[2] - 70.];
    field.actor_mut(1).face(0.);
    field.step(FieldInput {
        pressed_buttons: [Button::Ring].into(),
        ..Default::default()
    })?;
    field.until(dialogue_input(), |f| {
        Ok(f.events.world.event_flags.contains(&232) && f.player_has_control())
    })?;
    assert!(field.player_has_control());
    assert_eq!(
        field
            .events
            .world
            .fade
            .as_ref()
            .map_or(0., |f| f.alpha(field.events.tick())),
        0.
    );
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn post_base_triet_scene() -> Result<()> {
    let mut field = enter(TRIET_START, 527, Some(1_202_000))?;
    field.advance_until(FieldSession::player_has_control)?;
    assert_eq!(field.story_progress()?, 1_203_000);
    Ok(())
}

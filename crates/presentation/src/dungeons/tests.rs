//! Device-free scene regressions using fresh progression fixtures.
#[path = "crafting_tests.rs"]
mod crafting;
#[path = "fog_tests.rs"]
mod fog;
#[path = "iselia_tests.rs"]
mod iselia;
#[path = "movement_tests.rs"]
mod movement;
use super::{destinations::Fixture, new_game};
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
            for _ in 0..4 {
                until(&mut field, dialogue_input(), |f| {
                    skip_battle(f)?;
                    if let Some(movie) = &f.events.world.movie {
                        movie.operation.complete(None).map_err(anyhow::Error::msg)?;
                    }
                    Ok(f.player_has_control() || f.events.world.field_transition.is_some())
                })?;
                anyhow::ensure!(
                    field.events.exploration_error.is_none(),
                    "{:?}",
                    field.events.exploration_error
                );
                if field.events.world.field_transition.is_some() {
                    field = follow_transition(&field)?;
                } else {
                    anyhow::ensure!(
                        field
                            .events
                            .world
                            .fade
                            .as_ref()
                            .is_none_or(|fade| fade.alpha(field.events.tick()) == 0.),
                        "checkpoint returned control behind a fade"
                    );
                    return Ok(());
                }
            }
            anyhow::bail!("checkpoint never returned control")
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
    for (map, portal, final_room) in [
        (350, [150., 0.], 350),
        (307, [0., 2775.], 309),
        (220, [-2131., -154.], 221),
        (9, [-2555., -611.], 10),
        (509, [11., 2000.], 510),
        (366, [-852., 2321.], 369),
        (275, [-1000., 950.], 276),
        (206, [0., 875.], 207),
        (214, [0., 1060.], 217),
        (196, [2065., 6100.], 197),
        (227, [600., -2.], 232),
        (149, [0., 4860.], 535),
    ] {
        let destination = super::DESTINATIONS
            .iter()
            .find(|d| d.map == map && (d.name.contains("BEFORE") || d.name.ends_with("END")))
            .unwrap();
        let package = new_game::FieldPackage::prepare(&root, map, &mut cache, || false)?;
        let entry = destination.entry(
            Arc::new(package.files.json("game/session-data.json")?),
            new_game::available_fields(&root)?,
        )?;
        let mut field = Scene::enter(&package, entry)?;
        advance_until(&mut field, FieldSession::player_has_control)?;
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
            let camera = field.events.world.field_camera.as_ref().unwrap();
            let angle = -(camera.target[0] - camera.position[0])
                .atan2(camera.target[1] - camera.position[1]);
            field.step(FieldInput {
                alternate: map == 206 && tick == 0,
                direction: [
                    (angle.cos() * delta[0] + angle.sin() * delta[1]) / length,
                    (-angle.sin() * delta[0] + angle.cos() * delta[1]) / length,
                ],
                interact: tick % 2 == 0,
                accelerate_dialogue: true,
                ..Default::default()
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
            let mut boss = follow_transition(&field)?;
            until(&mut boss, dialogue_input(), |f| {
                if let Some(movie) = &f.events.world.movie {
                    movie.operation.complete(None).map_err(anyhow::Error::msg)?;
                }
                Ok(f.events.world.battle_request.is_some())
            })
            .with_context(|| format!("{} final encounter", destination.name))?;
            skip_battle(&mut boss)?;
            advance_until(&mut boss, FieldSession::player_has_control)
                .with_context(|| format!("{} seal scene", destination.name))?;
            let (flag, angel) = match final_room {
                221 => (200, 1),
                10 => (201, 1),
                510 => (202, 101),
                369 => (203, 201),
                _ => unreachable!(),
            };
            assert!(boss.events.world.event_flags.contains(&flag));
            assert_eq!(
                boss.events
                    .memory()
                    .read(0x4c, symphonia_script::Width::S32)?,
                angel
            );
        }
    }
    Ok(())
}

struct Scene {
    field: FieldSession,
    audio: crate::field_audio::validation::Playback,
}

impl std::ops::Deref for Scene {
    type Target = FieldSession;
    fn deref(&self) -> &FieldSession {
        &self.field
    }
}
impl std::ops::DerefMut for Scene {
    fn deref_mut(&mut self) -> &mut FieldSession {
        &mut self.field
    }
}
impl Scene {
    fn actor(&self, id: i32) -> &resonance_events::Actor {
        &self.events.world.actors[&id]
    }
    fn actor_mut(&mut self, id: i32) -> &mut resonance_events::Actor {
        self.events.world.actors.get_mut(&id).unwrap()
    }
    fn party(&self) -> &resonance_events::party::Party {
        self.events.world.party.as_ref().unwrap()
    }
    fn party_mut(&mut self) -> &mut resonance_events::party::Party {
        self.events.world.party.as_mut().unwrap()
    }
    fn step(&mut self, input: FieldInput) -> Result<()> {
        self.field.step(input)?;
        self.audio.step(&mut self.field)
    }
    fn enter(
        package: &new_game::FieldPackage,
        entry: resonance_game::field::FieldEntry,
    ) -> Result<Self> {
        let mut field = package.enter(entry)?;
        let audio =
            crate::field_audio::validation::Playback::new((*package.audio).clone(), &mut field);
        Ok(Self { field, audio })
    }
}
const SCENE_TIMEOUT: usize = 18_000;
fn until(
    field: &mut Scene,
    input: FieldInput,
    mut ready: impl FnMut(&mut FieldSession) -> Result<bool>,
) -> Result<()> {
    for tick in 0..SCENE_TIMEOUT {
        field.step(FieldInput {
            interact: input.interact && tick % 2 == 0,
            ..input
        })?;
        if ready(field)? {
            return Ok(());
        }
    }
    anyhow::bail!(
        "field {} timed out; control={}, waits={:?}",
        field.map_id,
        field.player_has_control(),
        field.events.pending_operations()
    )
}
fn dialogue_input() -> FieldInput {
    FieldInput {
        interact: true,
        accelerate_dialogue: true,
        ..Default::default()
    }
}

fn assets_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    ))
}
fn configured(
    destination: Fixture,
    map: u32,
    configure: impl FnOnce(&mut resonance_game::field::FieldEntry) -> Result<()>,
) -> Result<Scene> {
    let root = assets_root()?;
    let package = new_game::FieldPackage::prepare(&root, map, &mut Default::default(), || false)?;
    let data = Arc::new(package.files.json("game/session-data.json")?);
    let mut entry = destination
        .destination()
        .entry(data, new_game::available_fields(&root)?)?;
    configure(&mut entry)?;
    Scene::enter(&package, entry)
}
fn enter(destination: Fixture, map: u32, story: Option<i32>) -> Result<Scene> {
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
fn ticks(field: &mut Scene, count: usize, input: FieldInput) -> Result<()> {
    for _ in 0..count {
        field.step(input)?;
    }
    Ok(())
}
fn skip_battle(field: &mut FieldSession) -> Result<bool> {
    field
        .events
        .world
        .skip_battle_as_victory()
        .map_err(anyhow::Error::msg)
}
/// Run a scene with silent audio, simulated battle victories, and movie completion.
fn replay(field: &mut Scene, mut finished: impl FnMut(&FieldSession) -> bool) -> Result<usize> {
    let mut battles = 0;
    until(field, dialogue_input(), |field| {
        battles += usize::from(skip_battle(field)?);
        if let Some(movie) = &field.events.world.movie
            && movie.operation.is_pending()
        {
            movie.operation.complete(None).map_err(anyhow::Error::msg)?;
        }
        Ok(finished(field))
    })?;
    field.audio.finish()?;
    Ok(battles)
}

fn follow_transition(field: &FieldSession) -> Result<Scene> {
    let root = PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let request = field
        .events
        .world
        .field_transition
        .as_ref()
        .context("field transition")?;
    let package =
        new_game::FieldPackage::prepare(&root, request.map, &mut Default::default(), || false)?;
    Scene::enter(
        &package,
        resonance_game::field::FieldEntry {
            persistent: field.events.persistent_state()?,
            data: Some(Arc::new(package.files.json("game/session-data.json")?)),
            available_fields: new_game::available_fields(&root)?,
            position: request.position,
            heading: request.heading,
            camera: request.camera.clone(),
            ..Default::default()
        },
    )
}

fn advance_until(field: &mut Scene, mut ready: impl FnMut(&FieldSession) -> bool) -> Result<()> {
    until(field, dialogue_input(), |field| Ok(ready(field)))
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn martel_golem_battle_creates_a_pushable_block() -> Result<()> {
    let mut field = enter(Fixture::Martel, 308, Some(107_000))?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    let golem = *field
        .events
        .world
        .actors
        .iter()
        .find(|(_, actor)| actor.enemy.as_ref().is_some_and(|enemy| enemy.event == 101))
        .context("golem contact event")?
        .0;
    assert!(field.events.contact_enemy(golem)?);
    advance_until(&mut field, |field| {
        field.events.world.battle_request.is_some()
    })?;
    assert!(skip_battle(&mut field)?);
    advance_until(&mut field, FieldSession::player_has_control)?;
    let block = field
        .events
        .world
        .actors
        .get(&5000)
        .context("defeated golem block")?;
    assert!(block.pushable());
    assert!(block.model_collision.is_some());
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn guard_entrance_event_can_pause_the_guards() -> Result<()> {
    let mut field = enter(Fixture::GuardEntrance, 267, Some(1_105_000))?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    // Controlled approach to the exit, then ordinary movement through its
    // opening door and transition. This does not replay the cell escape.
    let player = field.events.world.controlled_actor;
    let actor = field.actor_mut(player);
    actor.position = [2252., 1000., 0.];
    actor.face(180.);
    until(
        &mut field,
        FieldInput {
            direction: [0., 1.],
            ..Default::default()
        },
        |field| Ok(field.events.world.field_transition.is_some()),
    )?;
    let mut field = follow_transition(&field)?;
    assert_eq!(field.map_id, 268);
    advance_until(&mut field, |field| {
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
    advance_until(&mut field, |field| {
        field.player_has_control() && mission(field, 0x40) == 1_105_100
    })?;
    let player = field.events.world.controlled_actor;
    let before = field.actor(player).position;
    for _ in 0..10 {
        field.step(FieldInput {
            direction: [0., -1.],
            ..Default::default()
        })?;
    }
    let after = field.actor(player).position;
    assert!(
        (after[0] - before[0]).hypot(after[1] - before[1]) > 20.,
        "player must be able to walk after the guards' scene"
    );
    until(&mut field, FieldInput::default(), |field| {
        Ok(field.events.world.battle_request.is_some())
    })?;
    // The resumed patrol can reach Lloyd. The dungeon selector grants this
    // encounter as a victory, after which exploration must remain usable.
    assert!(skip_battle(&mut field)?);
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn sylvarant_electrified_drones_open_the_panel_door() -> Result<()> {
    use resonance_events::ring::{ElectricOrbKind, SorcerersRing};
    let mut field = enter(Fixture::GuardEntrance, 268, Some(1_105_100))?;
    advance_until(&mut field, FieldSession::player_has_control)?;
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
            alternate: true,
            ..Default::default()
        })?;
        for _ in 0..60 {
            field.step(FieldInput::default())?;
        }
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
    until(&mut field, dialogue_input(), |field| {
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
        let camera = field.events.world.field_camera.as_ref().unwrap();
        let angle = (-(camera.target[0] - camera.position[0])
            .atan2(camera.target[1] - camera.position[1])
            .to_degrees())
        .trunc()
        .to_radians();
        field.step(FieldInput {
            direction: [-angle.cos(), angle.sin()],
            interact: tick % 2 == 0,
            accelerate_dialogue: true,
            ..Default::default()
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
    let mut field = configured(Fixture::Asgard, 210, |entry| {
        entry
            .persistent
            .memory
            .write(0xe0, symphonia_script::Width::S32, 3110)?;
        Ok(())
    })?;
    assert_eq!(
        replay(&mut field, |f| f.player_has_control()
            && mission(f, 0xe0) == 3200)?,
        1
    );
    assert!(!field.events.world.actors.contains_key(&300));
    assert!(!field.events.world.actors.contains_key(&200));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn asgard_kvar_preparation_can_restore_a_party_with_empty_slots() -> Result<()> {
    let mut field = enter(Fixture::Asgard, 213, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.trigger(6001, false)?);
    advance_until(&mut field, |field| {
        field.events.world.field_transition.is_some()
    })?;
    let mut field = follow_transition(&field)?;
    assert_eq!(field.map_id, 214);
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.trigger(6001, true)?);
    advance_until(&mut field, |field| {
        field.events.world.field_transition.is_some()
    })?;
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
    let mut field = enter(Fixture::Generator, 278, Some(2_404_000))?;
    field.events.world.event_flags.insert(190); // The generator is ready.
    advance_until(&mut field, |field| {
        field.events.world.actors.get(&1).is_some_and(|actor| {
            actor
                .animation
                .as_ref()
                .is_some_and(|a| a.resource == 65582)
        })
    })?;
    ticks(&mut field, 60, FieldInput::default())?;
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
    let mut field = enter(Fixture::Generator, 416, Some(2_405_000))?;
    advance_until(&mut field, |field| {
        field
            .events
            .world
            .billboards
            .values()
            .any(|e| e.recipe == 52)
    })?;
    advance_until(&mut field, |field| {
        field.player_has_control() && mission(field, 0x40) == 10_001_000
    })?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Mana scenery; no devices"]
fn mana_lamps_bind_the_animated_flame_texture() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let assets: resonance_content::field::FieldAssets =
        serde_json::from_slice(&std::fs::read(root.join("fields/map-362.json"))?)?;
    let mut field = enter(Fixture::Mana, 362, None)?;
    advance_until(&mut field, |field| {
        field.events.world.render_settings.get(&128) == Some(&1)
    })?;
    let [track] = assets.texture_animations.as_slice() else {
        anyhow::bail!("missing Mana flame atlas");
    };
    let world = &field.events.world;
    let actor = &world.actors[&track.actor.resolve(&world.render_settings)];
    assert_eq!(
        actor.resource,
        resonance_content::field::SCENERY_RESOURCE_BASE + 2
    );
    assert!(actor.visible);
    let texture = track.texture.resolve(&world.render_settings);
    let flames = assets.parts.iter().find(|part| part.resource == 2).unwrap();
    assert!(
        flames
            .materials
            .iter()
            .flat_map(|material| [&material.color, &material.multiply])
            .flatten()
            .any(|binding| binding.texture as i32 == texture)
    );
    let offset = |field: &FieldSession| {
        track.offset(
            field.events.world.texture_animation_tick,
            field.events.world.texture_animation_effect_tick,
        )
    };
    let first = offset(&field);
    ticks(&mut field, 7, FieldInput::default())?;
    assert_ne!(first, offset(&field));
    ticks(&mut field, 28, FieldInput::default())?;
    assert_eq!(first, offset(&field));
    Ok(())
}

fn mana_intro() -> Result<Scene> {
    let mut field = enter(Fixture::Mana, 362, None)?;
    advance_until(&mut field, |field| {
        field.events.world.field_transition.is_some()
    })?;
    assert_eq!(mission(&field, 0xcc), 11_000);
    let mut field = follow_transition(&field)?;
    assert_eq!(field.map_id, 364);
    for _ in 0..4 {
        advance_until(&mut field, |field| {
            field.player_has_control() || field.events.world.field_transition.is_some()
        })?;
        assert!(field.events.exploration_error.is_none());
        if field.player_has_control() {
            return Ok(field);
        }
        field = follow_transition(&field)?;
    }
    anyhow::bail!("Mana intro did not return control after its field transitions")
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
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.trigger(1101, false)?);
    advance_until(&mut field, |field| {
        field.player_has_control() && mission(field, 0xcc) == 13_600
    })?;
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
    let mut field = configured(Fixture::Palmacosta, 206, |entry| {
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
    let mut field = configured(Fixture::Palmacosta, 198, |entry| {
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
    for tick in 0..18_000 {
        field.step(FieldInput {
            interact: tick % 2 == 0,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        if field.events.world.field_transition.is_some() {
            field = follow_transition(&field)?;
            visited.push(field.map_id);
        }
        if field.events.world.world_transition.is_some() {
            assert_eq!(visited, [198, 207, 198]);
            assert_eq!(mission(&field, 0xb8), 8000);
            assert!(field.events.exploration_error.is_none());
            return Ok(());
        }
    }
    anyhow::bail!(
        "Palmacosta destruction did not finish; visited={visited:?}, mission={}, waits={:?}",
        mission(&field, 0xb8),
        field.events.pending_operations()
    )
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn iselia_enemies_cannot_enter_the_doorway_floor_region() -> Result<()> {
    use resonance_events::{Autonomy, Behavior};
    for (map, start_y, target_y, boundary) in
        [(194, 625., 900., 675.5), (196, 1950., 2280., 2099.74)]
    {
        let mut field = enter(Fixture::Iselia, map, None)?;
        advance_until(&mut field, FieldSession::player_has_control)?;
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
        for _ in 0..150 {
            field.step(FieldInput::default())?;
        }
        let stopped = field.actor(id).position;
        assert!(
            stopped[1] > start[1] && stopped[1] < boundary,
            "map {map}: enemy crossed the authored doorway boundary: {stopped:?}"
        );
        let actor = field.actor_mut(id);
        actor.enemy = None;
        configure(actor);
        for _ in 0..150 {
            field.step(FieldInput::default())?;
        }
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
        let mut field = enter(Fixture::Iselia, map, None)?;
        advance_until(&mut field, FieldSession::player_has_control)?;
        // Let reverse clips cross zero before measuring: they previously moved
        // once, then froze together at the first sample for the rest of play.
        for _ in 0..1200 {
            field.step(FieldInput::default())?;
        }
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
    let mut field = enter(Fixture::Iselia, 194, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    let player = field.events.world.controlled_actor;
    // Controlled approaches isolate the native choice and treasure service without
    // claiming the intervening route through this multi-level room.
    let actor = field.actor_mut(player);
    actor.position = [1600., -500., 300.];
    actor.face(0.);
    actor.motion = None;
    assert!(field.events.trigger(6100, false)?);
    advance_until(&mut field, |field| {
        field
            .events
            .world
            .choices
            .get(&0)
            .is_some_and(|c| c.operation.is_pending())
    })?;
    ticks(&mut field, 180, FieldInput::default())?;
    assert!(field.events.world.choices[&0].operation.is_pending());
    field.step(FieldInput {
        direction: [0., -1.],
        ..Default::default()
    })?;
    field.step(FieldInput::default())?;
    field.step(FieldInput {
        interact: true,
        ..Default::default()
    })?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert_eq!(field.events.world.choices[&0].selected_line, 1);
    assert!(!field.events.world.choices[&0].operation.is_pending());

    const REWARD: u16 = 390;
    let count = field.party().items.get(&REWARD).copied().unwrap_or(0);
    let actor = field.actor_mut(player);
    actor.position = [364., -852., 0.];
    actor.face(180.);
    actor.motion = None;
    field.step(FieldInput {
        interact: true,
        ..Default::default()
    })?;
    ticks(&mut field, 180, FieldInput::default())?;
    assert!(field.events.world.dialogue[&0].operation.is_pending());
    assert!(
        field.events.world.choices.is_empty(),
        "choice cursor leaked into the chest notice"
    );
    assert_eq!(field.party().items[&REWARD], count + 1);
    field.step(FieldInput {
        interact: true,
        ..Default::default()
    })?;
    ticks(&mut field, 30, FieldInput::default())?;
    assert!(field.player_has_control());
    assert!(field.party().travel.opened_treasures.contains(&157));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn iselia_exit_restores_the_party_and_finishes_the_scene() -> Result<()> {
    let mut field = enter(Fixture::Iselia, 193, Some(20_307_000))?;
    replay(&mut field, |f| f.events.world.field_transition.is_some())?;
    assert_eq!(field.story_progress()?, 20_308_000);
    let party = field.party();
    assert_eq!(party.formation.len(), 8);
    assert!(party.formation.iter().all(|id| *id != 0));
    assert_eq!(party.field_leader, 1);
    let mut outside = follow_transition(&field)?;
    let mut saw_movie = false;
    replay(&mut outside, |f| {
        saw_movie |= f.events.world.movie.is_some();
        saw_movie && (f.player_has_control() || f.events.world.field_transition.is_some())
    })?;
    assert!(saw_movie);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_mimic_blocks_walking_like_an_ordinary_chest() -> Result<()> {
    let mut field = enter(Fixture::FireSeal, 219, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
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
        for _ in 0..80 {
            field.step(FieldInput {
                direction: [-1., 0.],
                ..Default::default()
            })?;
        }
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
    let mut field = enter(Fixture::FireSeal, 220, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
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
fn seal_scene_retains_particle_tails_after_removing_the_emitter() -> Result<()> {
    for (destination, map, emitter) in [
        (Fixture::FireSeal, 221, 2000),
        (Fixture::AirSeal, 510, 1023),
    ] {
        let mut field = enter(destination, map, None)?;
        advance_until(&mut field, |field| {
            field
                .events
                .world
                .billboards
                .values()
                .any(|p| p.owner == Some(emitter))
        })?;
        let mut previous = Vec::new();
        let mut survivors = Vec::new();
        advance_until(&mut field, |field| {
            let world = &field.events.world;
            if !world.actors.contains_key(&emitter) {
                survivors = previous
                    .iter()
                    .filter(|id| world.billboards.contains_key(id))
                    .copied()
                    .collect();
                true
            } else {
                previous = world
                    .billboards
                    .iter()
                    .filter_map(|(&id, p)| (p.owner == Some(emitter)).then_some(id))
                    .collect();
                false
            }
        })?;
        assert!(
            !survivors.is_empty(),
            "map {map}: scene removed its particle tails"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn balacruf_light_column_stops_before_remiel_descends() -> Result<()> {
    let mut field = enter(Fixture::AirSeal, 510, None)?;
    let (mut saw_column, mut saw_remiel) = (false, false);
    replay(&mut field, |field| {
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
        saw_remiel && field.player_has_control()
    })?;
    assert!(saw_column);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_seal_scripted_wings_animate_and_emit_sparks() -> Result<()> {
    let mut field = enter(Fixture::Wings, Fixture::Wings.destination().map, None)?;
    let mut saw_wings = false;
    let battles = replay(&mut field, |field| {
        let world = &field.events.world;
        saw_wings |= world
            .actors
            .get(&resonance_events::COLETTE_WINGS_ACTOR)
            .is_some_and(|wing| {
                wing.resource == 0x0002_0164
                    && wing
                        .animation
                        .as_ref()
                        .is_some_and(|a| matches!(a.slot, 80 | 84) && a.rate > 0.)
                    && world
                        .billboards
                        .values()
                        .any(|spark| spark.recipe == resonance_content::effect::WING_SPARK_SPRITE)
            });
        saw_wings && field.player_has_control()
    })?;
    assert!(battles > 0);
    assert_eq!(mission(&field, 0x40), 1_303_000);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn thoda_ring_shots_light_both_torches_and_fill_the_upper_cup() -> Result<()> {
    use resonance_events::ring::SorcerersRing;
    let mut field = enter(Fixture::WaterSeal, 9, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
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
            for _ in 0..30 {
                let camera = field.events.world.field_camera.as_ref().unwrap();
                let angle = (-(camera.target[0] - camera.position[0])
                    .atan2(camera.target[1] - camera.position[1])
                    .to_degrees())
                .trunc()
                .to_radians();
                field.step(FieldInput {
                    direction: [angle.sin(), angle.cos()],
                    ..Default::default()
                })?;
            }
        } else {
            actor.position = [center[0] + 150., center[1], 0.];
            actor.face(270.);
        }
        field.step(FieldInput {
            alternate: true,
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
    let mut field = enter(Fixture::Martel, 332, Some(2500))?;
    field.party_mut().formation = vec![1, 2, 3];
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.trigger(2002, false)?);
    // The field requests two tutorial encounters. Supply victory explicitly;
    // this checks the field continuation, not an implemented combat runner.
    for formation in [1, 2] {
        advance_until(&mut field, |field| {
            field.events.world.battle_request.is_some()
        })?;
        let request = field.events.world.battle_request.as_ref().unwrap();
        assert_eq!(
            request.setup.encounter,
            resonance_events::battle::Encounter::Formation(formation)
        );
        skip_battle(&mut field)?;
    }
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Martel fields; no devices"]
fn martel_selector_starts_before_the_golem_scene_and_ring_pickup() -> Result<()> {
    let mut field = enter(Fixture::Martel, 308, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
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
    assert_eq!(ring.resource, (-1_179_645_i32) as u32);
    assert_eq!(ring.position, [-890., 1080., -260.]);
    assert!(field.events.trigger(1014, false)?);
    advance_until(&mut field, |field| {
        field.events.world.battle_request.is_some()
    })?;
    skip_battle(&mut field)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert_eq!(field.story_progress()?, 105_000);
    assert!(field.events.world.event_flags.contains(&207));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Thoda seal; no devices"]
fn thoda_seal_finishes_after_colette_releases_her_wings() -> Result<()> {
    let mut field = configured(Fixture::WaterSeal, 10, |entry| {
        entry
            .persistent
            .memory
            .write(0xc4, symphonia_script::Width::S32, 13_000)?;
        entry.position = [0.; 3];
        entry.heading = 180.;
        Ok(())
    })?;
    let mut wings = false;
    let battles = replay(&mut field, |f| {
        wings |= f
            .events
            .world
            .actors
            .get(&resonance_events::COLETTE_WINGS_ACTOR)
            .is_some_and(|a| a.visible);
        wings && f.player_has_control() && mission(f, 0xc4) == 21_000
    })?;
    assert!(battles > 0 && field.events.world.event_flags.contains(&201));
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; Thoda bridge, no devices"]
fn thoda_bridge_targets_a_live_material_and_scrolls() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let package = new_game::FieldPackage::prepare(&root, 6, &mut Default::default(), || false)?;
    let data = Arc::new(package.files.json("game/session-data.json")?);
    let mut entry = Fixture::WaterSeal
        .destination()
        .entry(data, new_game::available_fields(&root)?)?;
    entry
        .persistent
        .memory
        .write(0xc4, symphonia_script::Width::S32, 13_000)?;
    let mut field = Scene::enter(&package, entry)?;
    advance_until(&mut field, |field| {
        field.events.world.actors.contains_key(&6000)
    })?;
    let assets: resonance_content::field::FieldAssets =
        serde_json::from_slice(&std::fs::read(root.join("fields/map-6.json"))?)?;
    assets.validate()?;
    let bridge = assets
        .texture_animations
        .iter()
        .find(|track| {
            matches!(
                track.actor,
                resonance_content::field::RenderValue::Setting(2)
            )
        })
        .context("missing exterior bridge callback")?;
    let world = &field.events.world;
    assert_eq!(world.render_settings.get(&128), Some(&1));
    let id = bridge.actor.resolve(&world.render_settings);
    let texture = bridge.texture.resolve(&world.render_settings);
    let actor = world.actors.get(&id).context("bridge actor is absent")?;
    assert!(actor.visible);
    let model = assets
        .actors
        .iter()
        .find(|model| model.resource == actor.resource)
        .context("bridge model is absent")?;
    assert!(
        model
            .parts
            .iter()
            .flat_map(|part| &part.materials)
            .flat_map(|material| [&material.color, &material.multiply])
            .flatten()
            .any(|binding| binding.texture as i32 == texture),
        "bridge callback does not bind an actual material"
    );
    let before = bridge.offset(
        world.texture_animation_tick,
        world.texture_animation_effect_tick,
    );
    ticks(&mut field, 60, FieldInput::default())?;
    let world = &field.events.world;
    let after = bridge.offset(
        world.texture_animation_tick,
        world.texture_animation_effect_tick,
    );
    assert_eq!(before[0], after[0]);
    assert_ne!(before, after, "bridge material must animate");
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked Mana bridge assets; no devices"]
fn mana_enemies_respect_closed_bridge_barriers() -> Result<()> {
    let mut field = configured(Fixture::Mana, 366, |entry| {
        entry
            .persistent
            .memory
            .write(0xcc, symphonia_script::Width::S32, 12_000)?;
        entry.position = [40., 0., 7.];
        Ok(())
    })?;
    advance_until(&mut field, FieldSession::player_has_control)?;
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
        ticks(&mut field, 100, FieldInput::default())?;
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
    let mut field = configured(Fixture::FireSeal, 485, |entry| {
        entry
            .persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 1_306_000)?;
        entry.persistent.event_flags.insert(1749);
        Ok(())
    })?;
    let (mut getup, mut released) = (false, false);
    replay(&mut field, |f| {
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
        released && f.player_has_control()
    })?;
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn story_scenes_reach_their_next_stage() -> Result<()> {
    for (map, before, after, trigger) in [
        (347, 1_305_000, 1_402_000, Some(3003)),
        (276, 1_107_000, 1_108_000, None),
        (535, 2_302_000, 2_303_000, None),
    ] {
        let mut field = configured(Fixture::FireSeal, map, |entry| {
            entry
                .persistent
                .memory
                .write(0x40, symphonia_script::Width::S32, before)?;
            entry.position = [0.; 3];
            Ok(())
        })?;
        if let Some(trigger) = trigger {
            advance_until(&mut field, FieldSession::player_has_control)?;
            assert!(field.events.trigger(trigger, false)?);
        }
        until(&mut field, dialogue_input(), |f| {
            skip_battle(f)?;
            if let Some(movie) = &f.events.world.movie
                && movie.operation.is_pending()
            {
                movie.operation.complete(None).map_err(anyhow::Error::msg)?;
            }
            Ok(f.story_progress()? >= after)
        })
        .with_context(|| format!("scene {map}"))?;
        if map == 276 {
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
    let mut field = enter(Fixture::FireSeal, 220, Some(1_302_000))?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    let position = field.actor(5020).position;
    field.actor_mut(1).position = [position[0], position[1] + 250., position[2] - 70.];
    field.actor_mut(1).face(0.);
    field.step(FieldInput {
        alternate: true,
        ..Default::default()
    })?;
    until(&mut field, dialogue_input(), |f| {
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


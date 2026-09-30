//! Device-free scene regressions using fresh progression fixtures.
use super::{DESTINATIONS, new_game};
use anyhow::{Context, Result};
use resonance_game::field::{FieldInput, FieldSession};
use std::{path::PathBuf, sync::Arc};

fn assets_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    ))
}
fn configured(
    destination: usize,
    map: u32,
    configure: impl FnOnce(&mut resonance_game::field::FieldEntry) -> Result<()>,
) -> Result<FieldSession> {
    let root = assets_root()?;
    let package = new_game::FieldPackage::prepare(&root, map, &mut Default::default(), || false)?;
    let data = Arc::new(package.files.json("game/session-data.json")?);
    let mut entry = DESTINATIONS[destination].entry(data, new_game::available_fields(&root)?)?;
    configure(&mut entry)?;
    package.enter(entry)
}
fn enter(destination: usize, map: u32, story: Option<i32>) -> Result<FieldSession> {
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
fn ticks(field: &mut FieldSession, count: usize, input: FieldInput) -> Result<()> {
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
fn replay(
    field: &mut FieldSession,
    mut finished: impl FnMut(&FieldSession) -> bool,
) -> Result<usize> {
    let root = assets_root()?;
    let package =
        new_game::FieldPackage::prepare(&root, field.map_id, &mut Default::default(), || false)?;
    let mut audio = crate::field_audio::validation::Playback::new((*package.audio).clone(), field);
    let mut battles = 0;
    for tick in 0..18_000 {
        field.step(FieldInput {
            interact: tick % 2 == 0,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        audio.step(field)?;
        battles += usize::from(skip_battle(field)?);
        if let Some(movie) = &field.events.world.movie
            && movie.operation.is_pending()
        {
            movie.operation.complete(None).map_err(anyhow::Error::msg)?;
        }
        if finished(field) {
            audio.finish()?;
            return Ok(battles);
        }
    }
    anyhow::bail!(
        "scene {} did not finish: {:?}",
        field.map_id,
        field.events.pending_operations()
    )
}

fn follow_transition(field: &FieldSession) -> Result<FieldSession> {
    let root = PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let request = field
        .events
        .world
        .field_transition
        .as_ref()
        .context("field transition")?;
    let package =
        new_game::FieldPackage::prepare(&root, request.map, &mut Default::default(), || false)?;
    package.enter(resonance_game::field::FieldEntry {
        persistent: field.events.persistent_state()?,
        data: Some(Arc::new(package.files.json("game/session-data.json")?)),
        available_fields: new_game::available_fields(&root)?,
        position: request.position,
        heading: request.heading,
        camera: request.camera.clone(),
        ..Default::default()
    })
}

fn advance_until(field: &mut FieldSession, ready: impl Fn(&FieldSession) -> bool) -> Result<()> {
    for tick in 0..7200 {
        field.step(FieldInput {
            interact: tick % 2 == 0,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        if ready(field) {
            return Ok(());
        }
    }
    anyhow::bail!(
        "field {} did not reach the expected state; control={}, waits={:?}, dialogues={:?}",
        field.map_id,
        field.player_has_control(),
        field.events.pending_operations(),
        field
            .events
            .world
            .dialogue
            .values()
            .map(|d| &d.body.tokens)
            .collect::<Vec<_>>()
    )
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn martel_golem_battle_creates_a_pushable_block() -> Result<()> {
    let mut field = enter(0, 308, None)?;
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
    let mut field = enter(8, 268, Some(1_105_000))?;
    advance_until(&mut field, |field| {
        field
            .events
            .world
            .actors
            .get(&3004)
            .and_then(|actor| actor.enemy.as_ref())
            .is_some_and(|enemy| enemy.contact_cooldown == -1)
            && field
                .events
                .world
                .billboards
                .values()
                .any(|effect| effect.recipe == 42)
    })?;
    assert_eq!(
        field.events.world.actors[&3004]
            .enemy
            .as_ref()
            .unwrap()
            .stun_effect(),
        Some(resonance_events::effect::StunEffect::Electric)
    );
    advance_until(&mut field, |field| {
        field.player_has_control() && mission(field, 0x40) == 1_105_100
    })?;
    assert!(field.events.exploration_error.is_none());
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn asgard_kvar_preparation_can_restore_a_party_with_empty_slots() -> Result<()> {
    let mut field = enter(7, 213, None)?;
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
    assert!(
        !field
            .events
            .world
            .party
            .as_ref()
            .unwrap()
            .formation
            .is_empty()
    );
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn rheaird_crash_runs_the_debris_effect_and_restores_control() -> Result<()> {
    // Field 416's arrival branch compares story against 0x24B288.
    let mut field = enter(9, 416, Some(2_405_000))?;
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
#[ignore = "requires locally cooked fields; no devices"]
fn mana_selector_plays_the_opening_mechanism_scene() -> Result<()> {
    mana_intro()?;
    Ok(())
}

fn mana_intro() -> Result<FieldSession> {
    let mut field = enter(4, 362, None)?;
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
    let mut field = package.enter(resonance_game::field::FieldEntry {
        persistent,
        data: Some(Arc::new(package.files.json("game/session-data.json")?)),
        available_fields: new_game::available_fields(&root)?,
        position: [-400., 2600., 913.],
        ..Default::default()
    })?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    assert!(field.events.trigger(1101, false)?);
    advance_until(&mut field, |field| {
        field.player_has_control() && mission(field, 0xcc) == 13_600
    })?;
    let party = field.events.world.party.as_ref().unwrap();
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
fn palmacosta_post_boss_exit_runs_the_ranch_destruction() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let package = new_game::FieldPackage::prepare(&root, 198, &mut Default::default(), || false)?;
    let data = Arc::new(package.files.json("game/session-data.json")?);
    let mut entry = DESTINATIONS[6].entry(data, new_game::available_fields(&root)?)?;
    // The defeated Magnius scene writes 6100 before its ChangeField to 198.
    // Keep the selector's story stage so this catches its skipped cutscene.
    entry
        .persistent
        .memory
        .write(0xb8, symphonia_script::Width::S32, 6100)?;
    entry.position = [-751., -5721., -51.];
    let mut field = package.enter(entry)?;
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
fn iselia_damage_spheres_keep_moving() -> Result<()> {
    for (map, first, count) in [(194, 3021, 8), (196, 3301, 13)] {
        let mut field = enter(5, map, None)?;
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
                let actor = &field.events.world.actors[&(first + i as i32)];
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
fn iselia_exit_restores_the_party_and_finishes_the_scene() -> Result<()> {
    let mut field = enter(5, 193, Some(20_307_000))?;
    replay(&mut field, |f| f.events.world.field_transition.is_some())?;
    assert_eq!(field.story_progress()?, 20_308_000);
    let party = field.events.world.party.as_ref().unwrap();
    assert_eq!(party.formation.len(), 8);
    assert!(party.formation.iter().all(|id| *id != 0));
    assert_eq!(party.field_leader, 1);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_mimic_blocks_walking_like_an_ordinary_chest() -> Result<()> {
    let mut field = enter(1, 219, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    // Original actor 8300 uses local model 0 rather than the treasure service.
    let chest = field.events.world.actors[&8300].clone();
    assert_eq!(chest.position, [-4126., 1764., 15.]);
    assert!(chest.collidable);
    let player = field.events.world.controlled_actor;
    let radius = chest.radius + field.events.world.actors[&player].radius;
    let start = [
        chest.position[0] + 160.,
        chest.position[1],
        chest.position[2],
    ];
    field.events.world.field_camera = None;
    for enabled in [true, false] {
        field.events.world.actors.get_mut(&8300).unwrap().collidable = enabled;
        field.events.world.actors.get_mut(&player).unwrap().position = start;
        for _ in 0..80 {
            field.step(FieldInput {
                direction: [-1., 0.],
                ..Default::default()
            })?;
        }
        let position = field.events.world.actors[&player].position;
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
fn triet_seal_scripted_wings_animate_and_emit_sparks() -> Result<()> {
    let mut field = enter(10, DESTINATIONS[10].map, None)?;
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
    let mut field = enter(2, 9, None)?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    for (target, flag, ring) in [
        (102, 150, SorcerersRing::Fire),
        (103, 151, SorcerersRing::Fire),
        (100, 152, SorcerersRing::Water),
    ] {
        assert!(!field.events.world.event_flags.contains(&flag));
        let center = field.events.world.actors[&target].position;
        field
            .events
            .world
            .party
            .as_mut()
            .unwrap()
            .travel
            .sorcerers_ring = ring;
        let player = field.events.world.controlled_actor;
        let actor = field.events.world.actors.get_mut(&player).unwrap();
        actor.position = [center[0] + 150., center[1], 0.];
        actor.face(270.);
        field.step(FieldInput {
            alternate: true,
            ..Default::default()
        })?;
        advance_until(&mut field, |field| {
            field.events.world.event_flags.contains(&flag) && field.player_has_control()
        })
        .with_context(|| format!("ring {ring:?} did not activate Thoda target {target}"))?;
    }
    Ok(())
}

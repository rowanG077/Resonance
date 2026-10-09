use super::{assets_root as root, *};
use anyhow::ensure;
use resonance_events::input::Button;
use std::path::Path;

fn enter(root: &Path, map: u32, position: [f32; 3]) -> Result<Scene> {
    enter_flags(root, map, position, &[])
}

fn enter_flags(root: &Path, map: u32, position: [f32; 3], flags: &[u16]) -> Result<Scene> {
    enter_story(root, map, position, flags, 14_000_000)
}

fn enter_story(
    root: &Path,
    map: u32,
    position: [f32; 3],
    flags: &[u16],
    story: i32,
) -> Result<Scene> {
    let mut field = Scene::story(root, map, story, |entry| {
        entry.allow_incomplete_scripts = true;
        entry.position = position;
        entry
            .persistent
            .event_flags
            .extend([22].into_iter().chain(flags.iter().copied()));
        let party = entry.persistent.party.as_mut().unwrap();
        party.formation = vec![1, 2, 3, 4];
        party.gald = 100_000;
        party.travel.overworld = Some(resonance_content::overworld::TravelState {
            world: resonance_content::overworld::World::Sylvarant,
            position: resonance_content::overworld::Position::from_map([9770., 23500., 0.])?,
            heading: 0.,
            camera_yaw: 0.,
            alternate_perspective: false,
            map_display: resonance_content::overworld::MapDisplay::Small,
            mount: resonance_content::overworld::Mount::Foot,
            altitude: 0.,
        });
        Ok(())
    })?;
    settle(&mut field)?;
    Ok(field)
}

fn settle(field: &mut Scene) -> Result<()> {
    field.advance_until(|field| {
        field.player_has_control()
            || field.events.world.world_transition.is_some()
            || field.events.world.field_transition.is_some()
    })
}

fn player(field: &FieldSession) -> [f32; 3] {
    field.events.world.actors[&field.events.world.controlled_actor].position
}

fn walk_to(field: &mut Scene, target: [f32; 2], confirm: bool) -> Result<()> {
    for _ in 0..600 {
        let p = player(field);
        let d = [target[0] - p[0], target[1] - p[1]];
        let length = d[0].hypot(d[1]);
        if length < 8.
            || field.events.world.field_transition.is_some()
            || field.events.world.world_transition.is_some()
        {
            return Ok(());
        }
        field.step(FieldInput {
            pressed_buttons: field
                .walking(d.map(|v| v / length))
                .pressed_buttons
                .with(Button::Accept, confirm),
            ..field.walking(d.map(|v| v / length))
        })?;
        ensure!(
            field.events.exploration_error.is_none(),
            "walking fell back: {:?}",
            field.events.exploration_error
        );
    }
    anyhow::bail!("walking to {target:?} stuck at {:?}", player(field))
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked Salvation stair ramp and exits"]
fn salvation_stairs_and_world_exits_are_walkable() -> Result<()> {
    let root = root()?;
    let mut inside = enter(&root, 95, [-300., -200., 8.])?;
    for point in [
        [-475., -210.],
        [-486., -100.],
        [-500., -20.],
        [-488., 70.],
        [-458., 170.],
        [-447., 245.],
    ] {
        walk_to(&mut inside, point, false)?;
    }
    settle(&mut inside)?;
    ensure!(
        inside
            .events
            .world
            .field_transition
            .as_ref()
            .is_some_and(|r| r.map == 96),
        "stairs did not lead upstairs: {:?}",
        player(&inside)
    );
    for (start, exit) in [
        ([-110., 750., 6.], [-110., 940.]),
        ([-650., -950., 6.], [-650., -1200.]),
    ] {
        let mut outside = enter(&root, 81, start)?;
        walk_to(&mut outside, exit, true)?;
        settle(&mut outside)?;
        ensure!(
            outside.events.exploration_error.is_none(),
            "exit entered exploration fallback"
        );
        ensure!(
            outside
                .events
                .world
                .world_transition
                .as_ref()
                .is_some_and(|r| r.location == 28),
            "outside did not return to world"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked Linkite information sequence"]
fn linkite_conversations_restore_walking_on_the_floor() -> Result<()> {
    let mut field = enter(&root()?, 243, [0.; 3])?;
    for id in [101, 102, 103, 104] {
        ensure!(field.events.interact(id)?, "caravan interaction missing");
        settle(&mut field)?;
        ensure!(
            field.events.exploration_error.is_none(),
            "conversation failed: {:?}",
            field.events.exploration_error
        );
    }
    let before = player(&field);
    ensure!(
        field.collision().ground_surface(before).is_some(),
        "Lloyd was left above the walking floor"
    );
    field.ticks(
        30,
        FieldInput {
            direction: [1., 0.],
            ..Default::default()
        },
    )?;
    let after = player(&field);
    ensure!(
        (after[0] - before[0]).hypot(after[1] - before[1]) > 30.,
        "Lloyd could animate but not move"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked Triet hole reaction"]
fn triet_hole_examination_shows_lloyds_reaction() -> Result<()> {
    let mut field = enter(&root()?, 485, [-2701., 914., -86.])?;
    let id = field.events.world.controlled_actor;
    field.events.world.actors.get_mut(&id).unwrap().face(270.);
    ensure!(
        field.interaction_target() == Some(401),
        "hole was not reachable"
    );
    field.step(FieldInput {
        pressed_buttons: [Button::Accept].into(),
        ..Default::default()
    })?;
    for _ in 0..10 {
        if let Some(emote) = field.events.world.emotes.get(&-100) {
            ensure!(
                emote.actor == id
                    && emote.kind == resonance_events::emote::Kind::Sweat
                    && emote.duration == Some(30),
                "incorrect hole reaction: {emote:?}"
            );
            settle(&mut field)?;
            ensure!(
                field.events.exploration_error.is_none(),
                "hole script failed"
            );
            return Ok(());
        }
        field.step(Default::default())?;
    }
    anyhow::bail!("examining the hole produced no reaction")
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked denied Iselia entry"]
fn iselia_denial_preserves_the_return_to_world() -> Result<()> {
    let field = enter(&root()?, 346, [0.; 3])?;
    ensure!(
        field.events.exploration_error.is_none(),
        "denied entry became a broken preview"
    );
    ensure!(
        field
            .events
            .world
            .world_transition
            .as_ref()
            .is_some_and(|r| r.location == 2),
        "denied entry did not return to Iselia's landmark"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked inn rest and optional skit"]
fn palmacosta_inn_rest_preserves_field_control() -> Result<()> {
    let mut field = enter(&root()?, 59, [0.; 3])?;
    ensure!(
        field.events.interact(2001)?,
        "innkeeper interaction missing"
    );
    settle(&mut field)?;
    ensure!(
        field.events.exploration_error.is_none(),
        "inn rest failed: {:?}",
        field.events.exploration_error
    );
    let party = field.events.world.party.as_ref().unwrap();
    ensure!(
        party.gald == 99_760 && party.viewed_skits.contains(&454),
        "inn rest/skit did not complete"
    );
    walk_to(&mut field, [80., 0.], false)?;
    walk_to(&mut field, [80., -105.], true)?;
    settle(&mut field)?;
    ensure!(
        field
            .events
            .world
            .field_transition
            .as_ref()
            .is_some_and(|t| t.map == 54),
        "inn exit did not return to Palmacosta"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked upstairs bed prompt and rest"]
fn salvation_beds_preserve_npcs_camera_and_downstairs_exit() -> Result<()> {
    let mut field = enter_flags(&root()?, 96, [137., -37., 0.], &[192])?;
    field.ticks(40, Default::default())?;
    ensure!(
        field.events.exploration_error.is_none(),
        "bed approach failed: {:?}",
        field.events.exploration_error
    );
    ensure!(
        field
            .action_prompt()
            .is_some_and(|p| p.action == resonance_game::field::FieldAction::Rest),
        "missing Rest prompt"
    );
    field.step(FieldInput {
        pressed_buttons: [Button::Accept].into(),
        ..Default::default()
    })?;
    settle(&mut field)?;
    ensure!(
        field.events.exploration_error.is_none(),
        "rest failed: {:?}",
        field.events.exploration_error
    );
    ensure!(
        field.events.world.actors[&101].collidable,
        "grandma lost her collision"
    );
    ensure!(field.events.interact(101)?, "grandma stopped responding");
    settle(&mut field)?;
    walk_to(&mut field, [0., 100.], false)?;
    walk_to(&mut field, [137., -16.], false)?;
    field.ticks(40, Default::default())?;
    ensure!(
        field
            .action_prompt()
            .is_some_and(|p| p.action == resonance_game::field::FieldAction::Rest),
        "second Rest prompt missing"
    );
    field.step(FieldInput {
        pressed_buttons: [Button::Accept].into(),
        ..Default::default()
    })?;
    settle(&mut field)?;
    ensure!(
        field.events.exploration_error.is_none(),
        "second rest failed: {:?}",
        field.events.exploration_error
    );
    for point in [
        [0., 100.],
        [-75., 200.],
        [-75., 350.],
        [-150., 390.],
        [-249., 440.],
    ] {
        walk_to(&mut field, point, false)?;
    }
    settle(&mut field)?;
    ensure!(
        field
            .events
            .world
            .field_transition
            .as_ref()
            .is_some_and(|t| t.map == 95),
        "downstairs did not open"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; early-story repeated rest"]
fn early_salvation_repeated_rest_preserves_field_control() -> Result<()> {
    let mut field = enter_story(&root()?, 96, [137., -37., 0.], &[192], 1_101_000)?;
    let camera = field.events.world.field_camera.as_ref().unwrap();
    let view = (camera.angles, camera.distance);
    for attempt in 1..=3 {
        field.events.world.party.as_mut().unwrap().members[0].hp = 1;
        field.ticks(40, Default::default())?;
        ensure!(
            field
                .action_prompt()
                .is_some_and(|p| p.action == resonance_game::field::FieldAction::Rest),
            "missing rest prompt at attempt {attempt}"
        );
        field.step(FieldInput {
            pressed_buttons: [Button::Accept].into(),
            ..Default::default()
        })?;
        settle(&mut field)?;
        ensure!(
            field.events.exploration_error.is_none(),
            "rest {attempt} failed: {:?}",
            field.events.exploration_error
        );
        ensure!(
            field.events.world.party.as_ref().unwrap().members[0].hp > 1,
            "rest {attempt} did not heal"
        );
        ensure!(
            field.events.world.actors[&101].collidable,
            "rest {attempt} removed grandma's collision"
        );
        let camera = field.events.world.field_camera.as_ref().unwrap();
        assert_eq!(
            (camera.angles, camera.distance),
            view,
            "rest {attempt} reset the camera"
        );
        walk_to(&mut field, [0., 100.], false)?;
        walk_to(&mut field, [137., -16.], false)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked Fire Seal enemy contact"]
fn fire_seal_skipped_battles_preserve_field_control() -> Result<()> {
    let mut contacts = 0;
    let map = 219;
    let mut field = enter_story(&root()?, map, [0.; 3], &[], 1_101_000)?;
    ensure!(
        field.events.exploration_error.is_none(),
        "map {map} entry failed: {:?}",
        field.events.exploration_error
    );
    let enemies: Vec<_> = field
        .events
        .world
        .actors
        .iter()
        .filter_map(|(&id, a)| a.enemy.as_ref().map(|_| id))
        .collect();
    let start = player(&field);
    for enemy in enemies {
        // Test each battle from the same walkable area, before reaching a wall.
        let controlled = field.events.world.controlled_actor;
        field.actor_mut(controlled).position = start;
        let camera = field.events.world.field_camera.as_ref().unwrap();
        let view = (camera.angles, camera.distance);
        let battles = field.events.world.party.as_ref().unwrap().battles.total;
        ensure!(
            field.events.contact_enemy(enemy)?,
            "enemy {enemy} in map {map} did not accept contact"
        );
        field.advance_until(|field| {
            field.player_has_control()
                && field.events.world.party.as_ref().unwrap().battles.total == battles + 1
                && !field.events.world.actors.contains_key(&enemy)
        })?;
        ensure!(
            field.events.exploration_error.is_none(),
            "map {map}, enemy {enemy}: {:?}",
            field.events.exploration_error
        );
        let camera = field.events.world.field_camera.as_ref().unwrap();
        assert_eq!(
            (camera.angles, camera.distance),
            view,
            "battle reset map {map}'s camera"
        );
        let before = player(&field);
        field.ticks(
            10,
            FieldInput {
                direction: [1., 0.],
                ..Default::default()
            },
        )?;
        ensure!(
            (player(&field)[0] - before[0]).hypot(player(&field)[1] - before[1]) > 1.,
            "battle {contacts} against {enemy} left walking frozen: {before:?} -> {:?}; controlled={}",
            player(&field),
            field.player_has_control()
        );
        contacts += 1;
    }
    assert_eq!(contacts, 3, "Fire Seal test missed an enemy");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; cooked Thoda props and ambient audio"]
fn thoda_rocks_stay_paused_and_entrance_ambience_is_prepared() -> Result<()> {
    let root = root()?;
    let mut field = enter(&root, 6, [395., -213., 0.])?;
    for _ in 0..300 {
        let animation = field.events.world.actors[&6010]
            .animation
            .as_ref()
            .context("rock animation missing")?;
        ensure!(
            animation.sample(field.events.tick(), 0, animation.duration_ticks as f32) == 0.,
            "rocks replayed before their event"
        );
        field.step(Default::default())?;
    }
    let id = field.events.world.controlled_actor;
    field.events.world.actors.get_mut(&id).unwrap().position = [433., 2420., 333.];
    let first = field.audio.commands.len();
    field.ticks(10, Default::default())?;
    ensure!(
        field.audio.commands[first..]
            .iter()
            .any(|(_, _, command)| matches!(
                command,
                resonance_events::AudioCommand::Sound { id: 286, .. }
                    | resonance_events::AudioCommand::RepeatSound { id: 286, .. }
            )),
        "Thoda approach did not play its ambient sound"
    );
    ensure!(
        field.events.exploration_error.is_none(),
        "Thoda approach failed: {:?}",
        field.events.exploration_error
    );
    Ok(())
}

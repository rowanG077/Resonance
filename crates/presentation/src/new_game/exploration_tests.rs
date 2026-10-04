use super::*;
use std::path::PathBuf;

fn enter(root: &Path, map: u32, position: [f32; 3]) -> Result<FieldSession> {
    enter_flags(root, map, position, &[])
}

fn enter_flags(root: &Path, map: u32, position: [f32; 3], flags: &[u16]) -> Result<FieldSession> {
    enter_story(root, map, position, flags, 14_000_000)
}

fn enter_story(
    root: &Path,
    map: u32,
    position: [f32; 3],
    flags: &[u16],
    story: i32,
) -> Result<FieldSession> {
    let package = FieldPackage::prepare(root, map, &mut Default::default(), || false)?;
    let data: Arc<resonance_content::session::SessionData> =
        Arc::new(package.files.json("game/session-data.json")?);
    let mut party = resonance_events::party::Party::new(&data, Default::default())?;
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
    let mut persistent = resonance_events::PersistentState {
        party: Some(party),
        event_flags: [22].into_iter().chain(flags.iter().copied()).collect(),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, story)?;
    let mut field = package.enter(FieldEntry {
        allow_incomplete_scripts: true,
        persistent,
        data: Some(data),
        skits: Some(Arc::new(package.files.json("game/skits.json")?)),
        available_fields: available_fields(root)?,
        position,
        ..Default::default()
    })?;
    settle(&mut field)?;
    Ok(field)
}

fn settle(field: &mut FieldSession) -> Result<()> {
    for _ in 0..3600 {
        field.step(FieldInput {
            interact: true,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        if field.player_has_control()
            || field.events.world.world_transition.is_some()
            || field.events.world.field_transition.is_some()
        {
            return Ok(());
        }
    }
    anyhow::bail!("field did not settle")
}

fn root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    ))
}

fn player(field: &FieldSession) -> [f32; 3] {
    field.events.world.actors[&field.events.world.controlled_actor].position
}

fn walk_to(field: &mut FieldSession, target: [f32; 2], confirm: bool) -> Result<()> {
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
        let c = field.events.world.field_camera.as_ref().unwrap();
        let f = [c.target[0] - c.position[0], c.target[1] - c.position[1]];
        let angle = (-f[0].atan2(f[1]).to_degrees()).trunc().to_radians();
        let f = [-angle.sin(), angle.cos()];
        field.step(FieldInput {
            direction: [
                (f[1] * d[0] - f[0] * d[1]) / length,
                (f[0] * d[0] + f[1] * d[1]) / length,
            ],
            interact: confirm,
            ..Default::default()
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original Salvation stair ramp and exits"]
fn original_salvation_stairs_and_world_exits_are_walkable() -> Result<()> {
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original Linkite information sequence"]
fn original_linkite_conversations_restore_walking_on_the_floor() -> Result<()> {
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
    for _ in 0..30 {
        field.step(FieldInput {
            direction: [1., 0.],
            ..Default::default()
        })?;
    }
    let after = player(&field);
    ensure!(
        (after[0] - before[0]).hypot(after[1] - before[1]) > 30.,
        "Lloyd could animate but not move"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; original Triet hole reaction"]
fn original_triet_hole_examination_shows_lloyds_reaction() -> Result<()> {
    let mut field = enter(&root()?, 485, [-2701., 914., -86.])?;
    let id = field.events.world.controlled_actor;
    field.events.world.actors.get_mut(&id).unwrap().face(270.);
    ensure!(
        field.interaction_target() == Some(401),
        "hole was not reachable"
    );
    field.step(FieldInput {
        interact: true,
        ..Default::default()
    })?;
    for _ in 0..10 {
        if let Some(emote) = field.events.world.emotes.get(&-100) {
            ensure!(
                emote.actor == id && emote.kind == 10 && emote.duration == Some(30),
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original denied Iselia entry"]
fn original_iselia_denial_preserves_the_return_to_world() -> Result<()> {
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original inn rest and optional skit"]
fn original_palmacosta_inn_rest_preserves_field_control() -> Result<()> {
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original upstairs bed prompt and rest"]
fn original_salvation_beds_preserve_npcs_camera_and_downstairs_exit() -> Result<()> {
    let mut field = enter_flags(&root()?, 96, [137., -37., 0.], &[192])?;
    for _ in 0..40 {
        field.step(Default::default())?;
    }
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
        interact: true,
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
    for _ in 0..40 {
        field.step(Default::default())?;
    }
    ensure!(
        field
            .action_prompt()
            .is_some_and(|p| p.action == resonance_game::field::FieldAction::Rest),
        "second Rest prompt missing"
    );
    field.step(FieldInput {
        interact: true,
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
fn original_early_salvation_repeated_rest_preserves_field_control() -> Result<()> {
    let mut field = enter_story(&root()?, 96, [137., -37., 0.], &[192], 1_101_000)?;
    let camera = field.events.world.field_camera.as_ref().unwrap();
    let view = (camera.angles, camera.distance);
    for attempt in 1..=3 {
        field.events.world.party.as_mut().unwrap().members[0].hp = 1;
        for _ in 0..40 {
            field.step(Default::default())?;
        }
        ensure!(
            field
                .action_prompt()
                .is_some_and(|p| p.action == resonance_game::field::FieldAction::Rest),
            "missing rest prompt at attempt {attempt}"
        );
        field.step(FieldInput {
            interact: true,
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original Fire Seal enemy contact"]
fn original_fire_seal_skipped_battles_preserve_field_control() -> Result<()> {
    let mut contacts = 0;
    // The main dungeon room has three ordinary enemy symbols. Rooms with
    // unfinished ring/puzzle services remain outside the exploration test.
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
        field
            .events
            .world
            .actors
            .get_mut(&controlled)
            .unwrap()
            .position = start;
        let camera = field.events.world.field_camera.as_ref().unwrap();
        let view = (camera.angles, camera.distance);
        let battles = field.events.world.party.as_ref().unwrap().battles.total;
        ensure!(
            field.events.contact_enemy(enemy)?,
            "enemy {enemy} in map {map} did not accept contact"
        );
        settle(&mut field)?;
        ensure!(
            field.events.exploration_error.is_none(),
            "map {map}, enemy {enemy}: {:?}",
            field.events.exploration_error
        );
        ensure!(
            field.player_has_control(),
            "map {map} did not return control"
        );
        ensure!(
            !field.events.world.actors.contains_key(&enemy),
            "defeated symbol {enemy} remains"
        );
        assert_eq!(
            field.events.world.party.as_ref().unwrap().battles.total,
            battles + 1
        );
        let camera = field.events.world.field_camera.as_ref().unwrap();
        assert_eq!(
            (camera.angles, camera.distance),
            view,
            "battle reset map {map}'s camera"
        );
        let before = player(&field);
        for _ in 0..10 {
            field.step(FieldInput {
                direction: [1., 0.],
                ..Default::default()
            })?;
        }
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; original Thoda props and ambient audio"]
fn original_thoda_rocks_stay_paused_and_entrance_ambience_is_prepared() -> Result<()> {
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
    let audio: resonance_content::field_audio::FieldAudio =
        serde_json::from_slice(&std::fs::read(root.join("fields/map-6-audio.json"))?)?;
    let mut heard = false;
    for _ in 0..10 {
        field.step(Default::default())?;
        for command in std::mem::take(&mut field.events.world.audio_commands) {
            if let resonance_events::AudioCommand::Sound { id, .. }
            | resonance_events::AudioCommand::RepeatSound { id, .. } = command
            {
                ensure!(audio.sounds.contains_key(&id), "uncooked field sound {id}");
                heard |= id == 286;
            }
        }
    }
    ensure!(heard, "Thoda approach did not request its ambient sound");
    ensure!(
        field.events.exploration_error.is_none(),
        "Thoda approach failed: {:?}",
        field.events.exploration_error
    );
    Ok(())
}

use resonance_events::*;
use std::sync::Arc;
use symphonia_script::{
    NativeCall as Call, Program, Width,
    message::{Message, Token},
};

fn arg(code: &mut Vec<u16>, n: i32) {
    code.extend([0x0200, n as u16, (n as u32 >> 16) as u16, 0x3000, 0x4000]);
}
fn native(code: &mut Vec<u16>, op: Call, a: &[i32]) {
    for &a in a {
        arg(code, a);
    }
    code.push(0x2000 | u16::from(op as u8));
}
/// Encode a sequence of native calls followed by an event return.
fn script(calls: &[(Call, &[i32])]) -> Vec<u16> {
    let mut code = Vec::new();
    for &(call, args) in calls {
        native(&mut code, call, args);
    }
    code.push(0x20ff);
    code
}

fn steps(events: &mut EventRuntime, count: u32) {
    for _ in 0..count {
        events.step().unwrap();
    }
}

fn controlled_world() -> GameWorld {
    let mut world = GameWorld::default();
    world.input_enabled = true;
    world
}

fn interactive_effect(setup: &[u16], interaction: &[u16]) -> EventRuntime {
    let world = controlled_world();
    runtime(program(setup, interaction), Default::default(), world)
}

fn emitter(preset: i32, speed: i32, parameters: &[i32]) -> [i32; 18] {
    let mut args = [0; 18];
    args[0] = 500;
    args[5] = preset;
    args[7] = speed;
    args[8..8 + parameters.len()].copy_from_slice(parameters);
    args
}

fn enemy_resources() -> ResourceLibrary {
    ResourceLibrary {
        bindings: [(1, (ResourceKind::Model, 1))].into(),
        models: [(1, model([12, 36], 20))].into(),
        ..Default::default()
    }
}

#[test]
fn genis_fireball_travels_with_a_fading_trail_and_cancels_with_its_owner() {
    let cast = script(&[(
        Call::CreateEffectEmitter,
        &[
            5000, -1700, 1782, 847, 0, 51, 0, 0, 100, 78, 0, 0, -3110, 438, 100, 0, 0, 0,
        ],
    )]);
    let cancel = script(&[(Call::DespawnActor, &[5000])]);
    let mut events = interactive_effect(&cast, &cancel);
    steps(&mut events, 39);
    let midpoint = events.world.actors[&5000].position;
    let near = |position: [f32; 3], target: [f32; 3]| {
        position
            .into_iter()
            .zip(target)
            .all(|(a, b)| (a - b).abs() < 0.01)
    };
    assert!(near(midpoint, [-2405., 1110., 473.5]));
    assert!(
        events
            .world
            .billboards
            .values()
            .any(|p| p.position[0] > midpoint[0])
    );
    assert!(
        events
            .world
            .billboards
            .values()
            .any(|p| p.alpha(events.tick()) < 160.)
    );
    steps(&mut events, 39);
    assert!(near(
        events.world.actors[&5000].position,
        [-3110., 438., 100.]
    ));
    steps(&mut events, 12);
    assert!(events.world.billboards.is_empty());

    let mut events = interactive_effect(&cast, &cancel);
    steps(&mut events, 5);
    assert!(!events.world.billboards.is_empty());
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert!(!events.world.actors.contains_key(&5000));
    steps(&mut events, 2);
    assert!(events.world.billboards.is_empty());
}

#[test]
fn script_can_enable_and_disable_a_pushable_model_after_spawn() {
    let mut world = GameWorld::default();
    world.insert_actor(5000, Actor::new(267, [0.; 3]));
    let main = script(&[
        (Call::SetActorProperty, &[5000, 19, 3]),
        (Call::GetActorProperty, &[5000, 19]),
    ]);
    let child = script(&[(Call::SetActorProperty, &[5000, 19, 2])]);
    let mut events = runtime(program(&main, &child), Default::default(), world);
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 1);
    assert!(events.world.actors[&5000].pushable);
    assert_eq!(events.world.actors[&5000].radius, 50.);
    events.world.input_enabled = true;
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 1);
    assert!(!events.world.actors[&5000].pushable);
}

#[test]
fn ordinary_locomotion_replaces_same_slot_from_a_block_animation_bank() {
    use resonance_content::field::FIELD_SERVICE_MOTION_RESOURCE_BASE;
    use resonance_events::animation::{AnimationSource, slot};
    for (slot, speed) in [(slot::WALK, 4), (slot::RUN, 8)] {
        let mut actor = Actor::new(1, [0.; 3]);
        actor.animation = Some(Animation {
            source: AnimationSource::Resource,
            ..Animation::new(FIELD_SERVICE_MOTION_RESOURCE_BASE + 1, slot, 20, 0)
        });
        let mut world = GameWorld::default();
        world.controlled_actor = 1;
        world.input_enabled = true;
        world.insert_actor(1, actor);
        let resources = ResourceLibrary {
            models: [(1, model([slot], 30))].into(),
            ..Default::default()
        };
        let main = script(&[(Call::MoveActor, &[1, 0, -100, 0, speed])]);
        let mut events = runtime(program(&main, &[0x20ff]), resources, world);
        events.step().unwrap();
        let animation = events.world.actors[&1].animation.as_ref().unwrap();
        assert_eq!(animation.source, AnimationSource::Model);
        assert_eq!(animation.resource, 1);
        assert_eq!(animation.slot, slot);
    }
}

#[test]
#[ignore = "requires locally cooked party definitions; no devices"]
fn short_battle_command_waits_for_and_returns_the_real_outcome() {
    let data = cooked::<resonance_content::session::SessionData>("session-data.json");
    for (flags, outcome, defeat) in [
        (0, battle::Outcome::Victory, battle::DefeatPolicy::GameOver),
        (
            1,
            battle::Outcome::Defeat,
            battle::DefeatPolicy::ResumeEvent,
        ),
    ] {
        let mut world = GameWorld::default();
        world.party = Some(party::Party::new(&data, Default::default()).unwrap());
        let code = script(&[
            (Call::Unknown37, &[30, 79, flags]),
            (Call::SetEventBit, &[123]),
        ]);
        let mut events = runtime(program(&code, &[0x20ff]), Default::default(), world);
        let request = events.world.battle_request.clone().unwrap();
        assert_eq!(
            request.setup,
            battle::Setup {
                encounter: battle::Encounter::Formation(30),
                arena: 79,
                defeat,
                music: None,
                route: [0; 5],
            }
        );
        steps(&mut events, 3);
        assert!(!events.world.event_flags.contains(&123));
        request.complete(outcome).unwrap();
        events.world.battle_request = None;
        events.step().unwrap();
        assert_eq!(
            events.memory().read(0x20, Width::S32).unwrap(),
            outcome as i32
        );
        assert_eq!(
            events.memory().read(0x24, Width::S32).unwrap(),
            outcome as i32
        );
        assert!(events.world.event_flags.contains(&123));
    }
}

#[test]
#[ignore = "requires locally cooked party definitions; no devices"]
fn empty_slots_in_party_reunion_do_not_abort_the_event() {
    let session = Arc::new(cooked::<resonance_content::session::SessionData>(
        "session-data.json",
    ));
    let mut party = party::Party::new(&session, Default::default()).unwrap();
    party.formation = vec![1, 4];
    let mut world = GameWorld::default();
    world.party = Some(party);
    let resources = ResourceLibrary {
        session_data: Some(session),
        ..Default::default()
    };
    let setup = script(&[
        (Call::AddPartyMember, &[2]),
        (Call::AddPartyMember, &[0]),
        (Call::AddPartyMember, &[3]),
        (Call::AddPartyMember, &[0]),
        (Call::AddPartyMember, &[9]),
        (Call::AddPartyMember, &[0]),
        (Call::SetEventBit, &[123]),
    ]);
    let events = runtime(program(&setup, &[0x20ff]), resources, world);
    let party = events.world.party.as_ref().unwrap();
    assert_eq!(party.formation, [1, 4, 2, 3, 9]);
    assert_eq!(party.field_leader, 1);
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 1);
    assert!(events.world.event_flags.contains(&123));
}

#[test]
fn scripted_enemy_reaction_returns_the_previous_mode_and_reads_its_current_value() {
    let main = script(&[
        (
            Call::SpawnEnemyActor,
            &[90, 0, 0, 50, 0, 0, 0, 2, 4, 42, 1, 0, 1, 1, 600, 0],
        ),
        (Call::SetActorProperty, &[90, 54, -1]),
        (Call::SetActorProperty, &[90, 56, 5]),
        (Call::SetActorProperty, &[90, 56, 13]),
    ]);
    let resources = enemy_resources();
    let child = script(&[(Call::GetActorProperty, &[90, 56])]);
    let mut events = runtime(program(&main, &child), resources, GameWorld::default());
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 5);
    assert_eq!(
        events.world.actors[&90]
            .enemy
            .as_ref()
            .unwrap()
            .stun_effect(),
        Some(effect::StunEffect::Lightning)
    );
    events.world.input_enabled = true;
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 13);
}

#[test]
fn patrol_properties_update_enemy_movement() {
    for (property, value, previous, stored) in [(23, 3, 2, 3), (26, 3, 1, 3), (27, 1, 2, 1)] {
        let main = script(&[
            (
                Call::SpawnEnemyActor,
                &[90, 0, 0, 50, 0, 0, 0, 2, 4, 42, 1, 1, 2, 0, 600, 0],
            ),
            (Call::SetActorProperty, &[90, property, value]),
        ]);
        let resources = enemy_resources();
        let query = script(&[(Call::GetActorProperty, &[90, property])]);
        let mut events = runtime(program(&main, &query), resources, GameWorld::default());
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), previous);
        let actor = events.world.actors.get_mut(&90).unwrap();
        match property {
            23 => assert_eq!(actor.enemy.as_ref().unwrap().normal_speed, 3.),
            26 => {
                assert_eq!(
                    actor.autonomy.as_ref().unwrap().behavior,
                    Behavior::FollowPath
                );
                actor.path.count = 1;
                actor.path.points[0] = [100., 0., 0.];
            }
            27 => assert_ne!(actor.enemy.as_ref().unwrap().random_turns, 0),
            _ => unreachable!(),
        }
        events.world.input_enabled = true;
        assert!(events.trigger(42, true).unwrap());
        events.step().unwrap();
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), stored);
        if property == 26 {
            steps(&mut events, 10);
            assert!(events.world.actors[&90].position[0] >= 70.);
        }
    }
}

#[test]
fn scenario_scripts_can_replace_a_projectile_stun_timer() {
    for (effect, duration) in [
        (effect::StunEffect::None, 240),
        (effect::StunEffect::Electric, 420),
        (effect::StunEffect::Lightning, 240),
        (effect::StunEffect::Ice, 240),
        (effect::StunEffect::Darkness, 240),
        (effect::StunEffect::TetheallaElectric, 300),
    ] {
        for replacement in [0, -1] {
            let main = script(&[
                (
                    Call::SpawnEnemyActor,
                    &[90, 0, 0, 50, 0, 0, 0, 2, 4, 42, 1, 0, 1, 1, 600, 0],
                ),
                (Call::SetActorProperty, &[90, 54, -1]),
            ]);
            let child = script(&[(Call::SetActorProperty, &[90, 54, replacement])]);
            let resources = enemy_resources();
            let mut events = runtime(program(&main, &child), resources, GameWorld::default());
            events.world.input_enabled = true;
            let enemy = events
                .world
                .actors
                .get_mut(&90)
                .unwrap()
                .enemy
                .as_mut()
                .unwrap();
            enemy.pause_ticks = duration;
            enemy.reaction = effect;
            events.step().unwrap();
            let enemy = events.world.actors[&90].enemy.as_ref().unwrap();
            let remaining = enemy.pause_ticks;
            assert!((duration - 1..=duration).contains(&remaining));
            assert!(events.trigger(42, true).unwrap());
            events.step().unwrap();
            assert_eq!(
                events.memory().read(0x20, Width::S32).unwrap(),
                i32::from(remaining - 1)
            );
            for _ in 0..duration {
                events.step().unwrap();
            }
            let enemy = events.world.actors[&90].enemy.as_ref().unwrap();
            assert_eq!(enemy.pause_ticks, replacement as i16);
            assert_eq!(enemy.stun_effect().is_some(), replacement != 0);
        }
    }
}

#[test]
fn enemy_pause_property_retains_negative_values_and_counts_down_positive_values() {
    for (value, expected) in [(-1, -1), (2, 2)] {
        let main = script(&[
            (
                Call::SpawnEnemyActor,
                &[90, 0, 0, 50, 0, 0, 0, 2, 4, 42, 1, 0, 1, 1, 600, 0],
            ),
            (Call::SetActorProperty, &[90, 54, value]),
            (Call::GetActorProperty, &[90, 54]),
        ]);
        let resources = enemy_resources();
        let mut world = GameWorld::default();
        world.input_enabled = true;
        let mut events = runtime(program_kind(&main, &[0x20ff], 0), resources, world);
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), expected);
        let initial = events.world.actors[&90].position;
        assert!(!events.contact_enemy(90).unwrap());
        for _ in 0..2 {
            events.step().unwrap();
            assert_eq!(events.world.actors[&90].position, initial);
        }
        if expected < 0 {
            steps(&mut events, 60);
            assert_eq!(events.world.actors[&90].position, initial);
            assert_eq!(
                i32::from(events.world.actors[&90].enemy.as_ref().unwrap().pause_ticks),
                -1
            );
            assert!(!events.contact_enemy(90).unwrap());
        } else {
            assert_eq!(
                events.world.actors[&90].enemy.as_ref().unwrap().pause_ticks,
                0
            );
            assert!(events.contact_enemy(90).unwrap());
        }
    }
}

#[test]
#[ignore = "requires locally cooked party definitions; no devices"]
fn colette_costume_change_returns_previous_value_and_survives_save() {
    let setup = script(&[(Call::SetCharacterCostume, &[2, 3])]);
    let events = party_runtime(&setup, &[0x20ff]);
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 0);
    let query = script(&[(Call::SetCharacterCostume, &[2, -1])]);
    let restored = reload(&events, &query, &[0x20ff]);
    assert_eq!(restored.memory().read(0x20, Width::S32).unwrap(), 3);
    assert_eq!(restored.world.party.as_ref().unwrap().members[1].costume, 3);
}

#[test]
#[ignore = "requires locally cooked party definitions; no devices"]
fn field_countdown_runs_during_pause_and_reentry_preserves_countdown_and_conditions() {
    const CONDITIONS: i32 = 100;
    const STATUS: i32 = 0x8000_0080u32 as i32;
    let setup = script(&[
        (Call::SetFieldCountdown, &[4]),
        (Call::SetRingTimer, &[9]),
        (Call::SetActorProperty, &[1, CONDITIONS, STATUS]),
        (Call::DisableMappedInput, &[]),
        (Call::GetFieldCountdown, &[]),
        (Call::YieldCommand, &[0, 2]),
        (Call::GetFieldCountdown, &[]),
    ]);
    let query = script(&[(Call::GetFieldCountdown, &[])]);
    let mut events = party_runtime(&setup, &query);
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 4);
    events.step().unwrap();
    events.step().unwrap();
    // Scripts read the countdown before the common frame's decrement.
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 3);
    let read_conditions = script(&[
        (Call::EnableMappedInput, &[]),
        (Call::GetActorProperty, &[1, CONDITIONS]),
    ]);
    let mut restored = reload(&events, &read_conditions, &query);
    assert_eq!(restored.memory().read(0x20, Width::S32).unwrap(), STATUS);
    assert_eq!(
        restored
            .world
            .party
            .as_ref()
            .unwrap()
            .travel
            .field_countdown,
        2
    );
    assert_eq!(restored.world.party.as_ref().unwrap().travel.ring_timer, 9);
    for _ in 0..3 {
        restored.step().unwrap();
    }
    assert!(restored.trigger(42, true).unwrap());
    restored.step().unwrap();
    assert_eq!(restored.memory().read(0x20, Width::S32).unwrap(), 0);
}

#[test]
fn movement_behavior_changes_resume_chasing_and_grab_queries_follow_live_blocks() {
    const FRAGMENT: i32 = 2;
    const BLOCK: i32 = 3101;
    const BEHAVIOR: i32 = 34;
    const GRABBED_BLOCK: i32 = 53;
    let setup = script(&[(
        Call::SetActorProperty,
        &[FRAGMENT, BEHAVIOR, Behavior::ChasePlayer as i32],
    )]);
    // This property always reads the controlled player's block, regardless of target.
    let query = script(&[(Call::GetActorProperty, &[FRAGMENT, GRABBED_BLOCK])]);
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    world.insert_actor(1, Actor::new(1, [0., -100., 0.]));
    let mut fragment = Actor::new(1, [100., 0., 0.]);
    fragment.autonomy = Some(Autonomy::new(Behavior::Stationary, 3., fragment.position));
    world.insert_actor(FRAGMENT, fragment);
    let mut block = Actor::new(1, [300., 0., 0.]);
    block.pushable = true;
    world.insert_actor(BLOCK, block);
    world.grabbed_block = Some(BLOCK);
    let mut events = runtime(program(&setup, &query), Default::default(), world);
    // Chasing includes short random idle intervals.
    steps(&mut events, 60);
    let position = events.world.actors[&FRAGMENT].position;
    assert!(position[0] < 100. && position[1] < 0., "{position:?}");
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), BLOCK);
    events.world.actors.remove(&BLOCK);
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 0);
}

fn finish_effects(events: &mut EventRuntime) {
    for _ in 0..600 {
        events.step().unwrap();
        if events.world.billboards.is_empty()
            && events.world.model_particles.is_empty()
            && events.world.refractions.is_empty()
        {
            return;
        }
    }
    panic!("effect did not expire");
}

#[test]
fn travelling_effect_arrives_and_leaves_a_finite_tail() {
    let resources = ResourceLibrary {
        bindings: [(100, (ResourceKind::Model, 100))].into(),
        models: [(100, model([], 1))].into(),
        ..Default::default()
    };
    let setup = script(&[(
        Call::CreateEffectEmitter,
        &[
            500, 0, 0, 0, 100, 46, 0, 30, 52, 75, 25, -35, 105, 0, 0, 0, 0, 0,
        ],
    )]);
    let remove = script(&[(Call::DespawnActor, &[500])]);
    let world = controlled_world();
    let mut events = runtime(program(&setup, &remove), resources, world);
    for _ in 0..30 {
        events.step().unwrap();
        if events.world.actors[&500].position == [105., 0., 0.] {
            break;
        }
    }
    assert_eq!(events.world.actors[&500].position, [105., 0., 0.]);
    assert!(!events.world.model_particles.is_empty());
    assert!(!events.world.billboards.is_empty());
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert!(!events.world.model_particles.is_empty());
    finish_effects(&mut events);
}

#[test]
fn staged_effects_release_then_expire() {
    for (arguments, release) in [
        (emitter(11, 0, &[0]), 2),
        (emitter(38, 10, &[101, 500, 8, 399, 75]), 2),
        (emitter(23, 0, &[48, 175, 3, 10, 0, 4]), 1),
    ] {
        let setup = script(&[(Call::CreateEffectEmitter, &arguments)]);
        let release = script(&[(Call::SetActorProperty, &[500, 33, release])]);
        let mut events = interactive_effect(&setup, &release);
        steps(&mut events, 2);
        assert!(!events.world.billboards.is_empty());
        assert!(events.trigger(42, true).unwrap());
        steps(&mut events, 10);
        assert!(!events.world.billboards.is_empty());
        finish_effects(&mut events);
    }
}

#[test]
fn emitter_removal_obeys_the_current_tail_policy() {
    for (initial, changed, survives) in [(0, 1, false), (1, 0, true)] {
        let setup = script(&[
            (
                Call::CreateEffectEmitter,
                &emitter(38, 10, &[101, 500, 8, 300, 75, 0, 0, 0, initial]),
            ),
            (Call::SetActorProperty, &[500, 33, 2]),
        ]);
        let remove = script(&[
            (Call::SetActorProperty, &[500, 121, changed]),
            (Call::DespawnActor, &[500]),
        ]);
        let mut events = interactive_effect(&setup, &remove);
        events.step().unwrap();
        assert!(!events.world.billboards.is_empty());
        assert!(events.trigger(42, true).unwrap());
        events.step().unwrap();
        steps(&mut events, 2);
        assert_eq!(!events.world.billboards.is_empty(), survives);
        assert_eq!(!events.world.refractions.is_empty(), survives);
        finish_effects(&mut events);
    }
}

#[test]
fn quake_finishes_and_stops_shaking() {
    let setup = script(&[(Call::CreateEffectEmitter, &emitter(22, 0, &[]))]);
    let query = script(&[(Call::GetActorProperty, &[500, 33])]);
    let mut events = interactive_effect(&setup, &query);
    let mut shook = false;
    for _ in 0..300 {
        events.step().unwrap();
        shook |= events
            .world
            .field_camera
            .as_ref()
            .is_some_and(|c| c.shake.offset != [0.; 2]);
    }
    assert!(shook);
    assert_eq!(
        events.world.field_camera.as_ref().unwrap().shake.offset,
        [0.; 2]
    );
    assert!(events.world.billboards.is_empty() && events.world.refractions.is_empty());
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 3);
}

#[test]
fn controlled_actor_wait_finishes_the_vertical_motion_before_resuming() {
    for wait in [4, 6] {
        let setup = script(&[
            (Call::MoveActor, &[CONTROLLED_ACTOR, 0, 0, -100, 20]),
            (Call::YieldCommand, &[wait, CONTROLLED_ACTOR]),
            (Call::SetEventBit, &[42]),
        ]);
        let mut world = GameWorld::default();
        world.controlled_actor = 7;
        let mut actor = Actor::new(7, [0.; 3]);
        actor.grounded = false;
        world.insert_actor(7, actor);
        let mut events = runtime(program(&setup, &[0x20ff]), Default::default(), world);
        for _ in 0..6 {
            events.step().unwrap();
            assert!(!events.world.event_flags.contains(&42));
        }
        events.step().unwrap();
        assert!(events.world.event_flags.contains(&42));
        assert_eq!(events.world.actors[&7].position, [0., 0., -100.]);
    }
}

#[test]
fn rheaird_crash_debris_moves_without_spinning_and_expires() {
    for kind in 52..=54 {
        let setup = script(&[(
            Call::CreateEffectObject,
            &[
                kind, 162, 4618, -950, 158, 50, -2, 100, 261, 25, 255, 0, 33, 0,
            ],
        )]);
        let mut events = runtime(
            program(&setup, &[0x20ff]),
            Default::default(),
            Default::default(),
        );
        let debris = events.world.billboards[&1].clone();
        let speed = debris.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((speed - 2.61).abs() < 0.0001);
        events.step().unwrap();
        assert_eq!(events.world.billboards[&1].position, debris.position);
        events.step().unwrap();
        let moved = &events.world.billboards[&1];
        assert!(moved.position[0] > debris.position[0]);
        assert!(moved.position[1] < debris.position[1]);
        assert!(moved.position[2] > debris.position[2]);
        assert_eq!(moved.rotation, [0.; 3]);
        assert_eq!(debris.alpha(debris.born), 255.);
        assert!(debris.alpha(debris.born + 162) < 255.);
        for _ in 2..164 {
            events.step().unwrap();
        }
        assert!(events.world.billboards.is_empty());
    }
}

#[test]
fn stopped_seal_emitter_leaves_a_finite_particle_tail() {
    let setup = script(&[(
        Call::CreateEffectEmitter,
        &emitter(13, 8, &[35, 35, 25, 10, 25, 600, 180]),
    )]);
    let stop = script(&[(Call::SetActorProperty, &[500, 33, 1])]);
    let mut events = interactive_effect(&setup, &stop);
    steps(&mut events, 30);
    assert!(!events.world.billboards.is_empty());
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    let stopped = events.tick();
    steps(&mut events, 30);
    assert!(events.world.billboards.values().all(|p| p.born <= stopped));
    events.world.actors.remove(&500);
    events.step().unwrap();
    assert!(!events.world.billboards.is_empty());
    steps(&mut events, 200);
    assert!(events.world.billboards.is_empty());
}

#[test]
fn angel_lights_converge_and_leave_no_permanent_trails() {
    let setup = script(&[(
        Call::CreateEffectEmitter,
        &emitter(38, 10, &[101, 500, 8, 300, 75]),
    )]);
    let mut events = interactive_effect(&setup, &[0x20ff]);
    events.step().unwrap();
    let (&id, light) = events.world.billboards.iter().next().unwrap();
    let distance = |p: [f32; 3]| p.iter().map(|v| v * v).sum::<f32>().sqrt();
    let start = distance(light.position);
    steps(&mut events, 20);
    assert!(distance(events.world.billboards[&id].position) < start * 0.7);
    steps(&mut events, 100);
    assert!(events.world.billboards.is_empty());
}

#[test]
fn seal_blessing_descends_then_releases_an_expanding_sphere() {
    let mut args = emitter(31, -2, &[35, 20, 300, 1, 1, 50, 50, 255, -15, 20]);
    args[1..4].copy_from_slice(&[200, 300, 100]);
    let release = script(&[(Call::SetActorProperty, &[500, 33, 2])]);
    let mut events = interactive_effect(&script(&[(Call::CreateEffectEmitter, &args)]), &release);
    steps(&mut events, 30);
    let center = events.world.actors[&500].position;
    assert_eq!(center, [200., 300., 52.]);
    assert!(events.world.billboards.is_empty());
    events.trigger(42, true).unwrap();
    steps(&mut events, 5);
    assert!(
        events
            .world
            .billboards
            .values()
            .all(|p| p.position == center && p.size[0] > 0.)
    );
    assert!(!events.world.billboards.is_empty());
    steps(&mut events, 30);
    assert!(events.world.billboards.is_empty());
}

#[test]
fn arrival_rays_converge_toward_the_flash() {
    let mut events = interactive_effect(
        &script(&[(
            Call::CreateEffectEmitter,
            &emitter(27, 0, &[33, 1500, -30, 1000, 150]),
        )]),
        &[0x20ff],
    );
    events.step().unwrap();
    let (&id, ray) = events
        .world
        .billboards
        .iter()
        .find(|(_, p)| p.recipe == resonance_content::effect::STREAK_SPRITE)
        .unwrap();
    let distance = |p: [f32; 3]| p.into_iter().map(|v| v * v).sum::<f32>().sqrt();
    let start = distance(ray.position);
    assert!(start > 50.);
    steps(&mut events, 30);
    assert!(distance(events.world.billboards[&id].position) < start * 0.6);
}

#[test]
fn angel_aura_stays_behind_the_character_when_the_camera_moves() {
    let setup = script(&[(
        Call::CreateEffectEmitter,
        &emitter(55, 0, &[98, 15, 15, 0, 50, 0, 25, 5]),
    )]);
    let mut events = interactive_effect(&setup, &[0x20ff]);
    let mut camera = camera::CameraRig::default();
    camera.cameras[0].follow = true;
    camera.cameras[0].axes = [false; 3];
    camera.cameras[0].actor = 500;
    events.world.field_camera = Some(camera);
    for eye in [[100., 0., 50.], [0., 100., 50.]] {
        events.world.field_camera.as_mut().unwrap().cameras[0].position = eye;
        events.step().unwrap();
        let expected = [-eye[0] / 100. * 15., -eye[1] / 100. * 15., 0.];
        assert_eq!(
            events
                .world
                .billboards
                .values()
                .map(|p| p.position)
                .collect::<Vec<_>>(),
            vec![expected]
        );
    }
}

#[test]
fn seal_releases_visible_particles_and_removal_cleans_them_up() {
    let setup = script(&[(
        Call::CreateEffectEmitter,
        &emitter(49, 0, &[73, 50, 5, 50, 25]),
    )]);
    let release = script(&[(Call::SetActorProperty, &[500, 33, 1])]);
    let mut events = interactive_effect(&setup, &release);
    events.step().unwrap();
    assert!(!events.world.billboards.is_empty());
    assert!(events.trigger(42, true).unwrap());
    steps(&mut events, 3);
    assert!(
        events
            .world
            .billboards
            .values()
            .any(|p| p.velocity != [0.; 3])
    );
    events.world.actors.remove(&500);
    events.step().unwrap();
    assert!(events.world.billboards.is_empty());
}

#[test]
fn particle_fade_decreases_to_zero_at_expiry() {
    let setup = script(&[
        (
            Call::CreateEffectObject,
            &[0, 6, 0, 0, 0, 0, 0, 0, 0, 10, 20, 0, 0, 0],
        ),
        (Call::SetEffectProperty, &[1, 146, 8]),
    ]);
    let mut events = runtime(
        program(&setup, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    let particle = events.world.billboards[&1].clone();
    assert_eq!(particle.alpha(particle.born), 20.);
    assert!(particle.alpha(particle.born + 3) < 20.);
    assert_eq!(particle.alpha(particle.born + particle.lifetime), 0.);
    steps(&mut events, particle.lifetime + 1);
    assert!(events.world.billboards.is_empty());
}

#[test]
fn atan2_consumes_both_coordinates_and_preserves_signed_quadrants() {
    for (y, x, expected) in [
        (0, 0, 0),
        (1, 0, 90),
        (0, -1, 180),
        (-3, -2, -123),
        (3, 2, 56),
    ] {
        let code = script(&[(Call::Atan2Degrees, &[y, x])]);
        let events = runtime(
            program(&code, &[0x20ff]),
            Default::default(),
            Default::default(),
        );
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), expected);
    }
}

#[test]
fn scaled_square_root_preserves_the_native_units_and_signed_domain() {
    for (sample, expected) in [(2250, 1500), (250_000, 15_811), (0, 0), (-1000, -1000)] {
        let code = script(&[(Call::ScaledSquareRoot, &[sample])]);
        let events = runtime(
            program(&code, &[0x20ff]),
            Default::default(),
            Default::default(),
        );
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), expected);
    }
}

#[test]
fn feedback_expires_on_the_simulation_clock_and_stops_on_scene_cancel() {
    let setup = script(&[
        (Call::ShakeCamera, &[10, 0, 2]),
        (Call::RumbleController, &[0, 2, 1]),
    ]);
    let mut events = runtime(
        program(&setup, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    assert_eq!(
        events.world.rumble.unwrap().remaining(events.tick()),
        Some(2)
    );
    events.step().unwrap();
    assert_eq!(
        events.world.rumble.unwrap().remaining(events.tick()),
        Some(1)
    );
    events.step().unwrap();
    assert_eq!(
        events.world.rumble.unwrap().remaining(events.tick()),
        Some(0)
    );
    events.step().unwrap();
    assert_eq!(
        events.world.field_camera.as_ref().unwrap().shake.offset,
        [0.; 2]
    );
    events.world.rumble = Some(rumble::Rumble::new(0, -1, true, events.tick()).unwrap());
    events.cancel();
    assert!(events.world.rumble.is_none());
}

#[test]
fn bone_scale_tweens_from_bind_pose_and_survives_other_controller_commands() {
    const ACTOR: i32 = 1;
    const MODEL: u32 = 9;
    let setup = script(&[
        (
            Call::ConfigureActorBoneScale,
            &[CONTROLLED_ACTOR, 2, 0, 0, 100, 200, 4],
        ),
        (
            Call::ConfigureActorBoneTranslation,
            &[ACTOR, 2, 0, 0, 50, 0, 1],
        ),
        (Call::ConfigureActorAttachment, &[ACTOR, 2, 0, 0, 0, 90, 1]),
    ]);
    let mut world = GameWorld::default();
    world.controlled_actor = ACTOR;
    world.insert_actor(ACTOR, Actor::new(MODEL, [0.; 3]));
    let resources = ResourceLibrary {
        models: [(
            MODEL,
            ModelResource {
                names: vec!["Seal".into()],
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let events = runtime(program(&setup, &[0x20ff]), resources, world);
    let adjustment = &events.world.actors[&ACTOR].appearance.bone_adjustments[&2];
    let scale = adjustment.scale.as_ref().unwrap();
    let bind = [2., 3., 4.];
    assert_eq!(scale.sample(1, bind), [1., 2., 3.]);
    assert_eq!(adjustment.translation(1), [0., 50., 0.]);
    let interrupted = BoneScale::new(Some(scale), [0.; 3], 2, 1);
    assert_eq!(interrupted.sample(1, bind), [0.5, 1., 1.5]);
    assert_eq!(interrupted.sample(2, bind), [0.; 3]);
    assert_eq!(scale.sample(3, bind), [0., 1., 2.]);
}

#[test]
fn bound_particles_move_grow_fade_and_expire() {
    const RECIPE: i32 = 10;
    const GROWTH: i32 = 135;
    let leaf = ParticleKind::Flutter(resonance_content::effect::FlutterRecipe {
        texture: "leaf.png".into(),
        uv: [0., 0., 1., 1.],
        aspect_ratio: 1.,
        palette: vec![[255; 4]],
        fall_speed: 2.,
        fall_variation: 0.,
        spin: 1.,
    });
    for kind in [ParticleKind::Glow, leaf] {
        let flutter = matches!(kind, ParticleKind::Flutter(_));
        let setup = script(&[
            (
                Call::CreateParticle,
                &[RECIPE, 4, 0, 0, 10, 1, 0, 0, 20, 100, -10, 0, 0],
            ),
            (Call::SetEffectProperty, &[1, GROWTH, 200]),
        ]);
        let resources = ResourceLibrary {
            particles: [(RECIPE, kind)].into(),
            ..Default::default()
        };
        let mut events = runtime(program(&setup, &[0x20ff]), resources, GameWorld::default());
        assert_eq!(events.world.particles.len(), 1);
        steps(&mut events, 2);
        let (position, size, rgba) = events.world.particles[0].sample(events.tick());
        if flutter {
            assert_eq!(position[2], 6.);
        } else {
            assert_eq!(position, [2., 0., 10.]);
        }
        assert_eq!(size, 24.);
        assert_eq!(rgba[3], 80.);
        steps(&mut events, 3);
        assert!(events.world.particles.is_empty());
    }
}

#[test]
fn model_particles_animate_and_expire_without_aliasing_actor_handles() {
    const MODEL: i32 = 9;
    const PERMANENT: i32 = i16::MAX as i32;
    const ANGULAR_Z: i32 = 431;
    const SCALE_X: i32 = 432;
    const VELOCITY_X: i32 = 423;
    let setup = script(&[
        (
            Call::CreateModelParticle,
            &[MODEL, PERMANENT, 0, 0, 0, 0, 0, 0, 100, 255, 0],
        ),
        (Call::SetModelParticleProperty, &[1, ANGULAR_Z, 40]),
        (Call::SetModelParticleProperty, &[1, SCALE_X, 150]),
        (
            Call::CreateModelParticle,
            &[MODEL, 2, 0, 0, 0, 0, 0, 0, 100, 255, -10],
        ),
        (Call::SetModelParticleProperty, &[2, VELOCITY_X, 100]),
        (Call::SetModelParticleProperty, &[2, 448, 2]), // Linear fade.
    ]);
    let mut world = GameWorld::default();
    world.insert_actor(1, Actor::new(MODEL as u32, [99.; 3]));
    let resources = ResourceLibrary {
        bindings: [(MODEL, (ResourceKind::Model, MODEL as u32))].into(),
        ..Default::default()
    };
    let mut events = runtime(program(&setup, &[0x20ff]), resources, world);
    events.step().unwrap();
    assert_eq!(events.world.model_particles[&2].position, [1., 0., 0.]);
    events.step().unwrap();
    assert_eq!(events.world.model_particles[&2].position, [2., 0., 0.]);
    assert_eq!(events.world.model_particles[&2].rgba[3], 255 - 2 * 10);
    events.step().unwrap();
    assert!(!events.world.model_particles.contains_key(&2));
    let permanent = &events.world.model_particles[&1];
    assert!((permanent.rotation[2] - 1.2).abs() < 0.0001);
    assert_eq!(permanent.scale, [1.5, 1., 1.]);
    assert_eq!(events.world.actors[&1].position, [99.; 3]);
}

#[test]
fn actor_sounds_attenuate_pan_and_ignore_removed_emitters() {
    let code = script(&[
        (Call::PlayActorSound, &[10, 154, 100, 1000]),
        (Call::PlayActorSound, &[11, 154, 100, 1000]),
        (Call::PlayActorSound, &[12, 154, 100, 1000]),
        (Call::PlayActorSound, &[999, 154, 100, 1000]),
    ]);
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    for (id, x) in [(1, 0.), (10, -500.), (11, 500.), (12, 2000.)] {
        world.insert_actor(id, Actor::new(1, [x, 0., 0.]));
    }
    let mut camera = camera::CameraRig::default();
    camera.position = [0., -1000., 0.];
    camera.target = [0.; 3];
    world.field_camera = Some(camera);
    let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
    let commands = &events.world.audio_commands;
    assert_eq!(commands.len(), 3);
    assert!(matches!(
        commands[0],
        AudioCommand::Sound {
            id: 154,
            volume: 50,
            pan: 0..64,
            slot: None
        }
    ));
    assert!(matches!(
        commands[1],
        AudioCommand::Sound {
            id: 154,
            volume: 50,
            pan: 65..=127,
            slot: None
        }
    ));
    assert!(matches!(commands[2], AudioCommand::Sound { volume: 0, .. }));
}

#[test]
fn scripted_sound_minus_one_releases_only_an_explicit_slot() {
    let code = script(&[
        (Call::PlaySound, &[164, 0, 255, 0]),
        (Call::PlaySound, &[-1, 0, 255, 0]),
        (Call::PlaySound, &[-1, 0, 0, 4]),
        (Call::PlaySound, &[-1, 0, 255, 255]),
        (Call::PlaySoundSimple, &[-1, 0]),
    ]);
    let events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    let commands = &events.world.audio_commands;
    assert_eq!(commands.len(), 5);
    assert!(matches!(
        commands[0],
        AudioCommand::Sound {
            id: 164,
            slot: Some(0),
            ..
        }
    ));
    assert!(matches!(commands[1], AudioCommand::StopSound(0)));
    assert!(matches!(commands[2], AudioCommand::StopSound(4)));
    for command in &commands[3..] {
        assert!(matches!(
            command,
            AudioCommand::Sound {
                id: -1,
                slot: None,
                ..
            }
        ));
    }
}

#[test]
#[ignore = "requires locally cooked party definitions; no devices"]
fn field_system_leader_commands_read_and_change_the_party_selection() {
    let session = cooked("session-data.json");
    let mut world = GameWorld::default();
    world.party = Some(party::Party::new(&session, Default::default()).unwrap());
    world.party.as_mut().unwrap().field_leader = 3;
    world.controlled_actor = 1000;
    for (command, expected, leader) in [(13, 3, 3), (14, 3, 2), (13, 2, 2)] {
        let code = script(&[(Call::Unknown92, &[command, 2])]);
        let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), expected);
        assert_eq!(events.world.party.as_ref().unwrap().field_leader, leader);
        assert_eq!(events.world.controlled_actor, 1000);
        world = events.world;
    }
}

#[test]
fn scene_script_keys_address_the_first_instance_and_despawn_every_copy() {
    let create = script(&[
        (Call::CreateSceneActor, &[1010, 10, 20, 30, 0, 24, 0, 0]),
        (Call::CreateSceneActor, &[1010, 40, 50, 60, 0, 24, 0, 0]),
        (Call::SetActorProperty, &[1010, 8, 80]),
    ]);
    let resources = ResourceLibrary {
        locators: [24].into(),
        ..Default::default()
    };
    let events = runtime(program(&create, &[0x20ff]), resources, Default::default());
    assert_eq!(events.world.actors.len(), 2);
    let first = &events.world.actors[&1010];
    assert_eq!(first.position, [10., 20., 30.]);
    assert_eq!(first.opacity, 80);
    let copy = events
        .world
        .actors
        .iter()
        .find(|(id, _)| **id != 1010)
        .unwrap()
        .1;
    assert_eq!(copy.position, [40., 50., 60.]);
    assert_ne!(first.instance, copy.instance);
    assert_eq!(copy.opacity, 255);
    let remove = script(&[(Call::DespawnActor, &[1010])]);
    let events = runtime(
        program(&remove, &[0x20ff]),
        Default::default(),
        events.world,
    );
    assert!(events.world.actors.is_empty());
}

#[test]
fn field_texture_clock_pauses_resumes_and_resets_through_native_commands() {
    let command = |world, selector, value| {
        runtime(
            program(
                &script(&[(Call::ConfigureRendering, &[selector, value])]),
                &[0x20ff],
            ),
            Default::default(),
            world,
        )
    };
    let mut events = command(GameWorld::default(), 128, 1);
    steps(&mut events, 7);
    assert_eq!(events.world.texture_animation_tick, 7);
    assert_eq!(events.world.texture_animation_effect_tick, 7);
    let mut events = command(events.world, 128, 0);
    steps(&mut events, 3);
    assert_eq!(events.world.texture_animation_tick, 7);
    assert_eq!(events.world.effect_tick, 10);
    assert_eq!(events.world.texture_animation_effect_tick, 7);
    let mut events = command(events.world, 128, 1);
    events.step().unwrap();
    assert_eq!(events.world.texture_animation_tick, 8);
    assert_eq!(events.world.texture_animation_effect_tick, 11);
    let mut events = command(events.world, 129, 0);
    assert_eq!(events.world.texture_animation_tick, 0);
    assert_eq!(events.world.texture_animation_effect_tick, 11);
    events.step().unwrap();
    assert_eq!(events.world.texture_animation_tick, 1);
    assert_eq!(events.world.texture_animation_effect_tick, 12);
}

#[test]
fn invisible_interaction_actors_remain_ring_targets_while_ordinary_locators_do_not() {
    let code = script(&[
        (Call::CreateSceneActor, &[1, 0, 0, 0, 0, 24, 0, 0]),
        (Call::SpawnInteractionActor, &[2, 0, 0, 0, 0, 24, 0, 0]),
        (Call::SpawnActor, &[3, 0, 0, 0, 0, 24, 0, 0]),
        (Call::SpawnActor, &[4, 0, 0, 0, 0, 24, 0, 0]),
        (Call::SetActorProperty, &[4, 48, 0]),
    ]);
    let resources = ResourceLibrary {
        locators: [24].into(),
        ..Default::default()
    };
    let events = runtime(program(&code, &[0x20ff]), resources, Default::default());
    let [ordinary, interaction] = [&events.world.actors[&1], &events.world.actors[&2]];
    assert!(!ordinary.visible && !interaction.visible);
    assert!(!ordinary.projectile_target());
    assert!(interaction.projectile_target());
    assert_eq!(interaction.role, ActorRole::Interaction);
    assert!(events.world.actors[&3].ring_contact_disabled);
    assert!(!events.world.actors[&3].projectile_target());
    assert!(events.world.actors[&4].projectile_target());
}

#[test]
fn mapped_buttons_preserve_edges_and_honor_the_native_pause_override() {
    use input::Button::{Menu, Ring};
    let mut input = input::Input::default();
    let read = |input, player, mode, paused| {
        let mut world = GameWorld::default();
        world.input = input;
        world.mapped_input_disabled = paused;
        let code = script(&[(Call::ReadMappedInput, &[player, mode])]);
        runtime(program(&code, &[0x20ff]), Default::default(), world)
            .memory()
            .read(0x20, Width::S32)
            .unwrap()
    };
    input.sample([Ring, Menu].into_iter().collect(), Default::default());
    assert_eq!(read(input, 1, 1, false), 0x0c00);
    assert_eq!(read(input, 1, 1, true), 0);
    assert_eq!(read(input, 1, 0x8001, true), 0x0c00);
    input.sample([Ring].into_iter().collect(), Default::default());
    assert_eq!(read(input, 1, 0, false), 0x0400);
    assert_eq!(read(input, 1, 1, false), 0);
    assert_eq!(read(input, 1, 2, false), 0x0800);
    assert_eq!(read(input, 2, 0, false), 0);
    assert_eq!(read(input, -1, 0, false), 0x0400);
    input.sample(Default::default(), Default::default());
    assert_eq!(read(input, 1, 2, false), 0x0400);
    // A tap latched between fixed updates survives an already-released button.
    input.sample(Default::default(), [Ring].into_iter().collect());
    assert_eq!(read(input, 1, 1, false), 0x0400);
}

#[test]
fn current_field_query_uses_the_owning_scene() {
    let code = script(&[(Call::GetCurrentField, &[])]);
    for map in [96, 219, 3000] {
        let mut world = GameWorld::default();
        world.current_field = Some(map);
        let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), map as i32);
    }
}

#[test]
fn scenery_can_be_paused_in_its_creation_update_and_resumed_later() {
    use animation::slot;
    let resources = ResourceLibrary {
        models: [(7, model([slot::IDLE], 120))].into(),
        bindings: [(7, (ResourceKind::Model, 7))].into(),
        ..Default::default()
    };
    let setup = script(&[
        (Call::CreateSceneActor, &[6010, 0, 0, 0, 0, 7, 0, 0]),
        (Call::SetActorAnimationFlags, &[6010, 2]),
    ]);
    let resume = script(&[(Call::SetActorAnimationFlags, &[6010, 0])]);
    let world = controlled_world();
    let mut events = runtime(program(&setup, &resume), resources, world);
    for _ in 0..300 {
        let animation = events.world.actors[&6010].animation.as_ref().unwrap();
        assert_eq!(animation.sample(events.tick(), 0, 120.), 0.);
        events.step().unwrap();
    }
    assert!(events.trigger(42, true).unwrap());
    steps(&mut events, 20);
    assert!(
        events.world.actors[&6010]
            .animation
            .as_ref()
            .unwrap()
            .sample(events.tick(), 0, 120.)
            > 0.
    );
}

#[test]
fn native_music_requests_decode_before_reaching_the_mixer() {
    let code = script(&[
        (Call::AudioCommand, &[10]),
        (Call::AudioCommand, &[-2]),
        (Call::AudioCommand, &[97]),
        (Call::AudioCommand, &[-3]),
        (Call::AudioCommand, &[-1]),
    ]);
    let events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    let commands: Vec<_> = events
        .world
        .audio_commands
        .iter()
        .map(|command| {
            let AudioCommand::Music(command) = command else {
                panic!("unexpected audio command")
            };
            *command
        })
        .collect();
    assert_eq!(
        commands,
        [
            MusicCommand::Play(10),
            MusicCommand::Suspend,
            MusicCommand::PlayJingle(97),
            MusicCommand::Resume,
            MusicCommand::Stop
        ]
    );
    assert!(MusicCommand::try_from(-4).is_err());
}

#[test]
fn missing_cooked_destination_is_owned_by_the_scene_loader() {
    let code = script(&[
        (Call::PreloadField, &[340]),
        (Call::ChangeField, &[340, 10, 20, 30, 90]),
    ]);
    let mut events = runtime(
        program(&code, &[0x20ff]),
        ResourceLibrary::default(),
        GameWorld::default(),
    );
    assert_eq!(events.world.preload_field, Some(340));
    let request = events.world.field_transition.as_ref().unwrap().clone();
    assert_eq!(request.map, 340);
    steps(&mut events, 3);
    assert!(request.operation.is_pending());
    assert!(!events.player_has_control());
}

#[test]
fn world_exits_decode_landmarks_separately_from_field_positions() {
    let code = script(&[(Call::ChangeField, &[3000, 257, 999, -888, 6])]);
    let resources = ResourceLibrary {
        fields: [3000].into(),
        ..Default::default()
    };
    let mut events = runtime(program(&code, &[0x20ff]), resources, GameWorld::default());
    assert!(events.world.field_transition.is_none());
    let request = events.world.world_transition.as_ref().unwrap().clone();
    assert_eq!((request.location, request.direction), (257, 6));
    assert!(request.operation.is_pending());
    assert!(!events.player_has_control());
    steps(&mut events, 3);
    assert!(request.operation.is_pending());
    events.cancel();
    assert!(!request.operation.is_pending());
    assert!(events.world.world_transition.is_none());
}

#[test]
fn landmark_entry_is_exclusive_and_supplies_the_native_direction_word() {
    let child = script(&[(Call::YieldCommand, &[0, 1])]);
    let world = controlled_world();
    let mut events = runtime(
        program_kind(&[0x20ff], &child, 1),
        ResourceLibrary::default(),
        world,
    );
    assert!(events.enter_landmark(42, 6).unwrap());
    assert!(!events.enter_landmark(42, 2).unwrap());
    assert_eq!(events.memory().read(0x24, Width::S32).unwrap(), 6);
    assert!(events.enter_landmark(42, 8).is_err());
    events.step().unwrap();
    events.step().unwrap();
    assert!(events.player_has_control());
    assert!(events.enter_landmark(42, 2).unwrap());
    assert_eq!(events.memory().read(0x24, Width::S32).unwrap(), 2);
}

#[test]
fn world_entrance_preserves_fog_and_fixed_destination_camera_coordinates() {
    let code = script(&[
        (Call::SelectCamera, &[-1]),
        (
            Call::ConfigureCameraParameters,
            &[43, -2852, 434, -53, -1006, 141, 1, 1],
        ),
        (Call::ConfigureCameraParameters, &[0; 8]),
        (
            Call::ConfigureCameraAuxiliary,
            &[1, 1000, 14000, 150, 130, 130],
        ),
        (Call::ChangeField, &[29, -360, -424, 11, 0]),
    ]);
    let events = runtime(
        program(&code, &[0x20ff]),
        ResourceLibrary {
            fields: [29].into(),
            ..Default::default()
        },
        GameWorld::default(),
    );
    let camera = &events
        .world
        .field_transition
        .as_ref()
        .unwrap()
        .camera
        .as_ref()
        .unwrap()
        .camera;
    assert_eq!(camera.position_bounds, [[43.; 2], [-2852.; 2], [434.; 2]]);
    assert_eq!(camera.target_bounds, [[-53.; 2], [-1006.; 2], [141.; 2]]);
    assert_eq!(
        camera.fog,
        Some(camera::Fog {
            start: 1000.,
            end: 14000.,
            color: [150, 130, 130]
        })
    );
    let current = events.world.field_camera.as_ref().unwrap().current();
    assert!(current.fog.is_none());
    assert_eq!(current.position_bounds, [[-100000., 100000.]; 3]);
}
fn program(main: &[u16], child: &[u16]) -> Arc<Program> {
    program_kind(main, child, 2)
}
fn program_kind(main: &[u16], child: &[u16], kind: u16) -> Arc<Program> {
    program_record(main, child, kind, 42)
}
fn program_record(main: &[u16], child: &[u16], kind: u16, key: u32) -> Arc<Program> {
    let mut words = vec![
        10,
        0,
        0,
        1,
        0,
        kind,
        (key >> 16) as u16,
        key as u16,
        0,
        main.len() as u16,
    ];
    words.extend(main);
    words.extend(child);
    Arc::new(
        Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    )
}
fn runtime(program: Arc<Program>, resources: ResourceLibrary, world: GameWorld) -> EventRuntime {
    EventRuntime::with_state(program, Arc::new(resources), world, Default::default()).unwrap()
}
fn cooked<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
        });
    serde_json::from_slice(&std::fs::read(root.join("game").join(name)).unwrap()).unwrap()
}
fn party_runtime(main: &[u16], child: &[u16]) -> EventRuntime {
    let data = Arc::new(cooked("session-data.json"));
    let mut world = GameWorld::default();
    world.party = Some(party::Party::new(&data, Default::default()).unwrap());
    runtime(
        program(main, child),
        ResourceLibrary {
            session_data: Some(data),
            ..Default::default()
        },
        world,
    )
}
fn reload(events: &EventRuntime, main: &[u16], child: &[u16]) -> EventRuntime {
    let resources = Arc::new(ResourceLibrary {
        session_data: events.resources().session_data.clone(),
        ..Default::default()
    });
    let saved: SavedProgress =
        serde_json::from_slice(&serde_json::to_vec(&events.save_progress().unwrap()).unwrap())
            .unwrap();
    let (world, memory) = saved
        .into_state(resources.session_data.as_ref().unwrap())
        .unwrap()
        .into_world();
    EventRuntime::with_state(program(main, child), resources, world, memory).unwrap()
}
fn model(slots: impl IntoIterator<Item = u16>, duration_ticks: u32) -> ModelResource {
    ModelResource {
        clips: slots
            .into_iter()
            .map(|slot| {
                (
                    slot,
                    AnimationClip {
                        duration_ticks,
                        attachments: Default::default(),
                    },
                )
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn wing_profiles_follow_their_models_and_keep_instance_state_isolated() {
    use resonance_content::animation::{Bone, Skeleton, Transform, TransformChannels};
    let resource = 10;
    for id in [20, 30] {
        let wings = ModelResource {
            names: vec!["tip".into()],
            attachments: ModelAttachments {
                skeleton: Some(Arc::new(Skeleton {
                    bones: vec![Bone {
                        name: "tip".into(),
                        parent: None,
                        bind_channels: TransformChannels(0),
                        bind: Transform {
                            translation: [100., 0., 0.],
                            ..Default::default()
                        },
                    }],
                })),
                ..Default::default()
            },
            ..Default::default()
        };
        let resources = ResourceLibrary {
            models: [(resource, wings)].into(),
            ..Default::default()
        };
        let mut world = GameWorld::default();
        let mut wing = Actor::new(resource, [200., 0., 0.]);
        wing.set_wings(WingStyle::Layered);
        world.insert_actor(id, wing);
        let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
        steps(&mut events, 4);
        assert_eq!(events.world.billboards.len(), 1);
        let spark = events.world.billboards.values().next().unwrap();
        assert!((285. ..=316.).contains(&spark.position[0]));
        assert!((-15. ..=16.).contains(&spark.position[1]));
        events
            .world
            .actors
            .get_mut(&id)
            .unwrap()
            .appearance
            .model_hidden = true;
        steps(&mut events, 4);
        assert_eq!(events.world.billboards.len(), 1);

        events
            .world
            .insert_actor(2, Actor::new(resource, [1000., 0., 0.]));
        let wing = events.world.actors.get_mut(&id).unwrap();
        wing.appearance.model_hidden = false;
        wing.attachment = Some(resonance_events::Attachment {
            actor: 2,
            bone: "tip".into(),
        });
        events.step().unwrap();
        events.world.actors.remove(&2);
        steps(&mut events, 4);
        let spark = events.world.billboards.values().last().unwrap();
        assert!((1385. ..=1416.).contains(&spark.position[0]));

        // A replacement wing must not inherit the old instance's attachment frame.
        let mut replacement = Actor::new(resource, [200., 0., 0.]);
        replacement.set_wings(WingStyle::Layered);
        replacement.attachment = Some(resonance_events::Attachment {
            actor: 2,
            bone: "tip".into(),
        });
        events.world.insert_actor(id, replacement);
        assert!(events.step().is_err());
    }
    let mut world = GameWorld::default();
    let mut wing = Actor::new(resource, [200., 0., 0.]);
    wing.set_wings(WingStyle::Echo);
    world.insert_actor(40, wing);
    let mut events = runtime(
        program(&[0x20ff], &[0x20ff]),
        ResourceLibrary::default(),
        world,
    );
    steps(&mut events, 8);
    events.world.actors.get_mut(&40).unwrap().position[0] = 400.;
    steps(&mut events, 8);
    let wings = events.world.actors[&40].wings.as_ref().unwrap();
    let echoes = [0, 1].map(|pass| wings.layer(pass, events.tick()).echo.unwrap());
    assert_eq!(echoes.map(|echo| echo.position[0]), [200., 400.]);
    assert_eq!(echoes[0].rgba(events.tick()), [32, 32, 64, 255]);
}

#[test]
fn sparse_attachments_emit_each_tick_with_affine_parents_fractional_rate_and_pose_delay() {
    use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
    let skeleton = Arc::new(Skeleton {
        bones: [("parent", None), ("attachment", Some(0))]
            .into_iter()
            .map(|(name, parent)| Bone {
                name: name.into(),
                parent,
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            })
            .collect(),
    });
    let motion: Motion = serde_json::from_value(serde_json::json!({
        "duration_frames":2., "tracks":[
            {"bone":0,"bind_channels":0,"period_frames":2.,"times":[0.],
             "matrices":[[2.,0.5,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.]]},
            {"bone":1,"bind_channels":0,"period_frames":2.,"times":[0.,2.],
             "translation":{"interpolation":"linear","values":[[0.,1.,3.],[8.,1.,3.]]}}
        ]
    }))
    .unwrap();
    let motion = Arc::new(Motion::decode(&motion.encode().unwrap()).unwrap());
    let mut model = model([12], 4);
    model.names = vec!["parent".into(), "attachment".into()];
    model.attachment_pose_delay = 1;
    model.clips.get_mut(&12).unwrap().attachments =
        Some(AttachmentPose::new(skeleton, motion).unwrap());
    let resources = ResourceLibrary {
        models: [(1, model)].into(),
        ..Default::default()
    };
    let mut actor = Actor::new(1, [1.9, -1.9, 0.]);
    let mut animation = Animation::new(1, 12, 4, 0);
    animation.rate = 0.5;
    actor.animation = Some(animation);
    actor.scripted_animation = true;
    let mut world = GameWorld::default();
    world.actors.insert(1, actor);
    let mut code = Vec::new();
    for query in [[1, 1]; 5].into_iter().chain([[1, -1], [2, 0]]) {
        native(&mut code, Call::ReadActorAttachment, &query);
        for value in [10, 20] {
            arg(&mut code, value);
        }
        for axis in 0..3 {
            native(&mut code, Call::ReadCoordinateRegister, &[axis]);
            code.extend([0x3000, 0x4000]);
        }
        for value in [0, 0, 0, 25, 255, 0, 0, 0] {
            arg(&mut code, value);
        }
        code.extend([0x2000 | Call::CreateParticle as u16, 0x3000]);
        native(&mut code, Call::YieldCommand, &[0, 1]);
    }
    code.push(0x20ff);
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    // Startup and catch-up run the VM independently of rendered frames.
    steps(&mut events, 6);
    assert_eq!(
        events
            .world
            .billboards
            .values()
            .map(|p| (p.born, p.position))
            .collect::<Vec<_>>(),
        [
            (1, [2., 0., 3.]),
            (2, [2., 0., 3.]),
            (3, [4., 0., 3.]),
            (4, [6., 0., 3.]),
            (5, [8., 0., 3.]),
            (6, [8., 0., 3.]), // An invalidated node preserves the coordinate registers.
            (7, [0.; 3]),      // A missing actor resets them, as in Iselia's locator setup.
        ]
    );
}

#[test]
fn explicit_party_selection_recreates_the_actor_but_alias_and_query_preserve_it() {
    for selection in [-1, CONTROLLED_ACTOR, 1, 10] {
        let mut resources = ResourceLibrary::default();
        let mut member = model([animation::slot::IDLE], 58);
        member.hidden_nodes.insert(7);
        resources.models.insert(1, member);
        let mut world = GameWorld::default();
        world.controlled_actor = 1;
        let mut actor = Actor::new(1, [2., 3., 4.]);
        actor.face(90.);
        actor.target_heading = 140.;
        actor.depth_write = false;
        actor.appearance.expression = 3;
        actor.appearance.eyes = Some(EyeBlink { frame: 2, tick: 10 });
        world.insert_actor(1, actor);
        let instance = world.actors[&1].instance;
        let code = script(&[(Call::SelectPartyMember, &[selection])]);
        let events = runtime(program(&code, &[0x20ff]), resources, world);
        let actor = &events.world.actors[&1];
        assert_eq!(actor.position, [2., 3., 4.]);
        assert_eq!((actor.heading, actor.target_heading), (90., 140.));
        if selection > 0 && selection != CONTROLLED_ACTOR {
            assert_ne!(actor.instance, instance);
            assert!(actor.depth_write);
            assert_eq!(actor.appearance.expression, 0);
            assert!(actor.appearance.eyes.is_none());
            assert!(actor.appearance.hidden_nodes.contains(&7));
        } else {
            assert_eq!(actor.instance, instance);
            assert!(!actor.depth_write);
            assert_eq!(actor.appearance.eyes.unwrap().tick, 10);
        }
    }
}

#[test]
fn recreated_player_keeps_the_default_pose_until_its_idle_handler_runs() {
    use animation::slot;
    let resources = ResourceLibrary {
        models: [(1, model([slot::IDLE, slot::EVENT_IDLE], 100))].into(),
        ..Default::default()
    };
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.insert_actor(1, Actor::new(1, [0.; 3]));
    let code = script(&[(Call::SelectPartyMember, &[1])]);
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    assert_eq!(
        events.world.actors[&1].animation.as_ref().unwrap().slot,
        slot::IDLE
    );
    steps(&mut events, 20);
    let actor = &events.world.actors[&1];
    let animation = actor.animation.as_ref().unwrap();
    assert_eq!(animation.slot, slot::EVENT_IDLE);
    assert!(animation.sample(events.tick(), 0, 100.) > 0.);
    assert!(!actor.scripted_animation);
}

#[test]
fn confirmed_polygons_preserve_their_coordinates_and_transition_hints() {
    let code = script(&[
        (
            Call::CreateConfirmedTriangleTrigger,
            &[42, 18, 0, 243, 0, 0, 10, 100, 0, 20, 0, 100, 30, 200],
        ),
        (
            Call::CreateConfirmedAreaTrigger,
            &[
                43, 19, 1, 247, 0, 0, 0, 100, 0, 0, 100, 100, 0, 0, 100, 0, 300,
            ],
        ),
    ]);
    let events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    let triggers = &events.world.triggers;
    assert!(matches!(
        triggers[0].shape,
        TriggerShape::Triangle([[0., 0., 10.], [100., 0., 20.], [0., 100., 30.]])
    ));
    assert_eq!(triggers[0].transition, Some([18, 0, 243]));
    assert_eq!(triggers[0].height, 200.);
    assert!(matches!(triggers[1].shape, TriggerShape::Quad(_)));
    assert_eq!(triggers[1].transition, Some([19, 1, 247]));
    assert_eq!(triggers[1].height, 300.);
}

#[test]
fn touch_metadata_updates_the_first_touch_shape_without_changing_activation() {
    let code = script(&[
        (
            Call::CreateScriptRecordVariant,
            &[42, 18, 0, 330, 0, 0, 0, 10, 10, 0, 200],
        ),
        (
            Call::CreateAreaTrigger,
            &[42, 0, 0, 0, 10, 0, 0, 10, 10, 0, 0, 10, 0, 200],
        ),
        (Call::CreateScriptRecord, &[42, 0, 0, 0, 10, 10, 0, 200]),
        (Call::CreateScriptRecord, &[43, 0, 0, 0, 10, 10, 0, 200]),
        (Call::SetTouchTriggerMetadata, &[42, 0x10002, -1, -2]),
        (Call::SetTouchTriggerMetadata, &[43, 0, 1, 337]),
        (Call::SetTouchTriggerMetadata, &[99, 1, 2, 3]),
    ]);
    let events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    let triggers = &events.world.triggers;
    assert_eq!(triggers[0].transition, Some([18, 0, 330]));
    assert_eq!(triggers[0].touch_metadata, [0; 3]);
    assert_eq!(triggers[1].touch_metadata, [2, 65535, u32::MAX - 1]);
    assert_eq!(triggers[2].touch_metadata, [0; 3]);
    assert_eq!(triggers[3].touch_metadata, [0, 1, 337]);
    assert!(
        triggers[1..]
            .iter()
            .all(|trigger| trigger.transition.is_none())
    );
}

#[test]
fn emotes_resolve_the_current_party_leader_alias() {
    let code = script(&[(
        Call::SpawnActor,
        &[-100, 0, 0, 0, 10, resonance_events::CONTROLLED_ACTOR, 0, 30],
    )]);
    let mut world = GameWorld::default();
    world.controlled_actor = 7;
    world.insert_actor(7, Actor::new(7, [0.; 3]));
    let mut events = runtime(program(&code, &[0x20ff]), Default::default(), world);
    assert_eq!(events.world.emotes[&-100].actor, 7);
    steps(&mut events, 30);
    assert!(events.world.emotes.contains_key(&-100));
    events.step().unwrap();
    assert!(!events.world.emotes.contains_key(&-100));
}

#[test]
fn wandering_pauses_for_conversation_and_yields_to_scripted_motion() {
    use animation::slot;
    let setup = script(&[(Call::SpawnActor, &[42, 100, 200, 0, 0, 5, 2, 3])]);
    let resources = ResourceLibrary {
        bindings: [(5, (ResourceKind::Model, 5))].into(),
        models: [(5, model([slot::IDLE, slot::WALK], 100))].into(),
        ..Default::default()
    };
    let mut world = controlled_world();
    world.field_camera = Some(Default::default());
    let conversation = script(&[(Call::YieldCommand, &[0, 60])]);
    let mut events = runtime(program_kind(&setup, &conversation, 0), resources, world);
    for _ in 0..360 {
        events.step().unwrap();
        if events.world.actors[&42].position != [100., 200., 0.] {
            break;
        }
    }
    let position = events.world.actors[&42].position;
    assert_ne!(position, [100., 200., 0.]);
    events.world.input_enabled = false;
    steps(&mut events, 20);
    assert_eq!(events.world.actors[&42].position, position);
    events.world.input_enabled = true;
    assert!(events.interact(42).unwrap());
    events
        .world
        .actors
        .get_mut(&42)
        .unwrap()
        .autonomy
        .as_mut()
        .unwrap()
        .begin_conversation();
    steps(&mut events, 30);
    assert_eq!(events.world.actors[&42].position, position);
    // Scripted destinations remain usable while the conversation owns input.
    let destination = [position[0] + 100., position[1], position[2]];
    events.world.actors.get_mut(&42).unwrap().motion = Some(ActorMotion {
        target: destination,
        speed: 20.,
    });
    steps(&mut events, 6);
    assert_eq!(events.world.actors[&42].position, destination);
    for _ in 0..360 {
        events.step().unwrap();
        if events.player_has_control() && events.world.actors[&42].position != destination {
            break;
        }
    }
    assert!(events.player_has_control());
    assert_ne!(events.world.actors[&42].position, destination);
}

#[test]
fn item_notice_reusing_a_choice_window_does_not_inherit_its_cursor() {
    let mut world = GameWorld::default();
    let message = |text: &str| dialogue::ResolvedMessage {
        tokens: vec![dialogue::TextToken::Text { text: text.into() }],
    };
    let (notice, _) = world.show_choice_notice(message("Yes\nNo"), 0, 1).unwrap();
    world.choices[&0]
        .finish(dialogue::ChoiceExit::Confirm)
        .unwrap();
    notice.complete(None).unwrap();
    let received = world
        .show_notice(message("Received Apple Gel."), dialogue::flags::INSTANT)
        .unwrap();
    assert!(
        world.choices.is_empty(),
        "the preceding selection cursor leaked into the item notice"
    );
    assert!(received.is_pending());
    received.complete(None).unwrap();
    world.show_choice_notice(message("Yes\nNo"), 0, 1).unwrap();
    assert!(world.choices[&0].operation.is_pending());
}

#[test]
fn colette_has_red_eyes_only_during_the_soulless_story_stage() {
    for progress in [999, 1000, 1999, 2000] {
        let mut world = GameWorld::default();
        // The story override follows Colette even when a scene gives her another actor ID.
        world.insert_actor(2002, Actor::new(2, [0.; 3]));
        world.insert_actor(2001, Actor::new(1, [0.; 3]));
        let resources = ResourceLibrary {
            models: [1, 2]
                .map(|id| {
                    (
                        id,
                        ModelResource {
                            has_eyes: true,
                            ..Default::default()
                        },
                    )
                })
                .into(),
            ..Default::default()
        };
        let setup = script(&[
            (Call::SetActorFace, &[2002, 6]),
            (Call::SetActorFace, &[2001, 6]),
        ]);
        let mut memory = symphonia_script_vm::Memory::default();
        memory.write(0x4c, Width::S32, progress).unwrap();
        let mut events = EventRuntime::with_state(
            program(&setup, &[0x20ff]),
            Arc::new(resources),
            world,
            memory,
        )
        .unwrap();
        for _ in 0..10 {
            events.step().unwrap();
            let expected = if (1000..2000).contains(&progress) {
                15
            } else {
                4
            };
            assert!(
                matches!(events.world.actors[&2002].appearance.face, Face::Frame(frame) if frame == expected)
            );
            assert!(matches!(
                events.world.actors[&2001].appearance.face,
                Face::Frame(4)
            ));
        }
    }
}

#[test]
fn scripted_eye_modes_enable_blinking_or_a_fixed_expression() {
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.actors.insert(1, Actor::new(1, [0.; 3]));
    for mode in [1, 0, 6, 1, 1] {
        let code = script(&[(Call::SetActorFace, &[999_999, mode])]);
        let resources = ResourceLibrary {
            blink: Some(resonance_content::effect::BlinkCycle {
                frames: vec![0, 1, 2, 0, 0],
            }),
            models: [(
                1,
                ModelResource {
                    has_eyes: true,
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        };
        let mut events = runtime(program(&code, &[0x20ff]), resources, world);
        assert!(events.world.actors[&1].appearance.eyes.is_none());
        let mut frames = Vec::new();
        for _ in 0..10 {
            events.step().unwrap();
            let face = &events.world.actors[&1].appearance;
            if mode == 1 {
                frames.push(face.eyes.unwrap().frame);
            } else {
                assert!(face.eyes.is_none());
                assert!(matches!(
                    (mode, face.face),
                    (0, Face::Disabled) | (6, Face::Frame(4))
                ));
            }
        }
        if mode == 1 {
            assert!(frames.contains(&1) && frames.contains(&2));
        }
        world = events.world;
    }
}
#[test]
fn camera_target_accepts_the_controlled_actor_alias() {
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.field_camera = Some(Default::default());
    let camera = world.field_camera.as_mut().unwrap().current_mut();
    camera.position_bounds = [[-50., 50.]; 3];
    camera.target_bounds = [[-75., 75.]; 3];
    world.actors.insert(1, Actor::new(1, [0.; 3]));
    world.actors.insert(20, Actor::new(20, [100.; 3]));
    let code = script(&[
        (Call::SelectActor, &[20]),
        (Call::SelectActor, &[999_999]),
        (Call::ResetCameraBounds, &[]),
    ]);
    let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
    assert_eq!(
        events.world.field_camera.as_ref().unwrap().current().actor,
        1
    );
    let camera = events.world.field_camera.as_ref().unwrap().current();
    assert_eq!(camera.position_bounds, [[-100000., 100000.]; 3]);
    assert_eq!(camera.target_bounds, camera.position_bounds);
}

#[test]
fn location_caption_expires_without_retaining_an_unrenderable_actor() {
    let mut code = Vec::new();
    let id = 999_989;
    native(
        &mut code,
        Call::CreateOverlay,
        &[
            id, -1179647, 320, 240, -1, -1, 0, 255, 255, 255, 255, 1, 190,
        ],
    );
    code.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources
        .bindings
        .insert(-1179647, (ResourceKind::Overlay, 77));
    let mut events = runtime(program(&code, &[0x20ff]), resources, Default::default());
    for tick in 1..254 {
        events.step().unwrap();
        let overlay = &events.world.overlays[&id];
        assert_eq!(
            overlay.alpha(tick),
            if tick <= 190 {
                255
            } else {
                (255 - (tick - 190) * 4) as u8
            }
        );
    }
    events.step().unwrap();
    assert!(events.world.overlays.is_empty());
    assert!(!events.world.actors.contains_key(&id));
}

#[test]
fn free_control_allows_the_field_supervisor_and_ambient_scripts() {
    let main = script(&[(Call::SpawnEvent, &[42]), (Call::YieldCommand, &[0, 3])]);
    let child = script(&[(Call::YieldCommand, &[0, 10])]);
    let world = controlled_world();
    let mut events = runtime(program(&main, &child), Default::default(), world);
    assert!(events.player_has_control());
    steps(&mut events, 4);
    assert_eq!(events.active_instances(), 1);
    assert!(events.player_has_control());
    events.world.input_enabled = false;
    assert!(!events.player_has_control());
}

#[test]
fn background_event_controls_suspend_its_wait_without_stopping_the_foreground() {
    for (pause, resume) in [(1, 0), (51, 50)] {
        let main = script(&[
            (Call::SpawnEvent, &[42]),
            (Call::YieldCommand, &[0, 1]),
            (Call::ControlEvent, &[2, pause]),
            (Call::YieldCommand, &[0, 3]),
            (Call::ControlEvent, &[2, resume]),
        ]);
        let child = script(&[(Call::YieldCommand, &[0, 4]), (Call::SetEventBit, &[42])]);
        let mut events = runtime(
            program(&main, &child),
            Default::default(),
            Default::default(),
        );
        for _ in 1..7 {
            events.step().unwrap();
            assert!(!events.world.event_flags.contains(&42));
        }
        events.step().unwrap();
        assert!(events.world.event_flags.contains(&42));
        assert_eq!(events.active_instances(), 0);
    }
}

#[test]
fn line_events_use_their_registry_and_finish_once() {
    for confirmed in [false, true] {
        let child = script(&[
            (Call::DisableMappedInput, &[]),
            (Call::YieldCommand, &[0, 3]),
            (Call::EnableMappedInput, &[]),
        ]);
        let mut world = GameWorld::default();
        world.input_enabled = true;
        let mut events = runtime(
            program_kind(&[0x20ff], &child, if confirmed { 2 } else { 1 }),
            Default::default(),
            world,
        );
        assert!(!events.trigger(42, !confirmed).unwrap());
        assert!(events.trigger(42, confirmed).unwrap());
        assert!(!events.trigger(42, confirmed).unwrap());
        assert!(events.world.input_enabled);
        assert!(!events.player_has_control());
        events.step().unwrap();
        assert!(!events.world.input_enabled);
        steps(&mut events, 4);
        assert!(events.world.input_enabled);
        assert_eq!(events.active_instances(), 0);
        assert!(events.player_has_control());
    }
}
#[test]
fn a_guarded_line_event_does_not_take_control_or_restart_walking() {
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    let mut actor = Actor::new(1, [0.; 3]);
    actor.motion = Some(ActorMotion {
        target: [100., 0., 0.],
        speed: 4.,
    });
    world.actors.insert(1, actor);
    let mut events = runtime(
        program_kind(&[0x20ff], &[0x20ff], 1),
        Default::default(),
        world,
    );
    assert!(events.trigger(42, false).unwrap());
    assert!(events.world.input_enabled);
    assert!(events.world.actors[&1].motion.is_some());
    events.step().unwrap();
    assert_eq!(events.world.actors[&1].position, [4., 0., 0.]);
    assert!(events.world.input_enabled);
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn locomotion_changes_gait_and_preserves_phase_when_speed_changes() {
    let mut resources = ResourceLibrary::default();
    resources.models.insert(1, model([12, 36, 40, 120], 80));
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    let mut actor = Actor::new(1, [0.; 3]);
    actor.motion = Some(ActorMotion {
        target: [10000., 0., 0.],
        speed: 4.,
    });
    world.actors.insert(1, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
    steps(&mut events, 10);
    let walk = events.world.actors[&1].animation.as_ref().unwrap();
    assert_eq!(walk.slot, animation::slot::WALK);
    assert!(walk.sample(events.tick(), 0, 80.) > 0.);
    let tick = events.tick();
    let previous = events.world.actors[&1]
        .animation
        .as_ref()
        .unwrap()
        .sample(tick + 1, 0, 80.);
    events
        .world
        .actors
        .get_mut(&1)
        .unwrap()
        .motion
        .as_mut()
        .unwrap()
        .speed = 2.;
    events.step().unwrap();
    let a = events.world.actors[&1].animation.as_ref().unwrap();
    assert_eq!(a.start_tick, 1);
    assert_eq!(a.rate, 1.);
    assert_eq!(a.sample(events.tick(), 0, 80.), previous);
    events
        .world
        .actors
        .get_mut(&1)
        .unwrap()
        .motion
        .as_mut()
        .unwrap()
        .speed = 8.;
    events.step().unwrap();
    let a = events.world.actors[&1].animation.as_ref().unwrap();
    assert_eq!((a.slot, a.rate, a.blend_ticks), (40, 0.8, 2));
    events.world.input_enabled = false;
    events
        .world
        .actors
        .get_mut(&1)
        .unwrap()
        .motion
        .as_mut()
        .unwrap()
        .speed = 6.;
    let mut npc = Actor::new(1, [0.; 3]);
    npc.motion = events.world.actors[&1].motion.clone();
    events.world.actors.insert(2, npc);
    events.step().unwrap();
    for (id, slot) in [(1, 120), (2, 36)] {
        let a = events.world.actors[&id].animation.as_ref().unwrap();
        assert_eq!((a.slot, a.rate, a.blend_ticks), (slot, 1., 8));
    }
    let actor = events.world.actors.get_mut(&1).unwrap();
    actor.motion.as_mut().unwrap().target = actor.position;
    events.step().unwrap();
    let actor = &events.world.actors[&1];
    assert!(actor.motion.is_none());
    assert_eq!(actor.animation.as_ref().unwrap().slot, 120);
    events.step().unwrap();
    assert_eq!(events.world.actors[&1].animation.as_ref().unwrap().slot, 12);
}

#[test]
fn walking_faces_its_destination_without_changing_the_path() {
    let start = [0.; 3];
    let target: [f32; 3] = [25., 100., 0.];
    let mut actor = Actor::new(4, start);
    actor.face(180.);
    actor.motion = Some(ActorMotion { target, speed: 2. });
    let mut world = GameWorld::default();
    world.actors.insert(4, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), Default::default(), world);
    steps(&mut events, 10);
    let actor = &events.world.actors[&4];
    let desired = target[0].atan2(-target[1]).to_degrees().rem_euclid(360.);
    assert!((actor.heading - desired).abs() < 0.001);
    assert!(actor.position[0].hypot(actor.position[1]) > 0.);
    assert!((actor.position[0] * target[1] - actor.position[1] * target[0]).abs() < 0.001);
}

#[test]
fn dialogue_waits_for_the_speaker_to_turn_and_releases_them_after_dismissal() {
    let code = script(&[
        (Call::ConfigureDialogue, &[1, 64, -1, 1, 0, 0, 0, 0]),
        (Call::SetActorHeading, &[2, 351]),
        (Call::ConfigureDialogue, &[0, 64, -1, 2, 0, 0, 0, 0]),
        (Call::YieldCommand, &[2, 0]),
        (Call::SetActorHeading, &[2, 180]),
    ]);
    let mut actor = Actor::new(2, [0.; 3]);
    actor.face(180.);
    let mut world = GameWorld::default();
    world.actors.insert(2, actor);
    let mut lloyd = Actor::new(1, [0.; 3]);
    lloyd.face(180.);
    world.actors.insert(1, lloyd);
    let resources = ResourceLibrary {
        messages: vec![Message { tokens: vec![] }],
        ..Default::default()
    };
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    // An already-facing speaker needs no extra update before its window can open.
    assert_eq!(events.world.dialogue[&1].opening_actor, None);
    assert_eq!(events.world.dialogue[&0].opening_actor, Some(2));
    for _ in 0..120 {
        events.step().unwrap();
        if events.world.dialogue[&0].opening_actor.is_none() {
            break;
        }
    }
    assert_eq!(events.world.actors[&2].heading, 351.);
    assert_eq!(events.world.dialogue[&0].opening_actor, None);
    events.world.dialogue[&0].operation.complete(None).unwrap();
    for _ in 0..120 {
        events.step().unwrap();
        if events.world.actors[&2].heading == 180. {
            break;
        }
    }
    assert_eq!(events.world.actors[&2].heading, 180.);
}
#[test]
fn event_control_selects_the_leaders_idle_without_overriding_other_actors() {
    for has_event_idle in [false, true] {
        let mut resources = ResourceLibrary::default();
        resources.models.insert(
            3,
            model(
                [12, 60].into_iter().chain(has_event_idle.then_some(116)),
                60,
            ),
        );
        let mut world = GameWorld::default();
        world.controlled_actor = 3;
        world.input_enabled = true;
        world.actors.insert(3, Actor::new(3, [0.; 3]));
        let mut bystander = Actor::new(3, [20., 0., 0.]);
        bystander.idle_animation = 60;
        world.actors.insert(42, bystander);
        let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
        events.step().unwrap();
        assert_eq!(events.world.actors[&3].animation.as_ref().unwrap().slot, 12);
        events.world.input_enabled = false;
        events.step().unwrap();
        assert_eq!(
            events.world.actors[&3].animation.as_ref().unwrap().slot,
            if has_event_idle { 116 } else { 12 }
        );
        assert_eq!(
            events.world.actors[&42].animation.as_ref().unwrap().slot,
            60
        );
        // Authored event animation always wins over automatic idle selection.
        let actor = events.world.actors.get_mut(&3).unwrap();
        actor.scripted_animation = true;
        actor.animation.as_mut().unwrap().slot = 60;
        events.step().unwrap();
        assert_eq!(events.world.actors[&3].animation.as_ref().unwrap().slot, 60);
        events.world.actors.get_mut(&3).unwrap().scripted_animation = false;
        events.world.input_enabled = true;
        events.step().unwrap();
        assert_eq!(events.world.actors[&3].animation.as_ref().unwrap().slot, 12);
    }
}

#[test]
fn loaded_motion_and_party_model_keep_their_own_resource_namespaces() {
    use animation::AnimationSource;
    let mut resources = ResourceLibrary::default();
    resources.models.insert(8, model([12], 20));
    resources.bindings.insert(8, (ResourceKind::Model, 8));
    resources.animations.insert(8, model([12], 60).clips);
    let code = script(&[
        (Call::ResolveScriptResource, &[8]),
        (Call::ConfigureActorAnimation, &[80, 8, 12, 0, 8]),
        (
            Call::ConfigureActorAnimation,
            &[81, 0xffff0000_u32 as i32, 12, 0, 8],
        ),
    ]);
    let mut world = GameWorld::default();
    for id in [80, 81] {
        world.actors.insert(id, Actor::new(8, [0.; 3]));
    }
    let events = runtime(program(&code, &[0x20ff]), resources, world);
    for (id, source, duration) in [
        (80, AnimationSource::Model, 20),
        (81, AnimationSource::Resource, 60),
    ] {
        let animation = events.world.actors[&id].animation.as_ref().unwrap();
        assert_eq!((animation.resource, animation.source), (8, source));
        assert_eq!(animation.duration_ticks, duration);
    }
}

#[test]
fn zero_id_spawns_keep_independent_models_but_cannot_be_addressed() {
    let resources = ResourceLibrary {
        bindings: [(1, (ResourceKind::Model, 1))].into(),
        models: [(1, model([12], 20))].into(),
        ..Default::default()
    };
    let code = script(&[
        (Call::SpawnActor, &[0, 10, 20, 30, 0, 0, 0, 0]),
        (Call::SpawnActor, &[0, 40, 50, 60, 0, 0, 0, 0]),
        (Call::SetActorPosition, &[0, 100, 200, 300]),
        (Call::DespawnActor, &[0]),
    ]);
    let events = runtime(program(&code, &[0x20ff]), resources, GameWorld::default());
    assert!(!events.world.actors.contains_key(&0));
    assert_eq!(events.world.actors.len(), 2);
    let positions: Vec<_> = events
        .world
        .actor_order()
        .iter()
        .map(|id| {
            let actor = &events.world.actors[id];
            assert_eq!(actor.resource, 1);
            actor.position
        })
        .collect();
    assert_eq!(positions, [[10., 20., 30.], [40., 50., 60.]]);
}

#[test]
fn caller_palette_geometry_rejects_actor_instantiation_through_direct_and_loaded_handles() {
    let resource = 0xffee0000_u32;
    for handle in [resource as i32, 0xffff0000_u32 as i32] {
        let mut resources = ResourceLibrary::default();
        resources
            .bindings
            .insert(resource as i32, (ResourceKind::UnboundGeometry, resource));
        let code = script(&[
            (Call::ResolveScriptResource, &[resource as i32]),
            (Call::SpawnActor, &[80, 0, 0, 0, 0, handle, 0, 0]),
        ]);
        let error = match EventRuntime::new(program(&code, &[0x20ff]), Arc::new(resources)) {
            Ok(_) => panic!("geometry instantiated without its caller's palette"),
            Err(error) => error,
        };
        assert!(format!("{error:#}").contains("requires caller-supplied textures"));
    }
}

#[test]
fn animation_commands_ignore_removed_actors_before_resolving_their_resources() {
    let code = script(&[(Call::ConfigureActorAnimation, &[714, 12345, 80, 1, 8])]);
    let events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    assert!(events.world.actors.is_empty());
}

#[test]
fn animation_commands_select_the_last_clip_without_advancing_movement() {
    let setup = script(&[
        (Call::YieldCommand, &[0, 1]),
        (Call::MoveActor, &[2, 100, 0, 0, 4]),
        (Call::ConfigureActorAnimation, &[2, -1, 12, 0, 8]),
        (Call::ConfigureActorAnimation, &[2, 0, 0, 0, 0]),
        (Call::ConfigureActorAnimation, &[2, -1, 24, 0, 8]),
        (Call::GetActorProperty, &[2, 1]),
        (Call::YieldCommand, &[0, 2]),
    ]);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(2, model([12, 24], 32));
    let mut world = GameWorld::default();
    world.insert_actor(2, Actor::new(2, [0.; 3]));
    let mut events = runtime(program(&setup, &[0x20ff]), resources, world);
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 0);
    assert_eq!(events.world.actors[&2].animation.as_ref().unwrap().slot, 24);
    events.step().unwrap();
    assert_eq!(events.world.actors[&2].position, [4., 0., 0.]);
}

#[test]
fn animation_release_rebinds_the_same_slot_with_the_requested_blend() {
    let code = script(&[
        (Call::YieldCommand, &[0, 1]),
        (Call::ConfigureActorAnimation, &[2, -1, 12, 8, 8]),
        (Call::ConfigureActorAnimation, &[2, 0, 0, 3, 0]),
        (Call::SetActorPosition, &[2, 100, 200, 0]),
        (Call::YieldCommand, &[0, 1]),
    ]);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(2, model([12], 32));
    let mut world = GameWorld::default();
    world.actors.insert(2, Actor::new(2, [0.; 3]));
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    events.step().unwrap();
    let actor = &events.world.actors[&2];
    let animation = actor.animation.as_ref().unwrap();
    assert!(!actor.scripted_animation);
    assert!(
        animation.repeat,
        "release must replace the nonrepeating clip"
    );
    assert_eq!(animation.blend_ticks, 3);
    assert_eq!(animation.start_tick, events.tick());
    assert_eq!(actor.position, [100., 200., 0.]);
}

#[test]
fn eraser_rate_preserves_the_scripted_impact_cue() {
    // Play slot 80 at rate 50, wait 40 updates, then emit the impact cue.
    // The rate advances a quarter source frame per update; immediate binding
    // adds one sample, so impact occurs at frame 10.25.
    let mut code = Vec::new();
    native(
        &mut code,
        Call::ConfigureActorAnimation,
        &[100, -1, 80, 1, 8],
    );
    native(&mut code, Call::SetActorAnimationProperty, &[100, 0, 50]);
    code.push(0x3000);
    native(&mut code, Call::YieldCommand, &[0, 40]);
    native(&mut code, Call::PlaySound, &[236, 0, 255, 255]);
    native(
        &mut code,
        Call::CreateEffectObject,
        &[0, 180, 80, -635, 140, 0, 0, 0, 0, 10, 75, -1, 0, 0],
    );
    code.extend([0x3000, 0x20ff]);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(68196, model([80], 70));
    let mut world = GameWorld::default();
    world.actors.insert(100, Actor::new(68196, [0.; 3]));
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    steps(&mut events, 39);
    assert!(events.world.billboards.is_empty());
    assert!(events.world.audio_commands.is_empty());
    events.step().unwrap();
    let animation = events.world.actors[&100].animation.as_ref().unwrap();
    assert!((0. ..70.).contains(&animation.sample(events.tick(), 0, 70.)));
    assert_eq!(
        events.world.billboards.values().next().unwrap().born,
        events.tick() + 1
    );
    assert!(matches!(
        events.world.audio_commands.as_slice(),
        [AudioCommand::Sound { id: 236, .. }]
    ));
}
#[test]
fn billboard_angle_is_absolute_and_independent_of_spin_and_growth() {
    for angle in [10000, -4525] {
        let code = script(&[
            (
                Call::CreateEffectObject,
                &[0, 60, 10, 20, 30, 0, 0, 0, 0, 40, 255, 0, 0, 0],
            ),
            (Call::SetEffectProperty, &[1, 134, -125]),
            (Call::SetEffectProperty, &[1, 135, 250]),
            (Call::SetEffectProperty, &[1, 143, 9000]),
            (Call::SetEffectProperty, &[1, 143, angle]),
        ]);
        let mut events = runtime(
            program(&code, &[0x20ff]),
            Default::default(),
            Default::default(),
        );
        let degrees = angle as f32 / 100.;
        assert_eq!(events.world.billboards[&1].rotation, [0., 0., degrees]);
        events.step().unwrap();
        assert_eq!(events.world.billboards[&1].rotation, [0., 0., degrees]);
        events.step().unwrap();
        let effect = &events.world.billboards[&1];
        assert_eq!(effect.rotation, [0., 0., degrees - 1.25]);
        assert_eq!(effect.angular_velocity, [0., 0., -1.25]);
        assert_eq!(effect.size, [42.5; 2]);
        assert_eq!(effect.position, [10., 20., 30.]);
    }
}
#[test]
fn script_light_updates_preserve_order_and_default_selector_alias() {
    let mut code = Vec::new();
    native(&mut code, Call::SetEffectSetting, &[-1, 7, 128, 128, 128]);
    native(&mut code, Call::SetEffectSetting, &[0, 0, 240, 240, 240]);
    native(&mut code, Call::SetEffectSetting, &[1, 0, 256, 256, 256]);
    code.push(0x20ff);
    let events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    assert_eq!(events.world.character_lights[&0].bright, [60; 3]);
    assert_eq!(events.world.character_lights[&0].shade, [45; 3]);
    assert_eq!(events.world.character_lights[&1].bright, [64; 3]);
    let mut light = events.world.character_lights[&0].clone();
    let mut target = events.world.character_lights[&1].clone();
    target.color_step = 2;
    light.approach(&target);
    assert_eq!(light.bright, [62; 3]);
    light.approach(&target);
    assert_eq!(light.bright, [64; 3]);
}
#[test]
fn general_shims_run_non_title_events_with_shared_state_and_ordered_waits() {
    let mut main = Vec::new();
    native(&mut main, Call::SpawnEvent, &[42]);
    main.push(0x3000);
    native(&mut main, Call::YieldCommand, &[0, 2]);
    main.push(0x20ff);
    let mut child = Vec::new();
    native(&mut child, Call::YieldCommand, &[0, 0]);
    native(
        &mut child,
        Call::CreateSceneActor,
        &[77, 100, 200, 300, 0, 1234, 0, 0],
    );
    native(&mut child, Call::SetActorProperty, &[77, 2, 450]);
    // Preserve the previous Y returned by set_actor_property in shared data.
    child.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
    native(
        &mut child,
        Call::ConfigureActorAnimation,
        &[77, -1, 12, 0, 1],
    );
    native(&mut child, Call::YieldCommand, &[0, 1]);
    native(&mut child, Call::PlayCameraTrack, &[555, 0, 0]);
    child.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(1234, (ResourceKind::Model, 99));
    resources.bindings.insert(555, (ResourceKind::Camera, 22));
    resources.models.insert(99, model([12], 30));
    let mut events = runtime(program(&main, &child), resources, Default::default());
    assert_eq!(events.world.actors[&77].position, [100., 450., 300.]);
    assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 200);
    assert!(events.world.camera.is_none());
    assert_eq!(events.active_instances(), 2);
    events.step().unwrap();
    assert_eq!(events.world.camera.as_ref().unwrap().resource, 22);
    assert_eq!(events.world.camera.as_ref().unwrap().start_tick, 1);
    assert_eq!(events.active_instances(), 1);
    events.step().unwrap();
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn actor_attachments_start_hidden_and_script_toggles_are_instance_local() {
    let main = script(&[
        (Call::SpawnActor, &[7, 0, 0, 0, 0, 99, 0, 0]),
        (Call::SpawnActor, &[8, 0, 0, 0, 0, 99, 0, 0]),
        (Call::CreateSceneActor, &[9, 0, 0, 0, 0, 99, 0, 0]),
        (Call::SpawnInteractionActor, &[10, 0, 0, 0, 0, 99, 0, 0]),
        (Call::YieldCommand, &[0, 1]),
        (Call::SetActorAnimation, &[7, 1, 1]),
        (Call::YieldCommand, &[0, 1]),
        (Call::SetActorAnimation, &[7, 0, 0]),
    ]);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(99, (ResourceKind::Model, 99));
    resources.models.insert(
        99,
        ModelResource {
            names: vec!["body".into(), "optional-accessory".into()],
            hidden_nodes: [1].into(),
            ..Default::default()
        },
    );
    let mut events = runtime(program(&main, &[0x20ff]), resources, Default::default());
    assert_eq!(events.world.actors[&7].appearance.hidden_nodes, [1].into());
    assert_eq!(events.world.actors[&9].appearance.hidden_nodes, [1].into());
    assert_eq!(events.world.actors[&10].appearance.hidden_nodes, [1].into());
    events.step().unwrap();
    assert!(events.world.actors[&7].appearance.hidden_nodes.is_empty());
    assert_eq!(events.world.actors[&8].appearance.hidden_nodes, [1].into());
    events.step().unwrap();
    assert_eq!(events.world.actors[&7].appearance.hidden_nodes, [0].into());
    assert_eq!(events.world.actors[&8].appearance.hidden_nodes, [1].into());
}

#[test]
fn unknown_native_stops_with_event_and_pc_context() {
    let error = EventRuntime::new(
        program(&[0x20f0, 0x20ff], &[0x20ff]),
        Arc::new(ResourceLibrary::default()),
    )
    .err()
    .unwrap();
    let text = format!("{error:#}");
    assert!(text.contains("handle 1"));
    assert!(text.contains("PC 0x0000"));
    assert!(text.contains("0xf0"));
}

#[test]
fn actor_color_channels_start_at_native_neutral_and_retain_byte_values() {
    for channel in 42..=44 {
        let mut world = GameWorld::default();
        world.insert_actor(77, Actor::new(131077, [0.; 3]));
        let mut code = Vec::new();
        native(&mut code, Call::GetActorProperty, &[77, channel]);
        code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, Call::SetActorProperty, &[77, channel, 192]);
        code.extend([0x3000, 0x1200, 0x104, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, Call::SetActorProperty, &[77, channel, 256]);
        code.extend([0x3000, 0x1200, 0x108, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, Call::GetActorProperty, &[77, channel]);
        code.push(0x20ff);
        let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
        assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 64);
        assert_eq!(events.memory().read(0x104, Width::S32).unwrap(), 64);
        assert_eq!(events.memory().read(0x108, Width::S32).unwrap(), 192);
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 0);
    }
}

#[test]
fn a_script_can_enable_culling_for_an_initially_unculled_actor() {
    for property in [13, 14] {
        let mut actor = Actor::new(1, [0.; 3]);
        actor.cull_outside_view = false;
        let mut world = GameWorld::default();
        world.insert_actor(1, actor);
        let code = script(&[(Call::SetActorProperty, &[1, property, 0])]);
        let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
        assert!(events.world.actors[&1].cull_outside_view);
        assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 1);
    }
}

#[test]
fn depth_property_returns_the_previous_bit_and_changes_presentation_state() {
    let mut code = Vec::new();
    native(
        &mut code,
        Call::CreateSceneActor,
        &[77, 0, 0, 0, 0, 1234, 0, 0],
    );
    native(&mut code, Call::SetActorProperty, &[77, 46, 3]); // Low bit enables read-only depth.
    code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
    native(&mut code, Call::YieldCommand, &[0, 1]);
    native(&mut code, Call::SetActorProperty, &[77, 46, 2]); // Clear low bit restores writes.
    code.extend([0x3000, 0x1200, 0x104, 0x1200, 0x20, 0x3010, 0x3000, 0x20ff]);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(1234, (ResourceKind::Model, 99));
    let mut events = runtime(program(&code, &[0x20ff]), resources, Default::default());
    assert!(!events.world.actors[&77].depth_write);
    assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 0);
    events.step().unwrap();
    assert!(events.world.actors[&77].depth_write);
    assert_eq!(events.memory().read(0x104, Width::S32).unwrap(), 1);
}

#[test]
fn runtime_cannot_continue_after_a_failed_update() {
    let mut code = Vec::new();
    native(&mut code, Call::YieldCommand, &[0, 1]);
    code.extend([0x20f0, 0x20ff]);
    let mut events = runtime(
        program(&code, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    let error = format!("{:#}", events.step().unwrap_err());
    for context in ["handle 1", "PC", "0xf0"] {
        assert!(error.contains(context), "{error}");
    }
    let tick = events.tick();
    assert!(events.step().unwrap_err().to_string().contains("stopped"));
    assert_eq!(events.tick(), tick);
}

#[test]
fn dialogue_completion_resumes_only_its_caller_and_cannot_fire_twice() {
    let mut main = Vec::new();
    native(&mut main, Call::SpawnEvent, &[42]);
    main.push(0x3000);
    native(
        &mut main,
        Call::ConfigureDialogue,
        &[0, 4, -2, 4, 0, 0, 0, 1],
    );
    native(&mut main, Call::YieldCommand, &[2, 0]);
    native(&mut main, Call::ConfigureRendering, &[0, 7]);
    main.push(0x20ff);
    let mut child = Vec::new();
    native(&mut child, Call::YieldCommand, &[0, 1]);
    native(&mut child, Call::ConfigureRendering, &[1, 9]);
    child.push(0x20ff);
    let resources = ResourceLibrary {
        messages: vec![
            Message { tokens: vec![] },
            Message {
                tokens: vec![Token::Text {
                    text: "Wake up!".into(),
                }],
            },
        ],
        ..Default::default()
    };
    let mut events = runtime(program(&main, &child), resources, Default::default());
    let dialogue = events.world.dialogue[&0].operation.clone();
    dialogue.advance(8).unwrap(); // Text is ready; dismissal is still pending.
    steps(&mut events, 10);
    assert_eq!(events.world.texture_bindings.get(&1), Some(&9));
    assert!(!events.world.texture_bindings.contains_key(&0));
    dialogue.complete(None).unwrap();
    events.step().unwrap();
    assert!(!events.world.texture_bindings.contains_key(&0));
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.get(&0), Some(&7));
    assert_eq!(events.active_instances(), 0);
    assert!(dialogue.complete(None).is_err());
}

#[test]
fn choices_return_the_selected_line_and_completion_reason_once() {
    use resonance_events::dialogue::ChoiceExit;
    for (reason, flags, expected_reason) in [
        (ChoiceExit::Confirm, 0x104, 0),
        (ChoiceExit::Cancel, 4, 1),
        (ChoiceExit::Timeout, 0x104, -1),
    ] {
        let mut code = Vec::new();
        native(
            &mut code,
            Call::ConfigureDialogue,
            &[1, 0, -2, 7, 0, 0, 0, 1],
        );
        // Select lines 3..5, with line 4 initially selected. First line is
        // deliberately not 1, and therefore cannot be mistaken for a default.
        native(&mut code, Call::ShowChoice, &[1, 3, 5, 30, flags]);
        code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, Call::ConfigureRendering, &[0, 42]);
        code.push(0x20ff);
        let resources = ResourceLibrary {
            messages: vec![
                Message { tokens: vec![] },
                Message {
                    tokens: vec![Token::Text {
                        text: "Heading\nDescription\nOne\nTwo\nThree".into(),
                    }],
                },
            ],
            ..Default::default()
        };
        let mut events = runtime(program(&code, &[0x20ff]), resources, Default::default());
        assert_eq!(events.world.choices[&1].selected_line, 3);
        steps(&mut events, 4);
        assert!(events.world.texture_bindings.is_empty());
        let choice = events.world.choices.get_mut(&1).unwrap();
        choice.selected_line = 4;
        let callback = choice.clone();
        choice.finish(reason).unwrap();
        events.step().unwrap();
        assert!(events.world.texture_bindings.is_empty());
        events.world.dialogue[&1].operation.complete(None).unwrap();
        events.step().unwrap();
        assert!(events.world.texture_bindings.is_empty());
        events.step().unwrap();
        assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 5);
        assert_eq!(
            events.memory().read(0x24, Width::S32).unwrap(),
            expected_reason
        );
        assert_eq!(events.world.texture_bindings[&0], 42);
        assert_eq!(events.active_instances(), 0);
        assert!(callback.finish(reason).is_err());
        events.cancel();
        assert!(events.world.choices.is_empty());
    }
}

#[test]
fn replacing_a_choice_window_invalidates_its_waiting_callback() {
    let mut main = Vec::new();
    native(&mut main, Call::SpawnEvent, &[42]);
    main.push(0x3000);
    native(
        &mut main,
        Call::ConfigureDialogue,
        &[1, 0, -2, 7, 0, 0, 0, 0],
    );
    native(&mut main, Call::ShowChoice, &[1, 1, 1, 0, 0x100]);
    main.push(0x20ff);
    let child = script(&[
        (Call::YieldCommand, &[0, 1]),
        (Call::ConfigureDialogue, &[1, 0, -2, 7, 0, 0, 0, 0]),
    ]);
    let resources = ResourceLibrary {
        messages: vec![Message { tokens: vec![] }],
        ..Default::default()
    };
    let mut events = runtime(program(&main, &child), resources, Default::default());
    let old = events.world.choices[&1].clone();
    events.step().unwrap();
    assert!(
        old.finish(resonance_events::dialogue::ChoiceExit::Confirm)
            .is_err()
    );
    assert!(format!("{:#}", events.step().unwrap_err()).contains("cancelled"));
}

#[test]
fn external_media_wait_15_also_waits_for_dialogue_voice() {
    for stop_early in [false, true] {
        let main = script(&[
            (Call::YieldCommand, &[0, 1]),
            (Call::YieldCommand, &[14, 0]),
            (Call::ConfigureRendering, &[0, 1]),
            (Call::YieldCommand, &[15, 0]),
            (Call::ConfigureRendering, &[1, 2]),
        ]);
        let mut events = runtime(
            program(&main, &[0x20ff]),
            Default::default(),
            Default::default(),
        );
        events.world.voice = Some(resonance_events::VoicePlayback {
            resource: 655379,
            end_tick: 10,
        });
        for _ in 0..if stop_early { 4 } else { 9 } {
            events.step().unwrap();
        }
        assert_eq!(
            events.world.texture_bindings.len(),
            1,
            "decoded voice is ready, but its end wait must remain blocked"
        );
        if stop_early {
            events.world.voice = None;
        }
        events.step().unwrap();
        assert_eq!(events.world.texture_bindings.len(), 1);
        events.step().unwrap();
        assert_eq!(events.world.texture_bindings.len(), 2);
    }
}

#[test]
fn satisfied_service_waits_preserve_separate_resume_updates() {
    let main = script(&[
        (Call::YieldCommand, &[15, 0]),
        (Call::ConfigureRendering, &[0, 1]),
        (Call::YieldCommand, &[15, 0]),
        (Call::ConfigureRendering, &[1, 2]),
    ]);
    let mut events = runtime(
        program(&main, &[0x20ff]),
        Default::default(),
        Default::default(),
    );
    assert!(events.world.texture_bindings.is_empty());
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 1);
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 2);
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn movie_waits_follow_decoding_presentation_and_completion_not_elapsed_ticks() {
    let main = script(&[
        (Call::PlayMovie, &[8]),
        (Call::YieldCommand, &[14, 0]),
        (Call::ConfigureRendering, &[0, 1]),
        (Call::YieldCommand, &[19, 12]),
        (Call::ConfigureRendering, &[1, 2]),
        (Call::YieldCommand, &[15, 0]),
        (Call::ConfigureRendering, &[2, 3]),
    ]);
    let mut resources = ResourceLibrary::default();
    resources.movies.insert(8);
    let mut events = runtime(program(&main, &[0x20ff]), resources, Default::default());
    let movie = events.world.movie.as_ref().unwrap().operation.clone();
    steps(&mut events, 20);
    assert!(events.world.texture_bindings.is_empty());
    movie.advance(0).unwrap();
    events.step().unwrap();
    assert!(events.world.texture_bindings.is_empty());
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 1);
    movie.advance(11).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 1);
    movie.advance(12).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 1);
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 2);
    movie.complete(None).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 2);
    events.step().unwrap();
    assert_eq!(events.world.texture_bindings.len(), 3);
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn cancelling_a_scene_stops_its_scripts_and_invalidates_movie_callbacks() {
    let main = script(&[
        (Call::PlayMovieBlocking, &[8]),
        (Call::YieldCommand, &[15, 0]),
        (Call::ConfigureRendering, &[0, 1]),
    ]);
    let mut resources = ResourceLibrary::default();
    resources.movies.insert(8);
    let mut events = runtime(program(&main, &[0x20ff]), resources, Default::default());
    let callback = events.world.movie.as_ref().unwrap().operation.clone();
    events.cancel();
    assert!(callback.complete(None).is_err());
    events.step().unwrap();
    assert_eq!(events.active_instances(), 0);
    assert!(events.world.texture_bindings.is_empty());
}

#[test]
fn changing_fields_moves_globals_but_retires_locals_actors_and_callbacks() {
    use symphonia_script_vm::Memory;
    let main = script(&[
        (Call::ChangeField, &[340, -719, -371, 0, 0]),
        (Call::ConfigureRendering, &[0, 99]),
    ]);
    let resources = ResourceLibrary {
        fields: [340].into(),
        ..Default::default()
    };
    let mut memory = Memory::default();
    for offset in [0x3c, 0x40, 0x3fc, 0x400, 0x800] {
        memory.write(offset, Width::S32, 123).unwrap();
    }
    let mut world = GameWorld::default();
    world.tick = 47;
    world.random_state = 0x12345678;
    world.event_flags.insert(27);
    world.actors.insert(1, Actor::new(1, [1., 2., 3.]));
    let mut events = EventRuntime::with_state(
        program(&main, &[0x20ff]),
        Arc::new(resources),
        world,
        memory,
    )
    .unwrap();
    let callback = events
        .world
        .field_transition
        .as_ref()
        .unwrap()
        .operation
        .clone();
    let persistent = events.persistent_state().unwrap();
    assert!(callback.is_pending());
    assert_eq!(events.active_instances(), 1);
    assert!(events.world.event_flags.contains(&27));
    assert_eq!(events.memory().read(0x800, Width::S32).unwrap(), 123);
    events.cancel();
    assert!(callback.complete(None).is_err());
    assert_eq!(events.active_instances(), 0);
    assert!(events.world.field_transition.is_none());
    let (world, memory) = persistent.into_world();
    assert_eq!(world.tick, 47);
    assert_eq!(world.random_state, 0x12345678);
    assert!(world.event_flags.contains(&27));
    assert!(world.actors.is_empty());
    assert_eq!(memory.read(0x40, Width::S32).unwrap(), 123);
    assert_eq!(memory.read(0x3fc, Width::S32).unwrap(), 123);
    assert_eq!(memory.read(0x400, Width::S32).unwrap(), 0);
    assert_eq!(memory.read(0x800, Width::S32).unwrap(), 0);

    let mut child = EventRuntime::with_state(
        program(&[0x20ff], &[0x20ff]),
        Default::default(),
        world,
        memory,
    )
    .unwrap();
    child.set_global(16, -1).unwrap();
    child.set_global(255, i32::MAX).unwrap();
    events.copy_script_globals(&child).unwrap();
    assert_eq!(events.memory().read(0x3c, Width::S32).unwrap(), 123);
    assert_eq!(events.memory().read(0x40, Width::S32).unwrap(), -1);
    assert_eq!(events.memory().read(0x3fc, Width::S32).unwrap(), i32::MAX);
    assert_eq!(events.memory().read(0x400, Width::S32).unwrap(), 123);
    assert_eq!(events.memory().read(0x800, Width::S32).unwrap(), 123);
}

#[test]
fn settled_camera_follows_the_previous_pose_until_a_command_retargets_it() {
    let mut actor = Actor::new(1, [0.; 3]);
    actor.motion = Some(ActorMotion {
        target: [100., 0., 0.],
        speed: 4.,
    });
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    world.actors.insert(1, actor);
    let mut rig = camera::CameraRig::default();
    rig.current_mut().follow = true;
    rig.current_mut().anchor_to_actor = true;
    rig.position_rate = 8.;
    rig.target_rate = 8.;
    rig.snap_follow_view(&world.actors);
    world.field_camera = Some(rig);
    let child = script(&[(Call::SetCameraPosition, &[0, 0, 80])]);
    let mut events = runtime(
        program(&[0x20ff], &child),
        ResourceLibrary::default(),
        world,
    );
    for step in 0..2 {
        events.step().unwrap();
        let rig = events.world.field_camera.as_ref().unwrap();
        assert_eq!(rig.target, [step as f32 * 4., 0., 0.]);
        assert_eq!(events.world.actors[&1].position[0], (step + 1) as f32 * 4.);
        assert!(rig.settled());
    }
    events.world.actors.get_mut(&1).unwrap().motion = None;
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    events.step().unwrap();
    let rig = events.world.field_camera.as_ref().unwrap();
    assert_eq!(rig.target, [8., 0., 10.]);
    assert!(!rig.settled());
}

#[test]
fn entry_camera_commands_leave_the_presented_camera_alone() {
    let mut code = Vec::new();
    native(&mut code, Call::SelectCamera, &[-1]);
    code.push(0x3000);
    native(&mut code, Call::SetCameraProperty, &[3, 6]);
    code.push(0x3000);
    native(
        &mut code,
        Call::SetCameraTransitionValues,
        &[334, 0, 6, 1890],
    );
    native(&mut code, Call::SetCameraPosition, &[0, 0, 90]);
    for property in 8..=19 {
        native(&mut code, Call::SetCameraProperty, &[property, 123]);
        code.push(0x3000);
    }
    native(&mut code, Call::ResetCameraBounds, &[]);
    native(&mut code, Call::ChangeField, &[332, 1086, 1382, 1, 324]);
    code.push(0x20ff);
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.field_camera = Some(camera::CameraRig::default());
    world
        .field_camera
        .as_mut()
        .unwrap()
        .current_mut()
        .position_bounds = [[-50., 50.]; 3];
    let events = runtime(
        program(&code, &[0x20ff]),
        ResourceLibrary {
            fields: [332].into(),
            ..Default::default()
        },
        world,
    );
    let current = events.world.field_camera.as_ref().unwrap();
    assert_eq!(current.current().angles, [0.; 3]);
    assert_eq!(current.current().offset, [0.; 3]);
    assert_eq!(current.current().position_bounds, [[-50., 50.]; 3]);
    assert_eq!(current.position_rate, 6.);
    let entry = events
        .world
        .field_transition
        .as_ref()
        .unwrap()
        .camera
        .as_ref()
        .unwrap();
    assert_eq!(entry.camera.angles, [334., 0., 6.]);
    assert_eq!(entry.camera.offset, [0., 0., 90.]);
    assert_eq!(entry.camera.position_bounds, [[-100000., 100000.]; 3]);
    assert_eq!(entry.camera.target_bounds, [[-100000., 100000.]; 3]);
    assert!(!current.settled());
    assert_eq!(entry.position_rate, 8.);
    assert_eq!(entry.target_rate, 8.);
}

#[test]
fn script_fades_advance_before_drawing_and_keep_fractional_interruption_state() {
    let start = |calls: &[(Call, &[i32])], alpha| {
        let mut world = GameWorld::default();
        world.tick = 100;
        world.fade = Some(Fade {
            start_tick: 0,
            duration: 1,
            from: alpha,
            to: alpha,
            white: false,
        });
        runtime(
            program(&script(calls), &[0x20ff]),
            Default::default(),
            world,
        )
    };
    let alpha = |events: &EventRuntime| events.world.fade.as_ref().unwrap().alpha(events.tick());
    for mode in [1, 3] {
        let mut events = start(&[(Call::SetTransitionMode, &[mode, 20])], 0.);
        // Hole VI311/312/320/330: state and video both show the first opacity
        // step on the command's own update.
        for elapsed in 0..20 {
            if let Some(expected) = match elapsed {
                0 => Some(13.8),
                1 => Some(26.6),
                9 => Some(129.),
                19 => Some(255.),
                _ => None,
            } {
                assert!((alpha(&events) - expected).abs() < 0.0001);
                assert_eq!(alpha(&events) as u8, expected as u8);
            }
            events.step().unwrap();
        }
        assert_eq!(events.world.fade.as_ref().unwrap().white, mode == 3);
    }
    for (mode, from, expected) in [
        (0, 255., 229.5),
        (1, 0., 26.6),
        (2, 255., 229.5),
        (3, 0., 26.6),
    ] {
        let events = start(&[(Call::SetTransitionMode, &[mode, 0])], from);
        assert_eq!(events.world.fade.as_ref().unwrap().duration, 10);
        assert!((alpha(&events) - expected).abs() < 0.0001);
    }
    let mut events = start(
        &[
            (Call::SetTransitionMode, &[1, 20]),
            (Call::YieldCommand, &[0, 2]),
            (Call::SetTransitionMode, &[2, 3]),
        ],
        0.,
    );
    events.step().unwrap();
    assert!((alpha(&events) - 26.6).abs() < 0.0001);
    events.step().unwrap();
    let fade = events.world.fade.as_ref().unwrap();
    assert!((fade.from - 26.6).abs() < 0.0001);
    assert!((alpha(&events) - (26.6 - 26.6 / 3.)).abs() < 0.0001);
    assert!(fade.white);
    // Two commands before presentation see the initialized opacity one, not
    // the first command's not-yet-drawn 13.8, and preserve the fractional result.
    let events = start(
        &[
            (Call::SetTransitionMode, &[1, 20]),
            (Call::SetTransitionMode, &[0, 10]),
        ],
        0.,
    );
    assert!((alpha(&events) - 0.9).abs() < 0.0001);
    let mut events = start(&[(Call::SetTransitionMode, &[4, 10])], 0.);
    assert_eq!(
        alpha(&events),
        0.,
        "image dissolve must not raise a black overlay"
    );
    assert_eq!(
        events
            .world
            .scene_dissolve
            .as_ref()
            .unwrap()
            .alpha(events.tick()),
        255.
    );
    steps(&mut events, 5);
    assert!(
        (events
            .world
            .scene_dissolve
            .as_ref()
            .unwrap()
            .alpha(events.tick())
            - 127.)
            .abs()
            < 0.001
    );
    steps(&mut events, 5);
    assert_eq!(
        events
            .world
            .scene_dissolve
            .as_ref()
            .unwrap()
            .alpha(events.tick()),
        0.
    );
    let events = start(&[(Call::SetTransitionMode, &[5, 255])], 0.);
    assert_eq!(events.world.next_transition_white, Some(true));
    assert_eq!(
        alpha(&events),
        0.,
        "next-scene clear is not an immediate fade"
    );
}

#[test]
fn clear_field_handoff_releases_input_without_a_delayed_second_handoff() {
    let code = script(&[(Call::ReturnFieldControl, &[0]), (Call::SetEventBit, &[42])]);
    for opacity in [0., 0.5, 255.] {
        let mut world = GameWorld::default();
        world.input_enabled = false;
        world.fade = Some(Fade {
            start_tick: 0,
            duration: 10,
            from: opacity,
            to: opacity,
            white: false,
        });
        let mut events = runtime(program(&code, &[0x20ff]), Default::default(), world);
        assert!(!events.world.event_flags.contains(&42));
        assert!(events.control_handoff_pending());
        if opacity < 1. {
            assert!(events.player_has_control());
            assert_eq!(events.world.brightness(), 1.);
            assert_eq!(events.world.fade.as_ref().unwrap().duration, 0);
            // A new event taking control must not be overwritten when this
            // supervisor resumes after its already completed handoff.
            events.world.input_enabled = false;
            events.step().unwrap();
            assert!(!events.world.input_enabled);
        } else {
            assert!(!events.player_has_control());
            for _ in 0..9 {
                events.step().unwrap();
                assert!(!events.world.input_enabled);
            }
            events.step().unwrap();
            assert!(events.player_has_control());
        }
        assert!(events.world.event_flags.contains(&42));
        assert!(!events.control_handoff_pending());
    }
}

#[test]
fn door_exit_owns_control_and_finishes_its_pose_sound_and_hinge_before_handoff() {
    use resonance_content::field::{Door, FIELD_SERVICE_MOTION_RESOURCE_BASE};
    use resonance_events::animation::slot;
    let code = script(&[(Call::ChangeField, &[340, -762, -642, 0, 180])]);
    let mut resources = ResourceLibrary {
        fields: [340].into(),
        doors: vec![Door {
            bone: 0,
            position: [0.; 3],
            approach: [12., 0., 0.],
            heading: 90.,
            pull: false,
            angle: -30.,
        }],
        ..Default::default()
    };
    resources.animations.insert(
        FIELD_SERVICE_MOTION_RESOURCE_BASE + 1,
        model([20], 56).clips,
    );
    resources.models.insert(
        1,
        model([slot::IDLE, slot::EVENT_IDLE, slot::EVENT_WALK], 60),
    );
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    world.actors.insert(1, Actor::new(1, [0.; 3]));
    world.actors.insert(
        999_996,
        Actor::new(resonance_content::field::SCENERY_RESOURCE_BASE, [0.; 3]),
    );
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    assert!(events.world.actors[&1].animation.is_none());
    let mut contact = None;
    for update in 1..=100 {
        assert!(!events.player_has_control());
        events.step().unwrap();
        let actor = &events.world.actors[&1];
        match update {
            1 => {
                assert_eq!(actor.position, [0.; 3]);
                assert!(actor.motion.is_none());
                let animation = actor.animation.as_ref().unwrap();
                assert_eq!(
                    (animation.slot, animation.start_tick),
                    (slot::EVENT_IDLE, 1)
                );
            }
            2 => {
                assert_eq!(actor.position, [0.; 3]);
                assert_eq!(actor.motion.as_ref().unwrap().target, [12., 0., 0.]);
            }
            3 => assert_eq!(actor.position, [6., 0., 0.]),
            _ => {}
        }
        if !events.world.audio_commands.is_empty() {
            assert!(contact.is_none(), "door cue repeated");
            let animation = events.world.actors[&1].animation.as_ref().unwrap();
            assert_eq!(animation.resource, FIELD_SERVICE_MOTION_RESOURCE_BASE + 1);
            assert!(animation.elapsed(events.tick(), 0) >= 24.);
            assert!(matches!(
                events.world.audio_commands.as_slice(),
                [AudioCommand::Sound { id: 30, .. }]
            ));
            contact = Some(events.tick());
            assert!(
                (events.world.fade.as_ref().unwrap().alpha(events.tick()) - (1. + 256. / 33.))
                    .abs()
                    < 0.0001
            );
            events.world.audio_commands.clear();
        }
        if let Some(contact) = contact {
            let elapsed = events.tick() - contact;
            if (1..=3).contains(&elapsed) {
                let hinge = &events.world.actors[&999_996].appearance.bone_adjustments[&255];
                // Consecutive observed poses after contact: held, then opening.
                assert_eq!(hinge.angles[2], [0., -0.9375, -1.875][elapsed as usize - 1]);
            }
        }
        if let Some(request) = &events.world.field_transition {
            assert_eq!(request.map, 340);
            assert_eq!(events.tick() - contact.unwrap(), 33);
            assert_eq!(
                events.world.fade.as_ref().unwrap().alpha(events.tick()),
                255.
            );
            let hinge = &events.world.actors[&999_996].appearance.bone_adjustments[&255];
            assert_eq!(hinge.angles, [0., 0., -30.]);
            assert!(matches!(hinge.bone, resonance_events::BoneTarget::Index(0)));
            let operation = request.operation.clone();
            events.cancel();
            assert_eq!(operation.progress().outcome, Some(Outcome::Cancelled));
            assert!(events.world.field_transition.is_none());
            return;
        }
    }
    panic!("door exit never handed off");
}

#[test]
fn actor_queries_read_live_values_and_absent_actors_return_zero() {
    let mut actor = Actor::new(1, [-12.75, 23.75, 0.]);
    actor.autonomy = Some(Autonomy::new(Behavior::WanderNearHome, 1., actor.position));
    actor.face(42.);
    actor.target_heading = 180.;
    actor.light = Some(effect::CharacterLight {
        shade: [27, 37, 41],
        ..Default::default()
    });
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.actors.insert(1, actor);
    let mut code = Vec::new();
    for (slot, (call, args)) in [
        (Call::GetActorProperty, &[999999, 1][..]),
        (Call::GetActorProperty, &[1, 4][..]),
        (Call::GetActorProperty, &[77, 4][..]),
        (Call::GetActorProperty, &[1, 8][..]),
        (Call::SetActorProperty, &[1, 15, 150][..]),
        (Call::GetActorProperty, &[1, 15][..]),
        (Call::SetActorProperty, &[1, 15, 200][..]),
        (Call::GetActorProperty, &[999999, 57][..]),
        (Call::SetActorProperty, &[1, 58, 255][..]),
        (Call::GetActorProperty, &[1, 58][..]),
        (Call::GetActorProperty, &[1, 59][..]),
        (Call::GetActorProperty, &[77, 57][..]),
        (Call::SetActorProperty, &[1, 38, 0][..]),
        (Call::GetActorProperty, &[1, 38][..]),
        (Call::SetActorProperty, &[1, 40, 3][..]),
        (Call::GetActorProperty, &[1, 40][..]),
    ]
    .into_iter()
    .enumerate()
    {
        native(&mut code, call, args);
        code.extend([
            0x3000,
            0x1200,
            0x100 + slot as u16 * 4,
            0x1200,
            0x20,
            0x3010,
            0x3000,
        ]);
    }
    code.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(
        1,
        ModelResource {
            toon_lighting: true,
            ..Default::default()
        },
    );
    let events = runtime(program(&code, &[0x20ff]), resources, world);
    assert_eq!(
        (0..16)
            .map(|i| events.memory().read(0x100 + i * 4, Width::S32).unwrap())
            .collect::<Vec<_>>(),
        [
            -12, 42, 0, 255, 600, 150, 150, 27, 37, 37, 41, 0, 1, 0, 0, 1
        ]
    );
    assert_eq!(events.world.actors[&1].autonomy.unwrap().radius, 200.);
    assert!(events.world.actors[&1].appearance.secondary_motion_disabled);
}

#[test]
fn movement_speed_property_changes_an_active_move_and_survives_arrival() {
    const MOVEMENT_SPEED: i32 = 5;
    let setup = script(&[
        (Call::MoveActor, &[2, 25, 0, 0, i32::MIN | 4]),
        (Call::SetActorProperty, &[2, MOVEMENT_SPEED, 3]),
        (Call::YieldCommand, &[4, 2]),
        (Call::GetActorProperty, &[2, MOVEMENT_SPEED]),
    ]);
    let mut world = GameWorld::default();
    let mut actor = Actor::new(2, [0.; 3]);
    actor.autonomy = Some(Autonomy::new(Behavior::Stationary, 0., actor.position));
    world.insert_actor(2, actor);
    let mut events = runtime(program(&setup, &[0x20ff]), Default::default(), world);
    // The setter returns the truncated previous rate, including timed moves.
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 6);
    events.step().unwrap();
    assert_eq!(events.world.actors[&2].position, [3., 0., 0.]);
    steps(&mut events, 12);
    assert_eq!(events.world.actors[&2].position, [25., 0., 0.]);
    assert!(events.world.actors[&2].motion.is_none());
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 3);
}

#[test]
#[ignore = "requires locally cooked party and equipment definitions; no devices"]
fn actor_luck_commands_reroll_once_and_read_equipment_bonuses_without_a_field_actor() {
    let session = Arc::new(cooked("session-data.json"));
    let data = Arc::new(cooked("menu-data.json"));
    let mut party = party::Party::new(&session, Default::default()).unwrap();
    party.members[0].luck = 14;
    party.members[0].equipment = [0; 6];
    party.members[5].luck = 25;
    party.members[5].equipment = [0; 6];
    party.members[5].equipment[4] = 420; // Rabbit's Foot adds 30 derived luck.
    let mut world = GameWorld::default();
    world.party = Some(party);
    world.controlled_actor = 6;
    world.random_state = 0x12345678;
    world.insert_actor(6, Actor::new(6, [0.; 3]));
    let gameplay = world.gameplay_random.clone();
    let mut code = Vec::new();
    for (slot, (call, args)) in [
        (Call::GetActorProperty, &[6, 112][..]),
        (Call::SetActorProperty, &[1, 66, 0][..]),
        (Call::GetActorProperty, &[1, 112][..]),
        (Call::SetActorProperty, &[999999, 66, 12345][..]),
        (Call::GetActorProperty, &[999999, 112][..]),
        (Call::GetActorProperty, &[6, 66][..]),
    ]
    .into_iter()
    .enumerate()
    {
        arg(&mut code, slot as i32);
        native(&mut code, call, args);
        code.extend([0x3000, 0x4000, 0x2046]);
    }
    code.push(0x20ff);
    let events = runtime(
        program(&code, &[0x20ff]),
        ResourceLibrary {
            session_data: Some(session),
            menu_data: Some(data),
            ..Default::default()
        },
        world,
    );
    assert_eq!(
        events.world.texture_bindings,
        [(0, 55), (1, 0), (2, 14), (3, 290), (4, 59), (5, 0)].into()
    );
    assert_eq!(events.world.random_state, 191992145);
    assert_eq!(events.world.gameplay_random, gameplay);
    assert_eq!(events.world.party.as_ref().unwrap().members[5].luck, 29);
}

#[test]
#[ignore = "requires locally cooked character names and party definitions; no devices"]
fn dialogue_names_use_cooked_companion_names_and_saved_renames_but_reject_unknown_ids() {
    use resonance_events::dialogue::TextToken;
    let text: resonance_content::session::GameText = cooked("text.json");
    assert_eq!(text.characters.len(), 10);
    assert_eq!(text.characters[&10], "Noishe");
    let mut party = party::Party::new(&cooked("session-data.json"), Default::default()).unwrap();
    party.members[0].name = Some("Traveler".into());
    let name = |id| Message {
        tokens: vec![Token::Control {
            opcode: 1,
            expression: vec![0, id, 48, 0, 32, 255],
        }],
    };
    let resources = Arc::new(ResourceLibrary {
        actor_names: ResourceLibrary::character_names(),
        text: Arc::new(text),
        messages: vec![name(1), name(10), name(11)],
        ..Default::default()
    });
    let run = |body| {
        let mut world = GameWorld::default();
        world.party = Some(party.clone());
        let code = script(&[(Call::ConfigureDialogue, &[0, 64, -1, 1, 0, 0, 0, body])]);
        EventRuntime::with_state(
            program(&code, &[0x20ff]),
            resources.clone(),
            world,
            Default::default(),
        )
    };
    let events = run(1).unwrap();
    let dialogue = &events.world.dialogue[&0];
    assert!(
        matches!(dialogue.speaker.tokens.as_slice(), [TextToken::Text { text }] if text == "Traveler")
    );
    assert!(
        matches!(dialogue.body.tokens.as_slice(), [TextToken::Text { text }] if text == "Noishe")
    );
    let error = format!("{:#}", run(2).err().unwrap());
    assert!(
        error.contains("message character name 11 is not cooked"),
        "{error}"
    );
}

#[test]
fn sprite_overlay_handles_and_properties_preserve_independent_draw_state() {
    let mut code = Vec::new();
    let id = 77;
    let handle = 0xffff0000u32 as i32;
    native(&mut code, Call::ResolveScriptResource, &[38]);
    native(
        &mut code,
        Call::CreateOverlay,
        &[id, handle, 320, 240, -1, 72, 30, 255, 128, 0, 200, 0, 12],
    );
    native(&mut code, Call::ReleaseScriptResource, &[handle]);
    for (slot, (call, args)) in [
        (Call::GetActorProperty, &[id, 62][..]),
        (Call::SetActorProperty, &[id, 62, 258][..]),
        (Call::GetActorProperty, &[id, 62][..]),
        (Call::SetActorProperty, &[id, 30, 150][..]),
        (Call::SetActorProperty, &[id, 31, -50][..]),
        (Call::SetActorProperty, &[id, 32, 200][..]),
        (Call::SetActorProperty, &[id, 37, -45][..]),
        (Call::SetActorProperty, &[id, 42, 300][..]),
        (Call::SetActorProperty, &[id, 43, -1][..]),
        (Call::SetActorProperty, &[id, 44, 64][..]),
        (Call::SetActorProperty, &[id, 8, 0][..]),
        (Call::SetActorProperty, &[id, 4, 180][..]),
    ]
    .into_iter()
    .enumerate()
    {
        native(&mut code, call, args);
        code.extend([
            0x3000,
            0x1200,
            0x100 + slot as u16 * 4,
            0x1200,
            0x20,
            0x3010,
            0x3000,
        ]);
    }
    native(&mut code, Call::YieldCommand, &[0, 2]);
    native(&mut code, Call::DespawnActor, &[id]);
    code.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(38, (ResourceKind::Overlay, 900));
    let mut events = runtime(program(&code, &[0x20ff]), resources, Default::default());
    for (slot, previous) in [0, 0, 2, 100, 100, 100, 30, 255, 128, 0, 0, -45]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            events
                .memory()
                .read(0x100 + slot as u16 * 4, Width::S32)
                .unwrap(),
            previous
        );
    }
    let overlay = &events.world.overlays[&id];
    let OverlayKind::Sprite(sprite) = &overlay.kind else {
        panic!()
    };
    assert_eq!(
        (sprite.image, sprite.depth, sprite.scale),
        (2, 12, [1.5, -0.5, 2.])
    );
    assert_eq!(overlay.rgba, [44, 255, 64, 0]);
    assert_eq!(overlay.size, [-1, 72]);
    assert_eq!(events.world.actors[&id].resource, 900);
    events.step().unwrap();
    let actor = &events.world.actors[&id];
    assert_eq!((actor.heading, actor.target_heading), (-45., 180.));
    assert!(actor.visible);
    assert_eq!(events.world.overlays[&id].alpha(events.world.tick), 200);
    events.step().unwrap();
    assert!(!events.world.overlays.contains_key(&id));
    assert!(!events.world.actors.contains_key(&id));
}

#[test]
fn sprite_overlay_fades_advance_on_ticks_and_stop_without_removing_the_actor() {
    let id = 77;
    let code = script(&[
        (
            Call::CreateOverlay,
            &[id, 38, 0, 0, -1, -1, 0, 255, 255, 255, 120, 4, 0],
        ),
        (Call::YieldCommand, &[0, 4]),
        (Call::SetActorProperty, &[id, 15, -40]),
        (Call::YieldCommand, &[0, 5]),
        (Call::SetActorProperty, &[id, 8, 60]),
        (Call::SetActorProperty, &[id, 15, 30]),
    ]);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(38, (ResourceKind::Overlay, 900));
    let mut events = runtime(program(&code, &[0x20ff]), resources, Default::default());
    for (tick, alpha) in [0, 0, 30, 60, 90, 120, 80, 40, 0, 0, 0, 30, 60]
        .into_iter()
        .enumerate()
    {
        if tick > 0 {
            events.step().unwrap();
        }
        let overlay = &events.world.overlays[&id];
        // Rendering repeatedly, including at a future timestamp, never advances a fade.
        assert_eq!(overlay.alpha(events.world.tick), alpha);
        assert_eq!(overlay.alpha(events.world.tick + 1000), alpha);
        let OverlayKind::Sprite(sprite) = &overlay.kind else {
            panic!()
        };
        if matches!(tick, 8 | 12) {
            assert_eq!(sprite.alpha_step, 0.);
        }
    }
    assert!(events.world.actors[&id].visible);
    assert_eq!(events.world.overlays[&id].rgba[3], 60);
}

#[test]
fn numbered_world_cinematics_keep_the_following_scene_and_retire_the_caller() {
    for args in [[516, 416, 3615, -659, 396, 0], [518, 3001, 271, 0, 0, 6]] {
        let code = script(&[
            (Call::PlayWorldCinematic, &args),
            (Call::SetEventBit, &[100]),
        ]);
        let resources = ResourceLibrary {
            fields: [3000, 416].into(),
            ..Default::default()
        };
        let mut events = runtime(program(&code, &[0x20ff]), resources, GameWorld::default());
        let request = events.world.world_transition.as_ref().unwrap().clone();
        assert_eq!(request.location, args[0] as u16);
        assert_eq!(
            request.following,
            Some(SceneDestination {
                map: args[1] as u32,
                position: [args[2] as f32, args[3] as f32, args[4] as f32],
                heading: args[5] as f32,
            })
        );
        steps(&mut events, 120);
        assert!(!events.world.event_flags.contains(&100));
        assert!(request.operation.is_pending());
        events.cancel();
        assert_eq!(
            request.operation.progress().outcome,
            Some(Outcome::Cancelled)
        );
        assert!(!events.world.event_flags.contains(&100));
        assert!(request.operation.complete(None).is_err());
    }
}

#[test]
fn scenery_motion_layers_pause_and_clear_independently() {
    let code = script(&[
        (Call::ResolveScriptResource, &[123]),
        (
            Call::ConfigureSceneryAnimation,
            &[999996, -1, -65536, 3, 100, 2],
        ),
        (
            Call::ConfigureSceneryAnimation,
            &[999996, 0, -65536, 0, 100, 8],
        ),
    ]);
    let mut world = GameWorld::default();
    world.actors.insert(999996, Actor::new(1, [0.; 3]));
    let resources = ResourceLibrary {
        animations: [(123, model([12], 20).clips)].into(),
        ..Default::default()
    };
    let clear = script(&[
        (Call::ClearSceneryAnimation, &[999996, -1]),
        (Call::YieldCommand, &[0, 1]),
        (Call::ClearSceneryAnimation, &[1, 0]),
    ]);
    let mut events = runtime(program(&code, &clear), resources, world);
    steps(&mut events, 30);
    let actor = &events.world.actors[&999996];
    let paused = actor.animation.as_ref().unwrap();
    assert_eq!(paused.sample(events.tick(), 0, 20.), 6.);
    assert_eq!(
        actor.scenery_animations[&0].sample(events.tick(), 0, 20.),
        20.
    );
    events.world.input_enabled = true;
    assert!(events.trigger(42, true).unwrap());
    events.step().unwrap();
    let actor = &events.world.actors[&999996];
    assert!(actor.animation.is_none());
    assert_eq!(
        actor.scenery_animations[&0].sample(events.tick(), 0, 20.),
        20.
    );
    events.step().unwrap();
    assert!(events.world.actors[&999996].scenery_animations.is_empty());
}

#[test]
fn enemy_contact_uses_its_event_key_and_publishes_the_symbol_identity_once() {
    let main = script(&[(
        Call::SpawnEnemyActor,
        &[90, 0, 0, 50, 0, 0, 0, 2, 4, 42, 1, 0, 1, 1, 600, 0],
    )]);
    let child = script(&[(Call::SetEventBit, &[123]), (Call::YieldCommand, &[0, 2])]);
    let world = controlled_world();
    let resources = enemy_resources();
    let mut events = runtime(program_kind(&main, &child, 0), resources, world);
    assert!(events.contact_enemy(90).unwrap());
    assert!(!events.contact_enemy(90).unwrap());
    assert_eq!(events.memory().read(0x24, Width::S32).unwrap(), 90);
    events.step().unwrap();
    assert!(events.world.event_flags.contains(&123));
    assert!(!events.player_has_control());
}

#[test]
fn animation_frame_wait_resumes_at_the_authored_frame_before_clip_end() {
    let code = script(&[
        (Call::WaitActorAnimationFrame, &[1, 3]),
        (Call::SetEventBit, &[123]),
    ]);
    let mut actor = Actor::new(1, [0.; 3]);
    actor.scripted_animation = true;
    actor.animation = Some(Animation::new(1, 12, 30, 0));
    let mut world = GameWorld::default();
    world.actors.insert(1, actor);
    let mut events = runtime(
        program(&code, &[0x20ff]),
        ResourceLibrary {
            models: [(1, model([12], 30))].into(),
            ..Default::default()
        },
        world,
    );
    steps(&mut events, 5);
    assert!(!events.world.event_flags.contains(&123));
    steps(&mut events, 3);
    assert!(events.world.event_flags.contains(&123));
    assert!(events.tick() < 30);
}

#[test]
fn screen_copy_passes_are_independent_and_return_their_previous_depth() {
    let code = script(&[
        (Call::ConfigureScreenCopy, &[0, 12345]),
        (Call::ConfigureScreenCopy, &[1, 17890]),
        (Call::YieldCommand, &[0, 2]),
        (Call::ConfigureScreenCopy, &[4, 0]),
        (Call::YieldCommand, &[0, 2]),
        (Call::ConfigureScreenCopy, &[1, 0]),
    ]);
    let mut events = runtime(
        program(&code, &[0x20ff]),
        ResourceLibrary::default(),
        GameWorld::default(),
    );
    assert_eq!(events.world.screen_copy_depth, [123.45, 178.9]);
    steps(&mut events, 2);
    assert_eq!(events.world.screen_copy_depth, [174., 178.9]);
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 12345);
    steps(&mut events, 2);
    assert_eq!(events.world.screen_copy_depth, [174., 0.]);
}

#[test]
fn ring_station_runs_its_interaction_and_keeps_glows_bounded() {
    let main = script(&[(Call::CreateRingStation, &[42, 0, 0, 0, 13, 1])]);
    let child = script(&[(Call::SetEventBit, &[123])]);
    let mut world = controlled_world();
    world.controlled_actor = 1;
    world.insert_actor(1, Actor::new(1, [0., -60., 0.]));
    let sources = std::collections::BTreeMap::from([(
        "field::station".into(),
        include_str!("../../../scripts/field/station.sym").into(),
    )]);
    let station = symphonia_script_compiler::compile(
        "field::station",
        &sources,
        &authored::native_declarations(),
    )
    .unwrap()
    .program;
    let station = Arc::new(station);
    let resources = ResourceLibrary {
        station_script: Some(station.clone()),
        bindings: [(1, (ResourceKind::Model, 1))].into(),
        models: [(1, model([12], 20))].into(),
        ..Default::default()
    };
    let mut events = runtime(program_kind(&main, &child, 0), resources, world);
    assert!(events.has_interaction(42));
    assert!(events.interact(42).unwrap());
    steps(&mut events, 30);
    assert!(events.world.event_flags.contains(&123));
    assert!(!events.player_has_control());
    assert!(events.world.billboards.values().any(|p| p.recipe == 7));
    steps(&mut events, 150);
    assert!(events.world.event_flags.contains(&123));
    assert!(events.player_has_control());
    assert!(events.world.actors[&42].ring_station);
    assert!((4..=24).contains(&events.world.billboards.len()));

    // A transfer must stop when its player is replaced, even if the actor ID is reused.
    assert!(events.interact(42).unwrap());
    steps(&mut events, 4);
    events.world.insert_actor(1, Actor::new(1, [0., -60., 0.]));
    steps(&mut events, 4);
    assert!(events.player_has_control());

    let actor = events.world.authored_actor(42).unwrap();
    let task = events
        .start_authored(station, "field::station::interact", &[actor])
        .unwrap();
    steps(&mut events, 4);
    assert!(
        events
            .world
            .billboards
            .values()
            .any(|p| p.operation.as_ref().is_some_and(Operation::is_pending))
    );
    events.cancel_authored(task).unwrap();
    assert!(!events.world.billboards.values().any(|p| {
        p.operation
            .as_ref()
            .is_some_and(|op| op.progress().outcome == Some(Outcome::Cancelled))
    }));
    assert!(events.player_has_control());
}

#[test]
fn authored_interaction_waits_for_the_scenario_callback_without_releasing_control() {
    let child = script(&[
        (Call::GetEventActor, &[]),
        (Call::EnableMappedInput, &[]),
        (Call::YieldCommand, &[0, 2]),
        (Call::GetEventActor, &[]),
    ]);
    let world = controlled_world();
    let mut events = runtime(
        program_record(&[0x20ff], &child, 0, 102),
        Default::default(),
        world,
    );
    events.world.insert_actor(102, Actor::new(1, [0.; 3]));
    let actor = events.world.authored_actor(102).unwrap();
    events
        .start_authored(interaction_task(), "test::main", &[actor])
        .unwrap();
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 102);
    assert!(!events.world.input_enabled);
    steps(&mut events, 2);
    assert!(!events.world.event_flags.contains(&43));
    events.step().unwrap();
    assert!(events.world.event_flags.contains(&43));
    assert!(!events.world.event_flags.contains(&44));
    assert!(!events.player_has_control());
    steps(&mut events, 2);
    assert!(events.world.event_flags.contains(&44));
    assert!(events.player_has_control());
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn released_task_callbacks_wait_for_control_and_hold_it_until_completion() {
    let sources = [(
        "test".into(),
        r#"
        script field;
        use game::field;
        use game::actors;
        task recover() {
            await field::wait_ticks(2ticks);
            field::release_control();
        }
        pub task main(actor: actors::Actor, delay: ticks) {
            let recovery = spawn recover();
            await field::wait_ticks(delay);
            await actors::interact(actor);
            await recovery;
            await field::wait_ticks(20ticks);
        }
        pub task other() { await field::wait_ticks(8ticks); }
    "#
        .into(),
    )]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();
    let authored = Arc::new(
        symphonia_script_compiler::compile("test", &sources, &authored::native_declarations())
            .unwrap()
            .program,
    );
    let callback = script(&[
        (Call::SetEventBit, &[90]),
        (Call::EnableMappedInput, &[]),
        (Call::YieldCommand, &[0, 6]),
        (Call::SetEventBit, &[91]),
    ]);
    for delay in [1, 5, 12] {
        let mut world = GameWorld::default();
        world.input_enabled = true;
        let mut events = runtime(
            // Main retires into a slot before the authored parent. Late callbacks
            // must reserve control even when their VM won't run until next update.
            program_record(&script(&[(Call::YieldCommand, &[0, 5])]), &callback, 0, 102),
            Default::default(),
            world,
        );
        events.world.insert_actor(102, Actor::new(1, [0.; 3]));
        let actor = events.world.authored_actor(102).unwrap();
        let root = events
            .start_authored(authored.clone(), "test::main", &[actor, delay])
            .unwrap();
        steps(&mut events, 3);
        if delay == 1 {
            assert!(events.world.event_flags.contains(&90));
            assert!(!events.player_has_control()); // Recovery cannot unlock the callback.
        } else {
            assert!(events.player_has_control());
            let other = events
                .start_authored(authored.clone(), "test::other", &[])
                .unwrap();
            steps(&mut events, 7);
            assert!(events.is_active(other));
            assert!(!events.world.event_flags.contains(&90)); // Callback waits for the other owner.
        }
        for _ in 0..30 {
            if events.world.event_flags.contains(&91) {
                break;
            }
            events.step().unwrap();
            if events.tick() > delay as u32 {
                assert!(
                    !events.player_has_control() || events.world.event_flags.contains(&91),
                    "delay {delay}, tick {}",
                    events.tick()
                );
            }
        }
        assert!(events.world.event_flags.contains(&91));
        assert!(events.player_has_control());
        assert!(events.is_active(root));
        let other = events
            .start_authored(authored.clone(), "test::other", &[])
            .unwrap();
        events.cancel_authored(root).unwrap();
        assert!(events.is_active(other));
        assert!(!events.player_has_control()); // Cancelling the tail cannot unlock a new owner.
    }
}

#[test]
fn cancelling_an_interaction_task_cancels_its_legacy_dialogue_and_callback() {
    let child = script(&[
        (Call::DisableMappedInput, &[]),
        (Call::ConfigureDialogue, &[0, 4, -2, 4, 0, 0, 0, 1]),
        (Call::YieldCommand, &[2, 0]),
        (Call::SetEventBit, &[99]),
    ]);
    let resources = ResourceLibrary {
        messages: vec![
            Message { tokens: vec![] },
            Message {
                tokens: vec![Token::Text {
                    text: "The seal opens.".into(),
                }],
            },
        ],
        ..Default::default()
    };
    let world = controlled_world();
    let mut events = runtime(program_record(&[0x20ff], &child, 0, 102), resources, world);
    events.world.insert_actor(102, Actor::new(1, [0.; 3]));
    let actor = events.world.authored_actor(102).unwrap();
    let handle = events
        .start_authored(interaction_task(), "test::main", &[actor])
        .unwrap();
    events.step().unwrap();
    let dialogue = events.world.dialogue[&0].operation.clone();
    assert!(events.world.mapped_input_disabled);
    events.cancel_authored(handle).unwrap();
    assert!(!events.world.mapped_input_disabled);
    assert_eq!(dialogue.progress().outcome, Some(Outcome::Cancelled));
    assert!(events.world.dialogue.is_empty());
    assert_eq!(events.active_instances(), 0);
    events.step().unwrap();
    assert!(events.world.event_flags.is_empty());
    assert!(events.player_has_control());
}

#[test]
fn interaction_errors_retire_the_authored_parent() {
    let world = controlled_world();
    let mut events = runtime(
        program_record(&[0x20ff], &[0x2076, 0x20ff], 0, 102),
        Default::default(),
        world,
    );
    events.world.insert_actor(102, Actor::new(1, [0.; 3]));
    let actor = events.world.authored_actor(102).unwrap();
    events
        .start_authored(interaction_task(), "test::main", &[actor])
        .unwrap();
    let error = format!("{:#}", events.step().unwrap_err());
    assert!(error.contains("unsupported native 0x76"), "{error}");
    assert_eq!(events.active_instances(), 0);
    assert!(!events.world.event_flags.contains(&43));
}

fn interaction_task() -> Arc<Program> {
    let sources = [(
        "test".into(),
        r#"
        script field;
        use game::actors;
        use game::story;
        use game::field;
        pub task main(actor: actors::Actor) {
            await actors::interact(actor);
            story::set_flag(43, true);
            await field::wait_ticks(2ticks);
            story::set_flag(44, true);
        }
    "#
        .into(),
    )]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();
    Arc::new(
        symphonia_script_compiler::compile("test", &sources, &authored::native_declarations())
            .unwrap()
            .program,
    )
}

#[test]
fn ordinary_interactions_supply_their_actor() {
    let world = controlled_world();
    let mut events = runtime(
        program_kind(&[0x20ff], &script(&[(Call::GetEventActor, &[])]), 0),
        Default::default(),
        world,
    );
    assert!(events.player_has_control());
    assert!(events.interact(42).unwrap());
    events.step().unwrap();
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 42);
}

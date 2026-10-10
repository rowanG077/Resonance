use resonance_events::*;
use std::sync::Arc;
use symphonia_script::{NativeCall, Program};

fn native(words: &mut Vec<u16>, call: NativeCall, args: &[i32]) {
    for &arg in args {
        words.extend([
            0x0200,
            arg as u16,
            (arg as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    words.push(0x2000 | call as u16);
}
fn dialogue(words: &mut Vec<u16>, anchor: i32, speaker: i32) {
    native(
        words,
        NativeCall::ConfigureDialogue,
        &[0, 64, anchor, speaker, 0, 0, 0, 0],
    );
}
fn start(anchor: i32, speaker: i32, replacement: Option<(i32, i32)>, moving: bool) -> EventRuntime {
    let mut words = vec![4, 0, 0, 0];
    dialogue(&mut words, anchor, speaker);
    if let Some((anchor, speaker)) = replacement {
        native(&mut words, NativeCall::YieldCommand, &[0, 1]);
        dialogue(&mut words, anchor, speaker);
    }
    words.push(0x20ff);
    let program = Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.random_state = 0x6e8bedac;
    for id in [1, 2] {
        let mut actor = Actor::new(id as u32, [id as f32, 0., 0.]);
        actor.face(55.);
        actor.autonomy = Some(Autonomy {
            activity: Activity::Idle,
            initialized: true,
            remaining: 129,
            ..Autonomy::new(
                if id == 1 {
                    Behavior::Player
                } else {
                    Behavior::Stationary
                },
                0.,
                actor.position,
            )
        });
        if id == 1 && moving {
            actor.motion = Some(ActorMotion {
                target: [101., 0., 0.],
                speed: 5.,
            });
        }
        actor.animation = Some(Animation::new(id as u32, 12, 60, 0));
        world.insert_actor(id, actor);
    }
    let model = || ModelResource {
        clips: [12, 24, 36, 44, 48, 52, 112, 116]
            .into_iter()
            .map(|slot| {
                (
                    slot,
                    AnimationClip {
                        duration_ticks: 60,
                        attachments: None,
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    let resources = ResourceLibrary {
        models: [(1, model()), (2, model())].into(),
        messages: vec![symphonia_script::message::Message { tokens: vec![] }],
        ..Default::default()
    };
    EventRuntime::with_state(
        Arc::new(program),
        Arc::new(resources),
        world,
        Default::default(),
    )
    .unwrap()
}

#[test]
fn concrete_actor_dialogue_holds_its_timer_but_alias_and_screen_speakers_do_not() {
    for (anchor, speaker, owner) in [
        (-1, 1, Some(1)),
        (-1, 2, Some(2)),
        (-1, CONTROLLED_ACTOR, None),
        (-2, 1, None),
        (-10, 1, None),
        (-1, 404, None),
    ] {
        let mut events = start(anchor, speaker, None, false);
        for id in [1, 2] {
            let ai = events.world.actors[&id].autonomy.unwrap();
            assert_eq!(ai.conversing, owner == Some(id));
            assert_eq!(ai.dialogue_slot, (owner == Some(id)).then_some(0));
        }
        for _ in 0..6 {
            events.step().unwrap();
        }
        for id in [1, 2] {
            let actor = &events.world.actors[&id];
            assert_eq!(
                actor.autonomy.unwrap().remaining,
                if owner == Some(id) { 129 } else { 123 }
            );
            assert_eq!((actor.position, actor.heading), ([id as f32, 0., 0.], 55.));
        }
        assert_eq!(events.world.random_state, 0x6e8bedac);
    }
}

#[test]
fn replacing_a_window_preserves_existing_slot_owners_until_actor_retirement() {
    for replacement in [(-1, 2), (-1, CONTROLLED_ACTOR), (-2, 1)] {
        let mut events = start(-1, 1, Some(replacement), false);
        let first = events.world.dialogue[&0].operation.clone();
        events.step().unwrap(); // First owner visit, then replacement in the VM.
        assert_eq!(first.progress().outcome, Some(Outcome::Cancelled));
        assert_ne!(first.id(), events.world.dialogue[&0].operation.id());
        events.step().unwrap();
        assert_eq!(events.world.actors[&1].autonomy.unwrap().remaining, 129);
        assert_eq!(
            events.world.actors[&1].autonomy.unwrap().dialogue_slot,
            Some(0)
        );
        assert_eq!(
            events.world.actors[&2].autonomy.unwrap().dialogue_slot,
            (replacement == (-1, 2)).then_some(0)
        );
        let held = events.world.actors[&1].animation.clone().unwrap();
        assert_eq!(held.slot, 112);
        // The actor sees pending dialogue status before the operation's later VM completion.
        events
            .world
            .dialogue
            .get_mut(&0)
            .unwrap()
            .actor_activity_released = true;
        events.step().unwrap();
        let actor = &events.world.actors[&1];
        let ai = actor.autonomy.unwrap();
        assert_eq!(
            (
                ai.activity,
                ai.initialized,
                ai.conversing,
                ai.dialogue_slot,
                ai.remaining
            ),
            (Activity::Select, false, false, None, 129)
        );
        let animation = actor.animation.as_ref().unwrap();
        assert_eq!(
            (animation.slot, animation.start_tick),
            (held.slot, held.start_tick)
        );
        assert!(events.world.dialogue[&0].operation.is_pending());
        events.world.dialogue[&0].operation.complete(None).unwrap();
        events.step().unwrap();
        let actor = &events.world.actors[&1];
        assert_eq!(
            (
                actor.autonomy.unwrap().activity,
                actor.autonomy.unwrap().remaining
            ),
            (Activity::Idle, 129)
        );
        assert_eq!(
            actor.animation.as_ref().unwrap().start_tick,
            held.start_tick
        );
        events.step().unwrap();
        let actor = &events.world.actors[&1];
        assert!(actor.autonomy.unwrap().initialized);
        assert_eq!(
            (
                actor.animation.as_ref().unwrap().slot,
                actor.animation.as_ref().unwrap().start_tick
            ),
            (116, 5)
        );
    }
}

#[test]
fn removing_the_owned_slot_releases_once_without_restarting_the_retained_pose() {
    let mut events = start(-1, 1, None, false);
    events.step().unwrap();
    let held = events.world.actors[&1].animation.clone().unwrap();
    events.world.dialogue.remove(&0).unwrap().operation.cancel();
    events.step().unwrap();
    let actor = &events.world.actors[&1];
    let ai = actor.autonomy.unwrap();
    assert_eq!(
        (ai.activity, ai.dialogue_slot, ai.remaining),
        (Activity::Select, None, 129)
    );
    assert_eq!(
        actor.animation.as_ref().unwrap().start_tick,
        held.start_tick
    );
    assert_eq!(events.world.random_state, 0x6e8bedac);
    events.step().unwrap();
    assert_eq!(
        events.world.actors[&1].autonomy.unwrap().activity,
        Activity::Idle
    );
}

#[test]
fn an_early_close_releases_the_speaker_after_facing_finishes() {
    let mut events = start(-1, 1, None, false);
    let actor = events.world.actors.get_mut(&1).unwrap();
    actor.heading = 74.;
    actor.target_heading = 55.;
    events.world.dialogue.remove(&0).unwrap().operation.cancel();
    for _ in 0..16 {
        events.step().unwrap();
        let actor = &events.world.actors[&1];
        if !actor.autonomy.unwrap().conversing {
            assert!((actor.heading - actor.target_heading).abs() < 0.01);
            break;
        }
        assert_eq!(actor.autonomy.unwrap().dialogue_slot, Some(0));
    }
    let actor = &events.world.actors[&1];
    assert!(!actor.autonomy.unwrap().conversing);
    assert_eq!(actor.autonomy.unwrap().dialogue_slot, None);
}

#[test]
fn moving_speakers_keep_the_existing_motion_path_without_new_ownership() {
    let mut events = start(-1, 1, None, true);
    let actor = &events.world.actors[&1];
    assert!(!actor.autonomy.unwrap().conversing);
    assert_eq!(actor.autonomy.unwrap().dialogue_slot, None);
    events.step().unwrap();
    let actor = &events.world.actors[&1];
    assert_eq!(actor.position, [6., 0., 0.]);
    assert!(actor.motion.is_some());
    assert!(!actor.autonomy.unwrap().conversing);
    assert_eq!(actor.autonomy.unwrap().remaining, 129);
    assert_eq!(events.world.random_state, 0x6e8bedac);
}

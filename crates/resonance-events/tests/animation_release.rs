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

fn actor() -> Actor {
    let mut actor = Actor::new(3, [20., 10., 0.]);
    actor.face(150.);
    actor.autonomy = Some(Autonomy {
        activity: Activity::Idle,
        initialized: true,
        remaining: 100,
        ..Autonomy::new(Behavior::Stationary, 0., actor.position)
    });
    actor.animation = Some(Animation::new(3, 68, 60, 0));
    actor.scripted_animation = true;
    actor
}

fn release(subject: Actor, blend: i32, count: usize, operand: i32) -> EventRuntime {
    let mut words = vec![4, 0, 0, 0];
    for _ in 0..count {
        native(
            &mut words,
            NativeCall::ConfigureActorAnimation,
            &[operand, 0, 0, blend, 0],
        );
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
    world.tick = 20;
    world.controlled_actor = 3;
    world.actors.insert(3, subject);
    let resources = ResourceLibrary {
        models: [(
            3,
            ModelResource {
                clips: [12, 36, 44, 48, 68]
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
            },
        )]
        .into(),
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
fn release_blends_to_idle_without_advancing_actor_state() {
    let subject = actor();
    let mut events = release(subject.clone(), 60, 1, 3);
    let released = &events.world.actors[&3];
    assert!(!released.scripted_animation);
    assert_eq!(released.position, subject.position);
    assert_eq!(released.heading, subject.heading);
    assert_eq!(
        released.autonomy.unwrap().remaining,
        subject.autonomy.unwrap().remaining
    );
    let animation = released.animation.as_ref().unwrap();
    assert_eq!(animation.slot, 12);
    assert_eq!(animation.blend_ticks, 60);
    assert_eq!(animation.blend_weight(events.tick()), 0.);
    let mut previous = 0.;
    for _ in 0..60 {
        events.step().unwrap();
        let animation = events.world.actors[&3].animation.as_ref().unwrap();
        let weight = animation.blend_weight(events.tick());
        assert!((previous..=1.).contains(&weight));
        previous = weight;
    }
    assert_eq!(previous, 1.);
    events.step().unwrap();
    assert!(
        events.world.actors[&3]
            .animation
            .as_ref()
            .unwrap()
            .sample(events.tick(), 0, 60.)
            > 0.
    );
}

#[test]
fn repeated_release_and_controlled_alias_preserve_one_transition() {
    let once = release(actor(), 60, 1, 3);
    let repeated = release(actor(), 60, 3, CONTROLLED_ACTOR);
    let animation = |events: &EventRuntime| {
        let a = events.world.actors[&3].animation.as_ref().unwrap();
        (
            a.slot,
            a.start_tick,
            a.blend_ticks,
            a.sample(events.tick(), 0, 60.),
        )
    };
    assert_eq!(animation(&once), animation(&repeated));
    assert_eq!(once.world.random_state, repeated.world.random_state);
    assert_eq!(
        once.world.actors[&3].autonomy.unwrap().remaining,
        repeated.world.actors[&3].autonomy.unwrap().remaining
    );
    let long = release(actor(), 100_000, 2, 3);
    assert_eq!(
        long.world.actors[&3]
            .animation
            .as_ref()
            .unwrap()
            .blend_ticks,
        100_000
    );
}

#[test]
fn release_selects_locomotion_and_keeps_the_scripted_destination() {
    let mut subject = actor();
    subject.motion = Some(ActorMotion {
        target: [200., 10., 0.],
        speed: 2.,
    });
    let mut events = release(subject, 8, 2, 3);
    let released = &events.world.actors[&3];
    assert_eq!(released.animation.as_ref().unwrap().slot, 36);
    assert_eq!(released.motion.as_ref().unwrap().target, [200., 10., 0.]);
    assert_eq!(released.position, [20., 10., 0.]);
    events.step().unwrap();
    assert!(events.world.actors[&3].position[0] > 20.);
}

#[test]
fn release_without_a_model_clears_ownership_without_ticking_other_actors() {
    let mut subject = actor();
    subject.resource = 99;
    let events = release(subject, 0, 2, 3);
    let released = &events.world.actors[&3];
    assert!(!released.scripted_animation && released.animation.is_none());
    assert_eq!(released.position, [20., 10., 0.]);
    assert_eq!(released.autonomy.unwrap().remaining, 100);
}

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
fn program(main: &[u16], child: &[u16]) -> Arc<Program> {
    program_kind(main, child, 2)
}
fn program_kind(main: &[u16], child: &[u16], kind: u16) -> Arc<Program> {
    let mut words = vec![10, 0, 0, 1, 0, kind, 0, 42, 0, main.len() as u16];
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
        particles: [(10, ParticleKind::Glow)].into(),
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
    for _ in 0..5 {
        native(&mut code, Call::ReadActorAttachment, &[1, 1]);
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
    for _ in 0..4 {
        events.step().unwrap();
    }
    assert_eq!(
        events
            .world
            .particles
            .iter()
            .map(|p| (p.born, p.position))
            .collect::<Vec<_>>(),
        [
            (0, [2., 0., 3.]),
            (1, [2., 0., 3.]),
            (2, [4., 0., 3.]),
            (3, [6., 0., 3.]),
            (4, [8., 0., 3.])
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
        actor.properties.insert(46, 1);
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
            assert!(actor.properties.is_empty());
            assert_eq!(actor.appearance.expression, 0);
            assert!(actor.appearance.eyes.is_none());
            assert!(actor.appearance.hidden_nodes.contains(&7));
        } else {
            assert_eq!(actor.instance, instance);
            assert_eq!(actor.properties[&46], 1);
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
    let pose = |events: &EventRuntime| {
        let a = events.world.actors[&1].animation.as_ref().unwrap();
        (
            a.slot,
            a.sample(events.tick(), 0, 100.),
            a.blend_weight(events.tick()),
        )
    };
    // Hole source331 initializes IDLE at sample1. Source342 selects the
    // activity and retains IDLE at sample2; source343 first blends EVENT_IDLE.
    assert_eq!(pose(&events), (slot::IDLE, 1., 1.));
    events.step().unwrap();
    assert_eq!(pose(&events), (slot::IDLE, 2., 1.));
    assert!(!events.world.actors[&1].autonomy.unwrap().initialized);
    events.step().unwrap();
    assert_eq!(pose(&events), (slot::EVENT_IDLE, 0., 1. / 9.));
    for _ in 0..7 {
        events.step().unwrap();
    }
    assert_eq!(pose(&events), (slot::EVENT_IDLE, 0., 8. / 9.));
    events.step().unwrap();
    assert_eq!(pose(&events), (slot::EVENT_IDLE, 1., 1.));
}

#[test]
fn ambient_origin_accepts_an_uninitialized_idle_pose_before_action_selection() {
    let mut resources = ResourceLibrary::default();
    resources.models.insert(2, model([12, 36], 80));
    let mut actor = Actor::new(2, [-621., 36., 502.]);
    let mut autonomy = Autonomy::new(Behavior::Stationary, 0., actor.position);
    autonomy.remaining = -1;
    autonomy.floor_available = false;
    actor.autonomy = Some(autonomy);
    let mut origin = ActorOrigin {
        autonomy,
        position: actor.position,
        heading: 0.,
        target_heading: 0.,
        animation_slot: Some(12),
        animation_sample: 1.,
        animation_repeat: true,
    };
    let mut world = GameWorld::default();
    world.insert_actor(2, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
    events.apply_actor_origin(2, &origin).unwrap();
    assert_eq!(
        events.world.actors[&2].autonomy.unwrap().activity,
        Activity::Select
    );
    assert_eq!(
        events.world.actors[&2]
            .animation
            .as_ref()
            .unwrap()
            .sample(0, 0, 80.),
        1.
    );
    origin.autonomy.initialized = true;
    assert!(events.apply_actor_origin(2, &origin).is_err());
    origin.autonomy.initialized = false;
    origin.animation_slot = Some(36);
    assert!(events.apply_actor_origin(2, &origin).is_err());
}

#[test]
fn ambient_origin_preserves_the_existing_scripted_clip_binding() {
    let mut resources = ResourceLibrary::default();
    resources.models.insert(2, model([12], 20));
    resources.animations.insert(0x4002f, model([12], 64).clips);
    let mut actor = Actor::new(2, [-73., -134., 0.]);
    actor.autonomy = Some(Autonomy::new(Behavior::Stationary, 0., actor.position));
    actor.scripted_animation = true;
    actor.animation = Some(Animation {
        source: animation::AnimationSource::Resource,
        repeat: false,
        ..Animation::new(0x4002f, 12, 64, 0)
    });
    let mut origin = ActorOrigin {
        autonomy: actor.autonomy.unwrap(),
        position: actor.position,
        heading: 0.,
        target_heading: 0.,
        animation_slot: Some(12),
        animation_sample: 64.,
        animation_repeat: false,
    };
    let mut world = GameWorld::default();
    world.insert_actor(2, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
    events.apply_actor_origin(2, &origin).unwrap();
    let animation = events.world.actors[&2].animation.as_ref().unwrap();
    assert_eq!(animation.resource, 0x4002f);
    assert_eq!(animation.source, animation::AnimationSource::Resource);
    assert_eq!(animation.sample(0, 0, 64.), 64.);
    origin.animation_slot = Some(36);
    assert!(events.apply_actor_origin(2, &origin).is_err());
    assert_eq!(
        events.world.actors[&2].animation.as_ref().unwrap().resource,
        0x4002f
    );
    origin.animation_slot = Some(12);
    origin.autonomy.home = [-441., -284., 0.];
    assert!(events.apply_actor_origin(2, &origin).is_err());
    events.world.controlled_actor = 2;
    events
        .world
        .actors
        .get_mut(&2)
        .unwrap()
        .autonomy
        .as_mut()
        .unwrap()
        .behavior = Behavior::Player;
    origin.autonomy.behavior = Behavior::Player;
    events.apply_actor_origin(2, &origin).unwrap();
    assert_eq!(
        events.world.actors[&2].autonomy.unwrap().home,
        origin.autonomy.home
    );
    origin.autonomy.home[0] = f32::INFINITY;
    assert!(events.apply_actor_origin(2, &origin).is_err());

    for (background, gated, external, foreground) in [
        (true, true, false, false),
        (false, false, false, true),
        (true, false, false, false),
        (true, true, true, false),
        (true, true, false, true),
    ] {
        let mut resources = ResourceLibrary::default();
        for resource in [2, 0x4002f] {
            let mut bank = model([80], 40);
            bank.clips.extend(model([84], 20).clips);
            resources.models.insert(resource, bank);
        }
        resources.bindings.insert(9, (ResourceKind::Model, 0x4002f));
        let bind = [2, if external { 9 } else { -1 }, 80, 8, 8];
        let child = script(&[
            (Call::ConfigureActorAnimation, &bind),
            (Call::YieldCommand, &[7, 2]),
            (Call::SetEventBit, &[42]),
        ]);
        let mut main = Vec::new();
        native(&mut main, Call::SpawnEvent, &[42]);
        native(
            &mut main,
            Call::ControlEvent,
            &[2, if gated { 51 } else { 50 }],
        );
        if foreground {
            native(&mut main, Call::YieldCommand, &[0, 1]);
            native(&mut main, Call::ConfigureActorAnimation, &bind);
            native(&mut main, Call::YieldCommand, &[7, 2]);
        }
        main.push(0x20ff);
        let mut world = GameWorld::default();
        world.input_enabled = true;
        let mut actor = Actor::new(2, [0.; 3]);
        actor.autonomy = Some(Autonomy::new(Behavior::Stationary, 0., [0.; 3]));
        world.insert_actor(2, actor);
        let program = if background {
            program(&main, &child)
        } else {
            program(&child, &[0x20ff])
        };
        let mut events = runtime(program, resources, world);
        if background {
            events.step().unwrap();
        }
        let actor = &events.world.actors[&2];
        let origin = ActorOrigin {
            autonomy: actor.autonomy.unwrap(),
            position: actor.position,
            heading: actor.heading,
            target_heading: actor.target_heading,
            animation_slot: Some(84),
            animation_sample: 20.,
            animation_repeat: false,
        };
        let accepted = background && gated && !external && !foreground;
        assert_eq!(events.apply_actor_origin(2, &origin).is_ok(), accepted);
        let binding = events.world.actors[&2].animation.as_ref().unwrap();
        assert_eq!(binding.resource, if external { 0x4002f } else { 2 });
        assert_eq!(binding.slot, if accepted { 84 } else { 80 });
        if accepted {
            assert_eq!(binding.duration_ticks, 20);
            assert!(!binding.repeat);
            // Registration changes only the observed pose. The existing service
            // wait must still observe completion and resume on its next update.
            events.step().unwrap();
            assert!(!events.world.event_flags.contains(&42));
            events.step().unwrap();
            assert!(events.world.event_flags.contains(&42));
        }
    }
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
fn emotes_consume_shared_randomness_at_creation_for_every_kind() {
    let mut code = Vec::new();
    for kind in 0..20 {
        native(
            &mut code,
            Call::SpawnActor,
            &[-100 - kind, 0, 0, 0, kind, 7, 0, -1],
        );
        native(&mut code, Call::RandomMod, &[100]);
    }
    // Missing parents and IDs outside the emote range do not initialize a controller.
    native(&mut code, Call::SpawnActor, &[-200, 0, 0, 0, 0, 99, 0, -1]);
    native(&mut code, Call::SpawnActor, &[-300, 0, 0, 0, 0, 7, 0, -1]);
    code.push(0x20ff);
    let mut world = GameWorld::default();
    world.random_state = 0x12345678;
    world.insert_actor(7, Actor::new(7, [0.; 3]));
    let gameplay = world.gameplay_random.clone();
    let mut events = runtime(program(&code, &[0x20ff]), Default::default(), world);
    // Each initializer consumes one libc random value before the following
    // script call. The low bits seed local animation counters, not more draws.
    let phases = [
        17, 29, 12, 19, 4, 24, 11, 5, 8, 8, 10, 2, 9, 1, 1, 30, 14, 22, 6, 9,
    ];
    for _ in 0..3 {
        assert_eq!(events.world.emotes.len(), phases.len());
        for (kind, phase) in phases.into_iter().enumerate() {
            let emote = &events.world.emotes[&(-100 - kind as i32)];
            assert_eq!(
                (emote.kind, emote.phase, emote.start_tick),
                (kind as u16, phase, 0)
            );
        }
        assert_eq!(events.world.random_state, 1514963696);
        assert_eq!(events.world.gameplay_random, gameplay);
        events.step().unwrap();
    }
}

#[test]
fn leaf_birth_defers_random_motion_until_the_next_particle_update() {
    let code = script(&[
        (Call::RandomMod, &[30]),
        (
            Call::CreateParticle,
            &[25, 150, 2762, 980, 280, 0, 0, 0, 25, 255, 0, 0, 0],
        ),
        (Call::RandomMod, &[120]),
    ]);
    let resources = ResourceLibrary {
        particles: [(
            25,
            ParticleKind::Flutter(resonance_content::effect::FlutterRecipe {
                texture: "leaf.ktx2".into(),
                uv: [0., 0., 1., 1.],
                aspect_ratio: 1.,
                palette: vec![[13, 63, 4, 255]],
                fall_speed: 1.96,
                fall_variation: 0.02,
                spin: 0.2,
            }),
        )]
        .into(),
        ..Default::default()
    };
    // School-grounds VI 122: both script random calls precede leaf initialization.
    let mut world = GameWorld::default();
    world.random_state = 1618294421;
    world.input_enabled = true;
    let mut anchor = camera::anchor();
    anchor.autonomy.as_mut().unwrap().activity = Activity::Idle;
    world.insert_actor(camera::ANCHOR_ACTOR, anchor);
    let mut npc = Actor::new(48, [0.; 3]);
    npc.autonomy = Some(Autonomy {
        activity: Activity::Walk,
        initialized: true,
        remaining: -1,
        ..Autonomy::new(Behavior::WanderNearHome, 4., [0.; 3])
    });
    world.insert_actor(209, npc);
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    assert_eq!(events.world.random_state, 1527716763);
    let particle = &events.world.particles[0];
    assert_eq!(particle.position, [2762., 980., 280.]);
    assert_eq!(particle.flutter.as_ref().unwrap().rotation, [0.; 3]);
    // VI 123: the camera anchor and NPC update before leaf initialization.
    events
        .step_with_motion(37674, |_| Ok(()), |_, _, _, _| {}, |_| Ok(()))
        .unwrap();
    assert_eq!(events.world.random_state, 570894153);
    assert_eq!(
        events.world.actors[&camera::ANCHOR_ACTOR]
            .autonomy
            .unwrap()
            .remaining,
        177
    );
    assert_eq!(events.world.actors[&209].autonomy.unwrap().remaining, 71);
    let particle = &events.world.particles[0];
    assert_eq!(particle.position, [2761.191, 980., 278.1]);
    assert_eq!(
        particle.flutter.as_ref().unwrap().rotation,
        [23787., 15387., 8711.2]
    );
    events
        .step_with_motion(37675, |_| Ok(()), |_, _, _, _| {}, |_| Ok(()))
        .unwrap();
    assert_eq!(events.world.random_state, 570894153);
    assert_eq!(events.world.particles[0].position, [2760.3718, 980., 276.2]);
}

#[test]
fn event_controlled_player_selects_idle_before_initializing_its_timer() {
    // Pastor VI336–338: Select/-2, Idle/-2, then the first random timer181.
    // Free player input instead selects Idle before that actor's dispatch.
    for free_control in [false, true] {
        let mut world = GameWorld::default();
        world.input_enabled = free_control;
        world.random_state = 3758564392;
        let mut actor = Actor::new(1, [0.; 3]);
        actor.autonomy = Some(Autonomy {
            activity: Activity::Idle,
            initialized: true,
            remaining: -1,
            ..Autonomy::new(Behavior::Player, 0., [0.; 3])
        });
        world.insert_actor(1, actor);
        let mut events = runtime(
            program(&[0x20ff], &[0x20ff]),
            ResourceLibrary::default(),
            world,
        );
        events.step().unwrap();
        let ai = events.world.actors[&1].autonomy.unwrap();
        assert_eq!(
            (ai.activity, ai.initialized, ai.remaining),
            (Activity::Select, false, -2)
        );
        assert_eq!(events.world.random_state, 3758564392);
        events.step().unwrap();
        if !free_control {
            let ai = events.world.actors[&1].autonomy.unwrap();
            assert_eq!(
                (ai.activity, ai.initialized, ai.remaining),
                (Activity::Idle, false, -2)
            );
            assert_eq!(events.world.random_state, 3758564392);
            events.step().unwrap();
        }
        let ai = events.world.actors[&1].autonomy.unwrap();
        assert_eq!(
            (ai.activity, ai.initialized, ai.remaining),
            (Activity::Idle, true, 181)
        );
        assert_eq!(events.world.random_state, 2935932225);
    }
}

#[test]
fn conversation_preserves_decision_timer_until_the_foreground_event_finishes() {
    let child = script(&[(Call::YieldCommand, &[0, 3])]);
    let mut actor = Actor::new(24, [0.; 3]);
    actor.autonomy = Some(Autonomy {
        activity: Activity::Idle,
        remaining: 32,
        initialized: true,
        ..Autonomy::new(Behavior::Stationary, 0., actor.position)
    });
    let mut world = GameWorld::default();
    world.input_enabled = true;
    world.random_state = 1_624_982_312;
    world.insert_actor(42, actor);
    let mut events = runtime(
        program_kind(&[0x20ff], &child, 0),
        Default::default(),
        world,
    );
    assert!(events.interact(42).unwrap());
    let actor = events.world.actors.get_mut(&42).unwrap();
    let ai = actor.autonomy.as_mut().unwrap();
    ai.begin_conversation();
    ai.resolve_floor(false);
    // Event ownership, rather than an input flag, holds the conversation.
    events.world.input_enabled = true;
    for _ in 0..8 {
        events.step().unwrap();
        let ai = events.world.actors[&42].autonomy.unwrap();
        assert!(ai.conversing);
        assert_eq!(ai.remaining, 32);
        assert_eq!(events.world.random_state, 1_624_982_312);
        if events.active_instances() == 0 {
            break;
        }
    }
    assert_eq!(events.active_instances(), 0);
    events.world.input_enabled = false;
    for activity in [Activity::Select, Activity::Idle] {
        events.step().unwrap();
        let ai = events.world.actors[&42].autonomy.unwrap();
        assert_eq!(ai.activity, activity);
        assert!(!ai.conversing && !ai.initialized);
        assert_eq!(ai.remaining, 32);
        assert_eq!(events.world.random_state, 1_624_982_312);
    }
    events.step().unwrap();
    let ai = events.world.actors[&42].autonomy.unwrap();
    assert_eq!(ai.activity, Activity::Idle);
    assert!(ai.initialized);
    assert_eq!(ai.remaining, 120);
    assert_eq!(events.world.random_state, 616_691_777);
}

#[test]
fn spawned_npcs_wander_pause_during_events_and_yield_to_scripted_motion() {
    use animation::slot;
    let code = script(&[(Call::SpawnActor, &[42, 100, 200, 0, 0, 5, 2, 3])]);
    let resources = ResourceLibrary {
        bindings: [(5, (ResourceKind::Model, 5))].into(),
        models: [(5, model([slot::IDLE, slot::WALK], 100))].into(),
        ..Default::default()
    };
    let mut world = GameWorld::default();
    world.input_enabled = true;
    world.random_state = 1;
    world.field_camera = Some(Default::default());
    let conversation = script(&[(Call::YieldCommand, &[0, 200])]);
    let mut events = runtime(program_kind(&code, &conversation, 0), resources, world);
    for _ in 0..30 {
        events.step().unwrap();
    }
    let npc = &events.world.actors[&42];
    let ai = npc.autonomy.unwrap();
    assert_eq!(
        (ai.behavior, ai.home, ai.speed),
        (Behavior::WanderNearHome, [100., 200., 0.], 3.)
    );
    assert_ne!(npc.position, ai.home);
    assert_eq!(npc.animation.as_ref().unwrap().slot, slot::WALK);
    let position = npc.position;
    let animation_sample = npc
        .animation
        .as_ref()
        .unwrap()
        .sample(events.tick(), 0, 100.);
    assert!(animation_sample > 0.);
    let random = events.world.random_state;
    events.world.input_enabled = false;
    for _ in 0..20 {
        events.step().unwrap();
    }
    let npc = &events.world.actors[&42];
    assert_eq!(npc.position, position);
    assert_eq!(npc.autonomy.unwrap().remaining, ai.remaining);
    assert_eq!(events.world.random_state, random);
    assert_eq!(
        npc.animation
            .as_ref()
            .unwrap()
            .sample(events.tick(), 0, 100.),
        animation_sample
    );
    events.world.input_enabled = true;
    events.step().unwrap();
    let npc = &events.world.actors[&42];
    assert_ne!(npc.position, position);
    assert_eq!(
        npc.animation
            .as_ref()
            .unwrap()
            .sample(events.tick(), 0, 100.),
        animation_sample + 1.
    );

    let npc = events.world.actors.get_mut(&42).unwrap();
    let heading = npc.target_heading;
    npc.autonomy.as_mut().unwrap().resolve_floor(false);
    events.world.random_state = 1;
    events.step().unwrap();
    let npc = events.world.actors.get_mut(&42).unwrap();
    assert_eq!(
        (npc.target_heading - heading + 180.).rem_euclid(360.) - 180.,
        -90.
    );
    let position = npc.position;
    npc.motion = Some(ActorMotion {
        target: [position[0] + 100., position[1], position[2]],
        speed: 20.,
    });
    let random = events.world.random_state;
    events.step().unwrap();
    assert_eq!(
        events.world.actors[&42].position,
        [position[0] + 20., position[1], position[2]]
    );
    assert_eq!(events.world.random_state, random);
    let npc = events.world.actors.get_mut(&42).unwrap();
    let position = npc.position;
    npc.motion = None;
    npc.autonomy.as_mut().unwrap().begin_conversation();
    assert!(events.interact(42).unwrap());
    for _ in 0..100 {
        events.step().unwrap();
    }
    assert_eq!(events.world.actors[&42].position, position);
    assert_eq!(
        events.world.actors[&42].animation.as_ref().unwrap().slot,
        slot::IDLE
    );
    events.world.actors.get_mut(&42).unwrap().motion = Some(ActorMotion {
        target: [position[0] + 100., position[1], position[2]],
        speed: 20.,
    });
    events.step().unwrap();
    let npc = &events.world.actors[&42];
    assert!(!npc.autonomy.unwrap().conversing);
    assert_eq!(npc.position, [position[0] + 20., position[1], position[2]]);
}

#[test]
fn npc_walk_speed_and_turn_braking_match_dolphin_positions() {
    let mut actor = Actor::new(48, [1557.2792, 1834.712, 0.]);
    actor.face(64.);
    actor.autonomy = Some(Autonomy {
        activity: Activity::Walk,
        initialized: true,
        remaining: 29,
        ..Autonomy::new(Behavior::WanderNearHome, 2., [1472., 1507., 0.])
    });
    let mut world = GameWorld::default();
    world.input_enabled = true;
    world.actors.insert(210, actor);
    // Isolate this actor's next direction decision from the field's other emitters.
    world.random_state = 2_727_390_630;
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), Default::default(), world);
    for tick in 1..=31 {
        events.step().unwrap();
        let expected = match tick {
            1 => Some([1559.0768, 1833.8353, 0.]),
            30 => Some([1611.2074, 1808.4108, 0.]),
            31 => Some([1612.4254, 1807.8684, 0.]),
            _ => None,
        };
        if let Some(expected) = expected {
            assert_eq!(events.world.actors[&210].position, expected);
        }
    }
    let actor = &events.world.actors[&210];
    assert_eq!((actor.heading, actor.target_heading), (66., 66.));
    assert_eq!(actor.autonomy.unwrap().remaining, 85);
    // At the school wall, a new target crosses 360 before the turn is selected.
    // These three consecutive headings would differ if the target wrapped first.
    let actor = events.world.actors.get_mut(&210).unwrap();
    actor.autonomy = None;
    actor.heading = 28.;
    for (target, expected) in [(393., 23.), (33., 28.), (-57., 23.)] {
        events.world.actors.get_mut(&210).unwrap().target_heading = target;
        events.step().unwrap();
        assert_eq!(events.world.actors[&210].heading, expected);
    }
}

#[test]
fn scripted_eye_modes_restart_blinking_without_consuming_randomness_each_frame() {
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.actors.insert(1, Actor::new(1, [0.; 3]));
    for mode in [1, 0, 6, 1, 1] {
        let code = script(&[(Call::SetActorFace, &[999_999, mode])]);
        let resources = ResourceLibrary {
            blink: Some(resonance_content::effect::BlinkCycle {
                frames: vec![0, 1, 2, 0, 0],
                initial_tick: 3,
                initial_spread: 2,
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
        let mut expected = GameWorld::default();
        expected.random_state = world.random_state;
        if mode == 1 {
            expected.random();
        }
        let mut events = runtime(program(&code, &[0x20ff]), resources, world);
        assert!(events.world.actors[&1].appearance.eyes.is_none());
        let mut frames = Vec::new();
        for _ in 0..10 {
            events.step().unwrap();
            let face = &events.world.actors[&1].appearance;
            assert_eq!(events.world.random_state, expected.random_state);
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
            assert_eq!(&frames[..5], &frames[5..]);
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
    let mut world = GameWorld::default();
    world.input_enabled = true;
    let mut events = runtime(program(&main, &child), Default::default(), world);
    assert!(events.player_has_control());
    for _ in 0..4 {
        events.step().unwrap();
    }
    assert_eq!(events.active_instances(), 1);
    assert!(events.player_has_control());
    events.world.input_enabled = false;
    assert!(!events.player_has_control());
}

#[test]
fn observed_resource_waits_only_suspend_the_requesting_script() {
    let resource = 0x20041;
    let mut main = Vec::new();
    native(&mut main, Call::SpawnEvent, &[42]);
    main.push(0x3000);
    native(&mut main, Call::YieldCommand, &[0, 1]);
    let wrong_pc = main.len() as u32;
    native(&mut main, Call::ResolveScriptResource, &[resource]);
    main.push(0x3000);
    native(&mut main, Call::YieldCommand, &[1, 0xffff0000u32 as i32]);
    let wait_pc = main.len() as u32;
    native(&mut main, Call::SetEventBit, &[43]);
    main.push(0x20ff);
    let child = script(&[(Call::YieldCommand, &[0, 3]), (Call::SetEventBit, &[42])]);
    for (kind, pc) in [ResourceKind::Model, ResourceKind::Animation]
        .into_iter()
        .flat_map(|kind| [None, Some(wait_pc), Some(wrong_pc)].map(|pc| (kind, pc)))
    {
        let mut resources = ResourceLibrary::default();
        if kind == ResourceKind::Animation {
            resources
                .animations
                .insert(resource as u32, Default::default());
        } else {
            resources.bindings.insert(resource, (kind, 1));
        }
        let mut world = GameWorld::default();
        world.input_enabled = true;
        world.controlled_actor = 1;
        let mut actor = Actor::new(1, [0.; 3]);
        actor.motion = Some(ActorMotion {
            target: [100., 0., 0.],
            speed: 4.,
        });
        actor.animation = Some(Animation::new(1, 0, 100, 0));
        world.insert_actor(1, actor);
        let mut events = runtime(program(&main, &child), resources, world);
        if let Some(pc) = pc {
            events
                .register_resource_wait_observations(vec![ResourceWaitObservation {
                    pc,
                    resource,
                    request_tick: 1,
                    resume_tick: 5,
                }])
                .unwrap();
            assert!(events.finish_resource_wait_observations().is_err());
        }
        let first = events.step();
        if pc == Some(wrong_pc) {
            assert!(format!("{:#}", first.unwrap_err()).contains("wrong script PC"));
            continue;
        }
        first.unwrap();
        for tick in 1..=5 {
            if tick > 1 {
                events.step().unwrap();
            }
            assert_eq!(events.world.actors[&1].position, [tick as f32 * 4., 0., 0.]);
            assert_eq!(events.world.event_flags.contains(&42), tick >= 3);
            assert_eq!(
                events.world.event_flags.contains(&43),
                pc.is_none() || tick == 5
            );
        }
        assert!(
            events.world.actors[&1]
                .animation
                .as_ref()
                .unwrap()
                .sample(events.tick(), 0, 100.)
                > 0.
        );
        if pc.is_some() {
            events.finish_resource_wait_observations().unwrap();
        }
    }
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
        let mut origin = events.background_waits().pop().unwrap();
        origin.pc += 1;
        assert!(events.apply_background_wait_origin(&origin).is_err());
        origin.pc -= 1;
        origin.remaining = 5;
        events.apply_background_wait_origin(&origin).unwrap();
        for _ in 1..8 {
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
        for _ in 0..4 {
            events.step().unwrap();
        }
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
fn locomotion_matches_native_speed_and_keeps_its_phase_across_rate_changes() {
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
    for age in 0u32..125 {
        events.step().unwrap();
        let a = events.world.actors[&1].animation.as_ref().unwrap();
        // Dolphin walking checkpoint: 40 native frames, speed 1/frame,
        // blend duration 2. Includes three complete gait loops.
        assert_eq!(a.slot, 36);
        assert_eq!(a.start_tick, 1);
        assert_eq!(a.rate, 2.);
        assert_eq!(a.blend_ticks, 2);
        let elapsed = age.saturating_sub(1) as f32 * 2.;
        let expected = if elapsed > 80. {
            (elapsed - 1.).rem_euclid(80.) + 1.
        } else {
            elapsed
        };
        assert_eq!(a.sample(events.tick(), 0, 80.), expected);
    }
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
fn walking_settles_to_whole_degree_facing_without_quantizing_the_path() {
    // Raine's question checkpoint observes heading 181 even though the
    // displacement points at 181.956 degrees. Position remains continuous.
    let start = [222.20871, 12.970599, 0.];
    let target = [210., 370., 0.];
    let mut actor = Actor::new(4, start);
    actor.face(180.);
    actor.motion = Some(ActorMotion {
        target,
        speed: 1.875,
    });
    let mut world = GameWorld::default();
    world.actors.insert(4, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), Default::default(), world);
    for _ in 0..31 {
        events.step().unwrap();
    }
    let actor = &events.world.actors[&4];
    assert_eq!(actor.heading, 181.);
    assert_eq!(actor.target_heading, 181.);
    let traveled = (actor.position[0] - start[0]).hypot(actor.position[1] - start[1]);
    assert!((traveled - 31. * 1.875).abs() < 0.0001);
    let cross = (actor.position[0] - start[0]) * (target[1] - start[1])
        - (actor.position[1] - start[1]) * (target[0] - start[0]);
    assert!(
        cross.abs() < 0.05,
        "facing must not change the movement vector"
    );
}

#[test]
fn colette_oracle_turn_waits_before_opening_and_returns_in_34_updates() {
    // Recorded Colette turns: 180 → 351 and 351 → 180 each take 34 updates.
    // The final six-degree step enters the target’s angular sector.
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
    for age in 1..=34 {
        events.step().unwrap();
        let expected = if age == 34 {
            351.
        } else {
            180. + age as f32 * 5.
        };
        assert_eq!(events.world.actors[&2].heading, expected);
        assert_eq!(events.world.dialogue[&0].opening_actor, Some(2));
    }
    events.step().unwrap();
    assert_eq!(events.world.dialogue[&0].opening_actor, None);
    // Dismissing the page changes only the target; it cannot snap the actor.
    events.world.dialogue[&0].operation.complete(None).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.actors[&2].target_heading, 351.);
    events.step().unwrap(); // Completed service resumes on the next dispatch.
    assert_eq!(events.world.actors[&2].heading, 351.);
    assert_eq!(events.world.actors[&2].target_heading, 180.);
    for age in 1..=34 {
        events.step().unwrap();
        let expected = if age == 34 {
            180.
        } else {
            351. - age as f32 * 5.
        };
        assert_eq!(events.world.actors[&2].heading, expected);
    }
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
fn repeated_native_animation_bindings_remain_observable_after_replacing_the_clip() {
    let mut code = Vec::new();
    native(&mut code, Call::YieldCommand, &[0, 1]);
    native(&mut code, Call::ConfigureActorAnimation, &[2, -1, 12, 8, 8]);
    native(&mut code, Call::ConfigureActorAnimation, &[2, 0, 0, 0, 0]);
    native(&mut code, Call::ConfigureActorAnimation, &[2, -1, 12, 8, 8]);
    native(&mut code, Call::YieldCommand, &[0, 1]);
    native(&mut code, Call::ConfigureActorAnimation, &[2, -1, 12, 8, 8]);
    code.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(2, model([12], 32));
    let mut world = GameWorld::default();
    world.actors.insert(2, Actor::new(2, [0.; 3]));
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    events.step().unwrap();
    assert_eq!(
        events.world.actors[&2].animation_bindings,
        (events.tick(), 2)
    );
    events.step().unwrap();
    assert_eq!(
        events.world.actors[&2].animation_bindings,
        (events.tick(), 1)
    );
}

#[test]
fn resumed_script_binds_one_pose_after_the_actor_update() {
    let mut code = Vec::new();
    native(&mut code, Call::YieldCommand, &[0, 1]);
    native(&mut code, Call::ConfigureActorAnimation, &[2, -1, 12, 8, 8]);
    native(&mut code, Call::YieldCommand, &[7, 2]);
    native(&mut code, Call::PlaySound, &[236, 0, 255, 255]);
    code.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(2, model([12], 32));
    let mut world = GameWorld::default();
    world.actors.insert(2, Actor::new(2, [0.; 3]));
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    // Colette's observed binding at VI5586: rendered weights1/9..8/9,
    // then sample1 at5594 and sample23 at5616 (nominal60-Hz ticks).
    for age in 0u32..=39 {
        events.step().unwrap();
        let animation = events.world.actors[&2].animation.as_ref().unwrap();
        assert_eq!(
            animation.binding_timing,
            animation::BindingTiming::AfterDraw
        );
        let sample = age.saturating_sub(7).min(32) as f32;
        assert_eq!(animation.sample(events.tick(), 0, 32.), sample);
        let blend = if age < 8 { (age + 1) as f32 / 9. } else { 1. };
        assert_eq!(animation.blend_weight(events.tick()), blend);
        // The service observes the terminal pose, then resumes one update later.
        assert!(events.world.audio_commands.is_empty());
    }
    events.step().unwrap();
    assert!(matches!(
        events.world.audio_commands.as_slice(),
        [AudioCommand::Sound { id: 236, .. }]
    ));
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
    assert_eq!(
        events.world.actors[&100]
            .animation
            .as_ref()
            .unwrap()
            .binding_timing,
        animation::BindingTiming::BeforeDraw
    );
    for _ in 0..39 {
        events.step().unwrap();
    }
    assert!(events.world.billboards.is_empty());
    assert!(events.world.audio_commands.is_empty());
    events.step().unwrap();
    let animation = events.world.actors[&100].animation.as_ref().unwrap();
    assert_eq!(animation.sample(events.tick(), 0, 70.), 20.5);
    assert_eq!(
        events.world.billboards.values().next().unwrap().born,
        events.tick()
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
    for _ in 0..10 {
        events.step().unwrap();
    }
    assert_eq!(events.world.render_settings.get(&1), Some(&9));
    assert!(!events.world.render_settings.contains_key(&0));
    dialogue.complete(None).unwrap();
    events.step().unwrap();
    assert!(!events.world.render_settings.contains_key(&0));
    events.step().unwrap();
    assert_eq!(events.world.render_settings.get(&0), Some(&7));
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
        for _ in 0..4 {
            events.step().unwrap();
        }
        assert!(events.world.render_settings.is_empty());
        let choice = events.world.choices.get_mut(&1).unwrap();
        choice.selected_line = 4;
        let callback = choice.clone();
        choice.finish(reason).unwrap();
        events.step().unwrap();
        assert!(events.world.render_settings.is_empty());
        events.world.dialogue[&1].operation.complete(None).unwrap();
        events.step().unwrap();
        assert!(events.world.render_settings.is_empty());
        events.step().unwrap();
        assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 5);
        assert_eq!(
            events.memory().read(0x24, Width::S32).unwrap(),
            expected_reason
        );
        assert_eq!(events.world.render_settings[&0], 42);
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
            events.world.render_settings.len(),
            1,
            "decoded voice is ready, but its end wait must remain blocked"
        );
        if stop_early {
            events.world.voice = None;
        }
        events.step().unwrap();
        assert_eq!(events.world.render_settings.len(), 1);
        events.step().unwrap();
        assert_eq!(events.world.render_settings.len(), 2);
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
    assert!(events.world.render_settings.is_empty());
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 2);
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
    for _ in 0..20 {
        events.step().unwrap();
    }
    assert!(events.world.render_settings.is_empty());
    movie.advance(0).unwrap();
    events.step().unwrap();
    assert!(events.world.render_settings.is_empty());
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    movie.advance(11).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    movie.advance(12).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 2);
    movie.complete(None).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 2);
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 3);
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
    assert!(events.world.render_settings.is_empty());
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
    for offset in [0x40, 0x3fc, 0x400, 0x800] {
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
    use resonance_content::field::{DOOR_MOTION_RESOURCE_BASE, Door};
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
    resources
        .animations
        .insert(DOOR_MOTION_RESOURCE_BASE + 1, model([20], 56).clips);
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
        assert_eq!(
            actor.animation.as_ref().unwrap().binding_timing,
            animation::BindingTiming::BeforeDraw
        );
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
            assert_eq!(animation.resource, DOOR_MOTION_RESOURCE_BASE + 1);
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
    ]
    .into_iter()
    .enumerate()
    {
        arg(&mut code, slot as i32);
        native(&mut code, call, args);
        code.extend([0x3000, 0x4000, 0x2046]);
    }
    code.push(0x20ff);
    let events = runtime(program(&code, &[0x20ff]), Default::default(), world);
    assert_eq!(
        events.world.render_settings,
        [
            (0, -12),
            (1, 42),
            (2, 0),
            (3, 255),
            (4, 600),
            (5, 150),
            (6, 150)
        ]
        .into()
    );
    assert_eq!(events.world.actors[&1].autonomy.unwrap().radius, 200.);
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
        events.world.render_settings,
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

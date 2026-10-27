use super::*;
use crate::{
    field_view::prediction::{Endpoint, Key, Prediction},
    sparse_animation::{Binding, Clip, affine::Locals},
};
use bevy::ecs::{change_detection::Tick, system::RunSystemOnce};
use resonance_content::animation::{
    Motion, Track, TransformChannels, VectorCurve, VectorInterpolation,
};

#[test]
fn title_pose_cache_reuses_endpoints_and_invalidates_for_new_predictions_and_commits() {
    let bytes: Vec<_> = [4u16, 0, 0, 0, 0x20ff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let mut events = resonance_events::EventRuntime::new(
        Arc::new(symphonia_script::Program::decode(&bytes).unwrap()),
        Default::default(),
    )
    .unwrap();
    events.world.tick = 10;
    let mut world = World::new();
    world.insert_resource(Events(events));
    world.init_resource::<Assets<Clip>>();
    world.init_resource::<Locals>();
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![Track {
            bone: 0,
            bind_channels: TransformChannels(0),
            period_frames: 30.,
            times: vec![0., 30.],
            translation: Some(VectorCurve {
                interpolation: VectorInterpolation::Linear,
                values: vec![[0.; 3], [30., 0., 0.]],
                incoming: vec![],
                outgoing: vec![],
                ease: vec![],
            }),
            scale: None,
            rotation: None,
            euler_degrees: None,
            matrices: None,
        }],
    };
    let clip = world
        .resource_mut::<Assets<Clip>>()
        .add(Clip(Arc::new(motion)));
    let animated = world.spawn(Transform::IDENTITY).id();
    let rest = Transform::from_xyz(3., 4., 5.);
    let unanimated = world.spawn(Transform::from_xyz(99., 0., 0.)).id();
    let root = world
        .spawn((
            Transform::IDENTITY,
            Visibility::Inherited,
            AnimatedPart {
                resource: 0,
                autoplay: true,
                clips: vec![clip],
                bone_names: vec![],
                binding: Some(Binding(vec![
                    (animated, Transform::IDENTITY),
                    (unanimated, rest),
                ])),
                schedule: vec![SceneClip {
                    motion: "test.motion".into(),
                    resource_slot: 0,
                    duration_seconds: 1.,
                    animation_resource: None,
                    secondary_pose_nodes: vec![],
                }],
                pose_key: None,
                poses: vec![],
            },
        ))
        .id();
    let mut prediction = Prediction::default();
    prediction.reserve().unwrap();
    world.insert_resource(prediction);
    let publish = |world: &mut World, tick| {
        world.resource_scope(|world, mut p: Mut<Prediction>| {
            let endpoint = p
                .execute(|| {
                    let mut sources = resonance_events::OperationFork::default();
                    let mut next = world
                        .resource::<Events>()
                        .0
                        .fork_for_prediction(&mut sources);
                    next.world.tick = tick;
                    Endpoint::Title((Default::default(), next), sources)
                })
                .unwrap();
            p.publish(
                Key::Title {
                    events: Tick::new(1),
                    menu: Tick::new(1),
                    clock: tick,
                    input: Default::default(),
                },
                endpoint,
            );
        });
    };
    publish(&mut world, 11);
    for alpha in [0., 0.25, 0.5, 0.99] {
        world.resource_mut::<Prediction>().alpha = alpha;
        world.run_system_once(animate_field).unwrap();
        let x = world.get::<Transform>(animated).unwrap().translation.x;
        assert!((x - (5. + alpha * 0.5)).abs() < 0.00001);
        assert_eq!(*world.get::<Transform>(unanimated).unwrap(), rest);
        assert_eq!(
            world.get::<AnimatedPart>(root).unwrap().pose_key.unwrap().1,
            Some(1)
        );
    }
    // A changed prediction with the same committed tick rebuilds the target.
    publish(&mut world, 12);
    world.resource_mut::<Prediction>().alpha = 0.5;
    world.run_system_once(animate_field).unwrap();
    assert_eq!(world.get::<Transform>(animated).unwrap().translation.x, 5.5);
    // Losing the prediction restores the committed pose, without retaining the blend.
    world.resource_mut::<Prediction>().publish(
        Key::Menu(Tick::new(1), Default::default()),
        Endpoint::Hold(Default::default()),
    );
    world.run_system_once(animate_field).unwrap();
    assert_eq!(world.get::<Transform>(animated).unwrap().translation.x, 5.);
    world.resource_mut::<Events>().0.world.tick = 20;
    world.run_system_once(animate_field).unwrap();
    assert_eq!(world.get::<Transform>(animated).unwrap().translation.x, 10.);
}

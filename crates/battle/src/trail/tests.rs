use super::*;
use resonance_content::animation::{Matrix, Transform};
use std::sync::Arc;

fn identity() -> Matrix {
    Transform::default().matrix()
}

fn model() -> ModelFrame {
    ModelFrame {
        actor: ActorId(0),
        visible: true,
        tint: [255; 4],
        texture_layers: [0; 4],
        light: None,
        shadow: None,
        resource: 7,
        clip: 0,
        frame: 0.,
        blend_weight: 1.,
        root_translation: [0.; 3],
        world: identity(),
        bones: Arc::new(vec![identity()]),
    }
}

fn trail(endpoints: usize) -> Trail {
    Trail::new(
        TrailDefinition {
            slot: 1,
            resource: 10,
            source: TrailSource::Body(
                (0..endpoints)
                    .map(|i| Anchor {
                        bone: 0,
                        offset: [0., i as f32, 0.],
                    })
                    .collect(),
            ),
        },
        &model(),
        &[],
    )
    .unwrap()
}

fn moving(trail: &mut Trail, tick: usize, active: bool) {
    let points: Vec<_> = (0..trail.history.len())
        .map(|i| [tick as f32, i as f32, 0.])
        .collect();
    trail.advance(active, Some(&points));
}

#[test]
fn native_history_ramp_and_expiry_keep_the_persistent_object() {
    let mut trail = trail(2);
    for tick in 1..=10 {
        moving(&mut trail, tick, true);
        assert_eq!(trail.count, tick.min(8));
    }
    assert_eq!(
        trail.history[0],
        std::array::from_fn(|i| [10. - i as f32, 0., 0.])
    );
    assert_eq!(trail.alpha, std::array::from_fn(|i| 240 - i as u8 * 16));
    let frame = trail.frame(ActorId(0)).unwrap();
    assert_eq!((frame.slot, frame.resource, frame.rows.len()), (1, 10, 2));
    assert_eq!(frame.rows[0][0].position, [10., 0., 0.]);
    assert!((frame.rows[0][15].position[0] - 3.4375).abs() < 0.00001);
    assert_eq!(&frame.rows[0][15].position[1..], &[0., 0.]);
    assert_eq!(frame.rows[1][15].alpha, 0);

    moving(&mut trail, 11, false);
    assert_eq!(trail.count, 7);
    assert_eq!(trail.history[0][0], [11., 0., 0.]);
    assert_eq!(trail.alpha[0], 224);
    assert!(trail.frame(ActorId(0)).is_some());
    for tick in 12..=18 {
        moving(&mut trail, tick, false);
    }
    assert_eq!(trail.count, 0);
    assert!(trail.frame(ActorId(0)).is_none());
    for tick in 19..=21 {
        moving(&mut trail, tick, true);
    }
    assert!(trail.frame(ActorId(0)).is_some());
}

#[test]
fn hitstop_suspends_collection_and_stationary_expiry_decrements_twice() {
    let mut trail = trail(3);
    for tick in 1..=8 {
        moving(&mut trail, tick, true);
    }
    let history = trail.history.clone();
    trail.advance(true, None);
    assert_eq!(trail.count, 8);
    assert_eq!(trail.history, history);
    trail.advance(false, None);
    assert_eq!(trail.count, 7);
    assert_eq!(trail.alpha[0], 224);
    // Movement in the third row alone is ignored by 12C38.
    trail.advance(false, Some(&[[8., 0., 0.], [8., 1., 0.], [20., 2., 0.]]));
    assert_eq!(trail.count, 5);
    assert_eq!(trail.history, history);
    assert_eq!(trail.alpha[0], 208);
    trail.advance(true, Some(&[[8.05, 0., 0.], [8., 1., 0.], [20., 2., 0.]]));
    assert_eq!(trail.count, 4);
    assert_eq!(trail.history, history);
    assert_eq!(trail.alpha[0], 240);
}

#[test]
fn chord_length_natural_spline_excludes_the_oldest_endpoint() {
    let samples = spline(&[[0., 0., 0.], [1., 1., 0.], [2., 0., 0.]]);
    assert_eq!(samples[0], [0., 0., 0.]);
    assert_eq!(samples[4], [0.5, 0.6875, 0.]);
    assert_eq!(samples[8], [1., 1., 0.]);
    assert_eq!(samples[12], [1.5, 0.6875, 0.]);
    assert_eq!(samples[15], [1.875, 0.18652344, 0.]);
    assert_eq!(spline(&[[2., 3., 4.]; 3]), [[2., 3., 4.]; 16]);
}

#[test]
fn sampling_uses_body_world_and_ordered_offsets_even_when_hidden() -> Result<()> {
    let mut model = model();
    model.world[3] = [10., 20., 30., 1.];
    let mut bone = identity();
    bone[3] = [1., 2., 3., 1.];
    model.bones = Arc::new(vec![bone]);
    model.visible = false;
    let mut trail = trail(2);
    trail.tick(4, Some((&model, &[])))?;
    assert_eq!(trail.history[0][0], [11., 22., 33.]);
    assert_eq!(trail.history[1][0], [11., 23., 33.]);
    model.world[3][0] = f32::NAN;
    assert!(trail.tick(4, Some((&model, &[]))).is_err());
    assert_eq!(trail.history[0][0], [11., 22., 33.]);
    Ok(())
}

#[test]
fn weapon_endpoints_follow_the_live_pose_and_ignore_attachment_visibility() -> Result<()> {
    let model = model();
    let mut world = identity();
    world[3] = [10., 20., 30., 1.];
    let mut tip = identity();
    tip[3] = [1., 2., 3., 1.];
    let mut weapons = vec![WeaponFrame {
        owner: model.actor,
        slot: 1,
        visible: false,
        tint: [255; 4],
        resource: 44,
        clip: Some(60),
        frame: 0.,
        world,
        bones: Arc::new(vec![identity(), tip]),
        links: vec![],
    }];
    let definition = TrailDefinition {
        slot: 1,
        resource: 8,
        source: TrailSource::Weapon {
            slot: 1,
            bones: vec![1, 0],
        },
    };
    assert_eq!(
        definition.sample(&model, &weapons)?,
        [[11., 22., 33.], [10., 20., 30.]]
    );
    tip[3][0] = 4.;
    weapons[0].bones = Arc::new(vec![identity(), tip]);
    assert_eq!(definition.sample(&model, &weapons)?[0], [14., 22., 33.]);
    assert!(definition.sample(&model, &[]).is_err());
    Ok(())
}

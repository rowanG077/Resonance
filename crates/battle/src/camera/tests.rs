use super::*;

fn actors() -> Vec<Actor> {
    let mut leader = crate::tests::actor(crate::Side::Party);
    leader.control = Control::Manual;
    let mut target = crate::tests::actor(crate::Side::Enemy);
    target.position = [400., 0., 0.];
    let mut other = target.clone();
    other.position = [-4000., 0., 0.];
    for actor in [&mut leader, &mut target, &mut other] {
        actor.body.collider = None;
        actor.body.center_offset = [0., 60., 0.];
    }
    vec![leader, target, other]
}

fn camera(actors: &[Actor], adaptive: bool) -> Camera {
    Camera::new(
        CameraDefinition {
            leader: ActorId(0),
            stage_pitch: 0.,
            adaptive,
        },
        actors,
        ActorId(u8::from(actors.len() > 1)),
    )
    .unwrap()
}

#[test]
fn current_scaled_bodies_fit_inside_the_camera_margin_after_turns() -> Result<()> {
    let mut actors = actors();
    actors[1].body.scale = 4.;
    actors[1].position = [400., 700., 0.];
    actors[1].body.collider = Some(crate::Collider::sphere(150.));
    let mut camera = camera(&actors, false);
    camera.pose.yaw = 30.;
    camera.step(&actors, ActorId(1), Update::Tracking)?;
    for actor in &actors[..2] {
        let bounds = Bounds::actor(actor);
        for corner in 0..8 {
            let point = std::array::from_fn(|axis| {
                if corner & (1 << axis) == 0 {
                    bounds.minimum[axis]
                } else {
                    bounds.maximum[axis]
                }
            });
            let [x, y] = crate::control::project_screen_point(camera.pose, point);
            assert!(
                (79.99..=560.01).contains(&x) && (55.99..=392.01).contains(&y),
                "clipped body point {point:?}: {x}, {y}"
            );
        }
    }
    assert!(
        (distance::length(sub(camera.pose.eye, camera.pose.focus)) - camera.pose.radius).abs()
            < 0.01
    );
    Ok(())
}

#[test]
fn combatant_framing_excludes_unselected_distant_actors() -> Result<()> {
    let actors = actors();
    let mut pair = camera(&actors, false);
    let mut roster = camera(&actors, true);
    pair.step(&actors, ActorId(1), Update::Tracking)?;
    roster.step(&actors, ActorId(1), Update::Tracking)?;
    assert!(pair.pose.radius < roster.pose.radius);
    assert!(pair.pose.focus[0] > 0.);
    assert!(roster.pose.focus[0] < 0.);
    Ok(())
}

#[test]
fn selection_uses_explicit_target_while_gameplay_is_held() -> Result<()> {
    let actors = actors();
    let mut camera = camera(&actors, false);
    let before = camera.pose;
    camera.step(&actors, ActorId(1), Update::Held)?;
    assert_eq!(camera.pose, before);
    camera.step(&actors, ActorId(1), Update::Selecting(ActorId(2)))?;
    assert!(camera.pose.focus[0] < before.focus[0]);
    camera.step(&actors, ActorId(1), Update::Tracking)?;
    Ok(())
}

#[test]
fn results_keep_the_party_visible_in_formation_order_without_camera_assets() -> Result<()> {
    for count in [1, 4] {
        let actors: Vec<_> = (0..count)
            .map(|index| {
                let mut actor = crate::tests::actor(crate::Side::Party);
                actor.body.collider = None;
                actor.body.center_offset = [0., 60., 0.];
                actor.body.scale = 1. + index as f32;
                actor
            })
            .collect();
        let definition = camera(&actors, false).definition;
        let mut battle = crate::PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            Default::default(),
            1,
        )?
        .with_camera(definition)?
        .finish()?;
        battle.recognize_escape(true)?;
        battle.recognize_result();
        battle.retire_combat()?;
        let party: Vec<_> = (0..count).map(ActorId).collect();
        battle.arrange_result_actors()?;
        for &actor in &party {
            battle.reset_result_actor(actor)?;
        }
        let frame = battle.step(Default::default())?;
        let pose = frame.camera.unwrap();
        let mut previous_x = f32::NEG_INFINITY;
        for actor in &frame.actors {
            let [x, _] = crate::control::project_screen_point(pose, actor.position);
            assert!(
                x > previous_x,
                "a result actor is obscured behind another party member"
            );
            previous_x = x;
            for point in [
                actor.position,
                add(
                    actor.position,
                    actor.body.center_offset.map(|v| 2. * v * actor.body.scale),
                ),
            ] {
                let [x, y] = crate::control::project_screen_point(pose, point);
                assert!((0. ..=640.).contains(&x) && (0. ..=448.).contains(&y));
            }
        }
    }
    Ok(())
}

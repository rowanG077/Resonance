use super::*;
use crate::overworld::World;
use crate::overworld::{
    TileCoordinate,
    collision::tests::{rectangle, tables},
};

pub(crate) fn parameters() -> Arc<MovementParameters> {
    Arc::new(
        serde_json::from_str(include_str!(
            "../../../../content/tests/data/overworld-movement.json"
        ))
        .unwrap(),
    )
}
pub(crate) fn state(mount: Mount) -> State {
    State {
        world: World::Sylvarant,
        position: Position::from_map([3200., 3200., 0.]).unwrap(),
        heading: 0.,
        camera_yaw: 0.,
        alternate_perspective: false,
        map_display: Default::default(),
        mount,
        altitude: if mount.airborne() { 300. } else { 0. },
    }
}
pub(crate) fn terrain(surface: u32) -> collision::Terrain {
    let mut groups = Vec::new();
    for x in 0..16 {
        for z in 0..16 {
            let min = [x as f32 * 400. - 3200., z as f32 * 400. - 3200.];
            groups.push(rectangle(surface, min, min.map(|v| v + 400.), 0.));
        }
    }
    let center = TileCoordinate::new(0, 0).unwrap();
    collision::Terrain::new(
        (-1..=1)
            .flat_map(|x| (-1..=1).map(move |z| [x, z]))
            .map(|offset| {
                (
                    center.neighbor(offset),
                    collision::Mesh::new(&groups).unwrap(),
                )
            }),
        tables(),
    )
    .unwrap()
}
fn rules() -> Rules {
    Rules::new(crate::overworld::tests::tables()).unwrap()
}

#[test]
fn world_views_cycle_fade_and_survive_restore_for_every_mount() -> Result<()> {
    use resonance_content::overworld::MapDisplay;
    let rules = rules();
    let context = Context {
        event_flags: &Default::default(),
        rheairds_owned: true,
        landing_clear: true,
    };
    for mount in [Mount::Foot, Mount::Noishe, Mount::Rheairds, Mount::Ship] {
        let terrain = terrain(if mount == Mount::Ship { 0 } else { 1 });
        let mut travel = Travel::new(state(mount), parameters())?;
        let position = travel.state.position;
        let original_distance = travel.camera_distance();
        travel.step(
            Input {
                toggle_perspective: true,
                cycle_map: true,
                ..Default::default()
            },
            &context,
            &terrain,
            &rules,
        )?;
        assert!(travel.state.alternate_perspective);
        assert_eq!(travel.state.map_display, MapDisplay::Full);
        assert_eq!(travel.map_opacity(), [239, 16]);
        assert!(travel.camera_distance() > original_distance);
        assert!(travel.camera_distance() < original_distance + 600.);
        for _ in 0..30 {
            travel.step(Input::default(), &context, &terrain, &rules)?;
        }
        assert_eq!(travel.map_opacity(), [0, 255]);
        assert_eq!(travel.camera_distance(), original_distance + 600.);
        assert_eq!(travel.state.position, position);
        let checkpoint = travel.checkpoint()?;
        let encoded = serde_json::to_value(&checkpoint)?;
        let restored = Travel::new(serde_json::from_value(encoded.clone())?, parameters())?;
        assert_eq!(restored.state(), &checkpoint);
        assert_eq!(restored.map_opacity(), [0, 255]);
        assert_eq!(restored.camera_distance(), travel.camera_distance());
        let mut legacy = encoded;
        legacy.as_object_mut().unwrap().remove("map_display");
        assert_eq!(
            serde_json::from_value::<State>(legacy)?.map_display,
            MapDisplay::Small
        );
        for display in [MapDisplay::Hidden, MapDisplay::Small] {
            travel.step(
                Input {
                    cycle_map: true,
                    ..Default::default()
                },
                &context,
                &terrain,
                &rules,
            )?;
            for _ in 0..15 {
                travel.step(Input::default(), &context, &terrain, &rules)?;
            }
            assert_eq!(travel.state.map_display, display);
            assert_eq!(travel.map_opacity(), display.opacity());
        }
    }
    Ok(())
}
fn finish(travel: &mut Travel, context: &Context<'_>, terrain: &collision::Terrain, rules: &Rules) {
    for _ in 0..240 {
        if travel.player_has_control() {
            return;
        }
        travel
            .step(Input::default(), context, terrain, rules)
            .unwrap();
    }
    panic!("mount transition did not finish");
}

#[test]
fn noishe_waits_for_both_zoom_and_its_own_animation() -> Result<()> {
    let mut travel = Travel::new(state(Mount::Foot), parameters())?;
    let terrain = terrain(1);
    let rules = rules();
    let context = Context {
        event_flags: &[903].into(),
        rheairds_owned: false,
        landing_clear: true,
    };
    let start = travel.state.position;
    let cues = travel.step(
        Input {
            toggle_noishe: true,
            ..Default::default()
        },
        &context,
        &terrain,
        &rules,
    )?;
    let token = cues
        .iter()
        .find_map(|cue| {
            if let Cue::Animation { token, .. } = cue {
                Some(*token)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(travel.displayed_mount(), Mount::Noishe);
    assert!(travel.checkpoint().is_err());
    assert!(!travel.finish_animation(token + 1));
    for _ in 0..60 {
        travel.step(
            Input {
                stick: [1., 0.],
                toggle_noishe: true,
                ..Default::default()
            },
            &context,
            &terrain,
            &rules,
        )?;
    }
    assert_eq!(travel.state.position, start);
    assert!(!travel.player_has_control());
    assert_eq!(travel.camera_distance(), 2800.);
    assert!(travel.finish_animation(token));
    assert!(!travel.finish_animation(token));
    assert_eq!(
        travel.step(Input::default(), &context, &terrain, &rules)?,
        [Cue::Finished(Mount::Noishe)]
    );
    travel.step(
        Input {
            stick: [1., 0.],
            ..Default::default()
        },
        &context,
        &terrain,
        &rules,
    )?;
    assert!((travel.speed() - 80. / 6.8).abs() < 0.001);
    let locked_region = Context {
        event_flags: &BTreeSet::new(),
        ..context
    };
    let cues = travel.step(
        Input {
            toggle_noishe: true,
            ..Default::default()
        },
        &locked_region,
        &terrain,
        &rules,
    )?;
    let token2 = cues
        .iter()
        .find_map(|cue| {
            if let Cue::Animation { token, .. } = cue {
                Some(*token)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(token, token2);
    assert_eq!(travel.displayed_mount(), Mount::Noishe);
    travel.finish_animation(token2);
    finish(&mut travel, &locked_region, &terrain, &rules);
    assert_eq!(travel.displayed_mount(), Mount::Foot);
    assert_eq!(travel.camera_distance(), 1200.);
    assert_eq!(
        travel.step(
            Input {
                toggle_noishe: true,
                ..Default::default()
            },
            &locked_region,
            &terrain,
            &rules
        )?,
        [Cue::Denied]
    );
    Ok(())
}

#[test]
fn rheairds_require_ownership_and_permission_and_land_only_on_clear_ground() -> Result<()> {
    let mut travel = Travel::new(state(Mount::Foot), parameters())?;
    let ground = terrain(1);
    let rules = rules();
    let flags = [22].into();
    let mut context = Context {
        event_flags: &flags,
        rheairds_owned: false,
        landing_clear: true,
    };
    let board = Input {
        vehicle: true,
        ..Default::default()
    };
    assert_eq!(
        travel.step(board, &context, &ground, &rules)?,
        [Cue::Denied]
    );
    context.rheairds_owned = true;
    travel.step(board, &context, &ground, &rules)?;
    assert_eq!(travel.state.mount, Mount::Rheairds);
    assert_eq!(travel.state.altitude, 2.5);
    assert!(!travel.player_has_control());
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.state.altitude, 300.);
    let saved = travel.checkpoint()?;
    let restored = Travel::new(
        serde_json::from_str(&serde_json::to_string(&saved)?)?,
        parameters(),
    )?;
    assert_eq!(restored.state(), &saved);
    context.landing_clear = false;
    assert_eq!(
        travel.step(board, &context, &ground, &rules)?,
        [Cue::Denied]
    );
    context.landing_clear = true;
    assert_eq!(
        travel.step(board, &context, &terrain(11), &rules)?,
        [Cue::Denied]
    );
    travel.step(board, &context, &ground, &rules)?;
    assert_eq!(travel.state.mount, Mount::Foot);
    assert_eq!(travel.displayed_mount(), Mount::Rheairds);
    assert_eq!(travel.state.altitude, 280.);
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.state.altitude, 0.);
    assert_eq!(travel.displayed_mount(), Mount::Foot);
    Ok(())
}

#[test]
fn flight_inertia_is_separate_from_direct_motion_and_turning_caps_speed() -> Result<()> {
    let mut travel = Travel::new(state(Mount::Rheairds), parameters())?;
    let terrain = terrain(11); // Flight can traverse ocean, without a landing response.
    let rules = rules();
    let context = Context {
        event_flags: &BTreeSet::new(),
        rheairds_owned: true,
        landing_clear: true,
    };
    let origin = travel.state.position;
    travel.step(
        Input {
            secondary: [1., 0.],
            ..Default::default()
        },
        &context,
        &terrain,
        &rules,
    )?;
    assert!(travel.state.position.map()[0] > origin.map()[0] + 47.);
    travel.step(Input::default(), &context, &terrain, &rules)?;
    assert_eq!(travel.speed(), 0.);
    for _ in 0..31 {
        travel.step(
            Input {
                throttle: true,
                ..Default::default()
            },
            &context,
            &terrain,
            &rules,
        )?;
    }
    let maximum = 4. * 80. / 6.8;
    assert!((travel.speed() - maximum).abs() < 0.001);
    for _ in 0..31 {
        travel.step(
            Input {
                throttle: true,
                stick: [1., -1.],
                ..Default::default()
            },
            &context,
            &terrain,
            &rules,
        )?;
    }
    assert_eq!(travel.bank(), 48.);
    assert!((travel.speed() - (maximum - 16.)).abs() < 0.001);
    assert_eq!(travel.pitch(), -64.);
    assert!(travel.state.altitude > 300.);
    for _ in 0..31 {
        travel.step(Input::default(), &context, &terrain, &rules)?;
    }
    assert_eq!(travel.speed(), 0.);
    assert_eq!(travel.bank(), 0.);
    assert_eq!(travel.pitch(), 0.);
    Ok(())
}

#[test]
fn ship_transfers_at_boarding_completion_and_disembarks_only_at_paired_docks() -> Result<()> {
    let mut data = crate::overworld::tests::tables();
    for pair in &mut data.travel_points {
        pair[0].map_x = 3200;
        pair[0].map_z = 3200;
        pair[1].map_x = 3800;
        pair[1].map_z = 3200;
    }
    let rules = Rules::new(data)?;
    let mut travel = Travel::new(state(Mount::Foot), parameters())?;
    let context = Context {
        event_flags: &[23].into(),
        rheairds_owned: false,
        landing_clear: true,
    };
    let ground = terrain(15);
    let board = Input {
        vehicle: true,
        ..Default::default()
    };
    travel.step(board, &context, &ground, &rules)?;
    for _ in 1..29 {
        assert_eq!(travel.ship_pose().unwrap().0.map()[0], 3800.);
        travel.step(Input::default(), &context, &ground, &rules)?;
    }
    assert_eq!(travel.state.position.map()[0], 3200.);
    travel.step(Input::default(), &context, &ground, &rules)?;
    assert_eq!(travel.state.position.map()[0], 3800.);
    let water = terrain(11);
    finish(&mut travel, &context, &water, &rules);
    assert_eq!(travel.state.mount, Mount::Ship);
    travel.step(board, &context, &water, &rules)?;
    assert_eq!(travel.state.position.map()[0], 3200.);
    assert_eq!(travel.displayed_mount(), Mount::Ship);
    assert_eq!(travel.ship_pose().unwrap().0.map()[0], 3800.);
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.displayed_mount(), Mount::Foot);
    let mut away = state(Mount::Ship);
    away.position = Position::from_map([4000., 3200., 0.])?;
    let mut travel = Travel::new(away, parameters())?;
    assert_eq!(travel.step(board, &context, &water, &rules)?, [Cue::Denied]);
    Ok(())
}

#[test]
fn invalid_input_or_missing_tiles_cannot_partly_commit_an_update() -> Result<()> {
    let mut travel = Travel::new(state(Mount::Foot), parameters())?;
    let context = Context {
        event_flags: &[903].into(),
        rheairds_owned: false,
        landing_clear: true,
    };
    let rules = rules();
    let ground = terrain(1);
    let before = travel.checkpoint()?;
    assert!(
        travel
            .step(
                Input {
                    toggle_noishe: true,
                    stick: [f32::NAN, 0.],
                    ..Default::default()
                },
                &context,
                &ground,
                &rules
            )
            .is_err()
    );
    assert_eq!(travel.checkpoint()?, before);
    let incomplete = collision::Terrain::new(
        [(TileCoordinate::new(5, 5)?, collision::Mesh::new(&[])?)],
        tables(),
    )?;
    assert!(
        travel
            .step(
                Input {
                    toggle_noishe: true,
                    ..Default::default()
                },
                &context,
                &incomplete,
                &rules
            )
            .is_err()
    );
    assert_eq!(travel.checkpoint()?, before);
    Ok(())
}

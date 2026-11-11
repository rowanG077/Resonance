use super::*;
use crate::overworld::World;
use crate::overworld::{
    TileCoordinate,
    collision::tests::{rectangle, tables},
};
use resonance_content::overworld::TerrainSurface;

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
pub(crate) fn terrain(surface: TerrainSurface) -> collision::Terrain {
    terrain_at_height(surface, 0.)
}
pub(crate) fn terrain_at_height(surface: TerrainSurface, height: f32) -> collision::Terrain {
    let mut groups = Vec::new();
    for x in 0..16 {
        for z in 0..16 {
            let min = [x as f32 * 400. - 3200., z as f32 * 400. - 3200.];
            groups.push(rectangle(surface, min, min.map(|v| v + 400.), height));
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
fn enemy_visibility_uses_ground_distance_in_both_views() -> Result<()> {
    // Foot/Noishe horizons are 700/1,000 units, with a 255-unit fade.
    // Distance truncation preserves alpha one at the visible edge.
    for perspective in [false, true] {
        for (mount, samples) in [
            (
                Mount::Foot,
                [(445.25, 255), (600.5, 100), (699.75, 1), (700., 0)],
            ),
            (
                Mount::Noishe,
                [(745.25, 255), (900.5, 100), (999.75, 1), (1000., 0)],
            ),
        ] {
            let mut state = state(mount);
            state.alternate_perspective = perspective;
            // Cross the wrapping map seam and ignore vertical separation.
            state.position = Position::from_map([76790., 100., 80.])?;
            let travel = Travel::new(state, parameters())?;
            for (distance, expected) in samples {
                let position = travel.state.position.translated([distance, 0., 900.])?;
                assert_eq!(travel.enemy_opacity(position), expected);
            }
            assert_eq!(travel.enemy_opacity(travel.state.position), 255);
            assert_eq!(
                travel.enemy_opacity(travel.state.position.translated([1050., 0., 0.])?),
                0,
                "newly admitted symbols stay resident beyond their draw horizon"
            );
        }
    }
    Ok(())
}

#[test]
fn enemy_fade_horizon_tracks_mount_changes_during_perspective_zooms() -> Result<()> {
    let terrain = terrain(TerrainSurface::Grassland);
    let rules = rules();
    let context = Context {
        event_flags: &[903].into(),
        rheairds_owned: false,
        landing_clear: true,
    };
    let mut parameters = (*parameters()).clone();
    parameters.zoom_transition_ticks = 4.;
    let parameters = Arc::new(parameters);
    for perspective in [false, true] {
        for (mount, expected) in [
            (Mount::Foot, [25, 100, 175, 250]),
            (Mount::Noishe, [175, 100, 25, 0]),
        ] {
            // Change perspective with the mount action or one update before it.
            for perspective_change in [None, Some(0), Some(1)] {
                let mut state = state(mount);
                state.alternate_perspective = perspective;
                let mut travel = Travel::new(state, parameters.clone())?;
                let position = travel.state.position.translated([750.5, 0., 0.])?;
                if perspective_change == Some(1) {
                    travel.step(
                        Input {
                            toggle_perspective: true,
                            ..Default::default()
                        },
                        &context,
                        &terrain,
                        &rules,
                    )?;
                }
                for (tick, expected) in expected.into_iter().enumerate() {
                    let cues = travel.step(
                        Input {
                            toggle_noishe: tick == 0,
                            toggle_perspective: tick == 0 && perspective_change == Some(0),
                            ..Default::default()
                        },
                        &context,
                        &terrain,
                        &rules,
                    )?;
                    if tick == 0 {
                        let token = cues
                            .iter()
                            .find_map(|cue| match cue {
                                Cue::Animation { token, .. } => Some(*token),
                                _ => None,
                            })
                            .unwrap();
                        assert!(travel.finish_animation(token));
                    }
                    assert_eq!(travel.enemy_opacity(position), expected);
                }
            }
        }
    }
    for mount in [Mount::Rheairds, Mount::Ship] {
        let travel = Travel::new(state(mount), parameters.clone())?;
        assert_eq!(travel.enemy_opacity(travel.state.position), 0);
    }
    Ok(())
}

#[test]
fn perspective_zoom_keeps_the_enemy_horizon_on_foot_and_noishe() -> Result<()> {
    let terrain = terrain(TerrainSurface::Grassland);
    let rules = rules();
    let context = Context {
        event_flags: &[903].into(),
        rheairds_owned: false,
        landing_clear: true,
    };
    for perspective in [false, true] {
        for mount in [Mount::Foot, Mount::Noishe] {
            let mut state = state(mount);
            state.alternate_perspective = perspective;
            let mut travel = Travel::new(state, parameters())?;
            let radius = if mount == Mount::Foot { 700. } else { 1000. };
            let fading = travel.state.position.translated([radius - 49.5, 0., 0.])?;
            let hidden = travel.state.position.translated([radius + 50.5, 0., 0.])?;
            for tick in 0..30 {
                travel.step(
                    Input {
                        toggle_perspective: tick == 0,
                        ..Default::default()
                    },
                    &context,
                    &terrain,
                    &rules,
                )?;
                assert_eq!(travel.enemy_opacity(fading), 50);
                assert_eq!(travel.enemy_opacity(hidden), 0);
                assert!(travel.player_has_control());
            }
            assert_eq!(travel.state.alternate_perspective, !perspective);
            assert_eq!(
                travel.camera_distance(),
                travel.parameters.camera_distances.for_view(!perspective)
                    [usize::from(mount.long_range())]
            );
        }
    }
    Ok(())
}

#[test]
fn foot_travel_runs_by_default_and_walk_modifier_limits_speed_to_a_walk() -> Result<()> {
    let terrain = terrain(TerrainSurface::Grassland);
    let rules = rules();
    let context = Context {
        event_flags: &Default::default(),
        rheairds_owned: true,
        landing_clear: true,
    };
    let p = parameters();
    let maximum = p.stick_maximum / p.foot_stick_divisor;
    let limit = resonance_events::stride::MAXIMUM_WALK_SPEED;
    let walking = limit / maximum;
    assert!(walking < 0.5);
    for (magnitude, walk, fraction) in [
        (1., false, 1.),
        (1., true, walking),
        (0.5, true, walking),
        (0.5, false, 0.5),
        (0.25, false, 0.25),
        (0.25, true, 0.25),
    ] {
        let mut travel = Travel::new(state(Mount::Foot), p.clone())?;
        travel.step(
            Input {
                stick: [magnitude, 0.],
                walk,
                ..Default::default()
            },
            &context,
            &terrain,
            &rules,
        )?;
        assert!((travel.speed() - maximum * fraction).abs() < 0.001);
        assert!(!walk || travel.speed() <= limit);
    }
    Ok(())
}

#[test]
fn compass_steering_follows_camera_input_instead_of_accumulating() -> Result<()> {
    let terrain = terrain(TerrainSurface::Grassland);
    let rules = rules();
    let context = Context {
        event_flags: &Default::default(),
        rheairds_owned: true,
        landing_clear: true,
    };
    let mut travel = Travel::new(state(Mount::Foot), parameters())?;
    for steering in [0., 1., 1., -1., 0., 0.] {
        let previous = travel.state().camera_yaw;
        travel.step(
            Input {
                rotate_camera: steering,
                ..Default::default()
            },
            &context,
            &terrain,
            &rules,
        )?;
        assert_eq!(
            travel.camera_turn_input(),
            steering * parameters().stick_maximum
        );
        assert_eq!(travel.state().camera_yaw == previous, steering == 0.);
    }
    Ok(())
}

#[test]
fn world_views_cycle_fade_and_survive_restore_for_every_mount() -> Result<()> {
    use resonance_content::overworld::MapDisplay;
    let rules = rules();
    let context = Context {
        event_flags: &[903].into(),
        rheairds_owned: true,
        landing_clear: true,
    };
    for mount in [Mount::Foot, Mount::Noishe, Mount::Rheairds, Mount::Ship] {
        let terrain = terrain(if mount == Mount::Ship {
            TerrainSurface::Barrier
        } else {
            TerrainSurface::Grassland
        });
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
    let terrain = terrain(TerrainSurface::Grassland);
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
fn noishe_dismounts_after_crossing_only_when_the_destination_region_is_locked() -> Result<()> {
    let terrain = terrain(TerrainSurface::Grassland);
    for world in [World::Sylvarant, World::TetheAlla] {
        let mut tables = crate::overworld::tests::tables();
        tables.area_grids[world.index()][1][1] = 3;
        let rules = Rules::new(tables)?;
        let mut state = state(Mount::Noishe);
        state.world = world;
        state.position = Position::from_map([6395., 3200., 0.])?;
        let travel = Travel::new(state, parameters())?;
        let base = 900 + world.index() as u16 * 11;
        for unlocked in [false, true] {
            // The other world's region-zero flag cannot unlock this boundary.
            let mut flags: BTreeSet<_> = [base + 3, 900 + (1 - world.index() as u16) * 11].into();
            if unlocked {
                flags.insert(base);
            }
            let context = Context {
                event_flags: &flags,
                rheairds_owned: false,
                landing_clear: true,
            };
            let running = Input {
                stick: [1., 0.],
                walk: true,
                ..Default::default()
            };
            let mut travel = travel.clone();
            assert!(travel.step(running, &context, &terrain, &rules)?.is_empty());
            assert_eq!(rules.area(world, travel.state.position), 0);
            assert_eq!(travel.state.mount, Mount::Noishe);
            let crossed = travel.state.position;
            let expected_mount = if unlocked { Mount::Noishe } else { Mount::Foot };
            let mut released = travel.clone();
            released.step(Input::default(), &context, &terrain, &rules)?;
            assert_eq!(released.state.mount, expected_mount);
            assert_eq!(released.state.position, crossed);

            let cues = travel.step(running, &context, &terrain, &rules)?;
            assert_eq!(travel.state.mount, expected_mount);
            assert_eq!(travel.player_has_control(), unlocked);
            assert!((crossed.distance_to(travel.state.position) - 80. / 6.8).abs() < 0.05);
            if unlocked {
                assert!(cues.is_empty());
            } else {
                assert!(matches!(
                    cues.as_slice(),
                    [
                        Cue::Animation {
                            mounting: false,
                            ..
                        },
                        Cue::Started {
                            from: Mount::Noishe,
                            to: Mount::Foot
                        }
                    ]
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn forced_noishe_dismount_matches_manual_camera_and_enemy_visibility() -> Result<()> {
    let terrain = terrain(TerrainSurface::Grassland);
    let rules = rules();
    let unlocked = Context {
        event_flags: &[903].into(),
        rheairds_owned: false,
        landing_clear: true,
    };
    let locked = Context {
        event_flags: &Default::default(),
        ..unlocked
    };
    for alternate in [false, true] {
        for view_ticks in [0, 1] {
            let mut state = state(Mount::Noishe);
            state.alternate_perspective = alternate;
            let mut forced = Travel::new(state, parameters())?;
            for tick in 0..view_ticks {
                forced.step(
                    Input {
                        toggle_perspective: tick == 0,
                        ..Default::default()
                    },
                    &unlocked,
                    &terrain,
                    &rules,
                )?;
            }
            let mut manual = forced.clone();
            let distance = forced.camera_distance();
            let enemy = forced.state.position.translated([750.5, 0., 0.])?;
            let input = Input {
                toggle_perspective: view_ticks == 0,
                ..Default::default()
            };
            forced.step(input, &locked, &terrain, &rules)?;
            manual.step(
                Input {
                    toggle_noishe: true,
                    ..input
                },
                &locked,
                &terrain,
                &rules,
            )?;
            assert_eq!(forced.state.mount, Mount::Foot);
            assert!(forced.camera_distance() < distance);
            assert_eq!(forced.camera_distance(), manual.camera_distance());
            assert_eq!(forced.enemy_opacity(enemy), manual.enemy_opacity(enemy));
        }
    }
    Ok(())
}

#[test]
fn rheairds_require_ownership_and_permission_and_land_only_on_clear_ground() -> Result<()> {
    let mut travel = Travel::new(state(Mount::Foot), parameters())?;
    let ground = terrain(TerrainSurface::Grassland);
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
        travel.step(board, &context, &terrain(TerrainSurface::Water), &rules)?,
        [Cue::Denied]
    );
    travel.step(board, &context, &ground, &rules)?;
    assert_eq!(travel.state.mount, Mount::Foot);
    assert_eq!(travel.displayed_mount(), Mount::Rheairds);
    assert_eq!(travel.state.altitude, 280.);
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.state.altitude, 0.);
    assert_eq!(travel.displayed_mount(), Mount::Foot);
    // A raised flight floor changes the takeoff destination without snapping
    // the vehicle upward. Landing must still finish on the underlying terrain.
    travel.step(board, &context, &ground, &rules)?;
    travel.clear_flight_obstacle(500.)?;
    assert_eq!(travel.state.altitude, 2.5);
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.state.altitude, 800.);
    travel.step(board, &context, &ground, &rules)?;
    assert_eq!(travel.state.position.map()[2], 0.);
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.state.altitude, 0.);
    Ok(())
}

#[test]
fn flight_inertia_is_separate_from_direct_motion_and_turning_caps_speed() -> Result<()> {
    let mut travel = Travel::new(state(Mount::Rheairds), parameters())?;
    let terrain = terrain(TerrainSurface::Water); // Flight can traverse ocean, without a landing response.
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
        pair[0].height = 120.;
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
    let ground = terrain_at_height(TerrainSurface::Pier, 120.);
    let board = Input {
        vehicle: true,
        ..Default::default()
    };
    travel.step(Input::default(), &context, &ground, &rules)?;
    assert_eq!(
        travel.ship_action(&context, &rules).unwrap().0,
        ShipAction::Embark
    );
    assert!(
        travel
            .ship_action(
                &Context {
                    event_flags: &BTreeSet::new(),
                    ..context
                },
                &rules
            )
            .is_none()
    );
    assert!(
        travel
            .ship_action(
                &Context {
                    event_flags: &[22, 23].into(),
                    rheairds_owned: true,
                    ..context
                },
                &rules
            )
            .is_none()
    );
    travel.step(board, &context, &ground, &rules)?;
    assert!(travel.ship_action(&context, &rules).is_none());
    for _ in 1..29 {
        assert_eq!(travel.ship_pose().unwrap().0.map()[0], 3800.);
        travel.step(Input::default(), &context, &ground, &rules)?;
    }
    assert_eq!(travel.state.position.map()[0], 3200.);
    travel.step(Input::default(), &context, &ground, &rules)?;
    assert_eq!(travel.state.position.map()[0], 3800.);
    let water = terrain(TerrainSurface::Water);
    finish(&mut travel, &context, &water, &rules);
    assert_eq!(travel.state.mount, Mount::Ship);
    assert_eq!(
        travel.ship_action(&context, &rules).unwrap().0,
        ShipAction::Disembark
    );
    travel.step(board, &context, &water, &rules)?;
    assert_eq!(travel.state.position.map()[0], 3200.);
    assert_eq!(travel.displayed_mount(), Mount::Ship);
    assert_eq!(travel.ship_pose().unwrap().0.map()[0], 3800.);
    for _ in 1..30 {
        assert_eq!(travel.state.position.map()[2], 120.);
        assert_eq!(travel.state.altitude, 120.);
        travel.step(Input::default(), &context, &ground, &rules)?;
    }
    finish(&mut travel, &context, &ground, &rules);
    assert_eq!(travel.displayed_mount(), Mount::Foot);
    assert_eq!(travel.state.altitude, 120.);
    let mut away = state(Mount::Ship);
    away.position = Position::from_map([4000., 3200., 0.])?;
    let mut travel = Travel::new(away, parameters())?;
    assert!(travel.ship_action(&context, &rules).is_none());
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
    let ground = terrain(TerrainSurface::Grassland);
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

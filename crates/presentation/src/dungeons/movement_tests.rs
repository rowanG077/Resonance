use super::*;

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn martel_golem_blocks_fill_both_north_gaps() -> Result<()> {
    const GOLEM: i32 = 101;
    const MOVING_BLOCK: i32 = 5000;
    const FIRST_GAP: u16 = 209;
    const SECOND_GAP: u16 = 210;
    for regrab in [false, true] {
        // Before receiving the ring, neither gap is filled by story progression.
        let mut field = enter(MARTEL_START, 308, Some(105_000))?;
        advance_until(&mut field, FieldSession::player_has_control)?;
        for (cells, flag, placed) in [(3, FIRST_GAP, 5003), (4, SECOND_GAP, 5004)] {
            assert!(!field.events.world.event_flags.contains(&flag));
            assert!(field.actor(placed).model_collision.is_none());
            assert!(field.events.contact_enemy(GOLEM)?);
            advance_until(&mut field, |f| f.events.world.battle_request.is_some())?;
            assert!(skip_battle(&mut field)?);
            advance_until(&mut field, FieldSession::player_has_control)?;

            // Deliver the script-created block through the upper room's center hole.
            for (axis, target) in [(1, -1375.), (0, -890.)] {
                let start = field.actor(MOVING_BLOCK).position;
                let mut direction = [0.; 2];
                direction[axis] = (target - start[axis]).signum();
                let distance = ((target - start[axis]).abs() / 150.).round() as usize;
                if distance > 0 {
                    grab_block(&mut field, MOVING_BLOCK, direction)?;
                    push(&mut field, direction, distance * 50)?;
                    field.step(FieldInput::default())?;
                    assert!((field.actor(MOVING_BLOCK).position[axis] - target).abs() < 1.);
                }
            }
            ticks(&mut field, 70, FieldInput::default())?;
            assert!((field.actor(MOVING_BLOCK).position[2] + 800.).abs() < 1.);
            grab_block(&mut field, MOVING_BLOCK, [0., 1.])?;
            for cell in 1..=cells {
                push(&mut field, [0., 1.], 50)?;
                let block = field.actor(MOVING_BLOCK);
                let expected_y = -1375. + 150. * cell as f32;
                assert!(
                    (block.position[1] - expected_y).abs() < 1.,
                    "regrab={regrab}, gap={flag}: stopped before {expected_y}: {:?}",
                    block.position
                );
                if regrab && cell < cells {
                    field.step(FieldInput::default())?;
                    field.step(FieldInput {
                        interact: true,
                        ..Default::default()
                    })?;
                    assert_eq!(field.events.world.grabbed_block, Some(MOVING_BLOCK));
                }
            }
            ticks(&mut field, 70, FieldInput::default())?;
            assert!(field.events.world.event_flags.contains(&flag));
            assert!(!field.events.world.actors.contains_key(&MOVING_BLOCK));
            assert!(field.actor(placed).model_collision.is_some());
            assert!(field.player_has_control());
        }
        let player = field.events.world.controlled_actor;
        field.actor_mut(player).position = [-890., -1050., -800.];
        let walking = FieldInput {
            held_buttons: Default::default(),
            ..block_input(&field, [0., 1.])
        };
        ticks(&mut field, 75, walking)?;
        assert!(field.actor(player).position[1] > -800.);
        assert!((field.actor(player).position[2] + 799.).abs() < 1.);
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_two_blocks_fill_the_gap_and_allow_crossing() -> Result<()> {
    for (order, regrab) in [
        ([5000, 5001], false),
        ([5000, 5001], true),
        ([5001, 5000], false),
        ([5001, 5000], true),
    ] {
        let lane = 75.;
        let mut field = enter(TRIET_START, 220, None)?;
        advance_until(&mut field, FieldSession::player_has_control)?;
        for (index, block) in order.into_iter().enumerate() {
            let start = field.actor(block).position;
            let destination_x = if index == 0 { -1125. } else { -1275. };
            let routes = [
                (
                    [0., (lane - start[1]).signum()],
                    ((lane - start[1]).abs() / 150.).round() as usize,
                ),
                (
                    [-1., 0.],
                    ((start[0] - destination_x) / 150.).round() as usize,
                ),
            ];
            for (direction, cells) in routes {
                let start = field.actor(block).position;
                grab_block(&mut field, block, direction)?;
                let held = block_input(&field, direction);
                for cell in 1..=cells {
                    ticks(&mut field, 50, held)?;
                    let expected = [
                        start[0] + direction[0] * 150. * cell as f32,
                        start[1] + direction[1] * 150. * cell as f32,
                    ];
                    assert!(
                        (field.actor(block).position[0] - expected[0]).abs() < 1.
                            && (field.actor(block).position[1] - expected[1]).abs() < 1.,
                        "lane {lane}, regrab {regrab}, block {block} stopped before {expected:?}: {:?}",
                        field.actor(block).position
                    );
                    if regrab && cell < cells {
                        field.step(FieldInput::default())?;
                        field.step(FieldInput {
                            interact: true,
                            ..Default::default()
                        })?;
                        assert_eq!(
                            field.events.world.grabbed_block,
                            Some(block),
                            "regrabbing at {expected:?}"
                        );
                    }
                }
                ticks(&mut field, 30, FieldInput::default())?;
            }
            assert!(field.actor(block).position[2] < -100.);
            let placed = match (block, index) {
                (5000, 0) => 589,
                (5000, _) => 588,
                (5001, 0) => 590,
                _ => 230,
            };
            assert!(
                field.events.world.event_flags.contains(&placed),
                "block {block} at {:?} did not persist its placement",
                field.actor(block).position
            );
        }
        let player = field.events.world.controlled_actor;
        field.actor_mut(player).position = [-1000., lane, 0.];
        for _ in 0..70 {
            let camera = field.events.world.field_camera.as_ref().unwrap();
            let angle = -(camera.target[0] - camera.position[0])
                .atan2(camera.target[1] - camera.position[1]);
            field.step(FieldInput {
                direction: [-angle.cos(), angle.sin()],
                ..Default::default()
            })?;
        }
        assert!(
            field.actor(player).position[0] < -1250.,
            "cannot cross the filled gap: {:?}",
            field.actor(player).position
        );
        assert!(field.actor(player).position[2].abs() < 1.);
        for _ in 0..210 {
            let camera = field.events.world.field_camera.as_ref().unwrap();
            let angle = -(camera.target[0] - camera.position[0])
                .atan2(camera.target[1] - camera.position[1]);
            field.step(FieldInput {
                direction: [-angle.sin(), -angle.cos()],
                ..Default::default()
            })?;
        }
        assert!(
            field.actor(player).position[1] < -700. && field.actor(player).position[2] > 300.,
            "cannot leave filled gap for the ramp: {:?}",
            field.actor(player).position
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn triet_marked_tiles_stop_blocks() -> Result<()> {
    for (block, direction, stop) in [(5000, [0., 1.], 825.), (5001, [0., -1.], -825.)] {
        let mut field = enter(TRIET_START, 220, None)?;
        advance_until(&mut field, FieldSession::player_has_control)?;
        grab_block(&mut field, block, direction)?;
        push(&mut field, direction, 200)?;
        assert!(
            (field.actor(block).position[1] - stop).abs() < 1.,
            "block crossed marked tile: {:?}",
            field.actor(block).position
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn generator_blocks_reach_the_lift_and_drop_from_ledges() -> Result<()> {
    for (block, position, expected, ticks) in [
        (300, [-250., 150., 0.], [-675., 150., 0.], 100),
        (303, [-250., -150., 150.], [-675., -150., 0.], 100),
        (301, [500., 150., 0.], [-675., 150., 0.], 350),
    ] {
        let mut field = enter(BASE_GENERATOR, 279, None)?;
        advance_until(&mut field, FieldSession::player_has_control)?;
        if block == 301 {
            field.actor_mut(300).position = [-825., 150., 0.];
            field.actor_mut(301).position = [375., 150., 0.];
        }
        field.actor_mut(1).position = position;
        field.actor_mut(1).face(270.);
        field.step(FieldInput {
            interact: true,
            ..Default::default()
        })?;
        push(&mut field, [-1., 0.], ticks)?;
        super::ticks(&mut field, 30, FieldInput::default())?;
        assert_eq!(field.actor(block).position, expected);
        assert!(field.player_has_control());
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn forest_cliff_jumps_reach_each_ledge() -> Result<()> {
    let mut field = configured(MARTEL_START, 193, |entry| {
        entry
            .persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 303_000)?;
        entry.position = [-980., 1350., 0.];
        Ok(())
    })?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    for (jump, height) in [
        (2000, 250.),
        (2002, 0.),
        (2000, 250.),
        (2001, 500.),
        (2005, 250.),
        (2001, 500.),
        (2003, 750.),
        (2007, 500.),
    ] {
        assert!(field.events.trigger(jump, true)?);
        advance_until(&mut field, FieldSession::player_has_control)?;
        let player = &field.events.world.actors[&field.events.world.controlled_actor];

        assert!(player.attachment.is_none());
        assert!(
            (player.position[2] - height).abs() < 2.,
            "jump {jump}: {:?}, expected height {height}",
            player.position
        );
        let landed = player.position;
        for direction in [[1., 0.], [-1., 0.], [0., 1.], [0., -1.]] {
            ticks(
                &mut field,
                10,
                FieldInput {
                    direction,
                    ..Default::default()
                },
            )?;
            if field.actor(1).position != landed {
                break;
            }
        }
        assert_ne!(
            field.actor(1).position,
            landed,
            "cannot walk after jump {jump}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn stacked_blocks_can_be_moved_in_martel_triet_and_palmacosta() -> Result<()> {
    for (fixture, map, block) in [
        (MARTEL_START, 308, 5003),
        (TRIET_START, 220, 5000),
        (PALMACOSTA_RANCH, 202, 1001),
    ] {
        let mut field = enter(fixture, map, (map == 308).then_some(107_000))?;
        advance_until(&mut field, FieldSession::player_has_control)?;
        let mut upper = field.actor(block).clone();
        upper.position[2] = upper.collision_bounds().1[2];
        if map == 308 {
            upper.position = [-890., -925., -700.];
        }
        upper.visible = true;
        upper.pushable = true;
        upper.interaction_label = 20;
        upper.radius = 50.;
        field.events.world.insert_actor(6000, upper);
        if map != 308 {
            let mut support = field.actor(block).clone();
            support.position[1] -= 150.;
            support.pushable = false;
            field.events.world.insert_actor(6001, support);
        }
        ticks(&mut field, 30, FieldInput::default())?;
        let start = field.actor(6000).position;
        let player = field.events.world.controlled_actor;
        field.actor_mut(player).position = [start[0], start[1] - 125., start[2]];
        field.actor_mut(player).face(180.);
        field.step(FieldInput {
            interact: true,
            ..Default::default()
        })?;
        assert_eq!(field.events.world.grabbed_block, Some(6000), "map {map}");
        // Cross the stack edge onto ordinary ground, then return over it.
        let directions: &[f32] = if map == 308 { &[1., -1.] } else { &[1.] };
        for &sign in directions {
            let before = field.actor(6000).position[1];
            push(&mut field, [0., sign], 50)?;
            assert!(
                (field.actor(6000).position[1] - before) * sign > 100.,
                "map {map}, direction {sign}, from {before}: block {:?}, player {:?}, grip {:?}",
                field.actor(6000).position,
                field.actor(player).position,
                field.events.world.grabbed_block
            );
            if map == 308 && sign > 0. {
                let at_edge = field.actor(6000).position;
                push(&mut field, [0., sign], 50)?;
                assert_eq!(
                    field.actor(6000).position,
                    at_edge,
                    "block entered the marked approach to the stairs"
                );
            }
        }
        if map != 308 {
            ticks(&mut field, 30, FieldInput::default())?;
            assert!(
                field.actor(6000).position[2] < start[2] - 100.,
                "map {map}: block did not fall"
            );
        }
    }
    Ok(())
}

fn grab_block(field: &mut Scene, block: i32, direction: [f32; 2]) -> Result<()> {
    let start = field.actor(block).position;
    let player = field.events.world.controlled_actor;
    field.actor_mut(player).position = [
        start[0] - direction[0] * 125.,
        start[1] - direction[1] * 125.,
        start[2],
    ];
    field
        .actor_mut(player)
        .face(direction[0].atan2(-direction[1]).to_degrees());
    field.step(FieldInput {
        interact: true,
        ..Default::default()
    })?;
    assert_eq!(field.events.world.grabbed_block, Some(block));
    Ok(())
}

fn push(field: &mut Scene, direction: [f32; 2], count: usize) -> Result<()> {
    for _ in 0..count {
        field.step(block_input(field, direction))?;
    }
    Ok(())
}

fn block_input(field: &Scene, direction: [f32; 2]) -> FieldInput {
    use resonance_events::input::Button;
    let camera = field.events.world.field_camera.as_ref().unwrap();
    let angle =
        -(camera.target[0] - camera.position[0]).atan2(camera.target[1] - camera.position[1]);
    let angle = (angle / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2;
    let direction = [
        angle.cos() * direction[0] + angle.sin() * direction[1],
        -angle.sin() * direction[0] + angle.cos() * direction[1],
    ];
    FieldInput {
        held_buttons: [
            (Button::Accept, true),
            (Button::Left, direction[0] < -0.5),
            (Button::Right, direction[0] > 0.5),
            (Button::Down, direction[1] < -0.5),
            (Button::Up, direction[1] > 0.5),
        ]
        .into_iter()
        .filter_map(|(button, held)| held.then_some(button))
        .collect(),
        accelerate_dialogue: true,
        direction,
        ..Default::default()
    }
}

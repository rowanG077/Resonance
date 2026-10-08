use super::*;

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
        let directions: &[f32] = if map == 308 { &[1., -1.] } else { &[1.] };
        for &sign in directions {
            let before = field.actor(6000).position[1];
            push(&mut field, [0., sign], 50)?;
            assert!(
                (field.actor(6000).position[1] - before) * sign > 100.,
                "map {map}: {:?}",
                field.actor(6000).position
            );
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

fn push(field: &mut Scene, direction: [f32; 2], count: usize) -> Result<()> {
    for _ in 0..count {
        let camera = field.events.world.field_camera.as_ref().unwrap();
        let angle =
            -(camera.target[0] - camera.position[0]).atan2(camera.target[1] - camera.position[1]);
        let angle = (angle / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2;
        field.step(FieldInput {
            held_buttons: [resonance_events::input::Button::Accept]
                .into_iter()
                .collect(),
            direction: [
                angle.cos() * direction[0] + angle.sin() * direction[1],
                -angle.sin() * direction[0] + angle.cos() * direction[1],
            ],
            ..Default::default()
        })?;
    }
    Ok(())
}

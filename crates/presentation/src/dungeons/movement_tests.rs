use super::*;

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn generator_blocks_reach_the_lift_and_drop_from_ledges() -> Result<()> {
    for (block, position, expected, ticks) in [
        (300, [-250., 150., 0.], [-675., 150., 0.], 100),
        (303, [-250., -150., 150.], [-675., -150., 0.], 100),
        (301, [500., 150., 0.], [-675., 150., 0.], 350),
    ] {
        let mut field = enter(Fixture::Generator, 279, None)?;
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
        for _ in 0..ticks {
            let camera = field.events.world.field_camera.as_ref().unwrap();
            let angle = -(camera.target[0] - camera.position[0])
                .atan2(camera.target[1] - camera.position[1]);
            field.step(FieldInput {
                held_buttons: [resonance_events::input::Button::Accept]
                    .into_iter()
                    .collect(),
                direction: [-angle.cos(), angle.sin()],
                ..Default::default()
            })?;
        }
        for _ in 0..30 {
            field.step(FieldInput::default())?;
        }
        assert_eq!(field.actor(block).position, expected);
        assert!(field.player_has_control());
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn forest_cliff_jumps_reach_each_ledge() -> Result<()> {
    let mut field = configured(Fixture::Martel, 193, |entry| {
        entry
            .persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 303_000)?;
        entry.position = [-980., 1350., 0.];
        Ok(())
    })?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    for (jump, height) in [(2000, 250.), (2001, 500.), (2003, 750.)] {
        assert!(field.events.trigger(jump, true)?);
        advance_until(&mut field, FieldSession::player_has_control)?;
        let player = &field.events.world.actors[&field.events.world.controlled_actor];

        assert!(player.attachment.is_none());
        assert!((player.position[2] - height).abs() < 2.);
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn stacked_martel_block_can_be_pushed_and_pulled() -> Result<()> {
    let mut field = enter(Fixture::Martel, 308, Some(107_000))?;
    advance_until(&mut field, FieldSession::player_has_control)?;
    let mut upper = field.actor(5003).clone();
    upper.position = [-890., -925., -700.];
    upper.properties.insert(19, 1);
    upper.properties.insert(17, 20);
    upper.radius = 50.;
    field.events.world.insert_actor(6000, upper);
    ticks(&mut field, 30, FieldInput::default())?;
    let start = field.actor(6000).position;
    field.actor_mut(1).position = [start[0], start[1] - 125., start[2]];
    field.actor_mut(1).face(180.);
    field.step(FieldInput {
        interact: true,
        ..Default::default()
    })?;
    for sign in [1., -1.] {
        let before = field.actor(6000).position[1];
        for _ in 0..50 {
            let camera = field.events.world.field_camera.as_ref().unwrap();
            let angle = -(camera.target[0] - camera.position[0])
                .atan2(camera.target[1] - camera.position[1]);
            field.step(FieldInput {
                held_buttons: [resonance_events::input::Button::Accept]
                    .into_iter()
                    .collect(),
                direction: [sign * angle.sin(), sign * angle.cos()],
                ..Default::default()
            })?;
        }
        assert!((field.actor(6000).position[1] - before) * sign > 100.);
    }
    Ok(())
}

use super::*;

#[test]
fn page_scroll_taps_survive_until_fixed_update() {
    for analog in [false, true] {
        for (key, value, expected) in [
            (KeyCode::PageUp, 1., [true, false]),
            (KeyCode::PageDown, -1., [false, true]),
        ] {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<Controls>()
                .add_systems(Update, gather_controls);
            let pad = app.world_mut().spawn(Gamepad::default()).id();
            if analog {
                app.world_mut()
                    .get_mut::<Gamepad>(pad)
                    .unwrap()
                    .analog_mut()
                    .set(GamepadAxis::RightStickY, value);
            } else {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(key);
            }
            app.update();
            assert_eq!(
                app.world().resource::<Controls>().input.scroll_pressed,
                expected
            );
            app.world_mut()
                .get_mut::<Gamepad>(pad)
                .unwrap()
                .analog_mut()
                .set(GamepadAxis::RightStickY, 0.);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key);
            for _ in 0..60 {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .clear();
                app.update();
            }
            let mut controls = app.world_mut().resource_mut::<Controls>();
            let input = controls.consume();
            assert_eq!(input.scroll_direction, 0);
            assert_eq!(input.scroll_pressed, expected);
            assert_eq!(controls.consume().scroll_pressed, [false; 2]);
        }
    }
}

#[test]
fn field_producer_latches_combined_edges_until_one_consumer() {
    for key in [KeyCode::KeyZ, KeyCode::KeyQ, KeyCode::KeyE] {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controls>()
            .add_systems(Update, gather_controls);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Enter);
        keys.press(key);
        app.update();
        // A later render may observe release before the next fixed update.
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.release(KeyCode::Enter);
        keys.release(key);
        app.update();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        let input = controls.consume();
        assert!(input.pressed(resonance_events::input::Button::Accept));
        assert_eq!(
            input.pressed(resonance_events::input::Button::Skit),
            key == KeyCode::KeyZ
        );
        assert_eq!(
            input.pressed(resonance_events::input::Button::PreviousPage),
            key == KeyCode::KeyQ
        );
        assert_eq!(
            input.pressed(resonance_events::input::Button::NextPage),
            key == KeyCode::KeyE
        );
        let consumed = controls.consume();
        assert!(
            !consumed.pressed(resonance_events::input::Button::Accept)
                && !consumed.pressed(resonance_events::input::Button::Skit)
        );
        assert!(
            !consumed.pressed(resonance_events::input::Button::PreviousPage)
                && !consumed.pressed(resonance_events::input::Button::NextPage)
        );
    }
}

#[test]
fn consuming_field_edges_preserves_held_dialogue_acceleration() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Controls>()
        .add_systems(Update, gather_controls);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.update();
    let mut controls = app.world_mut().resource_mut::<Controls>();
    let first = controls.consume();
    assert!(
        first.pressed(resonance_events::input::Button::Accept)
            && first
                .held_buttons
                .contains(resonance_events::input::Button::Accept)
    );
    let held = controls.consume();
    assert!(
        !held.pressed(resonance_events::input::Button::Accept)
            && held
                .held_buttons
                .contains(resonance_events::input::Button::Accept)
    );
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.clear();
    keys.release(KeyCode::Enter);
    app.update();
    let released = app.world_mut().resource_mut::<Controls>().consume();
    assert!(
        !released.pressed(resonance_events::input::Button::Accept)
            && !released
                .held_buttons
                .contains(resonance_events::input::Button::Accept)
    );
}

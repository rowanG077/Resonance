use super::*;

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

//! Binary motor commands use the same device assignment as input.
use bevy::{
    input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest},
    prelude::*,
};
use std::time::Duration;

#[derive(Resource, Default)]
pub(super) struct State {
    devices: [Option<Entity>; 4],
}
impl State {
    fn update(
        &mut self,
        motors: [bool; 4],
        pads: [Option<Entity>; 4],
        enabled: bool,
        devices_enabled: bool,
        duration: Duration,
    ) -> Vec<GamepadRumbleRequest> {
        let mut requests = Vec::new();
        for slot in 0..4 {
            let next = (enabled && devices_enabled && motors[slot])
                .then_some(pads[slot])
                .flatten();
            if self.devices[slot] != next
                && let Some(gamepad) = self.devices[slot]
            {
                requests.push(GamepadRumbleRequest::Stop { gamepad });
            }
            if let Some(gamepad) = next {
                // Short overlapping leases preserve the binary motor while the
                // app runs and expire on a stall.
                requests.push(GamepadRumbleRequest::Add {
                    gamepad,
                    intensity: GamepadRumbleIntensity::STRONG_MAX,
                    duration,
                });
            }
            self.devices[slot] = next;
        }
        requests
    }
}

pub(super) fn present(
    owner: Option<Res<super::Owner>>,
    controls: Res<super::input::Controls>,
    output: Res<crate::display::OutputStage>,
    fixed: Res<Time<Fixed>>,
    mut state: ResMut<State>,
    mut messages: MessageWriter<GamepadRumbleRequest>,
) {
    let (motors, enabled) = owner
        .as_ref()
        .and_then(|owner| {
            let super::Phase::Scene(scene) = &owner.phase else {
                return None;
            };
            (scene.state == super::SceneState::Active
                && scene.failure.is_none()
                && scene.completed.is_none())
            .then_some((
                scene.feedback.motors(scene.feedback_paused()),
                scene.settings.rumble,
            ))
        })
        .unwrap_or(([false; 4], false));
    let requests = state.update(
        motors,
        controls.devices(),
        enabled,
        *output != crate::display::OutputStage::Framebuffer,
        fixed.timestep() * 2,
    );
    messages.write_batch(requests);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_delivery_uses_input_slots_stops_reassignment_and_never_drives_file_capture() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let mut state = State::default();
        let duration = Duration::from_millis(34);
        let motors = [false, true, false, false];
        let pads = [None, Some(first), None, None];
        assert!(state.update(motors, pads, true, false, duration).is_empty());
        assert!(
            state
                .update(motors, [None; 4], true, true, duration)
                .is_empty()
        );
        let requests = state.update(motors, pads, true, true, duration);
        assert!(
            matches!(&requests[..], [GamepadRumbleRequest::Add { gamepad, intensity, duration: lease }] if *gamepad == first && intensity.strong_motor == 1. && intensity.weak_motor == 0. && *lease == duration)
        );
        let requests = state.update(
            motors,
            [None, Some(second), None, None],
            true,
            true,
            duration,
        );
        assert!(
            matches!(&requests[..], [GamepadRumbleRequest::Stop { gamepad: previous }, GamepadRumbleRequest::Add { gamepad: current, .. }] if *previous == first && *current == second)
        );
        let requests = state.update(
            motors,
            [None, Some(second), None, None],
            false,
            true,
            duration,
        );
        assert!(
            matches!(&requests[..], [GamepadRumbleRequest::Stop { gamepad }] if *gamepad == second)
        );
        assert!(state.update(motors, pads, false, true, duration).is_empty());
        state.update(motors, pads, true, true, duration);
        assert!(
            matches!(&state.update([false;4],pads,false,true,duration)[..], [GamepadRumbleRequest::Stop { gamepad }] if *gamepad == first)
        );
    }

    #[test]
    fn production_presenter_stops_a_retired_owner_without_a_gamepad_plugin() {
        let mut app = App::new();
        app.init_resource::<super::super::input::Controls>()
            .init_resource::<Time<Fixed>>()
            .init_resource::<State>()
            .insert_resource(crate::display::OutputStage::Scanout)
            .add_message::<GamepadRumbleRequest>()
            .add_systems(Update, present);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut().resource_mut::<State>().devices[3] = Some(pad);
        app.update();
        let requests: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<GamepadRumbleRequest>>()
            .drain()
            .collect();
        assert!(
            matches!(&requests[..], [GamepadRumbleRequest::Stop { gamepad }] if *gamepad == pad)
        );
        app.world_mut()
            .insert_resource(crate::display::OutputStage::Framebuffer);
        app.update();
        assert!(
            app.world()
                .resource::<Messages<GamepadRumbleRequest>>()
                .is_empty()
        );
        assert_eq!(app.world().resource::<State>().devices, [None; 4]);
    }
}

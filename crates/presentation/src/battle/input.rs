//! Device edges are latched between renders and consumed by one fixed update.
use bevy::prelude::*;
use resonance_battle::{ActorId, ButtonInput as BattleButton, ControlInput};
use resonance_content::menu_data::CustomizeSettings;
use resonance_game::battle::command::Input as CommandInput;

#[derive(Resource, Default)]
pub(super) struct Controls {
    buttons: [BattleButton; 7],
    stick: [i8; 2],
    repeat: [u8; 2],
    previous_horizontal: i8,
    horizontal_pressed: i8,
}
impl Controls {
    pub fn clear(&mut self) {
        for button in &mut self.buttons {
            button.pressed = false;
            button.released = false;
        }
    }

    pub fn reset(&mut self) {
        self.clear();
        self.repeat = [0; 2];
    }

    /// 109C0: opening the command strip arms each source repeat byte to one.
    pub fn reset_command_repeat(&mut self) {
        self.repeat = [1; 2];
    }

    pub fn confirm(&self) -> bool {
        self.buttons[0].pressed
    }

    pub fn loading_update(&mut self, tick: u32) {
        self.target_step(tick);
        self.clear();
    }

    /// Sample command edges before `take_actions` clears the shared latches.
    pub fn command_input(&self, settings: &CustomizeSettings, step: i8) -> CommandInput {
        CommandInput {
            controller: 0,
            open: self.mapped(settings, 3),
            confirm_a: self.buttons[0],
            cancel_b: self.buttons[1],
            step,
        }
    }

    pub fn consume(
        &mut self,
        actor: ActorId,
        settings: &CustomizeSettings,
        target_step: i8,
    ) -> ControlInput {
        // 5C38 maps each physical button to its configured logical action.
        let [attack, technique, guard, target] = self.take_actions(settings);
        ControlInput {
            actor,
            stick: self.stick,
            horizontal_pressed: self.horizontal_pressed,
            attack,
            technique,
            guard,
            target,
            target_step,
        }
    }

    fn take_actions(&mut self, settings: &CustomizeSettings) -> [BattleButton; 4] {
        let actions = [0, 1, 2, 5].map(|action| self.mapped(settings, action));
        self.clear();
        actions
    }

    fn mapped(&self, settings: &CustomizeSettings, action: u8) -> BattleButton {
        self.buttons[settings
            .button_map
            .iter()
            .position(|&v| v == action)
            .unwrap()]
    }

    pub fn target_step(&mut self, tick: u32) -> i8 {
        self.horizontal_pressed = if self.stick[0] < -48 && self.previous_horizontal >= -48 {
            -1
        } else if self.stick[0] > 48 && self.previous_horizontal <= 48 {
            1
        } else {
            0
        };
        self.previous_horizontal = self.stick[0];
        let mut step = 0;
        // 10AB8 uses the battle input-update clock, including loading visits
        // before the selector opens; holding the selector cannot reset it.
        for (index, down) in [self.stick[0] < -48, self.stick[0] > 48]
            .into_iter()
            .enumerate()
        {
            if down {
                let repeat = &mut self.repeat[index];
                let emit = *repeat == 0 || *repeat == 30 && tick.is_multiple_of(8);
                *repeat = repeat.saturating_add(1).min(30);
                if emit {
                    step += if index == 0 { -1 } else { 1 };
                }
            } else {
                self.repeat[index] = 0;
            }
        }
        step
    }
}

pub(super) fn gather(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut controls: ResMut<Controls>,
) {
    use GamepadButton::*;
    let pad = pads.iter().next();
    for (index, (keys_for_button, button)) in [
        (&[KeyCode::Enter, KeyCode::Space][..], South),
        (&[KeyCode::Escape][..], East),
        (&[KeyCode::KeyX][..], West),
        (&[KeyCode::Tab][..], North),
        (&[KeyCode::KeyQ][..], LeftTrigger),
        (&[KeyCode::KeyE][..], RightTrigger),
        (&[KeyCode::KeyZ][..], Z),
    ]
    .into_iter()
    .enumerate()
    {
        let down = keys_for_button.iter().any(|key| keys.pressed(*key))
            || pad.is_some_and(|pad| pad.pressed(button));
        let edge = keys_for_button.iter().any(|key| keys.just_pressed(*key))
            || pad.is_some_and(|pad| pad.just_pressed(button));
        let previous = controls.buttons[index].held;
        controls.buttons[index].held = down;
        controls.buttons[index].pressed |= down && !previous || edge;
        controls.buttons[index].released |= !down && previous;
    }
    let axis = |positive: [KeyCode; 2], negative: [KeyCode; 2]| {
        f32::from(positive.into_iter().any(|key| keys.pressed(key)))
            - f32::from(negative.into_iter().any(|key| keys.pressed(key)))
    };
    let keys = Vec2::new(
        axis(
            [KeyCode::ArrowRight, KeyCode::KeyD],
            [KeyCode::ArrowLeft, KeyCode::KeyA],
        ),
        axis(
            [KeyCode::ArrowUp, KeyCode::KeyW],
            [KeyCode::ArrowDown, KeyCode::KeyS],
        ),
    );
    let stick =
        (keys + pad.map_or(Vec2::ZERO, Gamepad::left_stick)).clamp(Vec2::NEG_ONE, Vec2::ONE);
    controls.stick = stick.to_array().map(|v| {
        let value = (v * 70.).round() as i8;
        if value.abs() < 16 { 0 } else { value }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remapping_uses_physical_to_logical_lookup_and_consumes_edges_once() {
        let mut controls = Controls::default();
        let held_press = BattleButton {
            held: true,
            pressed: true,
            released: false,
        };
        let release = BattleButton {
            held: false,
            pressed: false,
            released: true,
        };
        controls.buttons[0] = held_press;
        controls.buttons[2] = release;
        controls.buttons[3] = held_press;
        let settings = CustomizeSettings {
            button_map: [2, 5, 1, 0, 3, 4, 6],
            ..Default::default()
        };
        assert!(controls.confirm()); // Results still use fixed physical A.
        let [attack, technique, guard, target] = controls.take_actions(&settings);
        assert_eq!(attack, held_press);
        assert_eq!(technique, release);
        assert_eq!(guard, held_press);
        assert_eq!(target, BattleButton::default());
        assert!(!controls.confirm());
        let [attack, technique, guard, target] = controls.take_actions(&settings);
        let held = BattleButton {
            held: true,
            ..Default::default()
        };
        assert_eq!(attack, held);
        assert_eq!(technique, BattleButton::default());
        assert_eq!(guard, held);
        assert_eq!(target, BattleButton::default());
    }

    #[test]
    fn default_b_and_x_map_to_technique_and_guard() {
        let mut controls = Controls::default();
        controls.buttons[1] = BattleButton {
            held: true,
            pressed: true,
            released: false,
        };
        controls.buttons[2].held = true;
        let [attack, technique, guard, target] =
            controls.take_actions(&CustomizeSettings::default());
        assert!(technique.pressed && technique.held);
        assert!(guard.held && !guard.pressed);
        assert_eq!(attack, BattleButton::default());
        assert_eq!(target, BattleButton::default());
    }
    #[test]
    fn selector_repeat_uses_strict_threshold_and_shared_tick_phase() {
        let mut controls = Controls::default();
        controls.stick[0] = 48;
        assert_eq!(controls.target_step(0), 0);
        controls.stick[0] = 49;
        assert_eq!(controls.target_step(1), 1);
        for tick in 2..=30 {
            assert_eq!(controls.target_step(tick), 0);
        }
        assert_eq!(controls.target_step(31), 0);
        assert_eq!(controls.target_step(32), 1);
        controls.stick[0] = 0;
        controls.target_step(33);
        controls.stick[0] = -49;
        assert_eq!(controls.target_step(34), -1);
    }

    #[test]
    fn command_open_arms_source_repeat_state() {
        let mut controls = Controls {
            repeat: [29, 30],
            ..Default::default()
        };
        controls.reset_command_repeat();
        assert_eq!(controls.repeat, [1, 1]);
    }
    #[test]
    fn mobility_edges_follow_global_input_visits_and_ignore_repeat_reset() {
        let mut controls = Controls {
            stick: [49, 0],
            ..Default::default()
        };
        controls.loading_update(1);
        assert_eq!(controls.horizontal_pressed, 1);
        controls.target_step(2);
        assert_eq!(controls.horizontal_pressed, 0);
        controls.reset_command_repeat();
        controls.target_step(3);
        assert_eq!(controls.horizontal_pressed, 0);
        controls.stick = [48, 0];
        controls.target_step(4);
        controls.stick = [49, 0];
        controls.target_step(5);
        assert_eq!(controls.horizontal_pressed, 1);
        controls.stick = [-49, 0];
        controls.target_step(6);
        assert_eq!(controls.horizontal_pressed, -1);
    }
}

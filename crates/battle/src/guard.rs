//! On-contact guard selection and resolution in fn_1_61578.
//! Actor controllers still own guard entry/exit, animation and action clocks.
use crate::{Actor, Affinity, DamageKind, distance, state::Random};
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Control {
    #[default]
    Manual,
    SemiAuto,
    Auto,
    Enemy,
}

/// The current actor controller's activity, observed by contact resolution.
/// Resident sequence age is independent of the actor controller's action clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Activity {
    #[default]
    Idle,
    /// Source death controller; availability changes before its motion finishes.
    Defeated,
    Approaching,
    Action {
        clock: i16,
        guard_window: [i16; 2],
    },
    Casting {
        /// Countdown during chanting; elapsed release time afterward.
        clock: i16,
        guard_window: [i16; 2],
    },
    Guarding,
    Hurt,
    KnockedDown,
    GettingUp,
    Stunned,
    Jumping,
    /// Includes the landing/recovery controller, which excludes party auto-guard.
    Recovering,
    /// Backstep and sidestep share this controller.
    Evading,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GuardKind {
    #[default]
    Normal,
    Special,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Guard {
    pub active: bool,
    pub kind: GuardKind,
    pub pressure: u8,
    pub break_pressure: i16,
    pub reduction: u8,
    /// Party controller chance, reset independently of enemy action data.
    pub auto_chance: u8,
    pub enemy_chance: i8,
    /// Resolved original guard preference contribution on controller entry.
    pub recovery_bonus: u8,
    pub allow_airborne: bool,
    pub auto_disabled: bool,
    pub recently_hurt: bool,
}

impl Guard {
    pub(crate) fn validate(self) -> Result<()> {
        ensure!(
            self.pressure <= 31 && matches!(self.recovery_bonus, 0 | 5 | 10 | 25),
            "invalid battle guard parameters"
        );
        Ok(())
    }

    /// 1A9AC: signed remainder, then two byte writes; every call draws, including
    /// enemy recovery even though enemy guard admission uses its action chance.
    pub(crate) fn reset_auto_chance(&mut self, base: u8, random: &mut Random) {
        self.auto_chance = (i16::from(base) + (random.next() as i16) % 5) as u8;
        self.auto_chance = self.auto_chance.wrapping_add(self.recovery_bonus);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardRule {
    pub enabled: bool,
    pub pressure: u8,
    pub breaks: bool,
    pub unbreakable: bool,
}

impl Default for GuardRule {
    fn default() -> Self {
        Self {
            enabled: true,
            pressure: 0,
            breaks: false,
            unbreakable: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GuardResult {
    #[default]
    None,
    Blocked {
        first: bool,
        special: bool,
    },
    Broken,
}

pub(crate) fn attempt(target: &mut Actor, affinity: Affinity, random: &mut Random) -> bool {
    let guard = &target.guard;
    let blocked_activity = matches!(
        target.activity,
        Activity::Hurt | Activity::KnockedDown | Activity::Stunned
    );
    let succeeds = match target.control {
        Control::Manual => false,
        Control::Enemy => {
            if blocked_activity
                || guard.auto_disabled
                || matches!(affinity, Affinity::Absorb | Affinity::Immune)
                || i16::from(guard.pressure) >= guard.break_pressure
                || target.position[1] > 0.1 && !guard.allow_airborne
            {
                return false;
            }
            if let Activity::Action {
                clock,
                guard_window: [start, duration],
            }
            | Activity::Casting {
                clock,
                guard_window: [start, duration],
            } = target.activity
                && !(i32::from(start)..i32::from(start) + i32::from(duration))
                    .contains(&i32::from(clock))
            {
                return false;
            }
            let roll = (random.next() % 100) as i16
                - if guard.recently_hurt { 20 } else { 0 }
                - if target.activity == Activity::Approaching {
                    40
                } else {
                    0
                };
            roll.max(0) < i16::from(guard.enemy_chance)
        }
        Control::SemiAuto | Control::Auto => {
            if blocked_activity
                || guard.kind == GuardKind::Special
                || matches!(
                    target.activity,
                    Activity::Action { .. }
                        | Activity::Casting { .. }
                        | Activity::Jumping
                        | Activity::Recovering
                        | Activity::Evading
                )
            {
                return false;
            }
            (random.next() % 100) < u16::from(target.guard.auto_chance)
        }
    };
    if succeeds {
        // Source sets active/automatic bits, replacing any special-guard stance.
        target.guard.active = true;
        target.guard.kind = GuardKind::Normal;
    }
    succeeds
}

pub(crate) fn resolve(
    target: &mut Actor,
    kind: DamageKind,
    rule: GuardRule,
    affinity: Affinity,
    incoming: [f32; 3],
    amount: &mut i32,
) -> GuardResult {
    if matches!(affinity, Affinity::Absorb | Affinity::Immune) || !target.guard.active {
        return GuardResult::None;
    }
    let guard = &mut target.guard;
    let first = guard.pressure == 0;
    if guard.kind == GuardKind::Normal {
        if !rule.enabled {
            guard.active = false;
            return GuardResult::None;
        }
        // The native five-bit accumulator wraps before checking its limit.
        guard.pressure = guard.pressure.wrapping_add(rule.pressure) & 31;
        let behind = distance::dot(target.facing_direction, incoming) > 0.75;
        let broken =
            !rule.unbreakable && (i16::from(guard.pressure) >= guard.break_pressure || behind);
        if broken {
            guard.pressure = (guard.break_pressure as u8) & 31;
        }
        if broken || rule.breaks {
            guard.active = false;
            return GuardResult::Broken;
        }
    }
    let special = guard.kind == GuardKind::Special;
    let percent = if special {
        20
    } else if kind == DamageKind::Magic {
        100
    } else {
        // Native narrows this subtraction to u8 before the upper clamp.
        i32::from(100u8.wrapping_sub(guard.reduction)).min(100)
    };
    *amount = amount.wrapping_mul(percent) / 100;
    GuardResult::Blocked {
        first: first && !special,
        special,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Side, tests::actor};

    fn defender() -> Actor {
        let mut actor = actor(Side::Enemy);
        actor.control = Control::Enemy;
        actor.guard = Guard {
            active: true,
            break_pressure: 10,
            reduction: 75,
            enemy_chance: 25,
            ..Default::default()
        };
        actor
    }

    #[test]
    fn enemy_action_window_is_half_open_and_ineligible_contacts_do_not_draw() {
        for (clock, draws) in [(0, false), (1, true), (4, true), (5, false)] {
            for activity in [
                Activity::Action {
                    clock,
                    guard_window: [1, 4],
                },
                Activity::Casting {
                    clock,
                    guard_window: [1, 4],
                },
            ] {
                let mut target = defender();
                target.activity = activity;
                let mut random = Random(1);
                attempt(&mut target, Affinity::Normal, &mut random);
                assert_eq!(random.0 != 1, draws);
            }
        }
        for activity in [Activity::Hurt, Activity::KnockedDown, Activity::Stunned] {
            let mut target = defender();
            target.activity = activity;
            let mut random = Random(1);
            assert!(!attempt(&mut target, Affinity::Normal, &mut random));
            assert_eq!(random.0, 1);
        }
        for field in 0..4 {
            let mut target = defender();
            let affinity = if field == 0 {
                Affinity::Absorb
            } else {
                Affinity::Normal
            };
            match field {
                1 => target.guard.pressure = 10,
                2 => target.guard.auto_disabled = true,
                3 => target.position[1] = 0.1001,
                _ => {}
            }
            let mut random = Random(1);
            assert!(!attempt(&mut target, affinity, &mut random));
            assert_eq!(random.0, 1);
        }
        let mut target = defender();
        target.position[1] = 50.;
        target.guard.allow_airborne = true;
        let mut random = Random(1);
        attempt(&mut target, Affinity::Normal, &mut random);
        assert_ne!(random.0, 1);
    }

    #[test]
    fn enemy_chance_uses_signed_action_byte_and_clamps_modified_roll_before_comparison() {
        // Seed 1 produces 16857: roll 57, 17 while approaching, 0 with both modifiers.
        for (chance, activity, recently_hurt, expected) in [
            (0, Activity::Idle, false, false),
            (-128, Activity::Approaching, true, false),
            (17, Activity::Approaching, false, false),
            (18, Activity::Approaching, false, true),
            (0, Activity::Approaching, true, false),
            (1, Activity::Approaching, true, true),
            (58, Activity::Idle, false, true),
        ] {
            let mut target = defender();
            target.guard.active = false;
            target.guard.enemy_chance = chance;
            target.guard.recently_hurt = recently_hurt;
            target.activity = activity;
            let mut random = Random(1);
            assert_eq!(
                attempt(&mut target, Affinity::Normal, &mut random),
                expected
            );
            assert_eq!(target.guard.active, expected);
            assert_eq!(random.0, 0x41d924f4); // Even chance zero/negative consumes a draw.
        }
    }

    #[test]
    fn manual_never_rolls_and_party_auto_has_distinct_gates() {
        let mut target = defender();
        target.control = Control::Manual;
        let mut random = Random(1);
        assert!(!attempt(&mut target, Affinity::Normal, &mut random));
        assert_eq!(random.0, 1);
        for control in [Control::SemiAuto, Control::Auto] {
            target.control = control;
            target.guard.auto_chance = 100;
            target.position[1] = 50.;
            target.guard.pressure = 10;
            assert!(attempt(&mut target, Affinity::Immune, &mut random));
            for activity in [
                Activity::Action {
                    clock: 2,
                    guard_window: [1, 4],
                },
                Activity::Casting {
                    clock: 2,
                    guard_window: [1, 4],
                },
                Activity::Jumping,
                Activity::Recovering,
                Activity::Evading,
                Activity::Hurt,
            ] {
                target.activity = activity;
                let before = random.0;
                assert!(!attempt(&mut target, Affinity::Normal, &mut random));
                assert_eq!(random.0, before);
            }
            target.activity = Activity::Idle;
            target.guard.kind = GuardKind::Special;
            let before = random.0;
            assert!(!attempt(&mut target, Affinity::Normal, &mut random));
            assert_eq!(random.0, before);
            target.guard.kind = GuardKind::Normal;
        }
    }

    #[test]
    fn normal_guard_marks_first_contact_then_breaks_at_pressure_limit() {
        let mut target = defender();
        let rule = GuardRule {
            pressure: 5,
            ..Default::default()
        };
        let mut amount = 43;
        assert_eq!(
            resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0.; 3],
                &mut amount
            ),
            GuardResult::Blocked {
                first: true,
                special: false
            }
        );
        assert_eq!((amount, target.guard.pressure), (10, 5));
        amount = 43;
        assert_eq!(
            resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0.; 3],
                &mut amount
            ),
            GuardResult::Broken
        );
        assert_eq!(
            (amount, target.guard.pressure, target.guard.active),
            (43, 10, false)
        );
    }

    #[test]
    fn behind_test_uses_cached_facing_instead_of_heading_or_movement() {
        let mut target = defender();
        target.heading = 0.;
        target.movement.direction = [0., 0., -1.];
        target.facing_direction = [1., 0., 0.];
        let mut amount = 40;
        assert_eq!(
            resolve(
                &mut target,
                DamageKind::Slash,
                GuardRule::default(),
                Affinity::Normal,
                [1., 0., 0.],
                &mut amount
            ),
            GuardResult::Broken
        );
    }

    #[test]
    fn pressure_wrap_and_unbreakable_flag_precede_forced_break() {
        let mut target = defender();
        target.guard.pressure = 31;
        let mut rule = GuardRule {
            pressure: 2,
            ..Default::default()
        };
        let mut amount = 40;
        assert_eq!(
            resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0.; 3],
                &mut amount
            ),
            GuardResult::Blocked {
                first: false,
                special: false
            }
        );
        assert_eq!(target.guard.pressure, 1);
        rule.unbreakable = true;
        rule.pressure = 10;
        amount = 40;
        assert!(matches!(
            resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0., 0., 20.],
                &mut amount
            ),
            GuardResult::Blocked { .. }
        ));
        assert_eq!(target.guard.pressure, 11);
        rule.breaks = true;
        assert_eq!(
            resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0.; 3],
                &mut amount
            ),
            GuardResult::Broken
        );
        assert_eq!(target.guard.pressure, 21); // Forced break alone does not assign the threshold.
    }

    #[test]
    fn behind_check_is_strict_and_keeps_incoming_vector_magnitude() {
        for (speed, expected) in [(0.75, false), (0.75001, true), (-20., false)] {
            let mut target = defender();
            let mut amount = 40;
            let result = resolve(
                &mut target,
                DamageKind::Slash,
                GuardRule::default(),
                Affinity::Normal,
                [0., 0., speed],
                &mut amount,
            );
            assert_eq!(result == GuardResult::Broken, expected);
            assert_eq!(target.guard.pressure, if expected { 10 } else { 0 });
        }
    }

    #[test]
    fn special_guard_ignores_pressure_and_break_rules_while_absorb_and_immune_skip_both() {
        for affinity in [Affinity::Normal, Affinity::Absorb, Affinity::Immune] {
            let mut target = defender();
            target.guard.kind = GuardKind::Special;
            target.guard.pressure = 9;
            let mut amount = 43;
            let rule = GuardRule {
                pressure: 10,
                breaks: true,
                ..Default::default()
            };
            let result = resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                affinity,
                [0., 0., 20.],
                &mut amount,
            );
            assert_eq!(
                result,
                if affinity == Affinity::Normal {
                    GuardResult::Blocked {
                        first: false,
                        special: true,
                    }
                } else {
                    GuardResult::None
                }
            );
            assert_eq!(amount, if affinity == Affinity::Normal { 8 } else { 43 });
            assert_eq!(target.guard.pressure, 9);
            assert!(target.guard.active);
        }
    }

    #[test]
    fn reduction_wraps_to_byte_before_clamping_and_preparation_checks_guard_fields() {
        let mut target = defender();
        target.guard.reduction = 101;
        let mut amount = 43;
        resolve(
            &mut target,
            DamageKind::Slash,
            GuardRule::default(),
            Affinity::Normal,
            [0.; 3],
            &mut amount,
        );
        assert_eq!(amount, 43);
        target.guard.pressure = 32;
        assert!(target.validate().is_err());
        target.guard.pressure = 0;
        target.guard.recovery_bonus = 11;
        assert!(target.validate().is_err());
        target.guard.recovery_bonus = 25;
        target.guard.auto_chance = 255;
        target.guard.enemy_chance = -128;
        assert!(target.validate().is_ok());
    }

    #[test]
    fn recovery_uses_signed_random_remainder_and_wraps_without_changing_enemy_chance() {
        // Original recovery observation: the high half is signed -30893,
        // whose remainder is -3 rather than the unsigned remainder of 3.
        for (base, bonus, expected) in [(75, 0, 72), (0, 0, 253), (255, 25, 21)] {
            let mut random = Random(3550590411);
            let mut guard = Guard {
                enemy_chance: -30,
                recovery_bonus: bonus,
                ..Default::default()
            };
            guard.reset_auto_chance(base, &mut random);
            assert_eq!(guard.auto_chance, expected);
            assert_eq!(guard.enemy_chance, -30);
            assert_eq!(random.0, 2270369782);
        }
    }
}

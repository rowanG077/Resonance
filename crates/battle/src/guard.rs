use crate::{Actor, Affinity, DamageKind, distance, state::Random};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Control {
    #[default]
    Manual,
    SemiAuto,
    Auto,
    Enemy,
}

/// The current actor controller's activity, observed by contact resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Activity {
    #[default]
    Idle,
    /// Death changes availability before its motion finishes.
    Defeated,
    Approaching,
    /// Departing after a successful escape.
    Escaping,
    Action,
    Casting {
        held: bool,
    },
    Item,
    /// A timed taunt; enemies also use it while waiting for an opening.
    Taunting,
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
    /// Recovery after a blocked hit; ordinary held guard has no countdown.
    pub recovery: u8,
    pub active: bool,
    pub kind: GuardKind,
    pub pressure: u32,
    pub break_pressure: u32,
    pub reduction: u8,
    /// Party controller chance, reset independently of enemy action data.
    pub auto_chance: u8,
    pub enemy_chance: i8,
    /// Guard preference contribution at controller entry.
    pub recovery_bonus: u8,
    pub allow_airborne: bool,
    pub auto_disabled: bool,
    pub recent_hurt_ticks: u8,
}

pub(crate) const BLOCK_RECOVERY_TICKS: u8 = 30;
const COUNTER_WINDOW_TICKS: u8 = 8;

impl Guard {
    pub(crate) fn can_counter(&self) -> bool {
        self.recovery > BLOCK_RECOVERY_TICKS - COUNTER_WINDOW_TICKS
    }

    /// Vary the controller's guard chance by at most four percentage points.
    pub(crate) fn reset_auto_chance(&mut self, base: u8, random: &mut Random) {
        let variation = (random.next_u16() % 9) as i16 - 4;
        self.auto_chance =
            (i16::from(base) + i16::from(self.recovery_bonus) + variation).clamp(0, 100) as u8;
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

pub(crate) fn attempt(
    target: &mut Actor,
    activity: Activity,
    affinity: Affinity,
    roll: impl FnOnce() -> u16,
) -> bool {
    let guard = &target.guard;
    let can_auto_guard = matches!(
        activity,
        Activity::Idle
            | Activity::Approaching
            | Activity::Guarding
            | Activity::Taunting
            | Activity::Jumping
    );
    let succeeds = match target.control {
        // Air guard requires height above 0.1, the jump state, and a fresh Guard command. It can
        // replace Special Guard deterministically.
        Control::Manual => {
            target.equipment.control_ex.aerial_guard
                && target.airborne()
                && activity == Activity::Jumping
                && target.input.motion == crate::action_selection::GroundMotion::Guard
                && target.input.action.is_none()
                && !target.input.jump
        }
        Control::Enemy => {
            if !can_auto_guard
                || guard.auto_disabled
                || matches!(affinity, Affinity::Absorb | Affinity::Immune)
                || guard.pressure >= guard.break_pressure
                || target.airborne() && !guard.allow_airborne
            {
                return false;
            }
            let roll = (roll() % 100) as i16
                - if guard.recent_hurt_ticks != 0 { 20 } else { 0 }
                - if activity == Activity::Approaching {
                    40
                } else {
                    0
                };
            roll.max(0) < i16::from(guard.enemy_chance)
        }
        Control::SemiAuto | Control::Auto => {
            if !can_auto_guard
                || guard.kind == GuardKind::Special
                || activity == Activity::Jumping && !target.equipment.control_ex.aerial_guard
            {
                return false;
            }
            (roll() % 100) < u16::from(target.guard.auto_chance)
        }
    };
    if succeeds {
        // Activate automatic guard, replacing any special stance.
        target.guard.active = true;
        target.guard.kind = GuardKind::Normal;
    }
    succeeds
}

impl crate::Battle {
    pub(crate) fn enter_player_guard(&mut self, index: usize) {
        let actor = &mut self.actors[index];
        actor.guard.auto_chance = 100;
        actor.reaction.protection = Default::default();
        actor.reaction.direction = actor.movement.direction;
        actor.guard.recovery = 0;
        actor.guard.active = true;
        actor.movement.gravity = -f32::from(u8::from(!actor.movement.flying));
        actor.movement.braking = 0.55;
    }

    /// Guard movement and recovery are shared by player and automatic controllers.
    pub(crate) fn update_guard(
        &mut self,
        index: usize,
        cues: &mut Vec<crate::Cue>,
    ) -> anyhow::Result<bool> {
        if self.activity(crate::ActorId(index as u8)) != Activity::Guarding {
            return Ok(false);
        }
        let id = crate::ActorId(index as u8);
        self.advance_guard_ex(id, cues)?;
        let actor = &mut self.actors[index];
        if actor.hit_stop == 0 {
            if matches!(actor.control, Control::Auto | Control::Enemy) && actor.guard.recovery == 0
            {
                self.enter_idle(id);
                return Ok(true);
            }
            actor.guard.recovery = actor.guard.recovery.saturating_sub(1);
            actor.movement.gravity = if actor.movement.flying {
                0.
            } else {
                crate::movement::GRAVITY
            };
            actor.movement.braking = 0.55;
            actor
                .movement
                .integrate_along(&mut actor.position, actor.reaction.direction);
            actor.movement.brake(actor.position[1], Activity::Guarding);
        }
        crate::movement::floor(actor);
        if let Some(turn_ticks) = self.prepared.actor_setup[index].turn_ticks() {
            crate::control::face(actor, actor.facing_direction, 180. / f32::from(turn_ticks));
        }
        Ok(true)
    }
}

/// Normal guard uses physical defense or halves spell damage; Special Guard is stronger.
fn percentage(owner: &Actor, target: &Actor, kind: DamageKind) -> i32 {
    if target.guard.kind == GuardKind::Special {
        if target.equipment.damage.special_guard_reduction {
            15
        } else {
            20
        }
    } else if kind == DamageKind::Magic {
        50
    } else {
        let mut percent = 100 - i32::from(target.guard.reduction);
        if owner.equipment.damage.guard_damage_boost {
            percent += 15;
        }
        if target.control_ex_state.guard_ready {
            percent -= 5;
        }
        percent.clamp(0, 100)
    }
}

/// Guard fixtures use an ordinary attacker.
#[cfg(test)]
pub(crate) fn resolve(
    target: &mut Actor,
    kind: DamageKind,
    rule: GuardRule,
    affinity: Affinity,
    incoming: [f32; 3],
    amount: &mut i64,
) -> GuardResult {
    let owner = crate::tests::actor(crate::Side::Party);
    resolve_from(&owner, target, kind, rule, affinity, incoming, amount)
}

pub(crate) fn resolve_from(
    owner: &Actor,
    target: &mut Actor,
    kind: DamageKind,
    rule: GuardRule,
    affinity: Affinity,
    incoming: [f32; 3],
    amount: &mut i64,
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
        guard.pressure = guard.pressure.saturating_add(u32::from(rule.pressure));
        let behind = !target.equipment.damage.rear_guard
            && distance::dot(target.facing_direction, incoming) > 0.75;
        let broken = !rule.unbreakable
            && (guard.pressure >= guard.break_pressure
                || behind
                || (kind != DamageKind::Magic
                    && owner.control_ex_state.charge.charged()
                    && !target.equipment.damage.single_charge_guard));
        if broken {
            guard.pressure = guard.break_pressure;
        }
        if broken || rule.breaks {
            guard.active = false;
            return GuardResult::Broken;
        }
    }
    let special = guard.kind == GuardKind::Special;
    let percent = percentage(owner, target, kind).min(100);
    *amount = *amount * i64::from(percent) / 100;
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
    fn automatic_guard_requires_free_work_and_eligible_controls() {
        for (control, activity, special, allowed) in [
            (Control::Manual, Activity::Idle, false, false),
            (Control::Enemy, Activity::Idle, false, true),
            (Control::Enemy, Activity::Approaching, false, true),
            (Control::Auto, Activity::Idle, false, true),
            (Control::SemiAuto, Activity::Idle, false, true),
            (Control::Enemy, Activity::Action, false, false),
            (
                Control::Auto,
                Activity::Casting { held: false },
                false,
                false,
            ),
            (Control::Auto, Activity::Jumping, false, false),
            (Control::Auto, Activity::Idle, true, false),
        ] {
            let mut target = defender();
            target.control = control;
            target.guard.active = false;
            target.guard.auto_chance = 100;
            target.guard.enemy_chance = 100;
            target.guard.kind = if special {
                GuardKind::Special
            } else {
                GuardKind::Normal
            };
            assert_eq!(
                attempt(&mut target, activity, Affinity::Normal, || {
                    assert!(allowed, "ineligible actors must not roll");
                    0
                }),
                allowed
            );
            assert_eq!(target.guard.active, allowed);
        }
    }

    #[test]
    fn enemy_guard_probability_accounts_for_approach_and_recent_hurt() {
        for (chance, activity, hurt, roll, expected) in [
            (50, Activity::Idle, false, 49, true),
            (50, Activity::Idle, false, 50, false),
            (10, Activity::Approaching, false, 49, true),
            (10, Activity::Approaching, false, 50, false),
            (10, Activity::Idle, true, 29, true),
            (10, Activity::Idle, true, 30, false),
            (0, Activity::Approaching, true, 0, false),
            (1, Activity::Approaching, true, 59, true),
            (-1, Activity::Approaching, true, 0, false),
        ] {
            let mut target = defender();
            target.guard.active = false;
            target.guard.enemy_chance = chance;
            target.guard.recent_hurt_ticks = u8::from(hurt);
            assert_eq!(
                attempt(&mut target, activity, Affinity::Normal, || roll),
                expected
            );
            assert_eq!(target.guard.active, expected);
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
    fn cumulative_pressure_breaks_guard_above_one_byte() {
        let mut target = defender();
        target.guard.break_pressure = 600;
        let rule = GuardRule {
            pressure: 100,
            ..Default::default()
        };
        for expected in [100, 200, 300, 400, 500] {
            let result = resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0.; 3],
                &mut 40,
            );
            assert!(matches!(result, GuardResult::Blocked { .. }));
            assert_eq!(target.guard.pressure, expected);
            assert!(target.guard.active);
        }
        assert_eq!(
            resolve(
                &mut target,
                DamageKind::Slash,
                rule,
                Affinity::Normal,
                [0.; 3],
                &mut 40
            ),
            GuardResult::Broken
        );
        assert_eq!(target.guard.pressure, 600);
        assert!(!target.guard.active);
    }

    #[test]
    fn pressure_saturates_and_unbreakable_guard_still_allows_forced_break() {
        let mut target = defender();
        target.guard.pressure = u32::MAX - 1;
        let mut rule = GuardRule {
            pressure: 2,
            unbreakable: true,
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
        assert_eq!(target.guard.pressure, u32::MAX);
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
        assert_eq!(target.guard.pressure, u32::MAX);
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
        assert_eq!(target.guard.pressure, u32::MAX);
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
    fn guard_reduction_is_bounded_and_preparation_accepts_arbitrary_recovery_bonuses() {
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
        assert_eq!(amount, 0);
        target.guard.recovery_bonus = 11;
        target.guard.auto_chance = 255;
        target.guard.enemy_chance = -128;
        assert!(target.validate().is_ok());
    }

    #[test]
    fn recovery_chance_is_bounded_without_changing_enemy_chance() {
        for (base, bonus, expected) in [(75, 11, 82..=90), (0, 0, 0..=4), (255, 255, 100..=100)] {
            let mut random = Random::new(1);
            for _ in 0..100 {
                let mut guard = Guard {
                    enemy_chance: -30,
                    recovery_bonus: bonus,
                    ..Default::default()
                };
                guard.reset_auto_chance(base, &mut random);
                assert!(expected.contains(&guard.auto_chance));
                assert_eq!(guard.enemy_chance, -30);
            }
        }
    }
}

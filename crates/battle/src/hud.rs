//! Actor-owned display values (1068..107A), visited in the original callback order.
use crate::Actor;
use anyhow::{Context, Result};

/// Gates sampled at the shared HUD visit, before later input can dismiss the
/// selector. A menu/pause produces no new `hud_update` at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HudHolds {
    pub notices: bool,
    pub combo_tracking: bool,
    /// Source bit0x10 holds intro/recovery animation while the strip is open.
    pub intro: bool,
}

/// Read-only sample for the HUD visit before actor callbacks and contacts.
/// Presentation projects this position using the visit's camera pose.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ComboTracking {
    pub hits: i32,
    pub position: [f32; 3],
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ActorHud {
    /// Prepared local control slot, independent of character identity.
    pub control_slot: u8,
    /// Original actor common-callback clock, wrapping at 360.
    pub phase: u16,
    pub hp: i16,
    pub tp: i16,
    /// Shared contact countdown for portrait bounce and world-space body shake.
    pub portrait_bounce: u16,
    /// Target-change indicator, also read by the original camera interpolation.
    pub target_highlight: u16,
    pub combo_tracking: ComboTracking,
    /// Original portrait state 1B0 is 22, including the spell recovery callback.
    pub cast_released: bool,
    /// Source result actor controller 23 selects the victory portrait row.
    pub result_performance: bool,
    /// Prepared profile flag 0x80000 suppresses the shared contact countdown.
    pub suppress_bounce: bool,
    pub hp_trail: i16,
    pub tp_trail: i16,
    /// Four actor-owned 21B18 records, visited from this rotating cursor.
    pub floating: [FloatingNumber; 4],
    pub number_cursor: u8,
    /// 6885C's independent party HP/TP recovery channels.
    pub recovery: [RecoveryNumber; 2],
    hp_target: i16,
    tp_target: i16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FloatingNumber {
    pub value: i16,
    pub alpha: u16,
    pub pulse: u8,
    pub palette: u8,
    pub style: u8,
    pub position: [f32; 3],
    pub offset: [i16; 2],
    hold: u16,
}

impl FloatingNumber {
    fn advance(&mut self) {
        if self.hold != 0 {
            self.pulse = self.pulse.saturating_sub(2);
            self.hold -= 1;
        } else if self.alpha != 0 {
            self.offset[0] = self.offset[0].wrapping_add(1);
            self.alpha = self.alpha.saturating_sub(8);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKind {
    Hp,
    Tp,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryNumber {
    pub value: i16,
    pub alpha: u16,
    /// Signed sixteenths relative to the original party-slot screen origin.
    pub x_delta: i16,
    velocity: i8,
    age: u8,
}

impl RecoveryNumber {
    fn emit(&mut self, value: i16) {
        self.value = if self.alpha == 0 {
            value
        } else {
            self.value.wrapping_add(value)
        };
        self.alpha = 255;
        self.x_delta = 0;
        self.velocity = 24;
        self.age = 0;
    }

    fn advance(&mut self) {
        if self.alpha != 0 {
            self.x_delta = self.x_delta.wrapping_add(i16::from(self.velocity));
            self.velocity = self.velocity.wrapping_sub(1);
            if self.age >= 20 {
                self.alpha = self.alpha.saturating_sub(8);
            }
            self.age = self.age.wrapping_add(1);
        }
    }
}

impl ActorHud {
    pub(crate) fn initialize(actor: &Actor) -> Self {
        Self {
            // 1CAA8 copies the low halfwords; 2B18C initializes bar targets.
            hp: actor.hp as i16,
            tp: actor.tp as i16,
            hp_target: actor.hp_percent(),
            tp_target: actor.tp_percent(),
            suppress_bounce: actor.hud.suppress_bounce,
            control_slot: actor.hud.control_slot,
            // 1CAA8 initializes the party's target-change indicator.
            target_highlight: if actor.side == crate::Side::Party {
                120
            } else {
                0
            },
            ..Self::default()
        }
    }

    /// 70AE4 -> 65E24 runs before model/actor callbacks and contacts.
    pub(crate) fn numbers(&mut self, hp: i32, tp: u16, command_pause: bool) {
        approach(&mut self.hp, hp, &[100, 10, 1]);
        approach(&mut self.tp, i32::from(tp as i16), &[10, 1]);
        for number in &mut self.floating {
            number.advance();
        }
        // 71230 retains only command-menu bit0x10 before 7140C's recovery
        // visit. HP/TP approach and floating numbers remain source-live; the
        // party recovery channels hold on the command frame.
        if !command_pause {
            for number in &mut self.recovery {
                number.advance();
            }
        }
    }

    /// 2503C: trails follow retained reset targets, not this visit's live HP/TP.
    pub(crate) fn common(&mut self) {
        self.phase = (self.phase + 1) % 360;
        self.portrait_bounce = self.portrait_bounce.saturating_sub(1);
        self.target_highlight = self.target_highlight.saturating_sub(1);
        for (value, target) in [
            (&mut self.hp_trail, self.hp_target),
            (&mut self.tp_trail, self.tp_target),
        ] {
            if *value < target {
                *value = target;
            } else if *value > target {
                *value -= 1;
            }
        }
    }

    /// Common contact tail 3D654 -> 3AB80, including non-damaging results.
    pub(crate) fn contact(&mut self, guard: crate::GuardResult) {
        if !self.suppress_bounce {
            self.portrait_bounce = match guard {
                crate::GuardResult::Broken => 14,
                crate::GuardResult::Blocked { .. } => 6,
                crate::GuardResult::None => 10,
            };
        }
    }
}

fn approach(value: &mut i16, target: i32, steps: &[i32]) {
    for &step in steps {
        // The last pair compares directly; the larger steps use strict gaps.
        let gap = if step == 1 { 0 } else { step };
        if i32::from(*value) + gap < target {
            *value = value.wrapping_add(step as i16);
        }
        if i32::from(*value) - gap > target {
            *value = value.wrapping_sub(step as i16);
        }
    }
}

impl Actor {
    /// 3C4B0..3C554: ordinary actor numbers use the nominal amount, not HP delta.
    pub(crate) fn show_hit_number(&mut self, hit: crate::HitResult) {
        use crate::{Affinity, GuardResult, HitProtection, Side};
        let style = if self.hp <= 0 {
            // 63428 replaces the entire source result mask after lethal damage.
            0
        } else if matches!(hit.affinity, Affinity::Absorb | Affinity::Immune)
            || hit.protection == HitProtection::Avoided
        {
            // Absorption owns a separate type6 effect, not a 21B18 record.
            return;
        } else if hit.guard != GuardResult::None || hit.affinity == Affinity::Resistant {
            1
        } else if hit.critical || hit.affinity == Affinity::Weak {
            2
        } else {
            0
        };
        if self.hud.floating.iter().all(|number| number.hold == 0) {
            self.hud.number_cursor = 0;
        }
        let slot = usize::from(self.hud.number_cursor);
        self.hud.floating[slot] = FloatingNumber {
            value: hit.amount as i16,
            alpha: 255,
            pulse: 12,
            palette: u8::from(self.side == Side::Enemy),
            style,
            position: self.body.center,
            offset: [0, -24],
            hold: 35,
        };
        self.hud.number_cursor = (self.hud.number_cursor + 1) % 4;
    }

    /// Integer divisions from 1DBD4/1DBAC, before the renderer scales by 56/100.
    pub fn hp_percent(&self) -> i16 {
        (self.hp.wrapping_mul(100) / self.max_hp) as i16
    }

    pub fn tp_percent(&self) -> i16 {
        if self.max_tp == 0 {
            0
        } else {
            (u32::from(self.tp) * 100 / u32::from(self.max_tp)) as i16
        }
    }

    /// 2B18C recovery and 28DF4 defeat entry refresh the retained bar targets.
    pub(crate) fn reset_hud_targets(&mut self) {
        self.hud.cast_released = false;
        self.hud.hp_target = self.hp_percent();
        self.hud.tp_target = self.tp_percent();
    }
}

impl crate::Battle {
    /// 6885C initializes one party recovery channel after the authored award.
    pub fn show_recovery(
        &mut self,
        actor: crate::ActorId,
        kind: RecoveryKind,
        amount: i16,
    ) -> Result<()> {
        let actor = self
            .actors
            .get_mut(actor.index())
            .context("unknown recovery actor")?;
        if actor.side == crate::Side::Party {
            actor.hud.recovery[kind as usize].emit(amount);
        }
        Ok(())
    }

    /// 1C40: 70AE4 then156FC, before the0x34 selector/menu hold gate.
    pub(crate) fn advance_hud(&mut self, command_pause: bool) {
        // 1C40 passes CB6C(0x14) to 70AE4, filtering out stored-spell pause
        // bits 1|2. Notices independently test 0x1c. Command bit0x10 joins
        // the selector gate, while orders/radar/combo clocks continue.
        let held_by_owner = self.target_selector.is_some() || command_pause;
        self.hud_holds = HudHolds {
            notices: held_by_owner,
            combo_tracking: held_by_owner,
            intro: command_pause,
        };
        self.advance_target_markers();
        for actor in &mut self.actors {
            actor.hud.combo_tracking = ComboTracking {
                hits: actor.reaction.combo_hits,
                position: actor.position,
            };
            actor.hud.numbers(actor.hp, actor.tp, command_pause);
        }
        self.hud_update = self.hud_update.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit() -> crate::HitResult {
        crate::HitResult {
            amount: 99,
            hp_change: -10,
            critical: false,
            affinity: crate::Affinity::Normal,
            guard: crate::GuardResult::None,
            auto_guard: false,
            armored: false,
            protection: crate::HitProtection::None,
        }
    }

    #[test]
    fn floating_numbers_keep_nominal_amount_sampled_point_and_source_hold() {
        let mut actor = crate::tests::actor(crate::Side::Enemy);
        actor.body.center = [10., 20., 30.];
        actor.show_hit_number(hit());
        actor.body.center = [50.; 3];
        let number = &mut actor.hud.floating[0];
        assert_eq!(
            (number.value, number.position, number.palette),
            (99, [10., 20., 30.], 1)
        );
        for _ in 0..35 {
            number.advance();
        }
        assert_eq!(
            (number.hold, number.alpha, number.pulse, number.offset),
            (0, 255, 0, [0, -24])
        );
        number.advance();
        assert_eq!((number.alpha, number.offset[0]), (247, 1));
        for _ in 0..31 {
            number.advance();
        }
        assert_eq!((number.alpha, number.offset[0]), (0, 32));
    }

    #[test]
    fn floating_cursor_reuses_zero_after_holds_even_while_older_digits_fade() {
        let mut actor = crate::tests::actor(crate::Side::Party);
        for amount in 1..=5 {
            actor.show_hit_number(crate::HitResult { amount, ..hit() });
        }
        assert_eq!(actor.hud.number_cursor, 1);
        assert_eq!(actor.hud.floating.map(|number| number.value), [5, 2, 3, 4]);
        for _ in 0..35 {
            actor.hud.numbers(actor.hp, actor.tp, false);
        }
        actor.show_hit_number(hit());
        assert_eq!(actor.hud.number_cursor, 1);
        assert_eq!(actor.hud.floating.map(|number| number.alpha), [255; 4]);
        assert_eq!(actor.hud.floating.map(|number| number.value), [99, 2, 3, 4]);
    }

    #[test]
    fn damage_style_honors_guard_priority_and_lethal_mask_reset() {
        use crate::{Affinity, GuardResult, HitProtection};
        for (affinity, critical, guard, protection, hp, expected) in [
            (
                Affinity::Weak,
                false,
                GuardResult::None,
                HitProtection::None,
                10,
                Some(2),
            ),
            (
                Affinity::Resistant,
                true,
                GuardResult::None,
                HitProtection::Reduced,
                10,
                Some(1),
            ),
            (
                Affinity::Weak,
                true,
                GuardResult::Broken,
                HitProtection::None,
                10,
                Some(1),
            ),
            (
                Affinity::Normal,
                false,
                GuardResult::None,
                HitProtection::Reduced,
                10,
                Some(0),
            ),
            (
                Affinity::Normal,
                false,
                GuardResult::None,
                HitProtection::Avoided,
                10,
                None,
            ),
            (
                Affinity::Immune,
                false,
                GuardResult::None,
                HitProtection::None,
                10,
                None,
            ),
            (
                Affinity::Absorb,
                false,
                GuardResult::None,
                HitProtection::None,
                10,
                None,
            ),
            (
                Affinity::Resistant,
                true,
                GuardResult::None,
                HitProtection::None,
                0,
                Some(0),
            ),
        ] {
            let mut actor = crate::tests::actor(crate::Side::Party);
            actor.hp = hp;
            actor.show_hit_number(crate::HitResult {
                affinity,
                critical,
                guard,
                protection,
                armored: true,
                ..hit()
            });
            let number = actor.hud.floating[0];
            assert_eq!((number.alpha != 0).then_some(number.style), expected);
        }
    }

    #[test]
    fn recovery_visits_match_original_fixed_point_and_accumulate_only_while_visible() {
        let mut number = RecoveryNumber::default();
        number.emit(12);
        for visit in 1..=52 {
            number.advance();
            let expected = match visit {
                1 => Some((24, 23, 255)),
                20 => Some((290, 4, 255)),
                21 => Some((294, 3, 247)),
                25 => Some((300, -1, 215)),
                52 => Some((-78, -28, 0)),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!((number.x_delta, number.velocity, number.alpha), expected);
            }
        }
        number.emit(5);
        assert_eq!(number.value, 5);
        number.advance();
        number.emit(7);
        assert_eq!(
            (
                number.value,
                number.x_delta,
                number.velocity,
                number.age,
                number.alpha
            ),
            (12, 0, 24, 0, 255)
        );
    }

    #[test]
    fn strict_decimal_gaps_and_sequential_steps() {
        for (target, expected) in [(10, 1), (11, 11), (100, 11), (101, 101), (111, 111)] {
            let mut hud = ActorHud::default();
            hud.numbers(target, target as u16, false);
            assert_eq!(hud.hp, expected);
        }
        let mut hud = ActorHud {
            hp: 100,
            tp: 100,
            ..Default::default()
        };
        hud.numbers(0, 0, false);
        assert_eq!((hud.hp, hud.tp), (89, 89));
    }

    #[test]
    fn loss_waits_for_recovery_then_decays_one_percent_per_actor_visit() {
        let mut actor = crate::tests::actor(crate::Side::Party);
        actor.hp = actor.max_hp;
        actor.hud = ActorHud::initialize(&actor);
        actor.hud.common();
        actor.hp = 20;
        actor.hud.common();
        assert_eq!(actor.hud.hp_trail, 100);
        actor.reset_hud_targets();
        actor.hud.common();
        assert_eq!(actor.hud.hp_trail, 99);
        actor.hp = 100;
        actor.reset_hud_targets();
        actor.hud.common();
        assert_eq!(actor.hud.hp_trail, 100);
    }

    #[test]
    fn contact_bounce_and_menu_hold_use_simulation_visits() {
        use crate::{Battle, BattleInput, GuardResult, Side};
        let prepared = crate::tests::prepared(
            "pub task run() {}",
            vec![
                crate::tests::actor(Side::Party),
                crate::tests::actor(Side::Enemy),
            ],
            1,
        );
        let mut battle = Battle::new(prepared);
        battle
            .show_recovery(crate::ActorId(0), RecoveryKind::Tp, 9)
            .unwrap();
        battle.actors[0].show_hit_number(hit());
        battle.actors[0].hud.contact(GuardResult::Broken);
        battle.actors[0].hp = 39;
        let paused = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            (
                paused.actors[0].hud.hp,
                paused.actors[0].hud.portrait_bounce
            ),
            (50, 14)
        );
        assert_eq!(paused.actors[0].hud.recovery[1].x_delta, 0);
        assert_eq!(paused.actors[0].hud.floating[0].pulse, 12);
        let next = battle.step(BattleInput::default()).unwrap();
        assert_eq!(
            (next.actors[0].hud.hp, next.actors[0].hud.portrait_bounce),
            (39, 13)
        );
        assert_eq!(next.actors[0].hud.recovery[1].x_delta, 24);
        assert_eq!(next.actors[0].hud.floating[0].pulse, 10);
        let hud = &mut battle.actors[0].hud;
        hud.contact(GuardResult::Blocked {
            first: true,
            special: false,
        });
        assert_eq!(hud.portrait_bounce, 6);
        hud.suppress_bounce = true;
        hud.contact(GuardResult::None);
        assert_eq!(hud.portrait_bounce, 6);
    }
}

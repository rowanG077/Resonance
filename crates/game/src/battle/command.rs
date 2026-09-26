//! Top-level command ownership (2108 -> 381C -> 9C20/954C/3534).
//! Presentation consumes the immutable frame; the game advances its clocks.
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, Battle, ButtonInput, Control, Side};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    pub controller: u8,
    /// Mapped action 3; also the fallback cancel in 954C.
    pub open: ButtonInput,
    pub confirm_a: ButtonInput,
    pub cancel_b: ButtonInput,
    /// Shared analog repeat event, -1/0/1.
    pub step: i8,
}

/// 5878 capability initialization for the admitted zero-override route.
/// Unsupported native_e7 cannot mutate 1F46 or 1E3C in an accepted session.
pub fn enabled_rows(escape_restricted: bool, story: i32) -> u8 {
    let mut enabled = 0xff;
    if escape_restricted {
        enabled &= !0x20;
    }
    if story < 0x156878 {
        enabled &= !0x02;
    }
    enabled
}

/// Formation order and capability are prepared once; actor eligibility is live.
#[derive(Debug, Clone)]
pub struct Setup {
    pub actors: Vec<ActorId>,
    pub enabled: u8,
}
impl Setup {
    pub fn admission(&self, battle: &Battle, controller: u8) -> Result<Option<Admission>> {
        ensure!(
            controller < 4,
            "invalid command controller slot {controller}"
        );
        for &id in &self.actors {
            let actor = battle
                .actors()
                .get(id.index())
                .context("command setup actor is absent from battle")?;
            ensure!(
                actor.side == Side::Party,
                "command setup actor is not a party member"
            );
            ensure!(
                actor.hud.control_slot < 4,
                "invalid command actor controller slot"
            );
        }
        if !battle.command_admission_allowed() {
            return Ok(None);
        }
        Ok(self.actors.iter().find_map(|&id| {
            let actor = &battle.actors()[id.index()];
            // 2254 rotates right4 and masks0xC: the byte offset is
            // ((107D >> 6) & 3) * 4. Modes2/3 alone require slot zero.
            let slot = actor.hud.control_slot;
            if slot != controller
                || matches!(actor.control, Control::Auto | Control::Enemy) && slot != 0
            {
                return None;
            }
            // 2108 does not reject KO/petrified owners. Prepared opening actions
            // prove the independent 10C2 command-block bit remains clear.
            Some(Admission {
                actor: id,
                controller: slot,
                enabled: self.enabled,
            })
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission {
    pub actor: ActorId,
    pub controller: u8,
    pub enabled: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub current: [i16; 2],
    pub previous: [i16; 2],
    pub trail_alpha: [i16; 8],
}
impl Cursor {
    fn at(selected: u8) -> Self {
        let point = [132 + i16::from(selected) * 56, 172];
        Self {
            current: point,
            previous: point,
            trail_alpha: [224, 208, 192, 176, 160, 144, 128, 112],
        }
    }
    fn move_to(&mut self, selected: u8) {
        let previous = self.current;
        *self = Self::at(selected);
        self.previous = previous;
    }
    fn advance(&mut self) {
        // 6E530: two iterations of FOUR unrolled halfword decrements.
        for alpha in &mut self.trail_alpha {
            *alpha = (*alpha - 8).max(0);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub actor: ActorId,
    pub selected: u8,
    /// Signed source word15958, wrapping on every retained3534 visit.
    pub animation: i32,
    pub enabled: u8,
    pub controller: u8,
    pub cursor: Cursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Opened {
        actor: ActorId,
    },
    Navigated {
        selected: u8,
    },
    Disabled {
        selected: u8,
    },
    /// Stage4 owns submenu dispatch. The host reports the missing route.
    Confirmed {
        selected: u8,
    },
    Cancelled,
    VoiceStreamsPaused(bool),
}

#[derive(Debug, Default)]
pub struct Step {
    pub events: Vec<Event>,
    /// Both admission and the closing visit still hold gameplay/intro clocks.
    pub pause_for_visit: bool,
    /// 381C resets repeat on admission; 9C20 resets it again on initialization.
    pub reset_repeat: bool,
}

#[derive(Debug, Clone, Copy)]
enum State {
    Closed,
    Pending(Admission),
    Open(Frame),
}

#[derive(Debug)]
pub struct Owner {
    state: State,
    selected: u8,
    animation: i32,
}
impl Default for Owner {
    fn default() -> Self {
        Self {
            state: State::Closed,
            selected: 0,
            animation: 0,
        }
    }
}
impl Owner {
    pub fn frame(&self) -> Option<Frame> {
        match self.state {
            State::Open(frame) => Some(frame),
            _ => None,
        }
    }
    pub fn step(&mut self, input: Input, admission: Option<Admission>) -> Step {
        match self.state {
            State::Closed => {
                let Some(admission) = admission.filter(|_| input.open.pressed) else {
                    return Step::default();
                };
                self.state = State::Pending(admission);
                // 381C sets dispatch7 and pause, but calls1878 without63F8.
                Step {
                    events: vec![Event::Opened {
                        actor: admission.actor,
                    }],
                    pause_for_visit: true,
                    reset_repeat: true,
                }
            }
            State::Pending(admission) => {
                // Next visit3534 calls9C20, preserving selection and age. Its
                // two cursor writes leave previous=current and full alpha.
                self.animation = self.animation.wrapping_add(1);
                self.state = State::Open(Frame {
                    actor: admission.actor,
                    selected: self.selected,
                    animation: self.animation,
                    enabled: admission.enabled,
                    controller: admission.controller,
                    cursor: Cursor::at(self.selected),
                });
                Step {
                    events: vec![Event::VoiceStreamsPaused(true)],
                    pause_for_visit: true,
                    reset_repeat: true,
                }
            }
            State::Open(mut frame) => {
                let mut result = Step {
                    pause_for_visit: true,
                    ..Step::default()
                };
                if input.controller != frame.controller {
                    frame.animation = frame.animation.wrapping_add(1);
                    frame.cursor.advance();
                    self.animation = frame.animation;
                    self.state = State::Open(frame);
                    return result;
                }
                if input.step != 0 {
                    frame.selected = (i16::from(frame.selected) + i16::from(input.step.signum()))
                        .rem_euclid(6) as u8;
                    frame.animation = 0;
                    frame.cursor.move_to(frame.selected);
                    result.events.push(Event::Navigated {
                        selected: frame.selected,
                    });
                }
                if input.confirm_a.pressed {
                    result
                        .events
                        .push(if frame.enabled & (1 << frame.selected) == 0 {
                            Event::Disabled {
                                selected: frame.selected,
                            }
                        } else {
                            Event::Confirmed {
                                selected: frame.selected,
                            }
                        });
                }
                // PhysicalB and mapped action3 are checked after A.
                let close = input.cancel_b.pressed || input.open.pressed;
                frame.animation = frame.animation.wrapping_add(1);
                self.selected = frame.selected;
                self.animation = frame.animation;
                if close {
                    result.events.push(Event::Cancelled);
                    result.events.push(Event::VoiceStreamsPaused(false));
                    self.state = State::Closed;
                } else {
                    frame.cursor.advance();
                    self.animation = frame.animation;
                    self.state = State::Open(frame);
                }
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{Actor, ActorAvailability, PreparedBattle};
    use std::sync::Arc;
    fn actor(side: Side) -> Actor {
        Actor {
            side,
            control: Default::default(),
            activity: Default::default(),
            availability: ActorAvailability::Active,
            guard: Default::default(),
            hp: 100,
            max_hp: 100,
            tp: 20,
            max_tp: 30,
            hud: Default::default(),
            overlimit: 0,
            overlimit_active: false,
            luck: 0,
            stats: Default::default(),
            elements: Default::default(),
            affinities: [resonance_battle::Affinity::Normal; 9],
            attack_power: 100,
            physical_arte_boost: false,
            recovery: Default::default(),
            petrified: false,
            position: [0.; 3],
            heading: 0.,
            facing_direction: [0., 0., 1.],
            effect_scale: 1.,
            framing: Default::default(),
            body: Default::default(),
            movement: Default::default(),
            reaction: Default::default(),
            hit_stop: 0,
        }
    }

    fn battle() -> (Battle, Setup) {
        let mut actor = actor(Side::Party);
        actor.control = Control::SemiAuto;
        let prepared =
            Arc::new(PreparedBattle::new(vec![actor], vec![], 1, vec![], vec![]).unwrap());
        let id = prepared.actor_ids().next().unwrap();
        (
            Battle::new(prepared),
            Setup {
                actors: vec![id],
                enabled: 0xdd,
            },
        )
    }
    fn edge() -> ButtonInput {
        ButtonInput {
            held: true,
            pressed: true,
            released: false,
        }
    }
    #[test]
    fn source_capability_retains_upper_bits_and_applies_independent_gates() {
        assert_eq!(enabled_rows(true, 2500), 0xdd);
        assert_eq!(enabled_rows(false, 2500), 0xfd);
        assert_eq!(enabled_rows(true, 0x156878), 0xdf);
        assert_eq!(enabled_rows(false, 0x156878), 0xff);
    }
    #[test]
    fn source_pilot_admission_initializer_navigation_disabled_escape_and_close() {
        let (battle, setup) = battle();
        let admission = setup.admission(&battle, 0).unwrap().unwrap();
        let mut owner = Owner::default();
        let mut events = Vec::new();
        for visit in 0..=107 {
            let mut input = Input::default();
            if visit == 0 {
                input.open = edge();
                input.confirm_a = edge();
            }
            if [15, 30, 45, 60, 75].contains(&visit) {
                input.step = 1;
            }
            if visit == 92 {
                input.confirm_a = edge();
            }
            if visit == 107 {
                input.cancel_b = edge();
            }
            let step = owner.step(input, Some(admission));
            assert!(step.pause_for_visit);
            assert_eq!(step.reset_repeat, visit <= 1);
            events.extend(step.events);
            match visit {
                0 | 107 => assert!(owner.frame().is_none()),
                1 => {
                    let f = owner.frame().unwrap();
                    assert_eq!(f.animation, 1);
                    assert_eq!(f.cursor.current, f.cursor.previous);
                    assert_eq!(
                        f.cursor.trail_alpha,
                        [224, 208, 192, 176, 160, 144, 128, 112]
                    );
                }
                2 | 15 | 30 | 45 | 60 | 75 => assert_eq!(
                    owner.frame().unwrap().cursor.trail_alpha,
                    [216, 200, 184, 168, 152, 136, 120, 104]
                ),
                92 => {
                    let f = owner.frame().unwrap();
                    assert_eq!((f.selected, f.animation, f.enabled), (5, 18, 0xdd));
                    assert_eq!(f.cursor.trail_alpha, [80, 64, 48, 32, 16, 0, 0, 0]);
                }
                _ => (),
            }
        }
        assert_eq!(events.len(), 10);
        assert_eq!(events[7], Event::Disabled { selected: 5 });
        assert_eq!(events[8], Event::Cancelled);
        assert_eq!(events[9], Event::VoiceStreamsPaused(false));
        assert!(
            !owner
                .step(Input::default(), Some(admission))
                .pause_for_visit
        );
        owner.step(
            Input {
                open: edge(),
                ..Input::default()
            },
            Some(admission),
        );
        owner.step(Input::default(), Some(admission));
        let reopened = owner.frame().unwrap();
        assert_eq!((reopened.selected, reopened.animation), (5, 34));
        assert_eq!(reopened.cursor.current, reopened.cursor.previous);
    }
    #[test]
    fn admission_uses_live_control_and_retains_source_ko_eligibility() {
        let mut ko = actor(Side::Party);
        ko.hp = 0;
        ko.control = Control::SemiAuto;
        let prepared = Arc::new(PreparedBattle::new(vec![ko], vec![], 1, vec![], vec![]).unwrap());
        let id = prepared.actor_ids().next().unwrap();
        let setup = Setup {
            actors: vec![id],
            enabled: 0xdd,
        };
        let mut battle = Battle::new(prepared);
        assert!(setup.admission(&battle, 1).unwrap().is_none());
        assert!(setup.admission(&battle, 0).unwrap().is_some());
        battle.recognize_result();
        assert!(setup.admission(&battle, 0).unwrap().is_none());
    }
    #[test]
    fn nonzero_controller_slot_admits_manual_and_semi_auto_only() {
        for (control, allowed) in [
            (Control::Manual, true),
            (Control::SemiAuto, true),
            (Control::Auto, false),
        ] {
            let mut party = actor(Side::Party);
            party.control = control;
            party.hud.control_slot = 1;
            let prepared =
                Arc::new(PreparedBattle::new(vec![party], vec![], 1, vec![], vec![]).unwrap());
            let id = prepared.actor_ids().next().unwrap();
            let setup = Setup {
                actors: vec![id],
                enabled: 0xdd,
            };
            let battle = Battle::new(prepared);
            assert_eq!(setup.admission(&battle, 1).unwrap().is_some(), allowed);
            assert!(setup.admission(&battle, 0).unwrap().is_none());
        }
    }
    #[test]
    fn malformed_setup_is_an_error_before_admission() {
        let (first_battle, mut setup) = battle();
        let foreign = Arc::new(
            PreparedBattle::new(
                vec![actor(Side::Party), actor(Side::Enemy)],
                vec![],
                1,
                vec![],
                vec![],
            )
            .unwrap(),
        );
        setup.actors = vec![foreign.actor_ids().nth(1).unwrap()];
        assert!(setup.admission(&first_battle, 0).is_err());
        let (battle, setup) = battle();
        assert!(setup.admission(&battle, 4).is_err());
    }
    #[test]
    fn confirm_then_mapped_cancel_retains_source_event_order() {
        let (battle, setup) = battle();
        let admission = setup.admission(&battle, 0).unwrap();
        let mut owner = Owner::default();
        owner.step(
            Input {
                open: edge(),
                ..Input::default()
            },
            admission,
        );
        owner.step(Input::default(), admission);
        let close = owner.step(
            Input {
                open: edge(),
                confirm_a: edge(),
                ..Input::default()
            },
            admission,
        );
        assert_eq!(
            close.events,
            [
                Event::Confirmed { selected: 0 },
                Event::Cancelled,
                Event::VoiceStreamsPaused(false)
            ]
        );
        assert!(close.pause_for_visit);
        assert!(owner.frame().is_none());
    }
}

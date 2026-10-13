//! Stored-scene preludes tint scenery independently of actor and effect lighting.
use resonance_battle::{BattleClock, Cue, StageTint};
use resonance_game::battle::stored_spells::TRANSITION_TICKS;

const PRELUDE_COLOR: [u8; 3] = [16; 3];
const PRELUDE_STEP: u8 = 4;
const RESTORE_STEP: u8 = 2;

pub(super) struct State {
    original: [u8; 4],
    color: [u8; 4],
    prelude_remaining: u16,
    transitioning: bool,
    scene: Option<StageTint>,
    unison: Option<[u8; 3]>,
    unison_generation: Option<u64>,
}

impl State {
    pub fn new(color: [u8; 4]) -> Self {
        Self {
            original: color,
            color,
            prelude_remaining: 0,
            transitioning: false,
            scene: None,
            unison: None,
            unison_generation: None,
        }
    }

    pub fn color(&self) -> [u8; 4] {
        self.unison
            .map_or(self.color, |[r, g, b]| [r, g, b, self.color[3]])
    }

    pub fn advance_unison(
        &mut self,
        clock: BattleClock,
        frame: Option<&resonance_battle::UnisonFrame>,
    ) {
        use resonance_battle::UnisonPhase;
        if clock.feedback_paused() {
            return;
        }
        let base = [self.color[0], self.color[1], self.color[2]];
        if let Some(frame) = frame {
            if self.unison_generation != Some(frame.generation) {
                self.unison_generation = Some(frame.generation);
                self.unison = Some(base);
            }
            if let Some(target) = frame.stage_color {
                let color = self.unison.get_or_insert(base);
                if frame.phase == UnisonPhase::CombinationPose {
                    *color = target;
                } else if frame.phase != UnisonPhase::Windup || frame.elapsed != 0 {
                    approach(
                        color,
                        target,
                        if frame.phase == UnisonPhase::Windup {
                            16
                        } else {
                            4
                        },
                    );
                }
                return;
            }
        } else {
            self.unison_generation = None;
        }
        if let Some(color) = &mut self.unison {
            approach(color, base, 2);
            if *color == base {
                self.unison = None;
            }
        }
    }

    pub fn advance(&mut self, clock: BattleClock, cues: &[Cue]) {
        if clock.feedback_paused() {
            return;
        }
        let transitioning = clock == BattleClock::SpellTransition;
        if transitioning && !self.transitioning {
            self.prelude_remaining = TRANSITION_TICKS;
        }
        self.transitioning = transitioning;
        if self.prelude_remaining != 0 {
            self.prelude_remaining -= 1;
        } else if let Some(scene) = &mut self.scene {
            scene.duration -= 1;
            if scene.duration == 0 {
                self.scene = None;
            }
        }
        let base = self.scene.map_or(
            [self.original[0], self.original[1], self.original[2]],
            |scene| scene.color,
        );
        let target = if self.prelude_remaining != 0 {
            std::array::from_fn(|axis| PRELUDE_COLOR[axis].min(base[axis]))
        } else {
            base
        };
        let step = if self.prelude_remaining != 0 || self.scene.is_some() {
            PRELUDE_STEP
        } else {
            RESTORE_STEP
        };
        for (color, target) in self.color[..3].iter_mut().zip(target) {
            *color = if *color < target {
                color.saturating_add(step).min(target)
            } else {
                color.saturating_sub(step).max(target)
            };
        }
        // Scene initialization follows this update's color step.
        for cue in cues {
            if let Cue::StageTint(request) = cue {
                self.scene = Some(self.scene.map_or(*request, |current| StageTint {
                    color: std::array::from_fn(|axis| current.color[axis].min(request.color[axis])),
                    duration: current.duration.max(request.duration),
                }));
            }
        }
    }
}

fn approach(color: &mut [u8; 3], target: [u8; 3], step: u8) {
    for (current, wanted) in color.iter_mut().zip(target) {
        *current = if *current < wanted {
            current.saturating_add(step).min(wanted)
        } else {
            current.saturating_sub(step).max(wanted)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_tint_holds_then_restores_prepared_scenery_color() {
        let original = [60, 68, 64, 123];
        let mut state = State::new(original);
        state.advance(BattleClock::Running, &[]);
        assert_eq!(state.color(), original);
        state.advance(BattleClock::SpellTransition, &[]);
        assert_eq!(state.color(), [56, 64, 60, 123]);
        for _ in 0..10 {
            state.advance(BattleClock::Held, &[]);
        }
        assert_eq!(state.color(), [56, 64, 60, 123]);
        for _ in 1..13 {
            state.advance(BattleClock::SpellTransition, &[]);
        }
        assert_eq!(state.color(), [16, 16, 16, 123]);
        for _ in 13..TRANSITION_TICKS - 1 {
            state.advance(BattleClock::SpellTransition, &[]);
        }
        assert_eq!(state.color(), [16, 16, 16, 123]);
        state.advance(BattleClock::SpellTransition, &[]);
        assert_eq!(state.color(), [18, 18, 18, 123]);
        for _ in 0..25 {
            state.advance(BattleClock::Running, &[]);
        }
        assert_eq!(state.color(), original);
        state.advance(BattleClock::SpellTransition, &[]);
        assert_eq!(state.color(), [56, 64, 60, 123]);
    }

    #[test]
    fn scene_color_waits_one_update_and_suspends_beneath_the_prelude() {
        let mut state = State::new([8, 64, 64, 127]);
        let tint = StageTint {
            color: [4, 8, 12],
            duration: 100,
        };
        state.advance(BattleClock::Running, &[Cue::StageTint(tint)]);
        assert_eq!(state.color(), [8, 64, 64, 127]);
        state.advance(BattleClock::Running, &[]);
        assert_eq!(state.color(), [4, 60, 60, 127]);
        assert_eq!(state.scene.unwrap().duration, 99);
        for _ in 0..TRANSITION_TICKS {
            state.advance(BattleClock::SpellTransition, &[]);
        }
        assert_eq!(state.scene.unwrap().duration, 99);
        assert_eq!(state.color(), [4, 8, 12, 127]);
        state.advance(BattleClock::Held, &[]);
        assert_eq!(state.scene.unwrap().duration, 99);
        for _ in 0..98 {
            state.advance(BattleClock::Running, &[]);
        }
        assert_eq!(state.color(), [4, 8, 12, 127]);
        state.advance(BattleClock::Running, &[]);
        assert!(state.scene.is_none());
        assert_eq!(state.color(), [6, 10, 14, 127]);
        for _ in 0..27 {
            state.advance(BattleClock::Running, &[]);
        }
        assert_eq!(state.color(), [8, 64, 64, 127]);
    }
}

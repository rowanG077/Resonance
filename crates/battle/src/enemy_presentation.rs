//! Optional enemy opening shots and final-enemy voice holds share the battle clock.
use crate::{Actor, ActorId, Battle, BattleClock, CameraPose, Cue, Side, UnisonFeedback};
use anyhow::{Result, ensure};
use resonance_content::battle_enemy::Presentation;
use resonance_content::battle_voice::TimedVoice;

// Scene time is measured in fixed battle updates, independently of audio playback.
const OPENING_TAIL: u16 = 30;
const DEFEAT_DELAY: u8 = 10;
const DEFEAT_FEEDBACK_TAIL: u16 = 60;
const DISSOLVE_STEP: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyPresentationFrame {
    pub hide_party: bool,
    pub hide_hud: bool,
    pub dissolve: u8,
    pub feedback: Option<UnisonFeedback>,
}

struct Opening {
    actor: ActorId,
    voice: TimedVoice,
    anchor: [f32; 3],
    age: u16,
}

struct Enemy {
    actor: ActorId,
    voice_ticks: u16,
    disabled_ticks: u8,
    finished: bool,
}

enum Hold {
    Voice(u16),
    Release,
}

#[derive(Default)]
pub(crate) struct State {
    enemies: Vec<Enemy>,
    opening: Option<Opening>,
    hold: Option<Hold>,
    dissolve: u8,
    feedback: Option<UnisonFeedback>,
    generation: u64,
}

impl State {
    pub(crate) fn new(
        actors: &[Actor],
        definitions: &[(ActorId, Presentation)],
        entry: bool,
    ) -> Self {
        let first_enemy = actors.iter().position(|actor| actor.side == Side::Enemy);
        Self {
            opening: definitions.iter().find_map(|&(actor, definition)| {
                (entry && Some(actor.index()) == first_enemy)
                    .then_some(definition.opening)
                    .flatten()
                    .map(|voice| Opening {
                        actor,
                        voice,
                        anchor: actors[actor.index()].effect_origin(),
                        age: 0,
                    })
            }),
            enemies: definitions
                .iter()
                .map(|&(actor, definition)| Enemy {
                    actor,
                    voice_ticks: definition.defeat.ticks,
                    disabled_ticks: 0,
                    finished: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    pub(crate) fn opening(&self) -> bool {
        self.opening.is_some()
    }

    pub(crate) fn holding(&self) -> bool {
        self.opening() || self.hold.is_some()
    }

    pub(crate) fn delays_result(&self, actors: &[Actor]) -> bool {
        self.holding()
            || (!actors
                .iter()
                .any(|actor| actor.side == Side::Enemy && actor.available())
                && self.enemies.iter().any(|enemy| !enemy.finished))
    }

    pub(crate) fn camera(&self) -> Option<CameraPose> {
        self.opening.as_ref().map(|opening| {
            let mut focus = opening.anchor;
            focus[1] += -20. + 0.1 * f32::from(opening.age);
            let eye = [focus[0] - 1050., 1500. * 5_f32.to_radians().sin(), focus[2]];
            let vertical = eye[1] - focus[1];
            CameraPose {
                eye,
                focus,
                pitch: vertical.atan2(1050.).to_degrees(),
                yaw: 180.,
                radius: 1050_f32.hypot(vertical),
            }
        })
    }

    pub(crate) fn frame(&self) -> Option<EnemyPresentationFrame> {
        (!self.enemies.is_empty()).then_some(EnemyPresentationFrame {
            hide_party: self.opening(),
            hide_hud: self.opening(),
            dissolve: self.dissolve,
            feedback: self.feedback,
        })
    }

    fn feedback(&mut self, remaining: u16, limit: u8, alpha: u8) {
        self.generation += 1;
        self.feedback = Some(UnisonFeedback {
            generation: self.generation,
            expansion: self.feedback.map_or(0, |feedback| feedback.expansion),
            alpha,
            limit,
            remaining,
        });
    }

    pub(crate) fn victory(&mut self) {
        if !self.enemies.is_empty() {
            self.feedback(45, 8, 128);
        }
    }

    fn tick_feedback(&mut self) {
        self.dissolve = self.dissolve.saturating_sub(DISSOLVE_STEP);
        if let Some(feedback) = &mut self.feedback {
            if feedback.remaining != 0 {
                feedback.remaining -= 1;
                if feedback.expansion < feedback.limit {
                    feedback.expansion += 1;
                }
            } else {
                feedback.expansion = feedback.expansion.saturating_sub(1);
                if feedback.expansion == 0 {
                    self.feedback = None;
                }
            }
        }
    }
}

impl crate::PreparedBattle {
    pub fn with_enemy_presentations(
        mut self,
        definitions: Vec<(ActorId, Presentation)>,
    ) -> Result<Self> {
        let mut actors = std::collections::BTreeSet::new();
        for &(actor, definition) in &definitions {
            ensure!(
                self.actors
                    .get(actor.index())
                    .is_some_and(|actor| actor.side == Side::Enemy)
                    && actors.insert(actor),
                "enemy presentation requires distinct enemy actors"
            );
            ensure!(
                definition.defeat.ticks <= u16::MAX - DEFEAT_FEEDBACK_TAIL
                    && definition
                        .opening
                        .is_none_or(|voice| voice.ticks < u16::MAX - OPENING_TAIL),
                "enemy presentation duration overflows battle time"
            );
        }
        self.resources.enemy_presentations = definitions;
        Ok(self)
    }
}

impl Battle {
    /// Returns true only when this update belongs to the opening or defeat hold.
    pub(crate) fn advance_enemy_presentation(&mut self, cues: &mut Vec<Cue>) -> Result<bool> {
        self.enemy_presentation.tick_feedback();
        if let Some(opening) = &mut self.enemy_presentation.opening {
            self.clock = BattleClock::Running;
            if opening.age == 0
                && let Some(sound) = opening.voice.sound
            {
                cues.push(Cue::Voice {
                    actor: opening.actor,
                    sound,
                    priority: crate::VoicePriority::Announcement,
                    position: self.actors[opening.actor.index()].audio_position(),
                    centered: true,
                });
            }
            opening.age += 1;
            let end = opening.voice.ticks + OPENING_TAIL;
            if opening.age == end {
                self.enemy_presentation.dissolve = u8::MAX;
            } else if opening.age > end {
                self.enemy_presentation.opening = None;
                if let Some(camera) = &mut self.camera {
                    *camera = crate::camera::Camera::new(
                        camera.definition.clone(),
                        &self.actors,
                        self.runtime[camera.definition.leader.index()].target,
                    )?;
                }
            }
            return Ok(true);
        }
        if let Some(hold) = self.enemy_presentation.hold.take() {
            self.clock = BattleClock::Held;
            self.enemy_presentation.hold = match hold {
                Hold::Voice(0) => Some(Hold::Release),
                Hold::Voice(ticks) => Some(Hold::Voice(ticks - 1)),
                Hold::Release => {
                    self.clock = BattleClock::Running;
                    None
                }
            };
            return Ok(true);
        }
        let active = self
            .actors
            .iter()
            .any(|actor| actor.side == Side::Enemy && actor.available());
        let mut hold_ticks: Option<u16> = None;
        for enemy in &mut self.enemy_presentation.enemies {
            if enemy.finished || self.actors[enemy.actor.index()].available() {
                continue;
            }
            if active {
                enemy.finished = true;
            } else if enemy.disabled_ticks < DEFEAT_DELAY {
                enemy.disabled_ticks += 1;
            } else {
                enemy.finished = true;
                hold_ticks = Some(hold_ticks.unwrap_or(0).max(enemy.voice_ticks));
            }
        }
        if let Some(ticks) = hold_ticks {
            self.enemy_presentation.hold = Some(Hold::Voice(ticks));
            self.enemy_presentation
                .feedback(ticks + DEFEAT_FEEDBACK_TAIL, 2, 176);
            self.clock = BattleClock::Held;
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActorAvailability, ActorSetup, BattleInput, BattlePhase, BattleResult, PreparedBattle,
        Sound,
    };

    #[test]
    fn enemy_opening_and_final_voice_hold_pause_and_finish_once() -> Result<()> {
        let definition = Presentation {
            opening: Some(TimedVoice {
                sound: Some(Sound::Stream(7)),
                ticks: 2,
            }),
            defeat: TimedVoice {
                sound: Some(Sound::Stream(8)),
                ticks: 3,
            },
        };
        let prepared = |companions| -> Result<PreparedBattle> {
            let mut actors = vec![
                (crate::tests::actor(Side::Party), ActorSetup::default()),
                (crate::tests::actor(Side::Enemy), ActorSetup::default()),
            ];
            if companions {
                actors.push((crate::tests::actor(Side::Enemy), ActorSetup::default()));
            }
            PreparedBattle::new(actors, Default::default(), 1)?
                .with_enemy_presentations(vec![(ActorId(1), definition)])
        };
        let mut battle = prepared(false)?.with_entry().finish()?;
        assert_eq!(battle.phase(), BattlePhase::Entry);
        assert!(battle.snapshot().enemy_presentation.unwrap().hide_party);
        let paused = battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(paused.update, 0);
        assert!(paused.cues.is_empty());
        let first = battle.step(BattleInput::default())?;
        assert_eq!(
            first
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Voice { centered: true, .. }))
                .count(),
            1
        );
        for _ in 1..=definition.opening.unwrap().ticks + OPENING_TAIL {
            let frame = battle.step(BattleInput::default())?;
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Voice { .. }))
            );
        }
        assert_eq!(battle.phase(), BattlePhase::Combat);
        assert!(!battle.snapshot().enemy_presentation.unwrap().hide_party);
        battle.actors[1].availability = ActorAvailability::Dead;
        battle.actors[1].hp = 0;
        for _ in 0..=DEFEAT_DELAY {
            assert_eq!(battle.recognize_result(), None);
            battle.step(BattleInput::default())?;
        }
        assert!(!battle.command_admission_allowed());
        assert!(battle.is_paused());
        let before = battle.snapshot().enemy_presentation;
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(battle.snapshot().enemy_presentation, before);
        for _ in 0..definition.defeat.ticks + 2 {
            assert_eq!(battle.recognize_result(), None);
            let frame = battle.step(BattleInput::default())?;
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Voice { .. }))
            );
        }
        assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));

        // A defeated boss with a surviving companion does not delay a later victory.
        let mut battle = prepared(true)?.finish()?;
        battle.actors[1].availability = ActorAvailability::Dead;
        battle.actors[1].hp = 0;
        battle.step(BattleInput::default())?;
        battle.actors[2].availability = ActorAvailability::Dead;
        battle.actors[2].hp = 0;
        assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));
        Ok(())
    }
}

//! The encounter owns combat, results, and the final handoff to the field.
//! Each update advances the world once and applies result mutations directly.
pub mod victory_selection;
use super::{
    command,
    results::{Candidate, Presentation},
};
use anyhow::{Result, ensure};
use resonance_battle::{
    Battle, BattleFrame, BattleInput, BattlePhase, BattleResult, TransitionKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    VictoryBanner,
    SynchronizeResults,
    NextResultPage,
    DefeatNotice,
    EscapeNotice,
    PlayMusic { track: u16, fade_ms: u16 },
}

#[derive(Debug, Default)]
pub struct Input {
    pub battle: BattleInput,
    pub command: command::Input,
    pub confirm: bool,
    pub cook: bool,
}

#[derive(Clone, Copy)]
enum Phase {
    Combat,
    VictoryFade,
    Results,
    Defeat,
    EscapeNotice { remaining: u8 },
    Closing,
    Finish,
    Finished,
    Faulted,
}

const VICTORY_MUSIC: u16 = 95;
const DEFEAT_MUSIC: u16 = 96;
// A fade takes roughly a quarter-second at 60 updates/second.
const FADE_STEP: u8 = 16;

pub struct Lifecycle {
    phase: Phase,
    command: command::Owner,
    command_setup: Option<command::Setup>,
    command_events: Vec<command::Event>,
}

impl Lifecycle {
    pub fn new(command_setup: Option<command::Setup>) -> Self {
        Self {
            phase: Phase::Combat,
            command: command::Owner::default(),
            command_setup,
            command_events: Vec::new(),
        }
    }

    pub fn step(
        &mut self,
        battle: &mut Battle,
        mut input: Input,
        candidate: &mut Candidate,
        presentation: &mut impl Presentation,
    ) -> Result<BattleFrame> {
        ensure!(
            !matches!(self.phase, Phase::Finished | Phase::Faulted),
            "encounter lifecycle has ended"
        );
        if battle.phase() != BattlePhase::Combat {
            self.command.close();
        }
        let result = (|| {
            if matches!(self.phase, Phase::Finish) {
                let frame = battle.finish_result()?;
                self.phase = Phase::Finished;
                return Ok(frame);
            }
            let cues = if input.battle.paused {
                candidate.world_update(battle, input.battle)?
            } else {
                let admission = self
                    .command_setup
                    .as_ref()
                    .map(|setup| setup.admission(battle, input.command.controller))
                    .transpose()?
                    .flatten();
                let command = self.command.step_with_candidate(
                    input.command,
                    admission,
                    battle,
                    candidate,
                )?;
                self.command_events.extend(command.events);
                if command.paused {
                    input.battle.paused = true;
                    candidate.world_update(battle, input.battle)?
                } else {
                    self.advance(battle, input, candidate, presentation)?
                }
            };
            let frame = battle.publish(cues);
            presentation.present(&frame)?;
            Ok(frame)
        })();
        if result.is_err() {
            self.phase = Phase::Faulted;
            self.command.close();
            self.command_events.clear();
            battle.invalidate();
        }
        result
    }

    /// Pending and visible command visits keep the admitted controller slot.
    pub fn command_controller(&self) -> Option<u8> {
        self.command.controller()
    }

    pub fn command_input_kind(&self) -> command::InputKind {
        self.command.input_kind()
    }

    pub fn equipment_character(&self) -> usize {
        self.command.equipment_character()
    }

    pub fn take_menu_memory(&mut self) -> command::Memory {
        self.command.close();
        self.command.memory()
    }

    pub fn restore_menu_memory(&mut self, memory: command::Memory) {
        self.command.restore_memory(memory);
    }

    /// Immutable command state is consumed directly by the presentation strip.
    pub fn command_frame(&self) -> Option<command::Frame> {
        self.command.frame()
    }

    pub fn recover_command_page(
        &mut self,
        battle: &mut Battle,
        candidate: &mut Candidate,
    ) -> Result<()> {
        match self.command.return_to_strip() {
            Some(command::View::Strategy(_)) => candidate.finish_strategy(battle)?,
            Some(command::View::Equipment(_)) => candidate.cancel_equipment(),
            _ => {}
        }
        Ok(())
    }

    pub fn take_command_events(&mut self) -> Vec<command::Event> {
        std::mem::take(&mut self.command_events)
    }

    fn advance(
        &mut self,
        battle: &mut Battle,
        input: Input,
        candidate: &mut Candidate,
        presentation: &mut impl Presentation,
    ) -> Result<Vec<resonance_battle::Cue>> {
        let mut entered = false;
        if matches!(self.phase, Phase::Combat)
            && let Some(result) = battle.recognize_update()
        {
            self.phase = match result {
                BattleResult::Victory => {
                    candidate.choose_victory(battle)?;
                    presentation.request(Request::VictoryBanner, None, battle)?;
                    battle.begin_transition(TransitionKind::FadeOut, [0; 3], FADE_STEP)?;
                    Phase::VictoryFade
                }
                BattleResult::Defeat => {
                    presentation.request(Request::DefeatNotice, None, battle)?;
                    presentation.request(
                        Request::PlayMusic {
                            track: DEFEAT_MUSIC,
                            fade_ms: 100,
                        },
                        None,
                        battle,
                    )?;
                    Phase::Defeat
                }
                BattleResult::Escaped => {
                    battle.begin_transition(TransitionKind::FadeOut, [0; 3], FADE_STEP)?;
                    if battle.forced_escape() {
                        Phase::Closing
                    } else {
                        presentation.request(Request::EscapeNotice, None, battle)?;
                        // Leave the success notice visible for one second before fading.
                        Phase::EscapeNotice { remaining: 60 }
                    }
                }
            };
            entered = true;
        }

        let mut cues = candidate.world_update(battle, input.battle)?;
        if !entered {
            self.phase = match self.phase {
                Phase::VictoryFade => {
                    if battle.advance_transition()? {
                        cues.extend(battle.retire_combat()?);
                        cues.extend(battle.stop_result_voices()?);
                        cues.extend(candidate.prepare_results(battle)?);
                        presentation.request(
                            Request::SynchronizeResults,
                            candidate.results(),
                            battle,
                        )?;
                        battle.begin_transition(TransitionKind::FadeIn, [0; 3], FADE_STEP)?;
                        if candidate.result_style().play_music {
                            presentation.request(
                                Request::PlayMusic {
                                    track: VICTORY_MUSIC,
                                    fade_ms: 50,
                                },
                                None,
                                battle,
                            )?;
                        }
                        Phase::Results
                    } else {
                        Phase::VictoryFade
                    }
                }
                Phase::Results => {
                    let visible = battle.advance_transition()?;
                    if input.cook && candidate.update_cooking(battle)? {
                        presentation.request(
                            Request::SynchronizeResults,
                            candidate.results(),
                            battle,
                        )?;
                    }
                    // Cooking owns a simultaneous press so its new outcome stays readable.
                    if visible && input.confirm && !input.cook {
                        if presentation.results_on_last_page() {
                            candidate.accept_victory()?;
                            battle.begin_transition(TransitionKind::FadeOut, [0; 3], FADE_STEP)?;
                            Phase::Closing
                        } else {
                            presentation.request(Request::NextResultPage, None, battle)?;
                            Phase::Results
                        }
                    } else {
                        Phase::Results
                    }
                }
                Phase::Defeat if input.confirm => {
                    battle.begin_transition(TransitionKind::FadeOut, [0; 3], FADE_STEP)?;
                    Phase::Closing
                }
                Phase::EscapeNotice { remaining: 0 } => Phase::Closing,
                Phase::EscapeNotice { remaining } => Phase::EscapeNotice {
                    remaining: remaining - 1,
                },
                Phase::Closing => {
                    if battle.advance_transition()? {
                        if battle.recognize_result() == Some(BattleResult::Escaped) {
                            candidate.record_escape(battle)?;
                        }
                        Phase::Finish
                    } else {
                        Phase::Closing
                    }
                }
                phase => phase,
            };
        }

        Ok(cues)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod victory_tests;

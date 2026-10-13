//! The encounter owns combat, results, and the final handoff to the field.
//! Each update advances the world once and applies result mutations directly.
pub mod victory_selection;
use super::{
    command,
    results::{Candidate, Presentation},
};
use anyhow::{Result, ensure};
use resonance_battle::{
    Battle, BattleFrame, BattleInput, BattlePhase, BattleResult, DepartureReason, TransitionKind,
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
    EnemyRetreat { remaining: u8 },
    Closing,
    Finish,
    Finished,
    Faulted,
}

pub(crate) const VICTORY_MUSIC: u16 = 95;
pub(crate) const DEFEAT_MUSIC: u16 = 96;
// A fade takes roughly a quarter-second at 60 updates/second.
const FADE_STEP: u8 = 16;
const FADE_UPDATES: u8 = u8::MAX.div_ceil(FADE_STEP);
/// Departure owns these updates before scene handoff; its closing fade is included.
const ENEMY_RETREAT_UPDATES: u8 = 65;

pub struct Lifecycle {
    phase: Phase,
    command: command::Owner,
    command_setup: Option<command::Setup>,
    command_events: Vec<command::Event>,
    events: Option<super::event::Sequence>,
}

impl Lifecycle {
    pub fn new(command_setup: Option<command::Setup>) -> Self {
        Self {
            phase: Phase::Combat,
            command: command::Owner::default(),
            command_setup,
            command_events: Vec::new(),
            events: None,
        }
    }

    pub fn with_events(
        mut self,
        plan: super::event::Plan,
        party: resonance_events::party::Party,
        boss: resonance_battle::ActorId,
        battle: &Battle,
    ) -> Result<Self> {
        self.events = Some(super::event::Sequence::new(plan, party, boss, battle)?);
        Ok(self)
    }

    pub fn has_events(&self) -> bool {
        self.events.is_some()
    }

    pub fn event_scenes(&self) -> &[std::sync::Arc<resonance_content::battle_event::Scene>] {
        self.events.as_ref().map_or(&[], |events| &events.scenes)
    }

    pub fn event(&self) -> Option<&super::event::Playback> {
        self.events
            .as_ref()
            .and_then(super::event::Sequence::playback)
    }

    pub fn take_event_audio(&mut self) -> Vec<resonance_events::AudioCommand> {
        self.events
            .as_mut()
            .map_or_else(Vec::new, super::event::Sequence::take_audio)
    }

    pub fn event_mut(&mut self) -> Option<&mut super::event::Playback> {
        self.events
            .as_mut()
            .and_then(super::event::Sequence::playback_mut)
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
            let event_visit = if let Some(events) = &mut self.events {
                events.step(battle.ledger().combat_ticks, input.confirm, input.confirm)?
            } else {
                super::event::Visit::Combat
            };
            if let Some(events) = &mut self.events {
                if let Some(actor) = events.take_reserve_activation()? {
                    battle.activate_event_party_member(actor)?;
                }
                battle.hold_event_boss_hp(events.holds_hp().then_some(events.boss))?;
            }
            if event_visit != super::event::Visit::Combat {
                self.command.close();
                let mut cues = candidate.world_update(
                    battle,
                    BattleInput {
                        paused: true,
                        ..Default::default()
                    },
                )?;
                if event_visit == super::event::Visit::Return
                    && let Some(events) = self
                        .events
                        .as_ref()
                        .filter(|events| events.restores_formation())
                {
                    cues.extend(battle.resume_event_combat(&events.formation)?);
                }
                if event_visit == super::event::Visit::Transition {
                    battle.advance_event_transition()?;
                }
                let mut frame = battle.publish(cues);
                if let Some(event) = self.event() {
                    super::event::apply_view(&event.events, &mut frame)?;
                }
                presentation.present(&frame)?;
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
                    battle.begin_transition(TransitionKind::FadeOut, [255; 3], FADE_STEP)?;
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
                    match battle.departure_reason() {
                        Some(DepartureReason::EnemyRetreat) => Phase::EnemyRetreat {
                            // This visit also performs the first departure world update.
                            remaining: ENEMY_RETREAT_UPDATES - 1,
                        },
                        Some(DepartureReason::ForcedEscape) => Phase::Closing,
                        _ => {
                            presentation.request(Request::EscapeNotice, None, battle)?;
                            // Leave the success notice visible for one second before fading.
                            Phase::EscapeNotice { remaining: 60 }
                        }
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
                        battle.begin_transition(TransitionKind::FadeIn, [255; 3], FADE_STEP)?;
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
                Phase::EnemyRetreat { remaining } => {
                    let faded = remaining <= FADE_UPDATES && battle.advance_transition()?;
                    if remaining <= 1 && faded {
                        Phase::Finish
                    } else {
                        Phase::EnemyRetreat {
                            remaining: remaining.saturating_sub(1),
                        }
                    }
                }
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

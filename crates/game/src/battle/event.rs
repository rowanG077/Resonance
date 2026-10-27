//! Encounter-owned scenario playback. The field caller stays suspended throughout.
use anyhow::{Context, Result, ensure};
use resonance_events::{EventRuntime, GameWorld, ResourceLibrary, party::Party};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;
use symphonia_script_vm::Memory;

/// Programs and ordinary native resources prepared before taking the battle view.
#[derive(Clone)]
pub struct Prepared {
    program: Arc<Program>,
    resources: Arc<ResourceLibrary>,
    scene: Option<Arc<resonance_content::battle_event::Scene>>,
}

impl Prepared {
    pub fn new(program: Arc<Program>, resources: Arc<ResourceLibrary>) -> Self {
        Self {
            program,
            resources,
            scene: None,
        }
    }

    pub fn with_scene(mut self, scene: Arc<resonance_content::battle_event::Scene>) -> Self {
        self.scene = Some(scene);
        self
    }

    pub fn enter(&self, party: &Party) -> Result<Playback> {
        let mut world = GameWorld::default();
        world.party = Some(party.clone());
        world.battle_event = Some(
            resonance_events::battle_event::State::new(resonance_battle::VERTICAL_FOV_DEGREES)
                .map_err(anyhow::Error::msg)?,
        );
        Ok(Playback {
            events: EventRuntime::with_state(
                self.program.clone(),
                self.resources.clone(),
                world,
                Memory::default(),
            )?,
            dialogue: BTreeMap::new(),
        })
    }
}

#[derive(Clone)]
pub struct Plan {
    opening: Prepared,
    rescue: Prepared,
}
impl Plan {
    pub fn vidarr(opening: Prepared, rescue: Prepared) -> Self {
        Self { opening, rescue }
    }
}

pub struct Playback {
    pub events: EventRuntime,
    pub dialogue: BTreeMap<u8, crate::dialogue::DialoguePlayer>,
}
impl Playback {
    fn fork_for_prediction(&self, fork: &mut resonance_events::OperationFork) -> Self {
        Self {
            events: self.events.fork_for_prediction(fork),
            dialogue: self
                .dialogue
                .iter()
                .map(|(&slot, player)| (slot, player.fork_for_prediction(fork)))
                .collect(),
        }
    }
    fn step(&mut self, confirm: bool, accelerate: bool) -> Result<()> {
        use resonance_events::input::{Button, Buttons};
        self.events.world.input.sample(
            Buttons::default().with(Button::Accept, accelerate),
            Buttons::default().with(Button::Accept, confirm),
        );
        self.events.step()?;
        for (&slot, request) in &self.events.world.dialogue {
            if request.operation.is_pending()
                && self
                    .dialogue
                    .get(&slot)
                    .is_none_or(|player| player.operation.id() != request.operation.id())
            {
                let speed = self.events.world.party.as_ref().map_or(3, |party| {
                    u16::from(party.settings.preferences.message_speed)
                });
                self.dialogue.insert(
                    slot,
                    crate::dialogue::DialoguePlayer::new(request, speed)?.with_voice_feedback(true),
                );
            }
        }
        crate::dialogue::step_requests(
            &mut self.events.world,
            &mut self.dialogue,
            confirm,
            accelerate,
        )?;
        ensure!(
            self.events.world.battle_request.is_none(),
            "nested battle event request"
        );
        Ok(())
    }

    fn finished(&self) -> bool {
        self.events.main_finished()
            && self
                .events
                .world
                .dialogue
                .values()
                .all(|request| !request.operation.is_pending())
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Opening,
    Combat,
    RescueTransition,
    RescueReady,
    Rescue,
    Finished,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Visit {
    Combat,
    /// Retire the interrupted combat effects before the new scene is published.
    Enter,
    Event,
    /// Combat owns the returned view; its clock resumes on the following visit.
    Return,
    /// Resource ownership changes while gameplay time advances and combat waits.
    Transition,
}

/// One opening, one timed interruption, then uninterrupted ordinary combat.
pub(super) struct Sequence {
    phase: Phase,
    playback: Option<Playback>,
    rescue: Option<Prepared>,
    party: Party,
    pub(super) boss: resonance_battle::ActorId,
    pub(super) scenes: Vec<Arc<resonance_content::battle_event::Scene>>,
    pub(super) formation: Vec<[f32; 3]>,
    reserve: Option<resonance_battle::ActorId>,
    audio: Vec<resonance_events::AudioCommand>,
}
impl Sequence {
    // Starting a different event changes resource and operation ownership.
    pub(super) fn prediction_available(&self) -> bool {
        !matches!(self.phase, Phase::RescueReady)
    }
    pub(super) fn fork_for_prediction(&self, fork: &mut resonance_events::OperationFork) -> Self {
        Self {
            phase: self.phase,
            playback: self
                .playback
                .as_ref()
                .map(|playback| playback.fork_for_prediction(fork)),
            rescue: self.rescue.clone(),
            party: self.party.clone(),
            boss: self.boss,
            scenes: self.scenes.clone(),
            formation: self.formation.clone(),
            reserve: self.reserve,
            audio: self.audio.clone(),
        }
    }
    pub(super) fn new(
        plan: Plan,
        party: Party,
        boss: resonance_battle::ActorId,
        battle: &resonance_battle::Battle,
    ) -> Result<Self> {
        let scenes = [&plan.opening.scene, &plan.rescue.scene]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let reserve = party
            .formation
            .iter()
            .take(4)
            .position(|&character| character == 9)
            .map(resonance_battle::ActorId::from_index)
            .transpose()?;
        let playback = plan.opening.enter(&party)?;
        Ok(Self {
            phase: Phase::Opening,
            playback: Some(playback),
            rescue: Some(plan.rescue),
            party,
            boss,
            scenes,
            reserve,
            formation: battle.actors().iter().map(|actor| actor.position).collect(),
            audio: Vec::new(),
        })
    }

    pub(super) fn playback(&self) -> Option<&Playback> {
        self.playback.as_ref()
    }
    pub(super) fn restores_formation(&self) -> bool {
        matches!(self.phase, Phase::Finished)
    }

    pub(super) fn holds_hp(&self) -> bool {
        matches!(self.phase, Phase::Opening | Phase::Combat)
    }

    pub(super) fn take_reserve_activation(&mut self) -> Result<Option<resonance_battle::ActorId>> {
        let requested = self
            .playback
            .as_mut()
            .and_then(|playback| playback.events.world.battle_event.as_mut())
            .is_some_and(|stage| std::mem::take(&mut stage.activate_reserve));
        if requested {
            Ok(Some(
                self.reserve
                    .context("battle rescue has no prepared reserve")?,
            ))
        } else {
            Ok(None)
        }
    }

    pub(super) fn take_audio(&mut self) -> Vec<resonance_events::AudioCommand> {
        if let Some(playback) = &mut self.playback {
            self.audio.append(&mut playback.events.world.audio_commands);
        }
        std::mem::take(&mut self.audio)
    }

    /// Event completion publishes an ordinary held view before combat resumes.
    pub(super) fn step(
        &mut self,
        combat_ticks: u32,
        confirm: bool,
        accelerate: bool,
    ) -> Result<Visit> {
        const RESCUE_COMBAT_TICK: u32 = 600;
        if matches!(self.phase, Phase::Combat) && combat_ticks >= RESCUE_COMBAT_TICK {
            self.phase = Phase::RescueTransition;
            return Ok(Visit::Transition);
        }
        if matches!(self.phase, Phase::RescueTransition) {
            self.phase = Phase::RescueReady;
            return Ok(Visit::Transition);
        }
        if matches!(self.phase, Phase::RescueReady) {
            let prepared = self
                .rescue
                .take()
                .context("battle rescue was already consumed")?;
            self.playback = Some(prepared.enter(&self.party)?);
            self.phase = Phase::Rescue;
            return Ok(Visit::Enter);
        }
        if let Some(playback) = &mut self.playback {
            playback.step(confirm, accelerate)?;
            if playback.finished() {
                self.audio.append(&mut playback.events.world.audio_commands);
                self.playback = None;
                self.phase = match self.phase {
                    Phase::Opening => Phase::Combat,
                    Phase::Rescue => Phase::Finished,
                    _ => unreachable!(),
                };
                return Ok(Visit::Return);
            }
            Ok(Visit::Event)
        } else {
            Ok(Visit::Combat)
        }
    }
}

/// Applied after ordinary combat publication while the event owns presentation.
pub fn apply_view(events: &EventRuntime, frame: &mut resonance_battle::BattleFrame) -> Result<()> {
    // Camera commands publish on the first event update. Construction only queues
    // their channels, so retain the battle view for the initial held frame.
    if events.tick() == 0 {
        return Ok(());
    }
    let stage = events
        .world
        .battle_event
        .as_ref()
        .context("battle event is not active")?;
    let view = stage.camera.frame();
    let direction: [f32; 3] = std::array::from_fn(|axis| view.eye[axis] - view.target[axis]);
    let horizontal = direction[0].hypot(direction[2]);
    frame.camera = Some(resonance_battle::CameraPose {
        eye: view.eye,
        focus: view.target,
        pitch: direction[1].atan2(horizontal).to_degrees(),
        yaw: direction[2].atan2(direction[0]).to_degrees(),
        radius: horizontal.hypot(direction[1]),
    });
    frame.camera_fov_degrees = Some(view.vertical_fov_degrees);
    frame.event_fade = Some(resonance_battle::TransitionFrame {
        color: stage.fade.color,
        alpha: stage.fade.opacity,
    });
    Ok(())
}

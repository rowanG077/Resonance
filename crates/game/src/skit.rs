//! Shared field/world skit playback. Preparation and simulation never read original assets.
use anyhow::{Context, Result};
use resonance_content::{prepared::Files, skit::SkitCatalog};
use resonance_events::{AudioCommand, EventRuntime, Operation, ResourceLibrary};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;

#[derive(Clone)]
pub struct Prepared {
    id: u16,
    title: String,
    program: Arc<Program>,
    resources: Arc<ResourceLibrary>,
}
impl Prepared {
    pub fn load(catalog: Arc<SkitCatalog>, files: &Files) -> Result<BTreeMap<u16, Self>> {
        let text: Arc<resonance_content::session::GameText> =
            Arc::new(files.json("game/text.json")?);
        let data: Arc<resonance_content::session::SessionData> =
            Arc::new(files.json("game/session-data.json")?);
        Self::load_with(
            catalog,
            files,
            &ResourceLibrary {
                text,
                session_data: Some(data),
                ..Default::default()
            },
            files.diagnostics(),
        )
    }

    pub(crate) fn load_with(
        catalog: Arc<SkitCatalog>,
        files: &Files,
        resources: &ResourceLibrary,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<BTreeMap<u16, Self>> {
        let mut prepared = BTreeMap::new();
        for (&id, paths) in &catalog.resources {
            let result = (|| -> Result<Self> {
                Ok(Self {
                    id,
                    title: catalog
                        .skits
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.title.clone())
                        .or_else(|| paths.title.clone())
                        .unwrap_or_default(),
                    program: Arc::new(Program::decode(&files.read(&paths.script)?)?),
                    resources: Arc::new(ResourceLibrary {
                        skits: Some(catalog.clone()),
                        text: resources.text.clone(),
                        session_data: resources.session_data.clone(),
                        messages: files.json(&paths.messages)?,
                        actor_names: ResourceLibrary::character_names(),
                        ..Default::default()
                    }),
                })
            })();
            if let Some(skit) = diagnostics.attempt(&format!("skit {id} preparation"), result)? {
                prepared.insert(id, skit);
            }
        }
        Ok(prepared)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Input {
    pub confirm: bool,
    pub cancel: bool,
    pub direction: i8,
    pub accelerate: bool,
    pub skip_dialogue: bool,
    pub skip: bool,
}

pub struct Playback {
    pub id: u16,
    pub title: String,
    pub events: EventRuntime,
    pub skippable: bool,
    pub dialogue: BTreeMap<u8, crate::dialogue::DialoguePlayer>,
    choices: crate::choice::ChoicePlayer,
    preview: bool,
    completion: Option<Operation>,
    finished: bool,
}
impl Playback {
    pub fn start(
        prepared: &Prepared,
        parent: &mut EventRuntime,
        skippable: bool,
        preview: bool,
        completion: Option<Operation>,
    ) -> Result<Self> {
        let id = prepared.id;
        let mut memory = symphonia_script_vm::Memory::default();
        memory.copy_from(parent.memory(), 0..0x2000)?;
        let mut world = resonance_events::GameWorld::default();
        world.current_field = parent.world.current_field;
        world.played_ticks = parent.world.played_ticks;
        world.skit = Some(Default::default());
        world.party = parent.world.party.clone();
        world.event_flags = parent.world.event_flags.clone();
        world.script_state = parent.world.script_state.clone();
        world.random_state = parent.world.random_state;
        let mut events = EventRuntime::with_state(
            prepared.program.clone(),
            prepared.resources.clone(),
            world,
            memory,
        )?;
        parent.world.audio_commands.push(AudioCommand::MusicVolume {
            volume: 127 / 2,
            duration_ticks: 60,
        });
        parent
            .world
            .audio_commands
            .append(&mut events.world.audio_commands);
        Ok(Self {
            id,
            title: prepared.title.clone(),
            events,
            dialogue: BTreeMap::new(),
            choices: Default::default(),
            skippable: skippable
                && !matches!(id, 472 | 657 | 680 | 696 | 251 | 452 | 618 | 827 | 833),
            preview,
            completion,
            finished: false,
        })
    }
    /// Returns true when the owner can retire this scene. Repeated completion is inert.
    pub fn step(&mut self, parent: &mut EventRuntime, input: Input) -> Result<bool> {
        if self.finished {
            return Ok(true);
        }
        self.events.world.played_ticks = parent.world.played_ticks;
        self.events.step()?;
        let choice_slot = self
            .events
            .world
            .choices
            .iter()
            .find(|(_, choice)| choice.operation.is_pending())
            .map(|(&slot, _)| slot);
        crate::dialogue::step_requests(
            &mut self.events.world,
            &mut self.dialogue,
            choice_slot.is_none() && (input.confirm || input.cancel || input.skip_dialogue),
            input.accelerate || input.confirm && choice_slot.is_some(),
        )?;
        if let Some(slot) = choice_slot {
            let player = self
                .dialogue
                .get_mut(&slot)
                .context("skit choice has no dialogue")?;
            let choice = self.events.world.choices.get_mut(&slot).unwrap();
            let (reason, moved) = self.choices.step(
                choice,
                crate::choice::ChoiceInput {
                    horizontal: 0,
                    direction: input.direction,
                    confirm: input.confirm,
                    cancel: input.cancel,
                },
                player.accepts_input() && player.fully_revealed(),
            );
            if moved {
                self.events.world.audio_commands.push(AudioCommand::Sound {
                    id: resonance_content::field_audio::ServiceCue::Navigate as i16,
                    pan: 64,
                    volume: 127,
                    slot: None,
                });
            }
            if let Some(reason) = reason {
                choice.finish(reason).map_err(anyhow::Error::msg)?;
                player.close();
                self.events
                    .world
                    .audio_commands
                    .push(AudioCommand::StopVoice);
            }
        }
        parent
            .world
            .audio_commands
            .append(&mut self.events.world.audio_commands);
        if !(self.events.main_finished()
            || input.skip && self.skippable && self.events.tick() >= 120)
        {
            return Ok(false);
        }
        self.complete(parent)?;
        Ok(true)
    }
    fn complete(&mut self, parent: &mut EventRuntime) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        if !self.preview {
            // Skits share story globals with their caller. Copy only persistent
            // variables; dispatcher registers belong to each suspended VM.
            parent.copy_script_globals(&self.events)?;
            parent.world.party = self.events.world.party.take();
            parent.world.event_flags = std::mem::take(&mut self.events.world.event_flags);
            parent.world.script_state = std::mem::take(&mut self.events.world.script_state);
            parent
                .world
                .party
                .as_mut()
                .context("skit completion has no party")?
                .viewed_skits
                .insert(self.id);
        }
        self.cancel(parent)
    }

    /// Retire a failed optional scene without publishing partial progress.
    pub fn cancel(&mut self, parent: &mut EventRuntime) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        if let Some(operation) = &self.completion {
            operation.complete(None).map_err(anyhow::Error::msg)?;
        }
        parent.world.random_state = self.events.world.random_state;
        parent.world.audio_commands.extend([
            AudioCommand::StopVoice,
            AudioCommand::MusicVolume {
                volume: 127,
                duration_ticks: 60,
            },
        ]);
        self.events.cancel();
        self.finished = true;
        Ok(())
    }
}

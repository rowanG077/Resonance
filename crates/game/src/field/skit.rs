//! Available skit titles are transient field notifications, not save-state data.
use super::*;
use anyhow::Context;
use resonance_content::skit::{SkitCatalog, SkitCondition, SkitLocation};

const REFRESH_TICKS: u32 = 1200;
const HOLD_TICKS: u16 = 1800;

#[derive(Debug, serde::Serialize)]
pub struct SkitPrompt<'a> {
    pub id: u16,
    pub title: &'a str,
    pub opacity: u8,
    pub text_opacity: u8,
    /// The announcement title follows the user's setting; the Z button does not.
    #[serde(skip_serializing)]
    pub title_visible: bool,
}

#[derive(Default)]
pub(super) struct Skits {
    data: Option<Arc<SkitCatalog>>,
    control_ticks: u32,
    selected: Option<usize>,
    remaining: u16,
    opacity: u8,
    text_opacity: u8,
    visible: bool,
}
impl Skits {
    pub fn new(data: Option<Arc<SkitCatalog>>) -> Self {
        Self {
            data,
            ..Default::default()
        }
    }

    pub fn next_field(&self) -> Self {
        Self {
            data: self.data.clone(),
            control_ticks: self.control_ticks,
            ..Default::default()
        }
    }

    /// Apply a source-observed notification phase for an oracle replay.
    /// Notifications remain transient and are never included in checkpoints.
    pub fn apply_origin(
        &mut self,
        id: u16,
        control_ticks: u32,
        remaining: u16,
        opacity: u8,
        text_opacity: u8,
    ) -> Result<()> {
        let data = self.data.as_ref().context("skit catalog is missing")?;
        let selected = data
            .skits
            .iter()
            .position(|skit| skit.id == id)
            .context("source skit id is not in the catalog")?;
        ensure!(remaining <= HOLD_TICKS, "source skit timer is out of range");
        self.control_ticks = control_ticks;
        self.selected = Some(selected);
        self.remaining = remaining;
        self.opacity = opacity;
        self.text_opacity = text_opacity;
        self.visible = true;
        Ok(())
    }

    pub fn prompt(&self) -> Option<SkitPrompt<'_>> {
        let skit = &self.data.as_ref()?.skits[self.selected?];
        self.visible.then_some(SkitPrompt {
            id: skit.id,
            title: &skit.title,
            opacity: self.opacity,
            text_opacity: self.text_opacity,
            title_visible: true,
        })
    }

    /// Consume the announcement when the player presses Z. Playback owns the
    /// field from this point; the timed title is no longer rendered.
    pub fn open(&mut self) -> Option<u16> {
        let id = self
            .visible
            .then_some(self.selected)
            .flatten()
            .and_then(|index| self.data.as_ref()?.skits.get(index).map(|skit| skit.id));
        if id.is_some() {
            self.visible = false;
            self.remaining = 0;
            self.opacity = 0;
            self.text_opacity = 0;
        }
        id
    }

    pub fn step(&mut self, events: &EventRuntime, map: u32, free_control: bool) -> Result<()> {
        self.visible = false;
        let world = &events.world;
        if !free_control
            || !world.input_enabled
            || world.field_transition.is_some()
            || world.blocked_by_movie()
        {
            return Ok(());
        }
        let (Some(data), Some(party)) = (&self.data, &world.party) else {
            return Ok(());
        };
        let story = events.memory().read(0x40, symphonia_script::Width::S32)?;
        let side = events.memory().read(0x50, symphonia_script::Width::S32)?;
        let mask = party
            .formation
            .iter()
            .fold(0, |mask, id| mask | (1u16 << id));
        self.control_ticks = self.control_ticks.wrapping_add(1);
        // Story notifications are selected on entry; ambient ones refresh while exploring.
        let refresh = self.control_ticks.is_multiple_of(REFRESH_TICKS);
        let mut selected_valid = false;
        for (index, skit) in data.skits.iter().enumerate() {
            if party.viewed_skits.contains(&skit.id)
                || skit
                    .story
                    .is_some_and(|[start, end]| !(start..=end).contains(&story))
                || mask & skit.party_mask != skit.party_mask
                || !match skit.location {
                    SkitLocation::Anywhere => true,
                    SkitLocation::Field => map < 3000,
                    SkitLocation::Map(id) => map == u32::from(id),
                    SkitLocation::Overworld(required) => {
                        map >= 3000 && required.is_none_or(|v| side == i32::from(v))
                    }
                }
            {
                continue;
            }
            match skit.condition {
                SkitCondition::Maps([start, end])
                    if !(u32::from(start)..=u32::from(end)).contains(&map) =>
                {
                    continue;
                }
                SkitCondition::Unimplemented => {
                    anyhow::bail!("skit {} needs an availability condition", skit.id)
                }
                _ => (),
            }
            if self.selected == Some(index) {
                selected_valid = true;
            }
            if (skit.id < 600 && self.control_ticks == 1) || (skit.id >= 600 && refresh) {
                self.selected = Some(index);
                selected_valid = true;
                self.remaining = HOLD_TICKS;
            }
        }
        if !selected_valid {
            self.remaining = 0;
        }
        if self.remaining == 0 {
            self.opacity = 0;
            return Ok(());
        }
        self.visible = true;
        self.opacity = if self.remaining < 20 {
            self.opacity.saturating_sub(12)
        } else {
            self.opacity.saturating_add(8)
        };
        self.text_opacity = if self.remaining < 20 {
            (self.remaining * 12) as u8
        } else {
            255
        };
        self.remaining -= 1;
        Ok(())
    }
}

pub struct Playback {
    pub id: u16,
    pub title: String,
    pub events: EventRuntime,
    pub skippable: bool,
    pub dialogue: BTreeMap<u8, crate::dialogue::DialoguePlayer>,
    preview: bool,
    completion: Option<resonance_events::Operation>,
}
pub(super) struct Prepared {
    program: Arc<Program>,
    resources: Arc<ResourceLibrary>,
}
impl FieldSession {
    /// Decode scenarios while preparing the field. Z never reads the filesystem.
    pub fn prepare_skits(&mut self, files: &resonance_content::prepared::Files) -> Result<()> {
        let Some(catalog) = self.skits.data.clone() else {
            return Ok(());
        };
        let text: Arc<resonance_content::session::GameText> =
            Arc::new(files.json("game/text.json")?);
        let data: Arc<resonance_content::session::SessionData> =
            Arc::new(files.json("game/session-data.json")?);
        for (&id, paths) in &catalog.resources {
            let program = Arc::new(Program::decode(&files.read(&paths.script)?)?);
            let resources = Arc::new(ResourceLibrary {
                skits: Some(catalog.clone()),
                text: text.clone(),
                session_data: Some(data.clone()),
                messages: files.json(&paths.messages)?,
                actor_names: ResourceLibrary::character_names(),
                ..Default::default()
            });
            self.skit_programs
                .insert(id, Prepared { program, resources });
        }
        Ok(())
    }
    pub(super) fn start_skit(
        &mut self,
        id: u16,
        skippable: bool,
        preview: bool,
        completion: Option<resonance_events::Operation>,
    ) -> Result<()> {
        let prepared = self
            .skit_programs
            .get(&id)
            .with_context(|| format!("skit {id} was not prepared; cook the skit assets"))?;
        let definition = self
            .skits
            .data
            .as_ref()
            .and_then(|c| c.skits.iter().find(|s| s.id == id))
            .context("skit title missing")?;
        let mut memory = symphonia_script_vm::Memory::default();
        for offset in (0..0x2000).step_by(4) {
            memory.write(
                offset,
                symphonia_script::Width::S32,
                self.events
                    .memory()
                    .read(offset, symphonia_script::Width::S32)?,
            )?;
        }
        let mut world = resonance_events::GameWorld::default();
        world.skit = Some(Default::default());
        world.party = self.events.world.party.clone();
        world.event_flags = self.events.world.event_flags.clone();
        world.random_state = self.events.world.random_state;
        let mut events = EventRuntime::with_state(
            prepared.program.clone(),
            prepared.resources.clone(),
            world,
            memory,
        )?;
        self.events
            .world
            .audio_commands
            .push(resonance_events::AudioCommand::MusicVolume {
                volume: 127 / 2,
                duration_ticks: 60,
            });
        self.events
            .world
            .audio_commands
            .append(&mut events.world.audio_commands);
        self.active_skit = Some(Playback {
            id,
            title: definition.title.clone(),
            events,
            dialogue: BTreeMap::new(),
            skippable: skippable
                && !matches!(id, 472 | 657 | 680 | 696 | 251 | 452 | 618 | 827 | 833),
            preview,
            completion,
        });
        Ok(())
    }
    pub(super) fn step_skit(&mut self, input: FieldInput) -> Result<()> {
        let skit = self.active_skit.as_mut().context("skit is not active")?;
        skit.events.step()?;
        crate::dialogue::step_requests(&mut skit.events.world, &mut skit.dialogue, input.interact)?;
        self.events
            .world
            .audio_commands
            .append(&mut skit.events.world.audio_commands);
        if skit.events.main_finished() || input.menu && skit.skippable && skit.events.tick() >= 120
        {
            if !skit.preview {
                self.events.world.party = skit.events.world.party.take();
                self.events.world.event_flags = skit.events.world.event_flags.clone();
                self.events
                    .world
                    .party
                    .as_mut()
                    .context("skit completion has no party")?
                    .viewed_skits
                    .insert(skit.id);
            }
            if let Some(operation) = &skit.completion {
                operation.complete(None).map_err(anyhow::Error::msg)?;
            }
            self.events.world.random_state = skit.events.world.random_state;
            self.events.world.audio_commands.extend([
                resonance_events::AudioCommand::StopVoice,
                resonance_events::AudioCommand::MusicVolume {
                    volume: 127,
                    duration_ticks: 60,
                },
            ]);
            self.active_skit = None;
            self.skits.control_ticks = 0;
            self.skits.selected = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_prompt_consumes_only_the_transient_notice() {
        let catalog = Arc::new(SkitCatalog {
            version: 1,
            skits: vec![resonance_content::skit::SkitDefinition {
                id: 600,
                title: "Test skit".into(),
                story: None,
                party_mask: 0,
                location: SkitLocation::Anywhere,
                condition: SkitCondition::None,
            }],
            resources: BTreeMap::new(),
            portraits: BTreeMap::new(),
            media: BTreeMap::new(),
        });
        let mut skits = Skits::new(Some(catalog));
        skits.apply_origin(600, 1, 100, 255, 255).unwrap();
        assert_eq!(skits.open(), Some(600));
        assert!(skits.prompt().is_none());
    }
}

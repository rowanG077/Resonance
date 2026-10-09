//! Field scenes with production event and audio stepping, without devices.
use crate::new_game;
use anyhow::{Context, Result};
use resonance_events::input::{Button, Buttons};
use resonance_game::field::{FieldEntry, FieldInput, FieldSession};
use std::sync::Arc;

pub(crate) const SCENE_TIMEOUT: usize = 18_000;
pub(crate) fn dialogue_input() -> FieldInput {
    FieldInput {
        pressed_buttons: [Button::Accept].into(),
        held_buttons: Buttons::default().with(Button::Accept, true),
        ..Default::default()
    }
}

pub(crate) struct Scene {
    pub(crate) field: FieldSession,
    pub(crate) audio: crate::field_audio::validation::Playback,
}

impl std::ops::Deref for Scene {
    type Target = FieldSession;
    fn deref(&self) -> &FieldSession {
        &self.field
    }
}
impl std::ops::DerefMut for Scene {
    fn deref_mut(&mut self) -> &mut FieldSession {
        &mut self.field
    }
}
impl Scene {
    pub(crate) fn restore(
        root: &std::path::Path,
        checkpoint: resonance_game::field::FieldCheckpoint,
    ) -> Result<Self> {
        let package = new_game::FieldPackage::prepare(
            root,
            checkpoint.map_id,
            &mut Default::default(),
            || false,
        )?;
        let mut field = package.restore(
            &checkpoint,
            Arc::new(package.files.json("game/session-data.json")?),
            Arc::new(package.files.json("game/skits.json")?),
            new_game::available_fields(root)?,
        )?;
        let audio =
            crate::field_audio::validation::Playback::new((*package.audio).clone(), &mut field);
        Ok(Self { field, audio })
    }

    pub(crate) fn story(
        root: &std::path::Path,
        map: u32,
        story: i32,
        configure: impl FnOnce(&mut FieldEntry) -> Result<()>,
    ) -> Result<Self> {
        let package =
            new_game::FieldPackage::prepare(root, map, &mut Default::default(), || false)?;
        let data = Arc::new(package.files.json("game/session-data.json")?);
        let mut entry = FieldEntry {
            persistent: resonance_events::PersistentState {
                party: Some(resonance_events::party::Party::new(
                    &data,
                    Default::default(),
                )?),
                ..Default::default()
            },
            data: Some(data),
            available_fields: new_game::available_fields(root)?,
            ..Default::default()
        };
        entry
            .persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, story)?;
        configure(&mut entry)?;
        Self::enter(&package, entry)
    }

    pub(crate) fn walking(&self, [x, y]: [f32; 2]) -> FieldInput {
        let camera = self.events.world.field_camera.as_ref().unwrap();
        let angle = (-(camera.target[0] - camera.position[0])
            .atan2(camera.target[1] - camera.position[1])
            .to_degrees())
        .trunc()
        .to_radians();
        FieldInput {
            direction: [
                angle.cos() * x + angle.sin() * y,
                -angle.sin() * x + angle.cos() * y,
            ],
            ..Default::default()
        }
    }
    pub(crate) fn actor(&self, id: i32) -> &resonance_events::Actor {
        &self.events.world.actors[&id]
    }
    pub(crate) fn actor_mut(&mut self, id: i32) -> &mut resonance_events::Actor {
        self.events.world.actors.get_mut(&id).unwrap()
    }
    pub(crate) fn party(&self) -> &resonance_events::party::Party {
        self.events.world.party.as_ref().unwrap()
    }
    pub(crate) fn party_mut(&mut self) -> &mut resonance_events::party::Party {
        self.events.world.party.as_mut().unwrap()
    }
    pub(crate) fn step(&mut self, input: FieldInput) -> Result<()> {
        self.field.step(input)?;
        self.audio.step(&mut self.field)
    }
    pub(crate) fn ticks(&mut self, count: usize, input: FieldInput) -> Result<()> {
        for _ in 0..count {
            self.step(input)?;
        }
        Ok(())
    }
    pub(crate) fn until(
        &mut self,
        input: FieldInput,
        mut ready: impl FnMut(&mut Self) -> Result<bool>,
    ) -> Result<()> {
        for tick in 0..SCENE_TIMEOUT {
            self.step(FieldInput {
                pressed_buttons: input.pressed_buttons.with(
                    Button::Accept,
                    input.pressed(Button::Accept) && tick % 2 == 0,
                ),
                ..input
            })?;
            if ready(self)? {
                return Ok(());
            }
        }
        anyhow::bail!(
            "field {} timed out; control={}, waits={:?}",
            self.map_id,
            self.player_has_control(),
            self.events.pending_operations()
        )
    }
    pub(crate) fn advance_until(
        &mut self,
        mut ready: impl FnMut(&FieldSession) -> bool,
    ) -> Result<()> {
        self.until(dialogue_input(), |scene| Ok(ready(scene)))
    }
    /// Play dialogue and complete fixture battles and movies through their normal handoffs.
    pub(crate) fn replay(
        &mut self,
        mut finished: impl FnMut(&mut Self) -> Result<bool>,
    ) -> Result<usize> {
        let mut battles = 0;
        self.until(dialogue_input(), |scene| {
            battles += usize::from(
                scene
                    .events
                    .world
                    .skip_battle_as_victory()
                    .map_err(anyhow::Error::msg)?,
            );
            if let Some(movie) = &scene.events.world.movie
                && movie.operation.is_pending()
            {
                movie.operation.complete(None).map_err(anyhow::Error::msg)?;
            }
            finished(scene)
        })?;
        Ok(battles)
    }
    pub(crate) fn replay_to_control(&mut self, root: &std::path::Path) -> Result<usize> {
        self.replay(|scene| {
            anyhow::ensure!(
                scene.events.exploration_error.is_none(),
                "{:?}",
                scene.events.exploration_error
            );
            if scene.events.world.field_transition.is_some() {
                scene.follow_transition(root)?;
                return Ok(false);
            }
            Ok(scene.player_has_control())
        })
    }
    pub(crate) fn follow_transition(&mut self, root: &std::path::Path) -> Result<()> {
        let request = self
            .events
            .world
            .field_transition
            .as_ref()
            .context("field transition")?;
        let package =
            new_game::FieldPackage::prepare(root, request.map, &mut Default::default(), || false)?;
        let mut field = package.transition(&self.field)?;
        self.audio.enter((*package.audio).clone(), &mut field)?;
        self.field.events.cancel();
        self.field = field;
        Ok(())
    }
    pub(crate) fn skip_event_step(&mut self) -> Result<bool> {
        let done = self.field.skip_event_step()?;
        if !done {
            self.audio.step(&mut self.field)?;
        }
        Ok(done)
    }
    pub(crate) fn enter(
        package: &new_game::FieldPackage,
        mut entry: resonance_game::field::FieldEntry,
    ) -> Result<Self> {
        entry.skits = Some(Arc::new(package.files.json("game/skits.json")?));
        let kind = entry.kind;
        let mut field = package.enter(entry)?;
        package.queue_entry(&mut field, kind);
        let audio =
            crate::field_audio::validation::Playback::new((*package.audio).clone(), &mut field);
        Ok(Self { field, audio })
    }
}

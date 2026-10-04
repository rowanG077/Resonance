//! Available skit titles are transient field notifications, not save-state data.
use super::*;
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
pub(crate) struct Skits {
    data: Option<Arc<SkitCatalog>>,
    control_ticks: u32,
    selected: Option<usize>,
    remaining: u16,
    opacity: u8,
    text_opacity: u8,
    visible: bool,
}
impl Skits {
    pub fn reset(&mut self) {
        self.control_ticks = 0;
        self.selected = None;
    }

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
            || world.world_transition.is_some()
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
                // An optional announcement with an uncooked predicate is not
                // eligible. Explicit event requests still play its prepared
                // script; an unavailable hint must not stop travel or a field.
                SkitCondition::Unimplemented => continue,
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

pub use crate::skit::{Playback, Prepared};
impl FieldSession {
    /// Decode scenarios while preparing the field. Z never reads the filesystem.
    pub fn prepare_skits(&mut self, files: &resonance_content::prepared::Files) -> Result<()> {
        if let Some(catalog) = self.skits.data.clone() {
            self.skit_programs = Prepared::load(catalog, files)?;
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
        self.active_skit = Some(Playback::start(
            prepared,
            &mut self.events,
            skippable,
            preview,
            completion,
        )?);
        Ok(())
    }
    pub(super) fn step_skit(&mut self, input: FieldInput) -> Result<()> {
        let skit = self.active_skit.as_mut().context("skit is not active")?;
        if skit.step(
            &mut self.events,
            crate::skit::Input {
                confirm: input.interact,
                cancel: input.cancel,
                direction: if input.direction[1] > 0.5 {
                    -1
                } else if input.direction[1] < -0.5 {
                    1
                } else {
                    0
                },
                accelerate: input.accelerate_dialogue,
                skip: input.menu || input.start,
            },
        )? {
            self.active_skit = None;
            self.skits.reset();
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
            version: 2,
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
            portrait_recipes: Vec::new(),
            media: BTreeMap::new(),
        });
        let mut skits = Skits::new(Some(catalog));
        skits.selected = Some(0);
        skits.visible = true;
        assert_eq!(skits.open(), Some(600));
        assert!(skits.prompt().is_none());
    }
}

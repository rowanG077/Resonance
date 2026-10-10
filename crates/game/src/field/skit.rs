//! Available skit titles are transient field notifications, not save-state data.
use super::*;
use resonance_content::skit::{SkitCatalog, SkitLocation};
use resonance_events::input::Button;
mod conditions;

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
    checked_entry: bool,
    selected: Option<usize>,
    remaining: u16,
    opacity: u8,
    text_opacity: u8,
    visible: bool,
}
impl Skits {
    pub fn reset(&mut self) {
        *self = Self::new(self.data.clone());
    }

    pub fn new(data: Option<Arc<SkitCatalog>>) -> Self {
        Self {
            data,
            ..Default::default()
        }
    }

    pub fn continue_from(&mut self, previous: &Self) {
        *self = Self {
            data: self.data.clone(),
            control_ticks: previous.control_ticks,
            ..Default::default()
        };
    }

    #[cfg(test)]
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

    pub fn step(
        &mut self,
        events: &EventRuntime,
        map: u32,
        free_control: bool,
        overworld: Option<(u8, Option<resonance_content::overworld::Terrain>)>,
    ) -> Result<()> {
        self.visible = false;
        let world = &events.world;
        if world
            .party
            .as_ref()
            .is_some_and(|party| party.travel.skit_prompts_disabled)
        {
            self.reset();
            return Ok(());
        }
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
        self.control_ticks = self.control_ticks.saturating_add(1);
        let entry = !self.checked_entry;
        self.checked_entry = true;
        let context = conditions::Context {
            events,
            party,
            map,
            exploration_ticks: self.control_ticks,
            overworld,
        };
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
            if !context.matches(&skit.condition)? {
                continue;
            }
            if self.selected == Some(index) {
                selected_valid = true;
            }
            if (skit.id < 600 && entry) || (skit.id >= 600 && refresh) {
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
            self.skit_programs = Prepared::load_with(
                catalog.clone(),
                files,
                self.events.resources(),
                &self.diagnostics,
            )?;
            let mut available = (*catalog).clone();
            available
                .skits
                .retain(|skit| self.skit_programs.contains_key(&skit.id));
            available
                .resources
                .retain(|id, _| self.skit_programs.contains_key(id));
            self.skits = Skits::new(Some(Arc::new(available)));
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
    pub fn cancel_skit(&mut self) -> Result<()> {
        if let Some(skit) = &mut self.active_skit {
            skit.cancel(&mut self.events)?;
        }
        self.active_skit = None;
        self.skits.reset();
        Ok(())
    }
    pub(super) fn step_skit(&mut self, input: FieldInput) -> Result<()> {
        let skit = self.active_skit.as_mut().context("skit is not active")?;
        if skit.step(
            &mut self.events,
            crate::skit::Input {
                confirm: input.pressed(Button::Accept),
                skip_dialogue: input.skip_dialogue,
                cancel: input.pressed(Button::Cancel),
                direction: if input.direction[1] > 0.5 {
                    -1
                } else if input.direction[1] < -0.5 {
                    1
                } else {
                    0
                },
                accelerate: input.held_buttons.contains(Button::Accept) || input.skip_dialogue,
                skip: input.pressed(Button::Menu) || input.pressed(Button::Start),
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
    use resonance_content::skit::SkitCondition;

    fn events() -> EventRuntime {
        let data = serde_json::from_value(serde_json::json!({
            "version":1, "executable_sha256":"0".repeat(64), "experience":[0,0,10],
            "items":[{"equipment_kind":null,"allowed_characters":511,"stack_limit":20}],
            "characters":vec![serde_json::json!({
                "level":1,"experience":0,"affinity":0,"base_stats":[100,20,30,40,50,60,70],
                "luck":10,"overlimit":0,"equipment":vec![0;6],"techniques":[],
                "allowed_techniques":[],"shortcuts":vec![0;4],"level_techniques":{},
                "growth":vec![serde_json::json!({"base":1,"random":0,"title_bonus":0});7]
            });9]
        }))
        .unwrap();
        let program = Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap();
        let mut events = EventRuntime::new(Arc::new(program), Arc::default()).unwrap();
        events.world.party =
            Some(resonance_events::party::Party::new(&data, Default::default()).unwrap());
        events.world.input_enabled = true;
        events
    }

    fn catalog(id: u16, condition: SkitCondition) -> Arc<SkitCatalog> {
        Arc::new(SkitCatalog {
            preview_order: Vec::new(),
            version: 2,
            skits: vec![resonance_content::skit::SkitDefinition {
                id,
                title: "Test skit".into(),
                story: None,
                party_mask: 0,
                location: SkitLocation::Anywhere,
                condition,
            }],
            resources: BTreeMap::new(),
            portraits: BTreeMap::new(),
            portrait_recipes: Vec::new(),
            media: BTreeMap::new(),
        })
    }

    #[test]
    fn opening_a_prompt_consumes_only_the_transient_notice() {
        let mut skits = Skits::new(Some(catalog(600, SkitCondition::None)));
        skits.selected = Some(0);
        skits.visible = true;
        assert_eq!(skits.open(), Some(600));
        assert!(skits.prompt().is_none());
    }

    #[test]
    fn skit_rules_use_progress_party_and_world_context() {
        use resonance_content::{overworld::Terrain, skit::SkitValue};
        let mut events = events();
        events.set_global(16 + 3, 100).unwrap();
        let party = events.world.party.as_mut().unwrap();
        party.formation = vec![1, 2];
        party.viewed_skits.insert(25);
        party.items.insert(2, 1);
        party.members[0].affinity = 100; // Lloyd is excluded from companion ranking.
        party.members[3].affinity = 10; // Raine ranks even when outside the formation.
        let rule: SkitCondition = serde_json::from_value(serde_json::json!({"all":[
            {"maps":[10,12]}, {"any":[{"flag":9},{"item":2}]},
            {"viewed":25}, {"not":{"member":5}}, {"terrain":"desert"},
            {"range":{"value":{"global":3},"min":100,"max":200}},
            {"range":{"value":{"affinity_rank":4},"min":1,"max":1}},
            {"range":{"value":"exploration_seconds","min":600}}
        ]}))
        .unwrap();
        rule.validate().unwrap();
        let matches = |events: &EventRuntime, ticks, overworld| {
            conditions::Context {
                events,
                party: events.world.party.as_ref().unwrap(),
                map: 12,
                exploration_ticks: ticks,
                overworld,
            }
            .matches(&rule)
            .unwrap()
        };
        let desert = Some((4, Some(Terrain::Desert)));
        assert!(!matches(&events, 35999, desert));
        assert!(matches(&events, 36000, desert));
        assert!(!matches(&events, 36000, None));
        assert!(!matches(
            &events,
            36000,
            Some((4, Some(Terrain::Grassland)))
        ));
        events.set_global(16 + 3, 201).unwrap();
        assert!(!matches(&events, 36000, desert));
        events.set_global(16 + 3, 200).unwrap();
        assert!(matches(&events, 36000, desert));
        events.world.party.as_mut().unwrap().members[2].affinity = 10;
        assert!(!matches(&events, 36000, desert)); // Ties favor Genis over Raine.
        events.world.party.as_mut().unwrap().members[2].affinity = 0;
        events.world.party.as_mut().unwrap().viewed_skits.clear();
        events.world.event_flags.insert(25);
        assert!(!matches(&events, 36000, desert)); // Event flags are not viewed skits.
        assert!(
            SkitCondition::Range {
                value: SkitValue::Level(0),
                min: Some(1),
                max: None
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn story_prompts_recheck_on_entry_and_timed_prompts_require_free_control() {
        let mut events = events();
        let mut skits = Skits::new(Some(catalog(7, SkitCondition::Maps([10, 12]))));
        skits.step(&events, 9, true, None).unwrap();
        assert!(skits.prompt().is_none());
        skits = skits.next_field();
        skits.step(&events, 10, true, None).unwrap();
        assert_eq!(skits.open(), Some(7));
        events.world.party.as_mut().unwrap().viewed_skits.insert(7);
        skits = skits.next_field();
        skits.step(&events, 10, true, None).unwrap();
        assert!(skits.prompt().is_none());

        let mut timed = Skits::new(Some(catalog(600, SkitCondition::Maps([10, 12]))));
        timed.control_ticks = REFRESH_TICKS - 1;
        timed.step(&events, 10, false, None).unwrap();
        assert!(timed.prompt().is_none());
        timed.step(&events, 10, true, None).unwrap();
        assert_eq!(timed.prompt().unwrap().id, 600);
        events
            .world
            .party
            .as_mut()
            .unwrap()
            .viewed_skits
            .insert(600);
        timed.step(&events, 10, true, None).unwrap();
        assert!(timed.prompt().is_none());
    }
}

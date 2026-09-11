//! Data that survives a field change. Actors, waits, and VM instances do not.
use crate::{GameWorld, party::Party, world::EventRecord};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script_vm::Memory;

pub(crate) const GLOBAL_BYTES: u16 = 0x400;
// The first sixteen words are expression temporaries and native/choice results.
// Persistent script variables begin with the story counter at 0x40.
pub(crate) const STORY_GLOBALS_START: u16 = 0x40;

/// Save only the data that already survives ordinary field changes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedProgress {
    pub script_globals: Vec<i32>,
    pub party: Party,
    pub event_flags: BTreeSet<u16>,
    pub event_records: BTreeMap<u8, EventRecord>,
    pub random_state: u32,
    #[serde(default)]
    pub gameplay_random: crate::GameplayRandom,
    pub tick: u32,
}
impl SavedProgress {
    pub fn cook(
        &mut self,
        data: &resonance_content::menu_data::MenuData,
    ) -> Result<crate::party::Meal, crate::party::CookingError> {
        self.party.cook(data, || self.gameplay_random.next_u32())
    }

    pub fn into_state(
        mut self,
        data: &resonance_content::session::SessionData,
    ) -> Result<PersistentState> {
        ensure!(
            self.script_globals.len() == usize::from(GLOBAL_BYTES / 4),
            "invalid persistent script globals"
        );
        self.party.bind_ex_skills(data);
        self.party.validate(data)?;
        ensure!(
            self.event_records.iter().all(|(id, r)| *id <= 200
                && r.tick <= self.tick
                && r.level
                    .is_none_or(|v| v > 0 && usize::from(v) < data.experience.len())
                && r.recorded_at
                    .is_none_or(|v| (0..=253402300799).contains(&v))),
            "invalid saved event record"
        );
        let mut memory = Memory::default();
        for (index, value) in self
            .script_globals
            .into_iter()
            .enumerate()
            .skip(usize::from(STORY_GLOBALS_START / 4))
        {
            memory.write(index as u16 * 4, symphonia_script::Width::S32, value)?;
        }
        Ok(PersistentState {
            memory,
            party: Some(self.party),
            event_flags: self.event_flags,
            event_records: self.event_records,
            random_state: self.random_state,
            gameplay_random: self.gameplay_random,
            tick: self.tick,
        })
    }
}

#[derive(Default)]
pub struct PersistentState {
    pub memory: Memory,
    pub party: Option<Party>,
    pub event_flags: BTreeSet<u16>,
    pub event_records: BTreeMap<u8, EventRecord>,
    pub random_state: u32,
    pub gameplay_random: crate::GameplayRandom,
    pub tick: u32,
}
impl PersistentState {
    pub fn into_world(self) -> (GameWorld, Memory) {
        let Self {
            memory,
            party,
            event_flags,
            event_records,
            random_state,
            gameplay_random,
            tick,
        } = self;
        (
            GameWorld {
                party,
                event_flags,
                event_records,
                random_state,
                gameplay_random,
                tick,
                ..Default::default()
            },
            memory,
        )
    }
}

//! Data that survives a field change. Actors, waits, and VM instances do not.
use crate::{GameWorld, party::Party, world::EventRecord};
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script_vm::Memory;

#[derive(Default)]
pub struct PersistentState {
    pub memory: Memory,
    pub party: Option<Party>,
    pub event_flags: BTreeSet<u16>,
    pub event_records: BTreeMap<u8, EventRecord>,
    pub random_state: u32,
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
            tick,
        } = self;
        (
            GameWorld {
                party,
                event_flags,
                event_records,
                random_state,
                tick,
                ..Default::default()
            },
            memory,
        )
    }
}

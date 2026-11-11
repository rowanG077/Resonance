//! Selected costumes and equipment share one CPU/GPU preparation queue.
use super::*;
use resonance_events::party::Party;
use resonance_game::battle::encounter::{Common, CommonBody, CommonEquipment};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Costume(u8, u8),
    Equipment(u16),
}

#[derive(Resource, Default)]
struct Selected {
    pending: Option<Pending>,
    retained: BTreeMap<Key, graphics::Batch>,
    failed: BTreeMap<Key, String>,
}
enum Pending {
    Cpu {
        key: Key,
        task: Task<Prepared>,
    },
    Gpu {
        key: Key,
        prepared: Prepared,
        batch: graphics::Batch,
    },
}

enum Prepared {
    Costume(Arc<CommonBody>),
    Equipment(Arc<CommonEquipment>),
}

impl Key {
    fn prepare(self, common: &Common, cancelled: &impl Fn() -> bool) -> Result<Prepared> {
        match self {
            Self::Costume(character, costume) => common
                .prepare_costume(character, costume, cancelled)
                .map(Prepared::Costume),
            Self::Equipment(item) => common
                .prepare_equipment(item, cancelled)
                .map(Prepared::Equipment),
        }
    }

    fn retire(self, common: &Common) {
        match self {
            Self::Costume(character, _) => common.retire_costume(character),
            Self::Equipment(item) => common.retire_equipment(item),
        }
    }
}

impl Prepared {
    fn batch(&self, world: &World) -> graphics::Batch {
        match self {
            Self::Costume(body) => graphics::Batch::new(
                world,
                body.source.parts.iter().chain(&body.source.wings),
                Vec::new(),
                body.files.clone(),
            ),
            Self::Equipment(equipment) => graphics::Batch::new(
                world,
                equipment.parts(),
                Vec::new(),
                equipment.files.clone(),
            ),
        }
    }

    fn install(self, common: &Common) {
        match self {
            Self::Costume(body) => common.install_costume(body),
            Self::Equipment(equipment) => common.install_equipment(equipment),
        }
    }
}

fn selected(
    party: &Party,
    data: &resonance_content::session::SessionData,
    characters: impl Iterator<Item = usize>,
) -> BTreeSet<Key> {
    let mut selected = BTreeSet::new();
    for index in characters {
        let Some(member) = party.members.get(index) else {
            continue;
        };
        if member.costume != 0 {
            selected.insert(Key::Costume(index as u8 + 1, member.costume));
        }
        for item in [member.equipment[0], member.equipment[5]] {
            if item != 0
                && data
                    .items
                    .get(usize::from(item))
                    .is_some_and(|item| matches!(item.equipment_kind, Some(0 | 3)))
            {
                selected.insert(Key::Equipment(item));
            }
        }
    }
    selected
}

fn required(party: &Party, data: &resonance_content::session::SessionData) -> BTreeSet<Key> {
    selected(
        party,
        data,
        party
            .formation
            .iter()
            .filter_map(|&id| id.checked_sub(1).map(usize::from)),
    )
}

impl Selected {
    fn ready(&self, required: &BTreeSet<Key>) -> Result<bool> {
        // Inspect every required failure before returning a pending result.
        // A speculative failure must become an error when its resource is selected.
        for key in required {
            if let Some(error) = self.failed.get(key) {
                anyhow::bail!("selected battle asset {key:?}: {error}");
            }
        }
        Ok(required.iter().all(|key| self.retained.contains_key(key)))
    }
}

/// Poll the same jobs used by background preparation. Never block the render
/// loop: GPU batches need subsequent application/render updates to complete.
pub(crate) fn battle_ready(world: &World) -> Result<bool> {
    let Some(state) = world.get_resource::<Selected>() else {
        return Ok(true);
    };
    if world
        .resource::<Resident>()
        .common
        .get()
        .is_none_or(|common| common.battle.is_none())
    {
        // Standalone callers and failed optional startup capture still use normal encounter preparation.
        return Ok(true);
    }
    let Some(session) = world.get_resource::<crate::new_game::Session>() else {
        return Ok(true);
    };
    let Some(party) = session.events().world.party.as_ref() else {
        return Ok(true);
    };
    state.ready(&required(party, session.data()))
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Selected>()
        .add_systems(Update, update.after(crate::new_game::transition));
}

pub(crate) fn update(world: &mut World) {
    if !startup::ready(world) {
        return;
    }
    let Some(common) = world
        .resource::<Resident>()
        .common
        .get()
        .and_then(|common| common.battle.clone())
    else {
        return;
    };
    world.resource_scope(|world, mut state: Mut<Selected>| {
        let session = world.get_resource::<crate::new_game::Session>();
        let party = world
            .get_resource::<crate::battle::Owner>()
            .and_then(crate::battle::Owner::prompt_party)
            .or_else(|| session.and_then(|session| session.events().world.party.as_ref()));
        let (wanted, required) = match (session, party) {
            (Some(session), Some(party)) => (
                selected(party, session.data(), 0..party.members.len()),
                if session.events().battle_pending() {
                    required(party, session.data())
                } else {
                    BTreeSet::new()
                },
            ),
            _ => (BTreeSet::new(), BTreeSet::new()),
        };
        state.retained.retain(|&key, _| {
            let keep = wanted.contains(&key);
            if !keep {
                key.retire(&common);
            }
            keep
        });
        state.failed.retain(|key, _| wanted.contains(key));
        // Cancel through the task's own token, then await retirement. Rapid
        // menu changes cannot accumulate workers or publish a superseded body.
        if let Some(pending) = state.pending.take() {
            match pending {
                Pending::Cpu { key, task } => {
                    if !wanted.contains(&key) || !required.is_empty() && !required.contains(&key) {
                        task.cancelled.store(true, Ordering::Relaxed);
                    }
                    match task.poll() {
                        Ok(None) => state.pending = Some(Pending::Cpu { key, task }),
                        result => {
                            if task.cancelled.load(Ordering::Relaxed) {
                                return;
                            }
                            match result
                                .and_then(|result| result.context("party asset task stopped"))
                                .and_then(|result| result)
                            {
                                Ok(prepared) => {
                                    let batch = prepared.batch(world);
                                    state.pending = Some(Pending::Gpu {
                                        key,
                                        prepared,
                                        batch,
                                    });
                                }
                                Err(error) => fail(&mut state, key, error),
                            }
                        }
                    }
                }
                Pending::Gpu {
                    key,
                    prepared,
                    mut batch,
                } => {
                    if !wanted.contains(&key) || !required.is_empty() && !required.contains(&key) {
                        return;
                    }
                    match batch.update(world) {
                        Ok(false) => {
                            state.pending = Some(Pending::Gpu {
                                key,
                                prepared,
                                batch,
                            })
                        }
                        Ok(true) => {
                            prepared.install(&common);
                            state.retained.insert(key, batch);
                            info!(?key, "Selected battle asset ready on CPU and GPU");
                        }
                        Err(error) => fail(&mut state, key, error),
                    }
                }
            }
            return;
        }
        // A pending battle gets its formation's work ahead of reserve members.
        for &key in required.iter().chain(&wanted) {
            if state.retained.contains_key(&key) || state.failed.contains_key(&key) {
                continue;
            }
            let common = common.clone();
            match Task::spawn(format!("party asset {key:?}"), move |stop| {
                key.prepare(&common, &|| stop.load(Ordering::Relaxed))
            }) {
                Ok(task) => state.pending = Some(Pending::Cpu { key, task }),
                Err(error) => fail(&mut state, key, error),
            }
            break;
        }
    });
}
fn fail(state: &mut Selected, key: Key, error: anyhow::Error) {
    warn!(?key, "Party asset preparation failed: {error:#}");
    state.failed.insert(key, format!("{error:#}"));
}

#[cfg(test)]
pub(crate) fn retained(world: &World) -> usize {
    world
        .resource::<Selected>()
        .retained
        .keys()
        .filter(|key| matches!(key, Key::Costume(..)))
        .count()
}

#[cfg(test)]
mod tests;

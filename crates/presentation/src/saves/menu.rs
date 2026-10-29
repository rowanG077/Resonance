//! File work runs off the presentation thread; the menu owns input until it ends.
use super::*;
use resonance_game::menu::{Command, Menu, SLOTS_PER_BANK, Slot};

enum Completion {
    Slots(Vec<Slot>),
    Saved(usize, Slot),
}
#[derive(Resource)]
struct Pending(loading::Task<Completion>);
#[derive(Resource)]
struct Loading(loading::Pending);

pub(super) fn loading(world: &World) -> bool {
    world.contains_resource::<Loading>()
}

pub(super) fn preparing(world: &World) -> bool {
    world
        .get_resource::<Pending>()
        .is_some_and(|task| task.0.is_running())
        || world
            .get_resource::<Loading>()
            .is_some_and(|task| task.0.is_running())
}

pub(super) fn update(world: &mut World) {
    if let Some(pending) = world.get_resource::<Pending>() {
        let result = match pending.0.poll() {
            Ok(None) => return,
            Ok(Some(result)) => result,
            Err(error) => Err(error),
        };
        world.remove_resource::<Pending>();
        match result {
            Ok(completion) => {
                if let Some(mut menu) = current(world) {
                    match completion {
                        Completion::Slots(slots) => {
                            menu.slots = slots;
                            menu.finish(None);
                        }
                        Completion::Saved(index, slot) => {
                            menu.slots[index] = slot;
                            menu.finish(Some("Save successful.".into()));
                        }
                    }
                }
            }
            Err(error) => failed(world, error),
        }
    }
    if let Some(pending) = world.get_resource::<Loading>() {
        let result = match pending.0.poll() {
            Ok(None) => return,
            Ok(Some(result)) => result,
            Err(error) => Err(error),
        };
        world.remove_resource::<Loading>();
        match result {
            Ok(candidate) => {
                crate::game_over::loaded(world);
                if let Some(mut session) = world.get_resource_mut::<new_game::Session>() {
                    let changing = session.map_id() != candidate.map_id()
                        || session.overworld().is_some()
                        || candidate.overworld().is_some();
                    session.replace_loaded(candidate);
                    restored(world, changing);
                } else {
                    new_game::activate(world, candidate);
                    field_view::reset_live(world);
                }
            }
            Err(error) => failed(world, error),
        }
    }
    let command = current(world).and_then(|mut menu| menu.take_command());
    if let Some(command) = command
        && let Err(error) = start(world, command)
    {
        failed(world, error);
    }
}

fn current(world: &mut World) -> Option<Mut<'_, Menu>> {
    if world.contains_resource::<title::TitleMenu>() {
        return world
            .get_resource_mut::<title::TitleMenu>()
            .map(|menu| menu.map_unchanged(|m| &mut m.0));
    }
    world
        .get_resource_mut::<new_game::Session>()?
        .filter_map_unchanged(|session| session.menu_mut())
}
fn failed(world: &mut World, error: anyhow::Error) {
    warn!("Save menu operation failed: {error:#}");
    if let Some(mut menu) = current(world) {
        menu.finish(Some(
            "Unable to complete the operation. Please choose another slot or go back.".into(),
        ));
    }
}
fn start(world: &mut World, command: Command) -> Result<()> {
    let persistence = world.resource::<Persistence>();
    ensure!(
        !persistence.is_writing(),
        "quicksave is still being written"
    );
    let store = persistence.store.clone();
    match command {
        Command::ReadSlots => {
            let library = world
                .resource::<loading::Resident>()
                .library(&world.resource::<super::super::RunOptions>().assets)?;
            world.insert_resource(Pending(loading::Task::spawn(move |_| {
                read_slots(&store, &library).map(Completion::Slots)
            })?));
        }
        Command::Save(index) => {
            let session = world.resource::<new_game::Session>();
            let menu = session.menu().context("save menu is closed")?;
            ensure!(menu.at_save_point, "saving is unavailable here");
            let checkpoint = menu
                .checkpoint
                .clone()
                .context("save menu has no checkpoint")?;
            let location = checkpoint.location();
            let played_ticks = checkpoint.played_ticks();
            let header = Header {
                version: resonance_persistence::VERSION,
                label: format!("Save {}", index + 1),
                location: location.clone(),
                played_ticks,
                saved_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            };
            let bytes = resonance_persistence::encode(&header, &checkpoint)?;
            let slot = slot_id(index)?;
            let party = checkpoint.into_progress().party;
            let vitals = party
                .formation
                .iter()
                .map(|&id| (id, party.members[usize::from(id - 1)].maximum_vitals()))
                .collect();
            world.insert_resource(Pending(loading::Task::spawn(move |_| {
                store.write(Kind::Save, &slot, &bytes)?;
                Ok(Completion::Saved(
                    index,
                    Slot::Saved {
                        location,
                        played_ticks,
                        party: Box::new(party),
                        vitals,
                    },
                ))
            })?));
        }
        Command::Load(index) => {
            let slot = slot_id(index)?;
            // Decode again at load time: another process may have replaced the
            // selected file since its summary was read.
            let bytes = store.read(Kind::Save, &slot)?;
            let pending = loading::Pending::start(
                world.resource::<super::super::RunOptions>().assets.clone(),
                world
                    .resource::<super::super::RunOptions>()
                    .script_root
                    .clone(),
                Some(bytes),
                None,
                world.resource::<loading::Resident>(),
            )?;
            world.insert_resource(Loading(pending));
        }
    }
    Ok(())
}

fn slot_id(index: usize) -> Result<SlotId> {
    ensure!(index < SLOTS_PER_BANK * 2, "save slot is out of range");
    SlotId::new(format!(
        "{}-{:03}",
        if index < SLOTS_PER_BANK { 'a' } else { 'b' },
        index % SLOTS_PER_BANK + 1
    ))
}
fn read_slots(store: &Store, library: &resonance_content::library::Library) -> Result<Vec<Slot>> {
    let existing = store.list(Kind::Save)?;
    let rules = if existing.is_empty() {
        None
    } else {
        let mut snapshot = library.snapshot();
        let items = snapshot
            .json::<resonance_content::items::Document>(
                resonance_content::items::PATH,
                resonance_content::preparation::JSON_LIMIT,
            )?
            .prepare()?;
        let ex = snapshot
            .json::<resonance_content::ex_skills::Document>(
                resonance_content::ex_skills::PATH,
                resonance_content::preparation::JSON_LIMIT,
            )?
            .prepare(&items.mappings)?;
        Some((items.menu, ex))
    };
    (0..SLOTS_PER_BANK * 2)
        .map(|index| {
            let id = slot_id(index)?;
            if existing.binary_search(&id).is_err() {
                return Ok(Slot::Empty);
            }
            let result = store
                .read(Kind::Save, &id)
                .and_then(|bytes| {
                    Ok(resonance_persistence::decode::<SceneCheckpoint>(&bytes)?.into_parts())
                })
                .and_then(|(header, checkpoint)| {
                    let party = checkpoint.into_progress().party;
                    ensure!(
                        (1..=8).contains(&party.formation.len())
                            && party.formation.iter().all(|&id| {
                                id > 0 && usize::from(id) <= party.members.len() && id <= 9
                            }),
                        "invalid slot party formation"
                    );
                    ensure!(
                        header
                            .location
                            .chars()
                            .all(|c| c.is_ascii_graphic() || c == ' '),
                        "invalid slot location label"
                    );
                    let (items, ex) = rules.as_ref().context("missing slot preview rules")?;
                    let vitals = party
                        .formation
                        .iter()
                        .map(|&id| {
                            Ok((
                                id,
                                party.members[usize::from(id - 1)].maximum_vitals_for(
                                    items,
                                    ex,
                                    usize::from(id - 1),
                                )?,
                            ))
                        })
                        .collect::<Result<_>>()?;
                    Ok((header, party, vitals))
                });
            Ok(match result {
                Ok((header, party, vitals)) => Slot::Saved {
                    location: header.location,
                    played_ticks: header.played_ticks,
                    party: Box::new(party),
                    vitals,
                },
                Err(error) => {
                    warn!("Cannot read save slot {}: {error:#}", id.as_str());
                    Slot::Invalid(
                        "This data is damaged or belongs to an incompatible version.".into(),
                    )
                }
            })
        })
        .collect()
}

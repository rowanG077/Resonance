//! File work runs off the presentation thread; the menu owns input until it ends.
use super::*;
use resonance_game::menu::{Command, Menu, SLOTS_PER_BANK, Slot};
use resonance_persistence::Identity;

enum Completion {
    Slots(Vec<Slot>),
    Saved(usize, Slot),
}
#[derive(Resource)]
struct Pending(loading::Task<Completion>);
#[derive(Resource)]
struct Loading(loading::Pending);

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
                if let Some(mut session) = world.get_resource_mut::<new_game::Session>() {
                    let changing = session.assets.map_id != candidate.assets.map_id
                        || session.overworld.is_some()
                        || candidate.overworld.is_some();
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
    if world.contains_resource::<title::LoadMenu>() {
        return world
            .get_resource_mut::<title::LoadMenu>()
            .map(|menu| menu.map_unchanged(|m| &mut m.0));
    }
    world
        .get_resource_mut::<new_game::Session>()?
        .filter_map_unchanged(|session| {
            if let Some(scene) = &mut session.overworld {
                scene.session.menu.as_mut()
            } else {
                session.field.menu.as_mut()
            }
        })
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
            let context = world
                .get_resource::<new_game::Session>()
                .map(|session| -> Result<_> {
                    let data = session
                        .events()
                        .resources()
                        .session_data
                        .clone()
                        .context("session definitions are missing")?;
                    Ok((session.identity.clone(), data))
                })
                .transpose()?;
            let root = world.resource::<crate::RunOptions>().assets.clone();
            world.insert_resource(Pending(loading::Task::spawn(move |_| {
                let (identity, data) = context.map_or_else(
                    || -> Result<_> {
                        Ok((
                            new_game::Session::identity(&root)?,
                            std::sync::Arc::new(slot_data(&root)?),
                        ))
                    },
                    Ok,
                )?;
                read_slots(&store, &identity, &data).map(Completion::Slots)
            })?));
        }
        Command::Save(index) => {
            let session = world.resource::<new_game::Session>();
            let identity = session.identity.clone();
            let menu = session
                .overworld
                .as_ref()
                .map_or(session.field.menu.as_ref(), |scene| {
                    scene.session.menu.as_ref()
                })
                .context("save menu is closed")?;
            ensure!(menu.at_save_point, "saving is unavailable here");
            let checkpoint = if let Some(scene) = &session.overworld {
                SceneCheckpoint::World(WorldCheckpoint {
                    overworld: scene.session.menu_checkpoint()?,
                    anchor_field: session.assets.map_id,
                })
            } else {
                SceneCheckpoint::Field(
                    menu.checkpoint
                        .clone()
                        .context("save menu has no checkpoint")?,
                )
            };
            let location = checkpoint.location();
            let played_ticks = checkpoint.played_ticks();
            let header = Header {
                identity,
                label: format!("Save {}", index + 1),
                location: location.clone(),
                played_ticks,
                saved_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            };
            let bytes = resonance_persistence::encode(&header, &checkpoint)?;
            let slot = slot_id(index)?;
            world.insert_resource(Pending(loading::Task::spawn(move |_| {
                store.write(Kind::Save, &slot, &bytes)?;
                Ok(Completion::Saved(
                    index,
                    Slot::Saved {
                        location,
                        played_ticks,
                        checkpoint: Box::new(checkpoint.menu_snapshot()),
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
                world.resource::<loading::Resident>(),
            )?;
            world.insert_resource(Loading(pending));
        }
    }
    Ok(())
}

fn slot_data(root: &std::path::Path) -> Result<resonance_content::session::SessionData> {
    let mut data: resonance_content::session::SessionData =
        serde_json::from_slice(&std::fs::read(root.join("game/session-data.json"))?)?;
    let menu: resonance_content::menu_data::MenuData =
        serde_json::from_slice(&std::fs::read(root.join("game/menu-data.json"))?)?;
    data.ex_skills = Some(std::sync::Arc::new(menu.ex_skills));
    Ok(data)
}
fn slot_id(index: usize) -> Result<SlotId> {
    ensure!(index < SLOTS_PER_BANK * 2, "save slot is out of range");
    SlotId::new(format!(
        "{}-{:03}",
        if index < SLOTS_PER_BANK { 'a' } else { 'b' },
        index % SLOTS_PER_BANK + 1
    ))
}
fn read_slots(
    store: &Store,
    identity: &Identity,
    data: &resonance_content::session::SessionData,
) -> Result<Vec<Slot>> {
    let existing = store.list(Kind::Save)?;
    (0..SLOTS_PER_BANK * 2)
        .map(|index| {
            let id = slot_id(index)?;
            if existing.binary_search(&id).is_err() {
                return Ok(Slot::Empty);
            }
            let result = store
                .read(Kind::Save, &id)
                .and_then(|bytes| {
                    resonance_persistence::decode::<SceneCheckpoint>(&bytes, identity)
                })
                .and_then(|(header, checkpoint)| {
                    let mut checkpoint = checkpoint.menu_snapshot();
                    checkpoint.progress.party.bind_ex_skills(data);
                    checkpoint.progress.party.validate(data)?;
                    ensure!(
                        header
                            .location
                            .chars()
                            .all(|c| c.is_ascii_graphic() || c == ' '),
                        "invalid slot location label"
                    );
                    Ok((header, checkpoint))
                });
            Ok(match result {
                Ok((header, checkpoint)) => Slot::Saved {
                    location: header.location,
                    played_ticks: header.played_ticks,
                    checkpoint: Box::new(checkpoint),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires cooked party and menu definitions; no devices"]
    fn clear_save_with_ex_skills_is_readable_in_the_slot_browser() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
            });
        let data = slot_data(&root)?;
        let mut party = resonance_events::party::Party::new(&data, Default::default())?;
        party.new_game_plus.cleared = true;
        party.members[0].ex_gems[0] = 1;
        party.members[0].ex_skills[0] = data.ex_skills.as_ref().unwrap().characters[0].levels[0][0];
        [party.members[0].hp, party.members[0].tp] = party.members[0].maximum_vitals();
        let mut script_globals = vec![0; 256];
        script_globals[0x40 / 4] = 1;
        let checkpoint = SceneCheckpoint::Field(FieldCheckpoint {
            allow_incomplete_scripts: false,
            map_id: 5,
            position: [0.; 3],
            heading: 0.,
            camera: None,
            progress: resonance_events::SavedProgress {
                script_globals,
                party,
                script_state: Default::default(),
                event_flags: Default::default(),
                event_records: Default::default(),
                random_state: 0,
                gameplay_random: Default::default(),
                tick: 0,
            },
            played_ticks: Some(9000),
        });
        let identity = Identity {
            schema: 2,
            content: [0; 32],
        };
        let header = Header {
            identity: identity.clone(),
            label: "Clear".into(),
            location: checkpoint.location(),
            played_ticks: checkpoint.played_ticks(),
            saved_unix_seconds: 0,
        };
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path());
        store.write(
            Kind::Save,
            &slot_id(0)?,
            &resonance_persistence::encode(&header, &checkpoint)?,
        )?;
        let slots = read_slots(&store, &identity, &data)?;
        assert!(matches!(&slots[0], Slot::Saved { location, checkpoint, .. }
            if location == "Game cleared" && checkpoint.progress.party.members[0].ex_gems[0] == 1));
        Ok(())
    }
}

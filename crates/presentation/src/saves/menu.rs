//! File work runs off the presentation thread; the menu owns input until it ends.
use super::*;
use resonance_game::menu::{Command, Menu, SLOTS_PER_BANK, Slot};
use resonance_persistence::Identity;
#[cfg(test)]
use std::path::Path;

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
                .map(|session| (session.identity.clone(), session.data.clone()));
            let root = world.resource::<crate::RunOptions>().assets.clone();
            let diagnostics = crate::diagnostics::policy(world);
            world.insert_resource(Pending(loading::Task::spawn(move |_| {
                let (identity, data) =
                    context.map_or_else(|| new_game::save_context(&root, diagnostics), Ok)?;
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
                    resonance_persistence::decode::<SceneCheckpoint>(&bytes)?.admit(identity)
                })
                .and_then(|(header, checkpoint)| {
                    let mut checkpoint = checkpoint.menu_snapshot();
                    checkpoint.progress.party.bind_rules(data);
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
use crate::test_support::field_checkpoint;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires current opening field/menu assets; no output devices"]
    fn save_slots_bind_admitted_ex_rules_before_validation() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            PathBuf::from,
        );
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let (data, _) =
            new_game::admit_definitions(|path| Ok(std::fs::read(root.join(path))?), &diagnostics)?;
        let files = resonance_content::prepared::Files::load(
            &root,
            &["fields/map-332.preload.json"],
            &mut Default::default(),
            || false,
        )?;
        let mut checkpoint = field_checkpoint(&files)?;
        let rules = &data
            .rules
            .as_ref()
            .context("gameplay rules not admitted")?
            .ex_skills;
        let gem = rules.gem_items[0];
        let skill = rules.characters[0].levels[0][0];
        let party = &mut checkpoint.progress.party;
        party.items.insert(gem, 1);
        assert!(
            party
                .set_ex_gem(&data, 0, 0, 1)
                .map_err(anyhow::Error::msg)?
        );
        assert!(
            party
                .set_ex_skill(&data, 0, 0, skill)
                .map_err(anyhow::Error::msg)?
        );
        party.validate(&data)?;
        let identity = Identity::load(&files)?;
        let header = Header {
            identity: identity.clone(),
            label: "Save 1".into(),
            location: "Iselia school grounds".into(),
            played_ticks: 0,
            saved_unix_seconds: 0,
        };
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().to_owned());
        store.write(
            Kind::Save,
            &slot_id(0)?,
            &resonance_persistence::encode(&header, &checkpoint)?,
        )?;
        let slots = read_slots(&store, &identity, &data)?;
        let Slot::Saved {
            checkpoint: saved, ..
        } = &slots[0]
        else {
            anyhow::bail!("EX-bearing save slot was rejected");
        };
        assert_eq!(saved.progress.party.members[0].ex_skills[0], skill);
        saved.progress.party.validate(&data)?;
        checkpoint.progress.party.members[0].ex_skills[0] = 255;
        store.write(
            Kind::Save,
            &slot_id(0)?,
            &resonance_persistence::encode(&header, &checkpoint)?,
        )?;
        assert!(matches!(
            read_slots(&store, &identity, &data)?[0],
            Slot::Invalid(_)
        ));
        Ok(())
    }
}

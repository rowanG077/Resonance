//! Shared preferences mirrored into the active party for gameplay and rendering.
use crate::new_game::Session;
use anyhow::{Context, Result};
use bevy::prelude::*;
use resonance_content::menu_data::CustomizeSettings;
use std::{fs, io::ErrorKind, path::PathBuf};

#[derive(Resource, Default)]
pub(crate) struct Preferences {
    /// At the title, battle_rank is the default difficulty for New Game.
    pub settings: CustomizeSettings,
    file: Option<PathBuf>,
}
impl Preferences {
    pub fn load(file: PathBuf) -> Result<Self> {
        let settings: CustomizeSettings = match fs::read_to_string(&file) {
            Ok(text) => toml::from_str(&text).context("read config.toml")?,
            Err(error) if error.kind() == ErrorKind::NotFound => Default::default(),
            Err(error) => return Err(error).context("read config.toml"),
        };
        settings.validate().context("invalid config.toml")?;
        Ok(Self {
            settings,
            file: Some(file),
        })
    }

    pub fn commit(&mut self, settings: CustomizeSettings) -> Result<()> {
        settings.validate()?;
        // Keep the applied settings for this session even if the disk is unavailable.
        self.settings = settings;
        if let Some(file) = &self.file {
            fs::create_dir_all(file.parent().unwrap())?;
            fs::write(file, toml::to_string_pretty(&self.settings)?)
                .context("write config.toml")?;
        }
        Ok(())
    }

    fn global_settings(&self, settings: &CustomizeSettings) -> CustomizeSettings {
        CustomizeSettings {
            battle_rank: self.settings.battle_rank,
            ..settings.clone()
        }
    }

    pub fn apply(&self, session: &mut Session, new_game: bool) {
        let apply = |party: &mut resonance_events::party::Party| {
            let settings = &mut party.settings.preferences;
            let forget_opposite_path =
                settings.learn_both_arte_paths && !self.settings.learn_both_arte_paths;
            let rank = if new_game {
                self.settings.battle_rank
            } else {
                settings.battle_rank
            };
            *settings = self.settings.clone();
            settings.battle_rank = rank;
            if !settings.learn_elemental_artes {
                party.forget_elemental_artes();
            }
            if forget_opposite_path {
                party.forget_opposite_path_artes();
            }
        };
        if let Some(party) = &mut session.events_mut().world.party {
            apply(party);
        }
        if let Some(menu) = session.menu_mut()
            && let Some(checkpoint) = &mut menu.checkpoint
        {
            apply(&mut checkpoint.progress_mut().party);
        }
    }
}

/// Runs before save loading and session retirement. Customize commits its draft
/// on exit, after any destructive changes have been confirmed.
pub(crate) fn update(world: &mut World) {
    let Some(preferences) = world.get_resource::<Preferences>() else {
        return;
    };
    let Some(settings) = world
        .get_resource::<Session>()
        .and_then(|session| session.events().world.party.as_ref())
        .map(|party| preferences.global_settings(&party.settings.preferences))
    else {
        return;
    };
    if settings == preferences.settings {
        return;
    }
    let result = world.resource_mut::<Preferences>().commit(settings);
    if let Err(error) = result {
        crate::saves::report(
            world,
            Err(error.context("Settings applied, but config.toml could not be saved")),
        );
    }
}

pub(crate) fn restore(world: &mut World) {
    if world.contains_resource::<Preferences>() && world.contains_resource::<Session>() {
        world.resource_scope(|world, preferences: Mut<Preferences>| {
            preferences.apply(&mut world.resource_mut::<Session>(), false);
        });
    }
}

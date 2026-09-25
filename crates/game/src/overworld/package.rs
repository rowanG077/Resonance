//! Verified world loading. Runtime travel never discovers files while stepping.
use super::{
    Assets, TileCoordinate, World, collision,
    landmarks::{Locations, Progress},
};
use anyhow::{Context, Result};
use resonance_content::{
    overworld::{PACKAGE_PATH, Package, TileResources},
    prepared::{Cache, Files},
};
use resonance_events::{PersistentState, ResourceLibrary};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use symphonia_script::Program;

pub struct Prepared {
    pub definition: Arc<Package>,
    pub files: Arc<Files>,
    pub resources: Arc<ResourceLibrary>,
    program: Arc<Program>,
    story_rules: Arc<super::scripts::Rules>,
    skits: Arc<std::collections::BTreeMap<u16, crate::skit::Prepared>>,
}

impl Prepared {
    pub fn load(
        root: &Path,
        cache: &mut Cache,
        mut available_fields: BTreeSet<u32>,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let definition: Package = serde_json::from_slice(&std::fs::read(root.join(PACKAGE_PATH))?)?;
        definition.validate()?;
        let files = Arc::new(Files::from_inventory(
            root,
            definition.files.clone(),
            cache,
            cancelled,
        )?);
        let mut data: resonance_content::session::SessionData =
            files.json("game/session-data.json")?;
        let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
        menu.validate()?;
        data.ex_skills = Some(Arc::new(menu.ex_skills.clone()));
        data.validate()?;
        let skits: resonance_content::skit::SkitCatalog = files.json("game/skits.json")?;
        skits.validate()?;
        available_fields.insert(3000);
        let resources = Arc::new(ResourceLibrary {
            session_data: Some(Arc::new(data)),
            menu_data: Some(Arc::new(menu)),
            text: Arc::new(files.json("game/text.json")?),
            skits: Some(Arc::new(skits)),
            messages: files.json(&definition.messages)?,
            actor_names: ResourceLibrary::character_names(),
            fields: available_fields,
            ..Default::default()
        });
        let program = Arc::new(Program::decode(&files.read(&definition.script.path)?)?);
        let skits = Arc::new(crate::skit::Prepared::load(
            resources.skits.clone().unwrap(),
            &files,
        )?);
        let story_rules =
            super::scripts::Rules::prepare(&mut Default::default(), &files.script_sources()?)?;
        Ok(Self {
            story_rules,
            definition: Arc::new(definition),
            files,
            resources,
            program,
            skits,
        })
    }

    /// Both collision and presentation use this selection, including destroyed
    /// ranches and the late Tethe'alla terrain. Missing alternates fail preparation.
    pub fn terrain<'a>(
        &'a self,
        world: World,
        persistent: &PersistentState,
    ) -> Result<Vec<(TileCoordinate, &'a TileResources)>> {
        let mut locations = Locations::new(
            Arc::new(self.definition.landmarks.clone()),
            Arc::new(self.definition.guideposts.clone()),
            self.story_rules.clone(),
        )?;
        let party = persistent.party.as_ref().context("world party missing")?;
        let progress = Progress::new(
            &persistent.memory,
            party,
            &persistent.event_flags,
            &persistent.script_state,
        );
        locations.refresh(&progress)?;
        let alternates = self
            .story_rules
            .terrain_variants(&locations, &progress, world)?;
        self.definition.worlds[world.index()]
            .iter()
            .map(|tile| {
                let coordinate = TileCoordinate::new(tile.column, tile.row)?;
                let resources = if alternates.contains(&coordinate) {
                    tile.alternate
                        .as_ref()
                        .context("required world terrain variant is missing")?
                } else {
                    &tile.base
                };
                Ok((coordinate, resources))
            })
            .collect()
    }

    pub fn assets(&self, world: World, persistent: &PersistentState) -> Result<Arc<Assets>> {
        let terrain = self
            .terrain(world, persistent)?
            .into_iter()
            .map(|(coordinate, resources)| {
                Ok((coordinate, collision::Mesh::new(&resources.ground)?))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Arc::new(Assets {
            world,
            terrain: Arc::new(collision::Terrain::new(
                terrain,
                self.definition.collision.clone(),
            )?),
            rules: Arc::new(super::Rules::new(self.definition.encounters.clone())?),
            movement: Arc::new(self.definition.movement.clone()),
            landmarks: Arc::new(self.definition.landmarks.clone()),
            guideposts: Arc::new(self.definition.guideposts.clone()),
            program: self.program.clone(),
            story_rules: self.story_rules.clone(),
            resources: self.resources.clone(),
            skits: self.skits.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::landmarks::tests::Global;
    use super::*;
    use resonance_content::overworld::Interaction;
    #[test]
    fn destroyed_landmarks_and_story_select_only_authored_terrain_variants() {
        let selected = |world: World, story, ranch, blocked| {
            let mut locations = Locations::new(
                Arc::new(super::super::landmarks::tests::definitions()),
                Default::default(),
                super::super::scripts::fixture(),
            )
            .unwrap();
            if let Some(appearance) = locations.appearances.get_mut(&blocked) {
                appearance.interaction = Interaction::Blocked;
            }
            let mut progress = super::super::landmarks::tests::progress();
            progress.set(Global::Story, story);
            progress.set(Global::UnknownId30, if ranch { 10_000 } else { 0 });
            locations
                .story_rules
                .terrain_variants(&locations, &progress.view(), world)
                .unwrap()
                .into_iter()
                .map(|tile| (tile.column(), tile.row()))
                .collect::<Vec<_>>()
        };
        assert_eq!(selected(World::Sylvarant, 0, false, 19), [(10, 8)]);
        assert_eq!(selected(World::Sylvarant, 0, false, 16), [(0, 6), (0, 7)]);
        assert_eq!(selected(World::Sylvarant, 0, true, 0), [(9, 6)]);
        assert!(selected(World::TetheAlla, 22_600_999, true, 19).is_empty());
        assert_eq!(
            selected(World::TetheAlla, 22_601_000, false, 0),
            [(5, 5), (6, 5)]
        );
    }
}

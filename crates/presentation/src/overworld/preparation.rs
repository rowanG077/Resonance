use super::*;
use resonance_content::{
    diagnostics::Diagnostics,
    library::{FIELDS, Library},
    overworld::document,
    preparation,
    prepared::Files,
    skit,
};
use std::path::Path;

impl Package {
    pub(crate) fn prepare(
        root: &Path,
        cache: &mut crate::loading::Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let library = Library::open(root, &[])?;
        Self::capture(&library, cache, cancelled, diagnostics).context("prepare named world")
    }

    pub(crate) fn capture(
        library: &Library,
        cache: &mut crate::loading::Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut snapshot = library.snapshot();
        let (definition, package) = document::capture(&mut snapshot, &cancelled)?;
        preparation::gameplay(&mut snapshot, &diagnostics, &cancelled)?;
        preparation::ui(&mut snapshot, &diagnostics, &cancelled)?;
        let skits = Arc::new(skit::document::capture(&mut snapshot, &cancelled)?);
        resonance_game::screen_resources::capture(
            &mut snapshot,
            std::iter::once(package.script.as_str())
                .chain(skits.resources.values().map(|skit| skit.script.as_str())),
            &diagnostics,
            &cancelled,
        )?;
        let choices = definition.audio.resolve(&mut snapshot)?;
        for scene in package.visuals.cinematics.values() {
            scene
                .presentation
                .validate_sound_selection(|id| choices.effects.contains_key(&id))?;
            for line in &scene.dialogue {
                anyhow::ensure!(
                    choices.voices.contains_key(&line.voice),
                    "cinematic voice {} is not selected by world audio",
                    line.voice
                );
            }
        }
        let environment = definition.audio.environment(&mut snapshot)?;
        let mut audio = cache.audio.capture_resolved(
            &environment,
            choices,
            &mut snapshot,
            diagnostics.clone(),
            &cancelled,
        )?;
        Arc::make_mut(&mut audio).capture_skits(&skits, &mut snapshot, &cancelled)?;
        let fields = snapshot
            .mappings(FIELDS)?
            .iter()
            .map(|(id, _)| id)
            .chain([3000])
            .collect();
        let files = Arc::new(Files::from_snapshot(snapshot, &cancelled, diagnostics)?);
        let world = Arc::new(game::Prepared::from_prepared(
            package, files, skits, fields,
        )?);
        anyhow::ensure!(!cancelled(), "world preparation cancelled");
        Ok(Self { world, audio })
    }
}

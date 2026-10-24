use super::*;
use resonance_content::{
    diagnostics::Diagnostics,
    library::{FIELDS, Library},
    overworld::document,
    preparation,
    prepared::Files,
    save_identity::Identity,
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
            std::iter::once(package.script.path.as_str())
                .chain(skits.resources.values().map(|skit| skit.script.as_str())),
            &diagnostics,
            &cancelled,
        )?;
        let choices: resonance_content::field_audio::document::Definition =
            snapshot.json(&definition.audio, preparation::JSON_LIMIT)?;
        let choices = choices.resolve(&mut snapshot)?;
        for scene in package.visuals.cinematics.values() {
            scene
                .presentation
                .validate_sound_selection(|id| choices.effects.contains_key(&id))?;
            for line in &scene.dialogue {
                anyhow::ensure!(
                    choices.voices.contains_key(&line.voice),
                    "cinematic voice {} is not selected by {}",
                    line.voice,
                    definition.audio
                );
            }
        }
        let mut audio = cache.audio.capture_named(
            &definition.audio,
            &mut snapshot,
            diagnostics.clone(),
            &cancelled,
        )?;
        cache
            .audio
            .capture_skits(&mut audio, &skits, &mut snapshot, &cancelled)?;
        let mut identity = Identity::capture(&mut snapshot, &cancelled)?;
        if let Some(scripts) = &cache.scripts {
            identity = scripts.capture()?.bind_identity(&identity)?;
        }
        let fields = snapshot
            .mappings(FIELDS)?
            .iter()
            .map(|(id, _)| id)
            .chain([3000])
            .collect();
        let files = Arc::new(
            Files::from_snapshot(snapshot, &mut cache.bytes, &cancelled, diagnostics)?
                .with_identity(identity)?,
        );
        let world = Arc::new(game::Prepared::from_prepared(
            package, files, skits, fields,
        )?);
        anyhow::ensure!(!cancelled(), "world preparation cancelled");
        Ok(Self { world, audio })
    }
}

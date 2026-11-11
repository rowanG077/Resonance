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
        if let Some(world) = cache.common.get().and_then(|common| common.world.as_ref()) {
            anyhow::ensure!(!cancelled(), "world preparation cancelled");
            return Ok(world.clone());
        }
        let mut snapshot = cache.snapshot(library)?;
        let mut inputs =
            Inputs::capture(&mut snapshot, &mut cache.audio, &cancelled, &diagnostics)?;
        preparation::gameplay(&mut snapshot, &diagnostics, &cancelled)?;
        preparation::ui(&mut snapshot, &diagnostics, &cancelled)?;
        let skits = Arc::new(skit::document::capture(&mut snapshot, &cancelled)?);
        let programs = resonance_game::screen_resources::capture(
            &mut snapshot,
            skits.resources.values().map(|skit| skit.script.as_str()),
            &diagnostics,
            &cancelled,
        )?;
        match cache.common.get() {
            Some(common) => Arc::make_mut(&mut inputs.audio).use_skits(&common.skit_voices),
            None => {
                Arc::make_mut(&mut inputs.audio).capture_skits(&skits, &mut snapshot, &cancelled)?
            }
        }
        let files = Arc::new(Files::from_snapshot(snapshot, &cancelled, diagnostics)?);
        inputs.prepare(files, skits, programs, &cancelled)
    }
}

/// World-only inputs can load alongside common gameplay, UI and skit inputs.
/// Assembly consumes their completed snapshot, without capturing common data again.
pub(crate) struct Inputs {
    package: resonance_content::overworld::Package,
    audio: Arc<crate::field_audio::Assets>,
    programs: resonance_game::screen_resources::Programs,
}

impl Inputs {
    pub(crate) fn capture(
        snapshot: &mut resonance_content::library::Snapshot,
        audio: &mut crate::field_audio::Cache,
        cancelled: &impl Fn() -> bool,
        diagnostics: &Diagnostics,
    ) -> Result<Self> {
        let (definition, package) = document::capture(snapshot, cancelled)?;
        let programs = resonance_game::screen_resources::capture(
            snapshot,
            std::iter::once(package.script.as_str()),
            diagnostics,
            cancelled,
        )?;
        let choices = definition.audio.resolve(snapshot)?;
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
        let environment = definition.audio.environment(snapshot)?;
        let audio = audio.capture_resolved(
            &environment,
            choices,
            snapshot,
            diagnostics.clone(),
            cancelled,
        )?;
        anyhow::ensure!(!cancelled(), "world preparation cancelled");
        Ok(Self {
            package,
            audio,
            programs,
        })
    }

    pub(crate) fn prepare(
        self,
        files: Arc<Files>,
        skits: Arc<skit::SkitCatalog>,
        mut programs: resonance_game::screen_resources::Programs,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Package> {
        anyhow::ensure!(!cancelled(), "world preparation cancelled");
        programs.extend(self.programs);
        let fields = files
            .json::<resonance_content::library::Mappings>(FIELDS)?
            .iter()
            .map(|(id, _)| id)
            .chain([3000])
            .collect();
        let world = Arc::new(game::Prepared::from_prepared(
            self.package,
            files,
            skits,
            fields,
            &programs,
        )?);
        anyhow::ensure!(!cancelled(), "world preparation cancelled");
        Ok(Package {
            world,
            audio: self.audio,
        })
    }
}

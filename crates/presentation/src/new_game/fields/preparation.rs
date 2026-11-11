//! The checkpoint's independent CPU branches over one captured namespace.
use super::*;
use resonance_content::{diagnostics::Diagnostics, field::document, library::Snapshot};

pub(super) struct Captured {
    pub snapshot: Snapshot,
    pub definition: document::Definition,
    pub assets: FieldAssets,
    pub audio: Arc<crate::field_audio::Assets>,
    pub program: resonance_game::field::PreparedProgram,
}

pub(super) fn capture(
    mut snapshot: Snapshot,
    map: u32,
    common: &crate::loading::startup::Common,
    audio_cache: &mut crate::field_audio::Cache,
    diagnostics: &Diagnostics,
    cancelled: &(impl Fn() -> bool + Sync),
    captured: &mut dyn FnMut(&Snapshot, &FieldAssets) -> Result<()>,
) -> Result<Captured> {
    let path = snapshot.field(map)?;
    let definition: document::Definition = snapshot.json(&path, 1024 * 1024)?;
    definition.validate()?;
    let capture = snapshot.concurrent();
    let mut scene_view = capture.view();
    let (scene, audio, program) = std::thread::scope(|scope| -> Result<_> {
        let mut audio_view = capture.view();
        let mut screen_view = capture.view();
        let audio_definition = &definition.audio;
        let script = &definition.script;
        let audio = scope.spawn(move || -> Result<_> {
            let mut audio = audio_cache.capture_named(
                audio_definition,
                &mut audio_view,
                diagnostics.clone(),
                cancelled,
            )?;
            Arc::make_mut(&mut audio).use_skits(&common.skit_voices);
            Ok(audio)
        });
        let screen = scope.spawn(move || -> Result<_> {
            Ok(resonance_game::field::PreparedProgram::new(
                resonance_game::screen_resources::capture_program(
                    &mut screen_view,
                    script,
                    diagnostics,
                    cancelled,
                )?,
            ))
        });
        let scene = definition.capture_scene(
            &mut scene_view,
            map,
            cancelled,
            common.field.as_ref(),
            captured,
        );
        // All workers retire even on error. Keep the checkpoint's scene/screen/audio error priority.
        let screen = screen
            .join()
            .map_err(|_| anyhow::anyhow!("field screen preparation panicked"));
        let audio = audio
            .join()
            .map_err(|_| anyhow::anyhow!("field audio preparation panicked"));
        let scene = scene?;
        let program = screen??;
        Ok((scene, audio??, program))
    })?;
    drop(scene_view);
    let snapshot = capture.finish()?;
    ensure!(!cancelled(), "field preparation cancelled");
    Ok(Captured {
        snapshot,
        definition,
        assets: scene,
        audio,
        program,
    })
}

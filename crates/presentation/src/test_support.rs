//! Helpers shared by presentation tests.

pub(crate) fn run_options(assets: impl Into<std::path::PathBuf>) -> crate::RunOptions {
    crate::RunOptions {
        assets: assets.into(),
        script_root: None,
        saves: Default::default(),
        presentation_start: None,
        replay: None,
        capture_at: None,
        capture: None,
        reveal: false,
        selected: 0,
        silent: true,
        paranoid: true,
        skip_intro: true,
        skip_battles: false,
        allow_incomplete_scripts: false,
        record_playthrough: None,
        record_title_ticks: 0,
    }
}

/// A save beside Iselia's save point, after the classroom sequence.
/// Host tests restore it directly; the game integration test owns story playback.
pub(crate) fn field_checkpoint(
    files: &resonance_content::prepared::Files,
) -> anyhow::Result<resonance_game::field::FieldCheckpoint> {
    use resonance_events::{
        SavedProgress,
        camera::{CameraRig, EntryCamera},
        party::Party,
    };
    let data = resonance_content::session::SessionData::load(files)?;
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![1, 2, 3];
    let leader = i32::from(party.field_leader);
    let mut progress = SavedProgress::new(party);
    // The field content stores its story counter in global word 16.
    progress.script_globals[16] = 2500;
    progress.event_flags.extend([31, 520]);
    let mut camera = CameraRig::default();
    *camera.current_mut() = EntryCamera::following(leader).camera;
    Ok(resonance_game::field::FieldCheckpoint {
        map_id: 332,
        position: [1968., 1005., 0.],
        heading: 276.,
        camera: Some(camera.settings(leader).map_err(anyhow::Error::msg)?),
        allow_incomplete_scripts: false,
        progress,
        played_ticks: 0,
    })
}

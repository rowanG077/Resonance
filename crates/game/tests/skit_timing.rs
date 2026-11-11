//! Source media request boundaries, independent of rendering and audio devices.
mod common;
use anyhow::Result;
use resonance_content::{prepared::Files, skit};
use resonance_events::{EventRuntime, ResourceLibrary};
use resonance_game::skit::{Input, Playback, Prepared};
use std::sync::Arc;

#[test]
#[ignore = "requires original cooked skits; source observations from the GQSEAF oracle fixtures"]
fn original_skit_media_requests_match_resident_playback_timing() -> Result<()> {
    let mut snapshot = common::library().snapshot();
    let catalog = Arc::new(skit::document::capture(&mut snapshot, &|| false)?);
    let files = Files::capture_snapshot(
        &snapshot,
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let resources = Arc::new(ResourceLibrary::default());
    let prepared = Prepared::load(catalog, &files, &resources, &Default::default())?;
    let end: Vec<_> = [4u16, 0, 0, 0, 0x20ff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let program = Arc::new(symphonia_script::Program::decode(&end)?);
    // Observed (request, ready) scene ticks from PlaySkit(id, 1, 1) captures.
    // Resident preparation takes ten updates. Exclude only measured extra
    // loading time from subsequent requests; retain playback timing errors.
    // Align the first request to exclude the preceding field->skit transition.
    for (id, source) in [
        (
            1,
            &[
                (31u32, 41u32),
                (233, 243),
                (475, 485),
                (901, 911),
                (1111, 1121),
                (1346, 1356),
                (1573, 1583),
            ][..],
        ),
        (
            8,
            &[
                (30, 43),
                (238, 248),
                (337, 347),
                (588, 598),
                (773, 783),
                (1136, 1146),
                (1233, 1243),
                (1438, 1448),
                (1591, 1601),
            ][..],
        ),
        (
            618,
            &[
                (34, 48),
                (85, 95),
                (171, 181),
                (424, 434),
                (492, 502),
                (891, 901),
                (1008, 1018),
            ][..],
        ),
    ] {
        let mut parent = EventRuntime::new(program.clone(), resources.clone())?;
        let mut playback = Playback::start(&prepared[&id], &mut parent, false, true, None)?;
        let mut requests = Vec::new();
        let mut previous = None;
        let mut complete = false;
        for update in 0..=3600 {
            complete = update != 0 && playback.step(&mut parent, Input::default())?;
            if let Some(media) = &playback.events.world.skit.as_ref().unwrap().media {
                let current = (media.id, media.started);
                if previous != Some(current) {
                    requests.push(media.started);
                    previous = Some(current);
                }
            }
            parent.world.audio_commands.clear();
            if complete {
                break;
            }
        }
        assert!(complete, "skit {id} did not finish");
        assert_eq!(requests.len(), source.len(), "skit {id} request count");
        let mut loading_delay = 0;
        let mut errors = Vec::new();
        for (&native, &(original, ready)) in requests.iter().zip(source) {
            let source_elapsed = (original - source[0].0) as i32 - loading_delay;
            errors.push((native - requests[0]) as i32 - source_elapsed);
            loading_delay += (ready - original) as i32 - 10;
        }
        assert!(
            errors.iter().all(|error| error.abs() <= 6),
            "skit {id}: {errors:?}"
        );
        eprintln!(
            "skit {id} resident timing errors: {errors:?}; excluded {loading_delay} loading updates"
        );
    }
    Ok(())
}

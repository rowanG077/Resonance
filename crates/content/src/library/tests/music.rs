use super::*;
use std::io::{Seek, SeekFrom};

#[test]
fn shared_music_capture_pins_mod_streams_and_reads_only_small_playback_metadata() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write(
        "base/audio/music/mappings.json",
        br#"{"1":"shared-song","2":"shared-song"}"#,
    );
    fixture.write("base/audio/music/shared-song/music.json", br#"{"version":1,"audio":"audio/music/shared-song/music.wav","loop":{"start_frame":3,"end_frame":10}}"#);
    fixture.write("base/audio/music/shared-song/music.wav", b"base recording");
    fixture.write(
        "mod/audio/music/shared-song/music.wav",
        b"first mod recording",
    );
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let first = snapshot.resolve(crate::music::MAPPINGS, 1, Some("music.json"))?;
    let other = snapshot.resolve(crate::music::MAPPINGS, 2, Some("music.json"))?;
    assert_eq!(first, other);
    let mut captured = crate::music::capture(&mut snapshot, &first, &|| false)?;
    assert!(snapshot.digest(&captured.definition.audio).is_err());
    fixture.write("mod/replacement.wav", b"second mod recording");
    fs::rename(
        fixture.0.join("mod/replacement.wav"),
        fixture.0.join("mod/audio/music/shared-song/music.wav"),
    )?;
    let mut repeated = crate::music::capture(&mut snapshot, &other, &|| false)?;
    let mut bytes = Vec::new();
    captured.audio.seek(SeekFrom::Start(0))?;
    captured.audio.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"first mod recording");
    bytes.clear();
    repeated.audio.seek(SeekFrom::Start(0))?;
    repeated.audio.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"first mod recording");
    bytes.clear();
    crate::music::capture(&mut library.snapshot(), &first, &|| false)?
        .audio
        .read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"second mod recording");
    assert!(crate::music::capture(&mut snapshot, &first, &|| true).is_err());
    fixture.write(
        "mod/audio/music/shared-song/music.json",
        br#"{"version":1,"audio":"audio/music/shared-song/music.wav","frames":123}"#,
    );
    assert!(crate::music::capture(&mut library.snapshot(), &first, &|| false).is_err());
    assert!(crate::music::capture(&mut snapshot, &first, &|| false).is_ok());
    Ok(())
}

#[test]
fn intermediate_runtime_views_keep_the_same_snapshot_while_dependencies_expand() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/battle/rules.json", b"first rules");
    fixture.write("base/battle/cue.json", b"selected cue");
    let library = fixture.open(&[])?;
    let mut snapshot = library.snapshot();
    snapshot.read("battle/rules.json", 128)?;
    let mut cache = crate::prepared::Cache::default();
    let first = crate::prepared::Files::capture_snapshot(
        &snapshot,
        &mut cache,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    fixture.write("base/battle/rules.json", b"next generation");
    snapshot.read("battle/cue.json", 128)?;
    let final_view = crate::prepared::Files::from_snapshot(
        snapshot,
        &mut cache,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    assert!(!first.contains_key("battle/cue.json"));
    assert_eq!(&*final_view.read("battle/cue.json")?, b"selected cue");
    assert_eq!(&*final_view.read("battle/rules.json")?, b"first rules");
    assert!(Arc::ptr_eq(
        &first.read("battle/rules.json")?,
        &final_view.read("battle/rules.json")?
    ));
    assert_eq!(
        &*library.snapshot().read("battle/rules.json", 128)?,
        b"next generation"
    );
    Ok(())
}

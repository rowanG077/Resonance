use super::*;
use std::io::{Seek, SeekFrom};

#[test]
fn shared_music_capture_pins_mod_streams_and_reads_only_small_playback_metadata() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write(
        "base/audio/music/mappings.json",
        br#"{"1":"shared-song.wav","2":"shared-song.wav"}"#,
    );
    fixture.write("base/audio/music/shared-song.wav", b"base recording");
    fixture.write("mod/audio/music/shared-song.wav", b"first mod recording");
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let first = snapshot.resolve(crate::music::MAPPINGS, 1, None)?;
    let other = snapshot.resolve(crate::music::MAPPINGS, 2, None)?;
    assert_eq!(first, other);
    let mut captured = crate::music::capture(&mut snapshot, &first, &|| false)?;
    assert!(!snapshot.contains_key(&first));
    fixture.write("mod/replacement.wav", b"second mod recording");
    fs::rename(
        fixture.0.path().join("mod/replacement.wav"),
        fixture.0.path().join("mod/audio/music/shared-song.wav"),
    )?;
    let mut repeated = crate::music::capture(&mut snapshot, &other, &|| false)?;
    let mut bytes = Vec::new();
    captured.seek(SeekFrom::Start(0))?;
    captured.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"first mod recording");
    bytes.clear();
    repeated.seek(SeekFrom::Start(0))?;
    repeated.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"first mod recording");
    bytes.clear();
    crate::music::capture(&mut library.snapshot(), &first, &|| false)?.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"second mod recording");
    assert!(crate::music::capture(&mut snapshot, &first, &|| true).is_err());
    assert!(
        crate::music::capture(
            &mut library.snapshot(),
            "audio/music/shared-song.json",
            &|| false
        )
        .is_err()
    );
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

    let first = crate::prepared::Files::capture_snapshot(
        &snapshot,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    fixture.write("base/battle/rules.json", b"next generation");
    snapshot.read("battle/cue.json", 128)?;
    let final_view = crate::prepared::Files::from_snapshot(
        snapshot,
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

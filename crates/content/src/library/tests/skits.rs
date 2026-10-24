use super::*;
use crate::skit::document;

fn wave(rate: u32, frames: u32) -> Result<Vec<u8>> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..frames {
        writer.write_sample(123i16)?;
    }
    writer.finalize()?;
    Ok(bytes.into_inner())
}

fn fixture() -> Result<Fixture> {
    let fixture = Fixture::new();
    fixture.write("base/skits/catalogue.json", &serde_json::to_vec(&serde_json::json!({
        "version": 1, "skits": [], "preview_order": [1], "portraits": {}, "portrait_recipes": [],
        "media": {
            "100": {"kind": "speech"},
            "101": {"kind": "speech", "sample_rate": 32028},
            "102": {"kind": "silence", "frames": 1000, "sample_rate": 32000}
        }
    }))?);
    fixture.write("base/skits/mappings.json", br#"{"1":"arrival"}"#);
    fixture.write("base/skits/arrival/skit.json", br#"{
        "script":"skits/arrival/scenario.ssb", "messages":"skits/arrival/messages.json", "allow_skip":false
    }"#);
    fixture.write("base/skits/arrival/scenario.ssb", b"scenario");
    fixture.write("base/skits/arrival/messages.json", b"[]");
    fixture.write(
        "base/audio/speech/skits/mappings.json",
        br#"{"100":"line.wav","101":"line.wav"}"#,
    );
    fixture.write("base/audio/speech/skits/line.wav", &wave(32000, 16)?);
    Ok(fixture)
}

#[test]
fn skit_media_uses_current_pcm_and_merged_mappings_with_explicit_silent_clocks() -> Result<()> {
    let fixture = fixture()?;
    fixture.write(
        "mod/audio/speech/skits/mappings.json",
        br#"{"100":"replacement.wav"}"#,
    );
    fixture.write("mod/audio/speech/skits/replacement.wav", &wave(24000, 32)?);
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let captured = document::capture(&mut snapshot, &|| false)?;
    assert_eq!(
        (
            captured.media[&100].frames,
            captured.media[&100].sample_rate
        ),
        (32, 24000)
    );
    assert_eq!(
        (
            captured.media[&101].frames,
            captured.media[&101].sample_rate
        ),
        (16, 32028)
    );
    assert_eq!(
        captured.media[&101]
            .voice
            .as_ref()
            .unwrap()
            .source_sample_rate,
        32000
    );
    assert_eq!(
        (
            captured.media[&102].frames,
            captured.media[&102].sample_rate
        ),
        (1000, 32000)
    );
    assert!(captured.media[&102].voice.is_none());
    assert!(!captured.resources[&1].allow_skip);
    fixture.write("mod/audio/speech/skits/replacement.wav", &wave(48000, 64)?);
    let held = document::capture(&mut snapshot, &|| false)?;
    assert_eq!(held.media[&100].frames, 32);
    let fresh = document::capture(&mut library.snapshot(), &|| false)?;
    assert_eq!(
        (fresh.media[&100].frames, fresh.media[&100].sample_rate),
        (64, 48000)
    );
    assert_ne!(
        fresh.media[&100].voice.as_ref().unwrap().asset.sha256,
        held.media[&100].voice.as_ref().unwrap().asset.sha256
    );
    let files = crate::prepared::Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    let resident = document::read(|path, _| files.read(path))?;
    assert_eq!(resident.media[&100].frames, 32);
    assert!(files.keys().all(|path| !path.ends_with(".preload.json")));
    Ok(())
}

#[test]
fn invalid_skit_replacements_and_cancellation_do_not_reuse_old_media() -> Result<()> {
    let fixture = fixture()?;
    let library = fixture.open(&[])?;
    assert!(document::capture(&mut library.snapshot(), &|| true).is_err());
    fixture.write("base/audio/speech/skits/line.wav", b"invalid WAV");
    let error = format!(
        "{:#}",
        document::capture(&mut library.snapshot(), &|| false).unwrap_err()
    );
    assert!(
        error.contains("decode skit speech 100: audio/speech/skits/line.wav"),
        "{error}"
    );
    Ok(())
}

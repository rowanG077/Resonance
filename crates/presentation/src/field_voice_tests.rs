use super::*;

#[test]
#[ignore = "requires cooked voices and pinned muted Dolphin recordings; no audio device"]
fn saved_dialogue_volume_matches_dolphin_attenuation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let case: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("tools/oracle/cases/customize-voice-audio.json")).unwrap(),
    )
    .unwrap();
    let number = |key: &str| case[key].as_u64().unwrap() as usize;
    let start = number("reference_start_frame");
    let count = number("frames");
    let read = |key: &str| {
        let bytes = fs::read(root.join(case[key]["path"].as_str().unwrap())).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case[key]["sha256"].as_str().unwrap()
        );
        let mut wave = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        assert_eq!((wave.spec().sample_rate, wave.spec().channels), (RATE, 2));
        assert!(wave.duration() as usize >= start + count);
        wave.seek(start as u32).unwrap();
        wave.samples::<i16>()
            .take(count * 2)
            .map(|v| f64::from(v.unwrap()))
            .collect::<Vec<_>>()
    };
    let baseline = read("baseline");
    let isolated = |key: &str| {
        read(key)
            .iter()
            .zip(&baseline)
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>()
    };
    let reference = isolated("reference");
    let reference_full = isolated("full_volume");
    let assets = Assets::load(&root.join("local/cooked")).unwrap();
    let render = |volume| {
        let (source, mut control) = assets.clone().session();
        control.levels([127, 127, volume]).unwrap();
        control
            .send(AudioCommand::Voice(number("voice") as u32))
            .unwrap();
        let mut frames = source.decoder();
        let samples = (0..count)
            .flat_map(|_| {
                frames
                    .frame()
                    .unwrap()
                    .unwrap()
                    .map(|v| f64::from(v) * 32768.)
            })
            .collect::<Vec<_>>();
        control.check().unwrap();
        samples
    };
    let volume = number("volume") as u8;
    let actual = render(volume);
    let actual_full = render(127);
    assert!(render(0).iter().all(|&v| v == 0.));
    let power = |samples: &[f64]| samples.iter().map(|v| v * v).sum::<f64>();
    let mut checked = 0;
    let mut maximum_error = 0f64;
    for at in (0..count * 2).step_by(number("window_frames") * 2) {
        let range = at..(at + number("window_frames") * 2).min(count * 2);
        let full = power(&reference_full[range.clone()]);
        if (full / range.len() as f64).sqrt() < case["minimum_reference_rms"].as_f64().unwrap() {
            continue;
        }
        let expected = power(&reference[range.clone()]) / full;
        let measured = power(&actual[range.clone()]) / power(&actual_full[range]);
        let error_db = 10. * (measured / expected).log10();
        maximum_error = maximum_error.max(error_db.abs());
        assert!(
            error_db.abs() <= case["maximum_level_error_db"].as_f64().unwrap(),
            "voice volume at PCM frame {}: {error_db} dB",
            at / 2
        );
        // The former linear slider must fail this same independently measured ratio.
        let old_gain = f64::from(volume) / 127.;
        assert!(10. * (old_gain * old_gain / expected).log10() > 7.);
        checked += 1;
    }
    assert!(checked >= 8, "insufficient voiced coverage");
    eprintln!(
        "Dialogue volume: {checked} windows, maximum error {maximum_error:.6} dB; linear-gain negative control rejected"
    );
}

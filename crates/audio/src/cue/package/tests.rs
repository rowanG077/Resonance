use super::*;
use crate::cue::Studio;
use crate::data::{Command, Interpolation};
use std::fs;

#[test]
fn program_cues_use_stream_completion_and_validate_resource_integrity() {
    let (root, program) = crate::cue::tests::fixture(vec![
        Command::Interpolation {
            mode: Interpolation::Direct,
            coefficients: 0,
        },
        Command::StartSample { sample: 2 },
        Command::Wait {
            milliseconds: Some(20),
            from_start: false,
            key_off: false,
            sample_end: false,
        },
        Command::StopSample,
        Command::End,
    ]);
    let bytes = serde_json::to_vec(&program).unwrap();
    fs::write(root.0.join("program.json"), &bytes).unwrap();
    let mut manifest = Manifest {
        version: VERSION,
        sample_rate: 32028,
        reverbs: program.reverbs,
        cues: BTreeMap::from([(
            "navigate".into(),
            Asset {
                program: Program {
                    path: "program.json".into(),
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                },
            },
        )]),
    };
    let save = |manifest: &Manifest| {
        let bytes = serde_json::to_vec(manifest).unwrap();
        fs::write(root.0.join("bank.json"), &bytes).unwrap();
        format!("{:x}", Sha256::digest(bytes))
    };
    let loaded = Manifest::load(&root.0, "bank.json", &save(&manifest), |_, error| {
        Err(error)
    })
    .unwrap();
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    studio.play(loaded.cues["navigate"].clone()).unwrap();
    let mut output = Vec::new();
    while !studio.voices.is_empty() && output.len() < 2000 {
        output.push(studio.next_frame(Err).unwrap());
    }
    let middle = crate::volume::frames_from_millis(10).unwrap() as usize;
    assert!(output.get(middle).is_some_and(|frame| *frame != [0; 2]));
    assert!(studio.voices.is_empty(), "finite program did not complete");
    assert!(Manifest::load(&root.0, "bank.json", "incorrect", |_, error| Err(error)).is_err());
    manifest.version = VERSION - 1;
    assert!(
        Manifest::load(&root.0, "bank.json", &save(&manifest), |_, error| Err(
            error
        ))
        .is_err()
    );
    manifest.version = VERSION;
    fs::write(root.0.join("program.json"), b"damaged").unwrap();
    assert!(
        Manifest::load(&root.0, "bank.json", &save(&manifest), |_, error| Err(
            error
        ))
        .is_err()
    );
    fs::write(root.0.join("program.json"), &bytes).unwrap();
    fs::write(root.0.join("sample.wav"), b"damaged").unwrap();
    assert!(
        Manifest::load(&root.0, "bank.json", &save(&manifest), |_, error| Err(
            error
        ))
        .is_err()
    );
}

#[test]
fn rejected_programs_leave_healthy_cues_playable() {
    let (root, mut program) = crate::cue::tests::fixture(vec![
        Command::Interpolation {
            mode: Interpolation::Direct,
            coefficients: 0,
        },
        Command::StartSample { sample: 2 },
        Command::End,
    ]);
    let sample = program.samples.get_mut(&2).unwrap();
    sample.first_frames = 6;
    sample.loop_start = 0;
    sample.loop_length = 0;
    let bytes = serde_json::to_vec(&program).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    fs::write(root.0.join("program.json"), &bytes).unwrap();
    fs::write(root.0.join("damaged.json"), b"damaged").unwrap();
    fs::write(root.0.join("invalid.json"), b"{}").unwrap();
    let broken = [
        ("../program.json", digest.clone()),
        ("absent.json", digest.clone()),
        ("damaged.json", digest.clone()),
        ("invalid.json", format!("{:x}", Sha256::digest(b"{}"))),
    ]
    .map(|(path, sha256)| serde_json::json!({"program": {"path": path, "sha256": sha256}}));
    for asset in broken.into_iter().chain([
        serde_json::json!(false),
        serde_json::json!({"program": false}),
        serde_json::json!({"program": {"path": 1, "sha256": digest}}),
        serde_json::json!({"program": {"path": "program.json"}}),
        serde_json::json!({"program": {"path": "program.json", "sha256": digest}, "unknown": 1}),
    ]) {
        let manifest = Manifest {
            version: VERSION,
            sample_rate: crate::SOURCE_RATE,
            reverbs: program.reverbs,
            cues: [
                ("broken".into(), asset),
                (
                    "navigate".into(),
                    serde_json::json!({"program": {"path": "program.json", "sha256": digest}}),
                ),
            ]
            .into(),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let manifest_digest = format!("{:x}", Sha256::digest(&bytes));
        fs::write(root.0.join("bank.json"), bytes).unwrap();
        let mut rejected = Vec::new();
        let loaded = Manifest::load(&root.0, "bank.json", &manifest_digest, |name, error| {
            rejected.push((name.to_owned(), error.to_string()));
            Ok(())
        })
        .unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].0, "broken");
        assert_eq!(
            loaded.cues.keys().map(String::as_str).collect::<Vec<_>>(),
            ["navigate"]
        );
        let mut studio = Studio::new(loaded.reverbs).unwrap();
        studio.play(loaded.cues["navigate"].clone()).unwrap();
        assert!((0..160).any(|_| studio.next_frame(Err).unwrap() != [0; 2]));
        let error = Manifest::load(&root.0, "bank.json", &manifest_digest, |_, error| {
            Err(error)
        })
        .err()
        .expect("strict loading accepted a broken program");
        assert_eq!(error.to_string(), rejected[0].1);
    }
}

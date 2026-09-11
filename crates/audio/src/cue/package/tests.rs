use super::*;
use std::fs;

struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preserves_envelope_pcm_and_rejects_damaged_controls_and_samples() {
    let root = Directory(
        std::env::temp_dir().join(format!("resonance-cue-package-{}", std::process::id())),
    );
    fs::create_dir(&root.0).unwrap();
    let mut wave = hound::WavWriter::create(
        root.0.join("sample.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: 32028,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for sample in [1000i16, 0, -1000] {
        wave.write_sample(sample).unwrap();
    }
    wave.finalize().unwrap();
    let mut manifest = Manifest {
        version: VERSION,
        sample_rate: 32028,
        reverbs: [[0., 0., 1., 0., 0.]; 2],
        tables: Tables {
            volume: std::array::from_fn(|i| i as f32 / 128.),
            alternate_volume: [0.; 129],
            pan: [1.; 4],
            volume_16_scale: 1. / (127. * 65536.),
            controller_14_scale: 1. / 16383.,
            pan_16_scale: 1. / (63. * 65536.),
        },
        cues: BTreeMap::from([(
            "navigate".into(),
            Asset {
                frames: 3,
                sample: Some(Sample {
                    path: "sample.wav".into(),
                    sha256: format!(
                        "{:x}",
                        Sha256::digest(fs::read(root.0.join("sample.wav")).unwrap())
                    ),
                }),
                program: None,
                controls: vec![Control {
                    volume: 127 << 16,
                    controller: 16383,
                    pan: 64,
                    post: [0; 2],
                }],
            },
        )]),
    };
    let save = |manifest: &Manifest| {
        let bytes = serde_json::to_vec(manifest).unwrap();
        fs::write(root.0.join("bank.json"), &bytes).unwrap();
        format!("{:x}", Sha256::digest(bytes))
    };
    let hash = save(&manifest);
    let loaded = Manifest::load(&root.0, "bank.json", &hash).unwrap();
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    studio.set_group_volume(0.5).unwrap();
    studio.play(loaded.cues["navigate"].clone()).unwrap();
    assert_eq!(studio.next_frame(), [496; 2]);
    assert_eq!(studio.next_frame(), [0; 2]);
    assert_eq!(studio.next_frame(), [-497; 2]);
    assert!(Manifest::load(&root.0, "bank.json", "incorrect").is_err());
    manifest.cues.get_mut("navigate").unwrap().frames = 4;
    assert!(Manifest::load(&root.0, "bank.json", &save(&manifest)).is_err());
    manifest.cues.get_mut("navigate").unwrap().frames = 3;
    let control = manifest
        .cues
        .get_mut("navigate")
        .unwrap()
        .controls
        .pop()
        .unwrap();
    assert!(Manifest::load(&root.0, "bank.json", &save(&manifest)).is_err());
    manifest
        .cues
        .get_mut("navigate")
        .unwrap()
        .controls
        .push(control);
    fs::write(root.0.join("sample.wav"), [0; 16]).unwrap();
    assert!(Manifest::load(&root.0, "bank.json", &save(&manifest)).is_err());
}

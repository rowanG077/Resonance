use super::*;
use crate::digest;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    fn write(&self, path: &str, bytes: &[u8]) -> String {
        write_atomic(&self.0.join(path), bytes).unwrap();
        digest(bytes)
    }
    fn json(&self, path: &str, value: &Value) -> String {
        self.write(path, &serde_json::to_vec(value).unwrap())
    }
    fn inputs(&self) -> Inputs {
        Inputs {
            field: "fields/test.json".into(),
            audio: Default::default(),
            movies: Default::default(),
        }
    }
}

fn script() -> Vec<u8> {
    let bytes = scenario::assemble(
        r".scenario
.code_base 4
.word 4
.word 0
.word 0
.word 0
entry:
    push.s8 0
    calc 0
    branch_false alternate
    proc 0x10
    call helper
    jump finish
alternate:
    proc 0xD3
    call helper
finish:
    end
helper:
    proc 0x9B
    branch_false helper
    ret
registered_only:
    proc 0x56
    end
",
    )
    .unwrap();
    let code = &bytes[8..];
    // Add an event root pointing at the last two words, without changing code PCs.
    let mut result: Vec<_> = [10u16, 0, 0, 1]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    result.extend(
        [1u32, 77, (code.len() / 2 - 2) as u32]
            .into_iter()
            .flat_map(u32::to_be_bytes),
    );
    result.extend(code);
    result
}

fn fixture() -> Fixture {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "resonance-field-preload-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&fixture.0).unwrap();
    let mut files = BTreeMap::new();
    for (path, bytes) in [
        ("fields/test/events.ssb", script()),
        ("fields/test/messages.json", b"[]".to_vec()),
        ("fields/test/room.glb", b"fixture mesh".to_vec()),
        ("fields/test/optional.glb", b"hidden actor mesh".to_vec()),
        ("textures/shared.ktx2", b"fixture texture".to_vec()),
    ] {
        files.insert(path.to_string(), fixture.write(path, &bytes));
    }
    let hash = "0".repeat(64);
    let ui_texture = json!({"path":"textures/shared.ktx2", "width":16, "height":16});
    for (path, value) in [
        (
            "ui/dialogue.json",
            json!({"version":2,"font":"ui/font.json",
            "textures":vec![ui_texture.clone();9], "cursor":ui_texture,
            "selection":{"mode":0,"color":vec![255;4],"row_offsets":vec![0;9],"bob_amplitude":0.,"bob_step":0.},
            "source_sha256":hash}),
        ),
        (
            "ui/font.json",
            json!({"version":1,"texture":"textures/shared.ktx2",
            "width":16,"height":16,"line_height":8,"glyphs":{"A":{"rect":[0,0,1,1],"advance":1}},
            "source_sha256":hash,"executable_sha256":hash}),
        ),
        (
            "effects/test.json",
            json!({"version":1,"dust_texture":"textures/shared.ktx2",
            "emote_texture":"textures/shared.ktx2","dust_uv":[0.,0.,1.,1.],"emotes":{},"mouth_cycle":[0]}),
        ),
    ] {
        files.insert(path.into(), fixture.json(path, &value));
    }
    let part = json!({"resource":0,"mesh":"fields/test/room.glb","textures":["textures/shared.ktx2"],
        "materials":[{"color":{"texture":0,"wrap_u":"repeat","wrap_v":"clamp","nearest_min":false,"nearest_mag":true},
            "multiply":null,"blend":false,"depth_write":true,"draw_order":0}],
        "translation":[0.,0.,0.],"clips":[{"resource_slot":12,"duration_seconds":1.},{"resource_slot":13,"duration_seconds":1.}],
        "autoplay":false,"texture_animations":[],"bone_names":["root"]});
    let mut actor_part = part.clone();
    actor_part["mesh"] = json!("fields/test/optional.glb");
    let field = json!({"version":5,"map_id":123,"source_sha256":hash,
        "script":{"path":"fields/test/events.ssb","sha256":files["fields/test/events.ssb"]},
        "messages":"fields/test/messages.json", "parts":[part],
        "actors":[{"resource":700,"model_sha256":hash,"animation_sha256":hash,"parts":[actor_part],"hidden_nodes":[0]}],
        "ground":[{"surface":0,"vertices":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],"triangles":[[0,1,2]]}],"regions":[],
        "contact_shadow":{"texture":"textures/shared.ktx2","uv_size":[1.,1.],"half_size":1.,"height_offset":0.,"alpha":255,"anchor_node":0},
        "toon_ramp":"textures/shared.ktx2","effects":"effects/test.json","files":files});
    fixture.json("fields/test.json", &field);
    fixture
}

#[test]
fn both_branches_loops_helpers_and_registered_events_are_analyzed() {
    let script = analyze_script("test.ssb", &script(), &NativeRegistry::gqseaf()).unwrap();
    let calls: Vec<_> = script.native_calls.iter().map(|n| n.opcode).collect();
    assert_eq!(calls, [0x10, 0x56, 0x9b, 0xd3]);
    assert_eq!(script.entry_pcs.len(), 2);
    assert!(script.native_calls.iter().all(|n| n.pcs.len() == 1));
}

#[test]
fn unknown_native_is_retained_and_undecoded_code_is_rejected() {
    let bytes: Vec<_> = [4u16, 0, 0, 0, 0x2005, 0x20ff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let report = analyze_script("unknown.ssb", &bytes, &NativeRegistry::gqseaf()).unwrap();
    assert_eq!(report.native_calls[0].opcode, 5);
    assert_eq!(report.native_calls[0].pcs, [0]);
    let mut invalid = bytes;
    invalid[8..10].copy_from_slice(&0x2105u16.to_be_bytes());
    assert!(analyze_script("invalid.ssb", &invalid, &NativeRegistry::gqseaf()).is_err());
}

#[test]
fn complete_field_includes_hidden_actors_all_clips_and_deduplicates_files() {
    let root = fixture();
    let first = cook(&root.0, root.inputs()).unwrap();
    assert!(first.is_complete());
    assert_eq!(first.scenes.len(), 2);
    assert_eq!(first.scenes[1].actor_resource, Some(700));
    assert_eq!(first.scenes[1].animation_indices, [0, 1]);
    assert_eq!(first.scenes[1].material_indices, [0]);
    assert!(first.features.contains(&Feature::Billboards));
    assert!(first.features.contains(&Feature::Choices));
    assert_eq!(first.files.len(), 9); // One shared texture, despite all its users.
    assert!(first.files.contains_key("fields/test/optional.glb"));
    assert_eq!(
        first.total_file_bytes,
        first.files.values().map(|f| f.bytes).sum::<u64>()
    );
    let output = root.0.join(root.inputs().manifest_path().unwrap());
    let before = fs::read(&output).unwrap();
    cook(&root.0, root.inputs()).unwrap();
    assert_eq!(before, fs::read(output).unwrap()); // No timestamps / self-dependency.
    let roundtrip: Manifest = serde_json::from_slice(&before).unwrap();
    roundtrip.validate().unwrap();
    let mut invalid = roundtrip;
    invalid.total_file_bytes += 1;
    assert!(invalid.validate().is_err());
}

#[test]
fn missing_media_is_explicit_then_closes_when_cooked() {
    let root = fixture();
    let mut inputs = root.inputs();
    inputs.movies.insert("movies/test.json".into());
    inputs.audio.insert("audio/test.json".into());
    let missing = build(&root.0, inputs.clone()).unwrap();
    assert!(!missing.is_complete());
    assert_eq!(missing.missing_inputs.len(), 2);
    let hash = root.write("movies/test.mkv", b"movie fixture");
    root.json("movies/test.json", &json!({"version":1,"path":"movies/test.mkv","sha256":hash,
        "width":640,"height":480,"frames":30,"frame_micros":33333,"sample_rate":32000,"channels":2,"audio_frames":32000}));
    root.json(
        "audio/test.json",
        &json!({"version":1,"music":{},"sounds":{},"voices":{},"recipe":{}}),
    );
    let ready = build(&root.0, inputs).unwrap();
    assert!(ready.is_complete());
    assert!(ready.files["movies/test.mkv"].roles.contains(&Role::Movie));
    assert!(ready.features.contains(&Feature::Audio));
}

fn audio_package(root: &Fixture) -> String {
    use resonance_audio::{
        data::Score,
        dls, mix, modulation,
        music_voice::{Controls, Tables},
        package::{Package, SampleAsset},
        pitch, resample,
    };
    let hash = root.write("audio/sample.wav", b"instrument fixture");
    let package = Package {
        version: resonance_audio::package::VERSION,
        programs: BTreeMap::new(),
        samples: [(
            1,
            SampleAsset {
                path: "audio/sample.wav".into(),
                sha256: hash,
                key: 60,
                rate: 32000,
                first_frames: 1,
                loop_start: 0,
                loop_length: 0,
            },
        )]
        .into(),
        score: Score {
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 0,
            end_tick: 100,
            has_master_track: false,
            tempos: vec![],
            controls: [Controls::default(); 16],
            first_events: vec![],
            loop_events: vec![],
        },
        tables: Tables {
            mix: mix::Tables {
                volume: [1.; 129],
                alternate_volume: [1.; 129],
                pan: [1.; 4],
                volume_16_scale: 1.,
                controller_14_scale: 1.,
                pan_16_scale: 1.,
            },
            pitch: pitch::Tables {
                up: [1.; 128],
                down: [1.; 128],
                semitone: 1.05946,
            },
            dls: dls::Tables {
                attenuation: [0; 194],
                inverse: [0; 1024],
                sustain: [0.; 128],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        },
        reverbs: [[0., 0., 1., 0., 0.]; 2],
    };
    root.json(
        "audio/package.json",
        &serde_json::to_value(package).unwrap(),
    )
}

#[test]
fn audio_closure_includes_samples_shared_packages_and_voices() {
    let root = fixture();
    let package_hash = audio_package(&root);
    let voice_hash = root.write("audio/voice.wav", b"voice fixture");
    root.json("audio/test.json", &json!({"version":1,
        "music":{"7":{"path":"audio/package.json","sha256":package_hash}},
        "sounds":{"80":{"path":"audio/package.json","sha256":package_hash}},
        "voices":{"42":{"path":"audio/voice.wav","sha256":voice_hash,"frames":1,"sample_rate":32000,
            "source_sample_rate":32000,"channels":1,"source_name":"42.ahx","source_sha256":"0".repeat(64)}},"recipe":{}}));
    let mut inputs = root.inputs();
    inputs.audio.insert("audio/test.json".into());
    let manifest = build(&root.0, inputs.clone()).unwrap();
    assert_eq!(manifest.files.len(), 13);
    assert!(
        manifest.files["audio/sample.wav"]
            .roles
            .contains(&Role::InstrumentSample)
    );
    assert!(
        manifest.files["audio/voice.wav"]
            .roles
            .contains(&Role::Voice)
    );
    root.write("audio/sample.wav", b"changed sample");
    assert!(
        build(&root.0, inputs)
            .unwrap_err()
            .to_string()
            .contains("audio/sample.wav")
    );
}

#[test]
fn rejects_stale_inventory_missing_payload_and_unsafe_paths() {
    let root = fixture();
    root.write("textures/shared.ktx2", b"modified");
    assert!(
        build(&root.0, root.inputs())
            .unwrap_err()
            .to_string()
            .contains("hash differs")
    );
    fs::remove_file(root.0.join("fields/test/optional.glb")).unwrap();
    assert!(build(&root.0, root.inputs()).is_err());
    let mut inputs = root.inputs();
    inputs.movies.insert("../outside.json".into());
    assert!(build(&root.0, inputs).is_err());
}

#[test]
fn malformed_existing_media_is_not_treated_as_uncooked() {
    let root = fixture();
    let mut inputs = root.inputs();
    inputs.audio.insert("audio/test.json".into());
    root.write("audio/test.json", b"not json");
    assert!(build(&root.0, inputs).is_err());
}

#[test]
fn recipe_refresh_backfills_both_fields_after_later_media_cooks() {
    let root = fixture();
    let mut field: Value =
        serde_json::from_slice(&fs::read(root.0.join("fields/test.json")).unwrap()).unwrap();
    for (name, id) in [("iselia-classroom", 340), ("new-game-setup", 5)] {
        field["map_id"] = json!(id);
        root.json(&format!("fields/{name}.json"), &field);
    }
    let read = |name| -> Manifest {
        serde_json::from_slice(
            &fs::read(root.0.join(format!("fields/{name}.preload.json"))).unwrap(),
        )
        .unwrap()
    };
    crate::field::refresh_preloads(&root.0).unwrap();
    assert_eq!(read("iselia-classroom").missing_inputs.len(), 1);
    assert_eq!(read("new-game-setup").missing_inputs.len(), 2);
    root.json(
        "fields/iselia-classroom-audio.json",
        &json!({"version":1,"music":{},"sounds":{},"voices":{},"recipe":{}}),
    );
    crate::field::refresh_preloads(&root.0).unwrap();
    assert!(read("iselia-classroom").is_complete());
    assert!(!read("new-game-setup").is_complete());
    let hash = root.write("movies/story.mkv", b"movie fixture");
    root.json("story-intro.json", &json!({"version":1,"path":"movies/story.mkv","sha256":hash,
        "width":640,"height":480,"frames":30,"frame_micros":33333,"sample_rate":32000,"channels":2,"audio_frames":32000}));
    crate::field::refresh_preloads(&root.0).unwrap();
    assert!(read("new-game-setup").is_complete());
    let classroom = read("iselia-classroom");
    assert_eq!(classroom.map_id, 340);
    assert!(!classroom.files.contains_key("movies/story.mkv"));
}

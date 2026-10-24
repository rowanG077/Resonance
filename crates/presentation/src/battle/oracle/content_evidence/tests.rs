use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn pin(root: &Path, path: &str, bytes: &[u8]) -> Result<Value> {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, bytes)?;
    Ok(json!({"path": path, "sha256": format!("{:x}", Sha256::digest(bytes))}))
}

#[test]
fn current_scene_evidence_verifies_documents_and_original_byte_ranges() -> Result<()> {
    let work = tempfile::tempdir()?;
    let root = work.path();
    let documents = [
        (
            "battle/scenes/rain/scene.json",
            json!({"version":1, "rules":"battle/scenes/rain/rules.json",
            "effects":"battle/scenes/rain/effects/effects.json", "cutin":null, "weapon":null}),
        ),
        (
            "battle/scenes/rain/rules.json",
            json!({"version":1, "projectiles":[], "actions":{},
            "actions_error":null, "duration":120, "stage_color":null, "camera":null,
            "strike_points":[], "recovery_radius":null, "spell_recipe":null}),
        ),
        (
            "battle/scenes/rain/effects/effects.json",
            json!({"version":1,
            "programs":"battle/scenes/rain/effects/programs.json",
            "actors":"battle/scenes/rain/effects/actors.json", "art":null}),
        ),
        ("battle/scenes/rain/effects/programs.json", json!([])),
        ("battle/scenes/rain/effects/actors.json", json!([])),
    ];
    let mut files = BTreeMap::new();
    for (path, value) in documents {
        files.insert(
            path.to_owned(),
            pin(root, path, &serde_json::to_vec(&value)?)?,
        );
    }
    let bank = b"ef1\0original effect evidence";
    let package = [b"header".as_slice(), bank.as_slice(), b"tail".as_slice()].concat();
    let source = pin(root, "evidence/package.bin", &package)?;
    let bank_source = pin(root, "evidence/effects.bin", bank)?;
    let effects: BTreeMap<_, _> = files
        .iter()
        .filter(|(path, _)| path.contains("/effects/"))
        .map(|(path, pin)| (path.clone(), pin.clone()))
        .collect();
    let mut registration = json!({
        "package_sha256":source["sha256"], "bank_sha256":bank_source["sha256"],
        "effects_range":[6, 6+bank.len()],
        "scene":{"version":1,"entry":"battle/scenes/rain/scene.json","files":files,"source":source},
        "effects":{"version":1,"entry":"battle/scenes/rain/effects/effects.json",
            "files":effects,"source":bank_source},
    });
    let (scene, effects) = registered_scene(&registration)?;
    assert_eq!(scene.duration, 120);
    assert!(effects.programs.is_empty());
    assert!(
        scene.source_sha256.is_empty(),
        "native byte identity belongs to evidence"
    );

    registration["effects_range"][0] = json!(5);
    assert!(registered_scene(&registration).is_err());
    registration["effects_range"][0] = json!(6);
    fs::write(
        root.join("battle/scenes/rain/effects/programs.json"),
        b"[[]]",
    )?;
    assert!(
        registered_scene(&registration).is_err(),
        "changed nested input must fail its pin"
    );
    fs::write(root.join("battle/scenes/rain/effects/programs.json"), b"[]")?;
    fs::write(root.join("evidence/effects.bin"), b"changed original")?;
    assert!(
        registered_scene(&registration).is_err(),
        "changed original input must fail its pin"
    );
    assert!(
        Evidence::parse(&registration["scene"]["files"]["battle/scenes/rain/scene.json"]).is_err(),
        "an old single-file scene pin must not activate an old cooked reader"
    );
    Ok(())
}

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
            json!({"effects":"battle/scenes/rain/effects/effects.json", "cutin":null, "weapon":null,
            "projectiles":[], "actions":{},
            "actions_error":null, "duration":120, "stage_color":null, "camera":null,
            "strike_points":[], "recovery_radius":null, "spell_recipe":null}),
        ),
        (
            "battle/scenes/rain/effects/effects.json",
            json!({"programs":[]}),
        ),
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
    assert!(serde_json::to_value(&scene)?.get("source_sha256").is_none());

    registration["effects_range"][0] = json!(5);
    assert!(registered_scene(&registration).is_err());
    registration["effects_range"][0] = json!(6);
    fs::write(
        root.join("battle/scenes/rain/effects/effects.json"),
        br#"{"programs":[[]]}"#,
    )?;
    assert!(
        registered_scene(&registration).is_err(),
        "changed effect input must fail its pin"
    );
    fs::write(
        root.join("battle/scenes/rain/effects/effects.json"),
        br#"{"programs":[]}"#,
    )?;
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

#[test]
fn original_template_receipt_is_independent_of_current_order() -> Result<()> {
    let value = json!({"version":1,"entry":"effects.json","files":{"effects.json":{"path":"unused","sha256":"unused"}},
        "source":{"path":"unused","sha256":"unused"},"effect_templates":[[19],[2,7],[0]]});
    let evidence = Evidence::parse(&value)?;
    assert_eq!(evidence.current_template(19)?, 0);
    assert_eq!(evidence.current_template(7)?, 1);
    assert_eq!(evidence.source_template(2)?, 0);
    assert!(evidence.current_template(8).is_err());
    assert!(evidence.source_template(3).is_err());
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS; reads one enemy without cooking"]
fn enemy_evidence_expands_named_attacks_and_projectiles() -> Result<()> {
    use resonance_content::{battle_enemy, library::Library, prepared::Files};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let work = tempfile::tempdir()?;
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = Library::open(Path::new(&root), &[])?.snapshot();
    let entry = snapshot.resolve(battle_enemy::document::MAPPINGS, 0, Some("enemy.json"))?;
    let expected = battle_enemy::document::capture(&mut snapshot, 0, &diagnostics, &|| false)?;
    let captured = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let mut files = BTreeMap::new();
    for (path, bytes) in captured.iter() {
        files.insert(path, pin(work.path(), path, bytes)?);
    }
    // This fixture checks current rule expansion; it does not claim native-byte parity.
    let source = pin(
        work.path(),
        "fixture-source.bin",
        b"enemy expansion fixture",
    )?;
    let evidence = Evidence::parse(&json!({
        "version": 1, "entry": entry, "files": files, "source": source
    }))?;
    let (actual, _) = evidence.enemy(0)?;
    let actual = serde_json::to_value(actual)?;
    assert_eq!(actual, serde_json::to_value(expected.definition)?);
    assert!(
        actual["actions"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| {
                row["execution"]["attack"]["events"]
                    .as_array()
                    .is_some_and(|events| {
                        events
                            .iter()
                            .any(|event| event[1]["projectile"].is_object())
                    })
            })
    );
    assert!(
        evidence.enemy(1).is_err(),
        "enemy ID must match the pinned owner"
    );
    Ok(())
}

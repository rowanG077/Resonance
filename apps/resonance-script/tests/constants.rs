use std::{fs, path::Path, process::Command};

fn run(arguments: &[&str]) -> std::process::Output {
    let result = Command::new(env!("CARGO_BIN_EXE_resonance-script"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

#[test]
fn checker_uses_generated_standard_definitions_and_cooked_overrides() {
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
    let scripts = scripts.to_str().unwrap();
    run(&["check", "--host", "world", scripts, "world::rules"]);
    run(&[
        "check",
        scripts,
        "field::memory",
        "field::treasure",
        "preview::sword_dancer",
    ]);

    // --assets must select editable cooked definitions, rather than Rust defaults.
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let assets = root.path().join("assets");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(assets.join("scripts/std")).unwrap();
    fs::write(
        project.join("custom.sym"),
        "script library; use std::items::ModOnly; pub fn id() -> i32 { return ModOnly; }",
    )
    .unwrap();
    fs::write(
        assets.join("scripts/std/items.sym"),
        "script library; pub const ModOnly: i32 = 60000;",
    )
    .unwrap();
    run(&[
        "check",
        "--assets",
        assets.to_str().unwrap(),
        project.to_str().unwrap(),
        "custom",
    ]);
    fs::remove_file(assets.join("scripts/std/items.sym")).unwrap();
    fs::write(project.join("custom.sym"), "script library; use std::items::ItemId; pub fn id() -> i32 { return i32(ItemId::MemoryGem); }").unwrap();
    let missing = Command::new(env!("CARGO_BIN_EXE_resonance-script"))
        .args([
            "check",
            "--assets",
            assets.to_str().unwrap(),
            project.to_str().unwrap(),
            "custom",
        ])
        .output()
        .unwrap();
    assert!(
        !missing.status.success(),
        "missing cooked std must not use embedded defaults"
    );
}

#[test]
fn node_command_reads_the_edited_model_and_emits_a_compilable_reference() {
    let root = tempfile::tempdir().unwrap();
    let model = root.path().join("body.glb");
    let bytes = resonance_asset_writer::gltf::pack_glb(
        &serde_json::json!({
            "asset":{"version":"2.0"},
            "nodes":[
                {"name":"edited-head","extras":{"resonance_bone":0}},
                {"name":"hand","extras":{"resonance_bone":1}},
                {"name":"hand","extras":{"resonance_bone":2}}
            ]
        }),
        &[],
    )
    .unwrap();
    fs::write(&model, bytes).unwrap();
    let generated = run(&["nodes", model.to_str().unwrap()]);
    let text = String::from_utf8(generated.stdout).unwrap();
    assert!(text.contains("EditedHead: string = \"edited-head\""));
    assert!(text.contains("Hand0: string = \"hand#0\""));
    assert!(text.contains("Hand1: string = \"hand#1\""));
    fs::write(root.path().join("nodes.sym"), text).unwrap();
    run(&["check", root.path().to_str().unwrap(), "nodes"]);
}

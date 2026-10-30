use super::*;
use crate::library::{Library, Mod, Snapshot};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn mesh(names: [&str; 3]) -> Result<Vec<u8>> {
    // Reversed GLB node order demonstrates that these are gameplay joint IDs,
    // not accidental positions in the editor's node array.
    resonance_asset_writer::gltf::pack_glb(
        &json!({
            "asset":{"version":"2.0"}, "nodes":[
                {"name":names[2],"extras":{"resonance_bone":2}},
                {"name":names[1],"extras":{"resonance_bone":1}},
                {"name":names[0],"extras":{"resonance_bone":0},"children":[0,1]}
            ]
        }),
        &[],
    )
}
fn assemble(snapshot: &mut Snapshot) -> Result<Vec<ActorAssets>> {
    let actors: Actors = snapshot.json("actors.json", 4096)?;
    actors.assemble(&mut |p| snapshot.read(p, 1024 * 1024))
}
fn selections(actors: &[ActorAssets]) -> Vec<Vec<u16>> {
    actors.iter().map(|a| a.hidden_nodes.clone()).collect()
}

#[test]
fn field_visibility_uses_named_defaults_overrides_and_current_glb_joints() -> Result<()> {
    let base = tempfile::tempdir()?;
    let modifications = tempfile::tempdir()?;
    fs::create_dir_all(base.path().join("meshes/hero"))?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(&json!({
            "game":crate::library::GAME,"version":crate::library::VERSION
        }))?,
    )?;
    fs::write(
        base.path().join("meshes/mappings.json"),
        br#"{"1":"hero","2":{"name":"hero/alternate","model":"hero"}}"#,
    )?;
    fs::write(
        base.path().join("meshes/hero/model.json"),
        serde_json::to_vec(&json!({
            "parts":[{"mesh":"body.glb"}],"hidden_geometry":["hat"]
        }))?,
    )?;
    fs::write(base.path().join("body.glb"), mesh(["root", "hat", "coat"])?)?;
    let actors = json!({"models":["hero","hero/alternate",
        {"resource":3,"model":"meshes/hero/model.json","hidden_geometry":[]},
        {"resource":4,"model":"meshes/hero/model.json","hidden_geometry":["coat"]}
    ]});
    fs::write(
        base.path().join("actors.json"),
        serde_json::to_vec(&actors)?,
    )?;
    let library = Library::open(base.path(), &[])?;
    let mut retained = library.snapshot();
    assert_eq!(
        selections(&assemble(&mut retained)?),
        [vec![1], vec![1], vec![], vec![2]]
    );

    fs::write(
        modifications.path().join("body.glb"),
        mesh(["root", "coat", "hat"])?,
    )?;
    let modified = Library::open(
        base.path(),
        &[Mod {
            name: "joint edit".into(),
            root: modifications.path().into(),
        }],
    )?;
    assert_eq!(
        selections(&assemble(&mut modified.snapshot())?),
        [vec![2], vec![2], vec![], vec![1]]
    );
    assert_eq!(
        selections(&assemble(&mut retained)?),
        [vec![1], vec![1], vec![], vec![2]]
    );

    fs::write(
        modifications.path().join("body.glb"),
        mesh(["root", "cloak", "hat"])?,
    )?;
    assert!(
        format!("{:#}", assemble(&mut modified.snapshot()).unwrap_err())
            .contains("unknown hidden geometry joint coat")
    );
    let mut renamed = actors;
    renamed["models"][3]["hidden_geometry"] = json!(["cloak"]);
    fs::write(
        modifications.path().join("actors.json"),
        serde_json::to_vec(&renamed)?,
    )?;
    assert_eq!(
        selections(&assemble(&mut modified.snapshot())?),
        [vec![2], vec![2], vec![], vec![1]]
    );
    Ok(())
}

#[test]
fn visibility_rejects_unknown_and_ambiguous_names_without_retaining_numeric_format() -> Result<()> {
    let mut model: Model =
        serde_json::from_value(json!({"parts":[{"mesh":"body.glb"}],"hidden_geometry":["hat"]}))?;
    model.parts[0].prepare_bones(&mesh(["root", "coat", "hat"])?)?;
    assert_eq!(model.hidden_nodes(&model.hidden_geometry)?, [2]);
    assert!(model.hidden_nodes(&["missing".into()]).is_err());
    model.parts[0].prepare_bones(&mesh(["root", "hat", "hat"])?)?;
    assert!(
        model
            .hidden_nodes(&model.hidden_geometry)
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    assert!(model.hidden_nodes(&[]).unwrap().is_empty());
    for invalid in [
        json!({"parts":[{"mesh":"body.glb"}],"hidden_nodes":[2]}),
        json!({"parts":[{"mesh":"body.glb"}],"hidden_geometry":[2]}),
    ] {
        assert!(serde_json::from_value::<Model>(invalid).is_err());
    }
    assert!(
        serde_json::from_value::<Actor>(
            json!({"resource":1,"model":"model.json","hidden_nodes":[]})
        )
        .is_err()
    );
    Ok(())
}

#[test]
#[ignore = "requires installed library and layout 42 visibility migration evidence"]
fn installed_named_visibility_preserves_every_original_joint() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let root = Path::new(&root);
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/model-visibility");
    let migration: Value = serde_json::from_slice(&fs::read(evidence.join("migration.json"))?)?;
    let mappings: ModelMappings = serde_json::from_slice(&fs::read(root.join(MODELS))?)?;
    let mut models = 0;
    let mut joints = 0;
    for row in migration["models"].as_array().unwrap() {
        let file = row["file"].as_str().unwrap();
        let original: Value =
            serde_json::from_slice(&fs::read(evidence.join("backup").join(file))?)?;
        // Historical expectations identify unnamed owners by their machine ID.
        // Resolve those IDs through the current library, not migration aliases.
        let path = if let Some((owner, id)) = file
            .strip_suffix("/model.json")
            .unwrap()
            .rsplit_once("/model-")
        {
            if let Some(field) = owner.strip_suffix("/models") {
                let resource = u32::from_str_radix(id, 16)?;
                let actors: Actors =
                    serde_json::from_slice(&fs::read(root.join(format!("{field}/actors.json")))?)?;
                actors
                    .models
                    .into_iter()
                    .find_map(|binding| match binding {
                        ModelBinding::Local(actor) if actor.resource == resource => {
                            Some(actor.model)
                        }
                        _ => None,
                    })
                    .context("missing local visibility owner")?
            } else {
                mappings.model_path(id.parse()?)?
            }
        } else {
            file.to_owned()
        };
        let current: Value =
            serde_json::from_slice(&fs::read(root.join(&path)).with_context(|| path.clone())?)?;
        let mut model: Model = serde_json::from_value(current.clone())?;
        for part in &mut model.parts {
            part.prepare_bones(&fs::read(root.join(&part.mesh))?)?;
        }
        model.validate().with_context(|| file.to_owned())?;
        let expected: Vec<u16> = serde_json::from_value(original["hidden_nodes"].clone())?;
        let actual = model
            .hidden_nodes(&model.hidden_geometry)
            .with_context(|| file.to_owned())?;
        ensure!(actual == expected, "visibility changed in {file}");
        ensure!(
            serde_json::from_slice::<Value>(&serde_json::to_vec(&model)?)? == current,
            "noncanonical model publication in {file}"
        );
        models += 1;
        joints += actual.len();
    }
    assert_eq!((models, joints), (189, 611));
    println!(
        "All {models} named model visibility lists preserve {joints} original joint selections and publish canonically"
    );
    Ok(())
}

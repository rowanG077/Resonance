use super::*;
use crate::library::{Library, Mod};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn write(root: &Path, path: &str, value: Value) -> Result<()> {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec(&value)?)?;
    Ok(())
}

#[test]
fn named_rosters_aliases_and_banks_preserve_order_targets_and_mod_capture() -> Result<()> {
    let base = tempfile::tempdir()?;
    let edits = tempfile::tempdir()?;
    write(
        base.path(),
        "content.json",
        json!({"version":crate::library::VERSION,"game":crate::library::GAME}),
    )?;
    write(
        base.path(),
        MODELS,
        json!({"1":"hero","2":{"name":"hero/story","model":"hero"},"3":"absent"}),
    )?;
    write(
        base.path(),
        MODEL_GROUPS,
        json!({"party":["hero","hero/story"]}),
    )?;
    write(
        base.path(),
        "meshes/hero/model.json",
        json!({"parts":[{"mesh":"body.glb","clips":[{"motion":"base.glb","slots":[12],"banks":[77]}]}]}),
    )?;
    fs::write(
        base.path().join("body.glb"),
        resonance_asset_writer::gltf::pack_glb(
            &json!({
                "asset":{"version":"2.0"},"nodes":[{"name":"root","extras":{"resonance_bone":0}}]
            }),
            &[],
        )?,
    )?;
    write(base.path(), ANIMATIONS, json!({"77":"greeting"}))?;
    write(
        base.path(),
        ANIMATION_BINDINGS,
        json!({"joint_maps":{"hero/to/hero":{"root":"root"}},"banks":{"77":[
            {"actors":["hero","absent"],"parts":[0],"motion":"hello.glb","joint_map":"hero/to/hero"},
            {"actors":["hero/story"],"parts":[0],"motion":"story.glb","resource_slot":8},
            {"actors":["hero/story"],"parts":[0],"motion":"story-later.glb","resource_slot":4}
        ]}}),
    )?;
    let selection = json!({"models":[{"group":"party"},{"resource":9,"model":"meshes/hero/model.json"}],
        "animations":["greeting"],"local_animations":{"88":[{"actors":["hero/story"],"parts":[0],"motion":"local.glb"}]}});
    write(base.path(), "actors.json", selection.clone())?;
    let assemble = |snapshot: &mut crate::library::Snapshot| -> Result<Vec<ActorAssets>> {
        let actors: Actors = snapshot.json("actors.json", 65536)?;
        actors.assemble(&mut |p| snapshot.read(p, 65536))
    };
    let library = Library::open(base.path(), &[])?;
    let mut captured = library.snapshot();
    let actors = assemble(&mut captured)?;
    assert_eq!(
        actors.iter().map(|a| a.resource).collect::<Vec<_>>(),
        [1, 2, 9]
    );
    assert_eq!(actors[0].parts[0].clips.len(), 1);
    assert_eq!(actors[0].parts[0].clips[0].motion, "hello.glb");
    assert_eq!(
        actors[0].parts[0].clips[0].joints.get("root"),
        Some(&Some("root".into()))
    );
    let story = &actors[1].parts[0].clips;
    assert_eq!(
        story
            .iter()
            .map(|c| (c.animation_resource, c.resource_slot, c.motion.as_str()))
            .collect::<Vec<_>>(),
        [
            (Some(77), 8, "story.glb"),
            (Some(77), 4, "story-later.glb"),
            (Some(77), 12, "base.glb"),
            (Some(88), 12, "local.glb")
        ]
    );
    // An alias changes only its own model, even though it previously shared art.
    write(
        edits.path(),
        MODELS,
        json!({"2":{"name":"hero/story","model":"alternate"}}),
    )?;
    write(
        edits.path(),
        "meshes/alternate/model.json",
        json!({"parts":[{"mesh":"body.glb","translation":[25,0,0]}]}),
    )?;
    write(
        edits.path(),
        MODEL_GROUPS,
        json!({"party":["hero/story","hero"]}),
    )?;
    let modified = Library::open(
        base.path(),
        &[Mod {
            name: "actor edits".into(),
            root: edits.path().into(),
        }],
    )?;
    let after = assemble(&mut modified.snapshot())?;
    assert_eq!(
        after.iter().map(|a| a.resource).collect::<Vec<_>>(),
        [2, 1, 9]
    );
    assert_eq!(after[0].parts[0].translation, [25., 0., 0.]);
    assert_eq!(after[1].parts[0].translation, [0.; 3]);
    assert_eq!(assemble(&mut captured)?[0].resource, 1);
    // A field can expand a roster and replace one member with its own model.
    let mut local = selection;
    local["models"] =
        json!(["hero",{"resource":2,"model":"meshes/hero/model.json","hidden_geometry":[]}]);
    write(edits.path(), "actors.json", local)?;
    let field = assemble(&mut modified.snapshot())?;
    assert_eq!(field.iter().map(|a| a.resource).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(field[1].parts[0].translation, [0.; 3]);
    Ok(())
}

#[test]
fn actor_documents_reject_ambiguous_names_recursion_and_old_numeric_selections() -> Result<()> {
    for invalid in [
        r#"{"1":"hero","2":"hero"}"#,
        r#"{"1":"hero","1":"other"}"#,
        r#"{"01":"hero"}"#,
        r#"{"1":{"name":"hero","model":"../outside"}}"#,
        r#"{"1":{"name":"hero","model":"hero","unused":true}}"#,
    ] {
        assert!(
            serde_json::from_str::<ModelMappings>(invalid).is_err(),
            "{invalid}"
        );
    }
    assert!(serde_json::from_value::<Actors>(json!({"models":[1]})).is_err());
    assert!(serde_json::from_value::<Actors>(json!({"animations":[77]})).is_err());
    assert!(
        serde_json::from_value::<AnimationBinding>(
            json!({"actors":[1],"parts":[0],"motion":"a.glb"})
        )
        .is_err()
    );
    let default: AnimationBinding =
        serde_json::from_value(json!({"actors":["hero"],"parts":[0],"motion":"a.glb"}))?;
    assert_eq!(default.resource_slot, 12);
    assert!(
        serde_json::to_value(&default)?
            .get("resource_slot")
            .is_none()
    );
    let explicit: AnimationBinding = serde_json::from_value(
        json!({"actors":["hero"],"parts":[0],"motion":"a.glb","resource_slot":8}),
    )?;
    assert_eq!(serde_json::to_value(explicit)?["resource_slot"], 8);
    let group = || {
        vec![ModelBinding::Group {
            group: "party".into(),
        }]
    };
    let mut read = |_: &str| {
        Ok(Arc::<[u8]>::from(
            br#"{"party":[{"group":"party"}]}"#.as_slice(),
        ))
    };
    assert!(
        Actors::expand_models(group(), &mut read)
            .unwrap_err()
            .to_string()
            .contains("cannot contain other groups")
    );
    let mut read = |_: &str| Ok(Arc::<[u8]>::from(br#"{}"#.as_slice()));
    assert!(Actors::expand_models(group(), &mut read).is_err());
    Ok(())
}

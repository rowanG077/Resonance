use super::*;
use crate::{
    field_audio::document::Definition,
    library::{Library, Mod},
};
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::fs;

fn original() -> Mix {
    let field = [0.8, 0.7, 3.6, 0.6, 0.08];
    let battle = [0.7, 0.7, 2.5, 0.6, 0.05];
    let effects = [1., 0.5, 1., 0.8, 0.01];
    Mix {
        music_reverbs: MusicReverbs {
            presets: vec![field, battle],
            selectors: vec![0, 1, 2, 0],
        },
        voice_gains: (0..128).map(|v| v as f32 / 127.).collect(),
        field_reverbs: [field, effects],
        battle_reverbs: [battle, effects],
    }
}
fn document() -> Result<Value> {
    Ok(serde_json::from_slice(&serde_json::to_vec(&original())?)?)
}

#[test]
fn named_mix_preserves_parameters_gains_and_every_music_action() -> Result<()> {
    let before = original();
    let doc = document()?;
    assert_eq!(doc["reverbs"].as_object().unwrap().len(), 3);
    assert_eq!(doc["field_reverbs"]["auxiliary_b"], "effects");
    assert_eq!(doc["battle_reverbs"]["auxiliary_b"], "effects");
    assert_eq!(doc["music_reverbs"]["keep_current"], json!([0, 3]));
    let after: Mix = serde_json::from_value(doc)?;
    assert_eq!(after.field_reverbs, before.field_reverbs);
    assert_eq!(after.battle_reverbs, before.battle_reverbs);
    assert_eq!(after.voice_gains, before.voice_gains);
    for id in 0..4 {
        assert_eq!(
            after.music_reverbs.for_song(id)?,
            before.music_reverbs.for_song(id)?
        );
    }
    assert!(after.music_reverbs.for_song(4).is_err());
    Ok(())
}

#[test]
fn named_mix_mods_share_presets_keep_local_overrides_and_retained_snapshots() -> Result<()> {
    let base = tempfile::tempdir()?;
    let modification = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(
            &json!({"version":crate::library::VERSION,"game":crate::library::GAME}),
        )?,
    )?;
    fs::create_dir(base.path().join("audio"))?;
    fs::create_dir(modification.path().join("audio"))?;
    fs::write(base.path().join(PATH), serde_json::to_vec(&document()?)?)?;
    let library = Library::open(
        base.path(),
        &[Mod {
            name: "acoustics".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut retained = library.snapshot();
    let before = Mix::read(&mut retained)?;
    let mut edited = document()?;
    edited["reverbs"]["effects"]["decay_seconds"] = 0.25.into();
    edited["reverbs"]["field"]["predelay_seconds"] = 0.02.into();
    fs::write(modification.path().join(PATH), serde_json::to_vec(&edited)?)?;
    let after = Mix::read(&mut library.snapshot())?;
    assert_eq!(after.field_reverbs[1][2], 0.25);
    assert_eq!(after.battle_reverbs[1][2], 0.25);
    assert_eq!(after.music_reverbs.for_song(1)?.unwrap()[4], 0.02);
    assert_eq!(Mix::read(&mut retained)?, before);
    let mut local: Definition = serde_json::from_value(json!({"reverbs":{
        "auxiliary_a":{"coloration":0.1,"mix":0.2,"decay_seconds":0.3,"damping":0.4,"predelay_seconds":0.01},
        "auxiliary_b":{"coloration":0.5,"mix":0.6,"decay_seconds":0.7,"damping":0.8,"predelay_seconds":0.02}
    }}))?;
    let environment = local.environment(&mut library.snapshot())?;
    assert_eq!(
        environment.reverbs,
        [[0.1, 0.2, 0.3, 0.4, 0.01], [0.5, 0.6, 0.7, 0.8, 0.02]]
    );
    assert_eq!(environment.program_reverbs(Some(0))?, environment.reverbs);
    assert_eq!(
        environment.program_reverbs(Some(1))?[0],
        after.field_reverbs[0]
    );
    local.reverbs = None;
    assert_eq!(
        local.environment(&mut library.snapshot())?.reverbs,
        after.field_reverbs
    );
    Ok(())
}

#[test]
fn named_mix_accepts_an_additional_authored_preset_without_platform_slots() -> Result<()> {
    let mut doc = document()?;
    doc["reverbs"]["small-room"] = json!({"coloration":0.2,"mix":0.3,"decay_seconds":0.4,"damping":0.5,"predelay_seconds":0.01});
    doc["music_reverbs"]["keep_current"] = json!([0]);
    doc["music_reverbs"]["small-room"] = json!([3]);
    let mix: Mix = serde_json::from_value(doc)?;
    assert_eq!(mix.music_reverbs.presets.len(), 3);
    assert_eq!(
        mix.music_reverbs.for_song(3)?,
        Some([0.2, 0.3, 0.4, 0.5, 0.01])
    );
    let again: Mix = serde_json::from_slice(&serde_json::to_vec(&mix)?)?;
    for id in 0..4 {
        assert_eq!(
            again.music_reverbs.for_song(id)?,
            mix.music_reverbs.for_song(id)?
        );
    }
    Ok(())
}

#[test]
fn named_mix_rejects_missing_choices_unknown_names_and_invalid_units() -> Result<()> {
    for (pointer, value) in [
        ("/field_reverbs/auxiliary_a", json!("absent")),
        ("/music_reverbs/field", json!([1, 2])),
        ("/music_reverbs/battle", json!([])),
        ("/reverbs/field/decay_seconds", json!(-1)),
        ("/reverbs/field/predelay_seconds", json!(100)),
        ("/voice_gains", json!([])),
    ] {
        let mut doc = document()?;
        *doc.pointer_mut(pointer).context("fixture field")? = value;
        ensure!(
            serde_json::from_value::<Mix>(doc).is_err(),
            "accepted invalid mix {pointer}"
        );
    }
    let old = json!({"music_reverbs":{"presets":[[0.5,0.5,1.,0.5,0.]],"selectors":[0]},"voice_gains":original().voice_gains,"field_reverbs":vec![[0.5,0.5,1.,0.5,0.];2],"battle_reverbs":vec![[0.5,0.5,1.,0.5,0.];2]});
    assert!(serde_json::from_value::<Mix>(old).is_err());
    Ok(())
}

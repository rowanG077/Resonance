use super::*;
use serde_json::json;

fn defaults() -> EnvironmentDefaults {
    serde_json::from_value(json!({
        "snow": {
            "sprite":{"texture":"effects/field/textures/snow.ktx2","uv":[0,0,1,1]},
            "count":32,"horizontal_offsets":[-10,11],"spawn_height":80,
            "fall_speed_per_tick":[3,18],"size":12,
            "color":[240,240,255,112],"fog_color":[0,0,31,16],"fog_range":[100,10000]
        },
        "wind":{"first":[-0.8,0,-0.8],"step":[-0.03,0,-0.03],"count":32,
                "when":{"setting_nonzero":7}}
    }))
    .unwrap()
}

#[test]
fn atmosphere_defaults_require_opt_in_and_keep_local_overrides() -> Result<()> {
    let shared = defaults();
    assert!(EnvironmentDocument::default().resolve(&shared)?.is_empty());
    let source = json!({"particles":[{"kind":"snow"},{"kind":"snow","size":24.0}],
                      "wind":{"when":"always"}});
    let environment =
        serde_json::from_value::<EnvironmentDocument>(source.clone())?.resolve(&shared)?;
    assert!(matches!(
        environment.particles[0].emission,
        AmbientEmission::Snow { size: 12., .. }
    ));
    assert!(matches!(
        environment.particles[1].emission,
        AmbientEmission::Snow { size: 24., .. }
    ));
    assert!(
        environment
            .wind
            .as_ref()
            .unwrap()
            .gate
            .active(false, &BTreeMap::new(), |_| false)
    );
    assert_eq!(
        serde_json::to_value(EnvironmentDocument::from_environment(
            &environment,
            &shared
        )?)?,
        source
    );
    Ok(())
}

#[test]
fn atmosphere_rejects_unknown_properties_and_replaces_whole_nested_values() {
    for source in [
        json!({"particles":[{"kind":"snow","szie":4}]}),
        json!({"particles":[{"kind":"unknown"}]}),
        json!({"particles":[{"kind":"snow","sprite":{"texture":"new.ktx2"}}]}),
        json!({"particles":[{"kind":"snow","horizontal_offsets":[10,-10]}]}),
        json!({"wind":{"count":0}}),
        json!({"wind":{"when":{"setting_nonzero":8}}}),
    ] {
        assert!(
            serde_json::from_value::<EnvironmentDocument>(source.clone())
                .unwrap()
                .resolve(&defaults())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn atmosphere_snapshot_uses_shared_mod_values_without_changing_captured_values() -> Result<()> {
    use crate::library::{Descriptor, GAME, Library, VERSION};
    use std::fs;
    let base = tempfile::tempdir()?;
    let modification = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: VERSION,
            game: GAME.into(),
        })?,
    )?;
    fs::create_dir(base.path().join("fields"))?;
    fs::create_dir(modification.path().join("fields"))?;
    fs::write(
        base.path().join("fields/settings.json"),
        serde_json::to_vec(&defaults())?,
    )?;
    let library = Library::open_paths(base.path(), &[modification.path().to_owned()])?;
    let mut retained = library.snapshot();
    let before: EnvironmentDefaults = retained.json("fields/settings.json", 10000)?;
    let mut changed = defaults();
    changed
        .0
        .get_mut("snow")
        .unwrap()
        .insert("size".into(), json!(18));
    fs::write(
        modification.path().join("fields/settings.json"),
        serde_json::to_vec(&changed)?,
    )?;
    let current: EnvironmentDefaults = library.snapshot().json("fields/settings.json", 10000)?;
    let captured: EnvironmentDefaults = retained.json("fields/settings.json", 10000)?;
    for (shared, size) in [(&before, 12.), (&captured, 12.), (&current, 18.)] {
        let document: EnvironmentDocument =
            serde_json::from_value(json!({"particles":[{"kind":"snow"}]}))?;
        assert!(
            matches!(document.resolve(shared)?.particles[0].emission,AmbientEmission::Snow{size:actual,..} if actual==size)
        );
    }
    Ok(())
}

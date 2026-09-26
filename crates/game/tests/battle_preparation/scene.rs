use super::*;
use battle::model::{ModelSource, load_files};
use resonance_content::{animation::Motion, battle_scene, prepared::Cache};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires current Nurse scene publication; no devices"]
fn cold_nurse_scene_verifies_resources_and_rejects_corruption_even_with_live_cache() -> Result<()> {
    let root = common::asset_root();
    let mut cache = Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let path = battle_scene::path(237);
    let scene: battle_scene::Scene = field.json(&path)?;
    assert_eq!(
        scene.source_sha256,
        "05ccf2d9be955dda338760527d66a713daad38418f71cde6953fb2f620288c91"
    );
    let sources = [ModelSource::Scene(237)];
    let active = load_files(&root, field.clone(), &sources, &mut cache, || false)?;
    let effects: resonance_content::battle_effect::SourceBank = active.json(&scene.effects)?;
    assert_eq!(effects.programs.len(), 7);
    assert_eq!(
        active.read("scripts/battle/nurse.sym")?.as_ref(),
        include_bytes!("../../../../scripts/battle/nurse.sym")
    );
    let mut compiler = PreparationCache::default();
    let binding = ActionBinding {
        id: 99,
        phase: resonance_battle::ActionPhase::Resident,
        module: "battle::nurse".into(),
        entry: "recover".into(),
        duration: 250,
        tp_cost: 0,
    };
    battle::prepare(
        &mut compiler,
        &active,
        &[binding],
        vec![actor()],
        1,
        &mut Resources {
            paths: vec![],
            fail: false,
        },
        vec![],
    )?;
    assert_eq!(
        scene.models.keys().copied().collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    #[derive(serde::Deserialize)]
    struct ObservedModel {
        skeleton: resonance_content::animation::Skeleton,
        secondary_motion: resonance_content::secondary_motion::Definition,
    }
    let observed: ObservedModel = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/nurse-scene-models.json"
    ))?;
    for model in scene.models.values() {
        assert_eq!(
            serde_json::to_value(&model.rig.skeleton)?,
            serde_json::to_value(&observed.skeleton)?
        );
        for layer in &model.layers {
            assert_eq!(serde_json::to_value(&layer.scene.secondary_motion)?, {
                // Body and outline share the solver parameters, while
                // preserving their own source model name.
                let mut secondary = observed.secondary_motion.clone();
                secondary
                    .model
                    .clone_from(&layer.scene.secondary_motion.model);
                serde_json::to_value(&secondary)?
            });
            for clip in &layer.scene.clips {
                let bytes = active.read(&clip.motion)?;
                assert_eq!(
                    bytes.as_ref(),
                    include_bytes!("../../../battle/tests/fixtures/nurse.motion")
                );
                Motion::decode(&bytes)?.validate(&model.rig.skeleton)?;
            }
        }
    }
    for path in scene.files.keys() {
        active.read(path)?;
    }
    let reused = load_files(&root, field.clone(), &sources, &mut cache, || false)?;
    for path in scene.files.keys() {
        assert!(Arc::ptr_eq(&active.read(path)?, &reused.read(path)?));
    }

    let temporary = std::env::temp_dir().join(format!(
        "resonance-nurse-resources-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
    ));
    for path in scene.files.keys() {
        let target = temporary.join(path);
        fs::create_dir_all(target.parent().context("missing resource directory")?)?;
        fs::write(target, active.read(path)?)?;
    }
    let motion = &scene.models[&0].layers[0].scene.clips[0].motion;
    let original = active.read(motion)?;
    let mut corrupt = original.to_vec();
    corrupt[0] ^= 1;
    fs::write(temporary.join(motion), corrupt)?;
    let failed = load_files(&temporary, field.clone(), &sources, &mut cache, || false);
    assert!(
        failed
            .err()
            .context("accepted corrupt scene motion")?
            .to_string()
            .contains("digest differs")
    );
    fs::remove_file(temporary.join(motion))?;
    assert!(load_files(&temporary, field, &sources, &mut cache, || false).is_err());
    assert_eq!(active.read(motion)?, original);
    fs::remove_dir_all(temporary)?;
    Ok(())
}

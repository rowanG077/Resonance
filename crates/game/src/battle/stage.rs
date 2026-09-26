//! Verify a selected arena's complete dependency closure before activation.
use anyhow::Result;
use resonance_content::{
    battle_stage::{self, Stage},
    prepared::{Cache, Files},
};
use std::path::Path;

/// The descriptor must already belong to the caller's verified field snapshot.
/// Extending a cloned candidate preserves the suspended field on any failure.
pub fn prepare(
    root: &Path,
    files: Files,
    arena: u16,
    cache: &mut Cache,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, Stage)> {
    let stage: Stage = files.json(&battle_stage::path(arena))?;
    stage.validate()?;
    let files = files.with_dependencies(root, stage.files.clone(), cache, cancelled)?;
    Ok((files, stage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        field_preload::{File, Role},
        model_preview::PreviewPart,
    };
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stage_candidate_checks_cold_and_cached_files_without_changing_retained_field() -> Result<()>
    {
        let root = Fixture(std::env::temp_dir().join(format!(
            "resonance-stage-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
        )));
        fs::create_dir(&root.0)?;
        let bytes = b"stage mesh fixture";
        fs::write(root.0.join("stage.glb"), bytes)?;
        let part: PreviewPart = serde_json::from_value(serde_json::json!({
            "scene": { "resource":0, "mesh":"stage.glb", "textures":[], "materials":[],
                "translation":[0.,0.,0.], "clips":[], "autoplay":false, "texture_animations":[] },
            "animation":null, "attached_to":null, "additive":false
        }))?;
        let stage = Stage {
            source_sha256: "a".repeat(64),
            light_position: [0.; 3],
            chain_acceleration: [0.; 3],
            actor_color: [64, 64, 64, 255],
            translation: [0.; 3],
            yaw_degrees: 0.,
            color: [64, 64, 64, 255],
            camera_pitch_offset: 0.,
            layers: BTreeMap::from([(0, part)]),
            effects: None,
            textures: vec![],
            files: BTreeMap::from([(
                "stage.glb".into(),
                File {
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    bytes: bytes.len() as u64,
                    roles: [Role::Mesh].into(),
                },
            )]),
        };
        let mut retained = Files::default();
        retained
            .bytes
            .insert(battle_stage::path(13), serde_json::to_vec(&stage)?.into());
        retained
            .bytes
            .insert("field.bin".into(), Arc::from(&b"retained VM inputs"[..]));
        let before = retained.bytes.clone();
        let mut cache = Cache::default();
        let (cold, _) = prepare(&root.0, retained.clone(), 13, &mut cache, || false)?;
        let (warm, _) = prepare(&root.0, retained.clone(), 13, &mut cache, || false)?;
        assert!(Arc::ptr_eq(
            &cold.read("stage.glb")?,
            &warm.read("stage.glb")?
        ));
        assert!(prepare(&root.0, retained.clone(), 13, &mut cache, || true).is_err());
        assert!(prepare(&root.0, retained.clone(), 14, &mut cache, || false).is_err());
        fs::write(root.0.join("stage.glb"), b"corrupt mesh bytes")?;
        assert!(prepare(&root.0, retained.clone(), 13, &mut cache, || false).is_err());
        fs::remove_file(root.0.join("stage.glb"))?;
        assert!(prepare(&root.0, retained.clone(), 13, &mut cache, || false).is_err());
        let mut invalid = stage.clone();
        invalid.files.clear();
        assert!(invalid.validate().is_err());
        invalid = stage.clone();
        invalid.layers.get_mut(&0).unwrap().scene.autoplay = true;
        assert!(invalid.validate().is_err());
        assert_eq!(retained.bytes, before);
        assert!(cold.read("stage.glb").is_ok());
        Ok(())
    }
}

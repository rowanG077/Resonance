//! Fixed startup factories; dynamic models use the ordinary preparation path.
use super::*;
use resonance_battle::{PreparedEffectModel, ResourceId};
use resonance_content::battle_model::ModelPart;
use sha2::{Digest, Sha256};

type Key = ([u8; 32], bool);
struct Factory {
    model: PreparedEffectModel,
    dependencies: BTreeMap<String, Arc<[u8]>>,
}

impl Factory {
    fn capture(files: &Files, part: &ModelPart, animate: bool) -> Result<Self> {
        let mut dependencies = BTreeMap::new();
        let model = super::effect_from(part, ResourceId(0), animate, |path| {
            let bytes = files.read(path)?;
            dependencies.insert(path.to_owned(), bytes.clone());
            Ok(bytes)
        })?;
        Ok(Self {
            model,
            dependencies,
        })
    }

    fn matches(&self, files: &Files) -> bool {
        self.dependencies.iter().all(|(path, previous)| {
            files
                .read(path)
                .is_ok_and(|current| resonance_content::same_bytes(previous, &current))
        })
    }
}

#[derive(Default)]
pub(crate) struct EffectCache {
    entries: BTreeMap<Key, Factory>,
    #[cfg(any(test, feature = "test-support"))]
    hits: std::sync::atomic::AtomicU64,
}

fn key(part: &ModelPart, animate: bool) -> Result<Key> {
    // Prepared joint names are omitted by ScenePart's authored serializer.
    // Include them explicitly, and retain the exact playback slot order instead
    // of the authored serializer's grouped clip representation. Materials and
    // texture variants do not change this verified pose factory.
    let layers: Vec<_> = part
        .layers
        .iter()
        .map(|layer| {
            (
                &layer.scene.mesh,
                &layer.scene.bone_names,
                &layer.scene.clips,
                &layer.scene.secondary_motion,
                layer.scene.resource,
                layer.animation,
                &layer.attached_to,
            )
        })
        .collect();
    let descriptor = (&part.rig, &part.layer_skeletons, layers);
    Ok((
        Sha256::digest(serde_json::to_vec(&descriptor)?).into(),
        animate,
    ))
}

impl EffectCache {
    fn get(&self, key: &Key, files: &Files) -> Option<PreparedEffectModel> {
        let factory = self.entries.get(key)?;
        if !factory.matches(files) {
            return None;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(factory.model.clone())
    }

    /// Only fixed startup inputs are admitted here. Invalid unused art remains
    /// subject to the encounter's normal diagnostics when it is selected.
    pub(crate) fn warm(&mut self, files: &Files, part: &ModelPart, animate: bool) {
        let Ok(key) = key(part, animate) else { return };
        if self.entries.contains_key(&key) {
            return;
        }
        if let Ok(factory) = Factory::capture(files, part, animate) {
            self.entries.insert(key, factory);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hits(&self) -> u64 {
        self.hits.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

pub(crate) type EffectModels = Arc<EffectCache>;

impl EffectCache {
    pub(crate) fn prepare(
        &self,
        files: &Files,
        part: &ModelPart,
        resource: ResourceId,
        animate: bool,
    ) -> Result<PreparedEffectModel> {
        if !self.entries.is_empty()
            && let Some(model) = self.get(&key(part, animate)?, files)
        {
            return Ok(model.with_resource(resource));
        }
        super::effect_from(part, resource, animate, |path| files.read(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        animation::{Motion, Skeleton},
        battle_model::Rig,
    };
    use serde_json::json;

    fn fixture() -> Result<(Files, ModelPart)> {
        let skeleton = Skeleton::single("root");
        let mut files = Files::default();
        files.insert("mesh.glb".into(), resonance_asset_writer::gltf::pack_glb(
            &json!({"asset":{"version":"2.0"},"nodes":[{"name":"root","extras":{"resonance_bone":0}}]}), &[],
        )?.into());
        files.insert(
            "motion.glb".into(),
            Motion {
                duration_frames: 8.,
                tracks: vec![],
            }
            .encode(&skeleton)?
            .into(),
        );
        let mut layer: resonance_content::model_preview::PreviewPart =
            serde_json::from_value(json!({
                "mesh":"mesh.glb", "clips":[{"motion":"motion.glb", "slots":[0]}], "animation":0
            }))?;
        layer.scene.bone_names = vec!["root".into()];
        Ok((
            files,
            ModelPart {
                rig: Rig {
                    skeleton,
                    attachments: Default::default(),
                },
                layer_skeletons: Default::default(),
                layers: vec![layer],
            },
        ))
    }

    #[test]
    fn common_factories_rebind_resources_and_revalidate_changed_inputs() -> Result<()> {
        let (files, part) = fixture()?;
        let mut common = EffectCache::default();
        common.warm(&files, &part, false);
        common.warm(&files, &part, true);
        assert_eq!(common.len(), 2);
        let common = Arc::new(common);
        let models = common.clone();
        assert_eq!(
            models
                .prepare(&files, &part, ResourceId(11), true)?
                .resource(),
            ResourceId(11)
        );
        assert_eq!(
            models
                .prepare(&files, &part, ResourceId(22), true)?
                .resource(),
            ResourceId(22)
        );
        assert_eq!(common.hits(), 2);

        let mut changed = files.clone();
        let edited: Arc<[u8]> = Motion {
            duration_frames: 12.,
            tracks: vec![],
        }
        .encode(&part.rig.skeleton)?
        .into();
        changed.insert("motion.glb".into(), edited.clone());
        models.prepare(&changed, &part, ResourceId(33), true)?;
        assert_eq!(
            common.hits(),
            2,
            "changed curves cannot hit the startup factory"
        );
        let entry = &common.entries[&key(&part, true)?];
        assert!(!entry.matches(&changed));
        assert!(entry.matches(&files));
        changed.remove("motion.glb");
        assert!(
            models
                .prepare(&changed, &part, ResourceId(33), true)
                .is_err()
        );
        changed.insert("motion.glb".into(), Arc::from(&b"invalid motion"[..]));
        assert!(
            models
                .prepare(&changed, &part, ResourceId(33), true)
                .is_err()
        );
        changed = files.clone();
        changed.insert("mesh.glb".into(), Arc::from(&b"invalid mesh"[..]));
        assert!(
            models
                .prepare(&changed, &part, ResourceId(33), true)
                .is_err()
        );

        let mut altered = part.clone();
        altered.layers[0].scene.bone_names.clear();
        assert!(
            models
                .prepare(&files, &altered, ResourceId(44), true)
                .is_err()
        );
        altered = part.clone();
        altered.rig.skeleton.bones[0].bind.translation[0] = 9.;
        assert!(
            models
                .prepare(&files, &altered, ResourceId(44), true)
                .is_err()
        );
        altered = part.clone();
        altered.layers[0].animation = Some(99);
        assert!(
            models
                .prepare(&files, &altered, ResourceId(44), true)
                .is_err()
        );
        models.prepare(&files, &part, ResourceId(55), true)?;
        assert_eq!(
            common.len(),
            2,
            "encounter edits cannot expand the fixed startup set"
        );
        Ok(())
    }
}

//! Fixed default characters and weak lookups into the live fields' preparations.
use super::*;
use std::sync::{Mutex, Weak};

pub struct Common {
    pub actors: Vec<ActorAssets>,
    pub defaults: Option<Defaults>,
    motions: Motions,
    binding_bytes: Arc<[u8]>,
    bindings: Arc<SharedBankBindings>,
    recent: Mutex<BTreeMap<String, (Weak<[u8]>, Weak<crate::animation::NamedMotion>)>>,
    geometry: Mutex<BTreeMap<String, Weak<crate::field::attachments::Geometry>>>,
}

/// Validated default inputs, shared only when a preparation observes the same bytes.
pub struct Defaults {
    pub settings: Arc<settings::Settings>,
    settings_bytes: Arc<[u8]>,
    joints: Arc<BTreeMap<String, String>>,
    joint_bytes: Arc<[u8]>,
}

impl Defaults {
    fn capture(
        snapshot: &mut crate::library::Snapshot,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        ensure!(!cancelled(), "shared field settings cancelled");
        let settings_bytes = snapshot.read(settings::PATH, crate::preparation::JSON_LIMIT)?;
        let settings: settings::Settings =
            serde_json::from_slice(&settings_bytes).context(settings::PATH)?;
        settings.effects.validate()?;
        settings.contact_shadow.validate()?;
        let joint_bytes = snapshot.read(JOINT_MAPPINGS, crate::preparation::JSON_LIMIT)?;
        let joints = serde_json::from_slice(&joint_bytes).context(JOINT_MAPPINGS)?;
        for path in settings
            .effects
            .references()
            .chain([
                settings.contact_shadow.texture.as_str(),
                settings.toon_ramp.as_str(),
            ])
            .chain(
                settings
                    .particles
                    .values()
                    .map(|particle| particle.texture.as_str()),
            )
        {
            ensure!(!cancelled(), "shared field settings cancelled");
            snapshot.read(path, crate::preparation::ASSET_LIMIT)?;
        }
        ensure!(!cancelled(), "shared field settings cancelled");
        Ok(Self {
            settings: Arc::new(settings),
            settings_bytes,
            joints: Arc::new(joints),
            joint_bytes,
        })
    }
}

impl Common {
    pub fn capture(
        snapshot: &mut crate::library::Snapshot,
        diagnostics: &crate::diagnostics::Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        ensure!(!cancelled(), "common field preparation cancelled");
        let defaults = diagnostics.preload("shared field settings", cancelled, || {
            Defaults::capture(snapshot, cancelled)
        })?;
        let binding_bytes =
            snapshot.read(actors::ANIMATION_BINDINGS, crate::preparation::ASSET_LIMIT)?;
        let bindings =
            Arc::new(serde_json::from_slice(&binding_bytes).context(actors::ANIMATION_BINDINGS)?);
        let mappings: ModelMappings = snapshot.json(MODELS, crate::preparation::JSON_LIMIT)?;
        let mut read = |path: &str| {
            ensure!(!cancelled(), "common field preparation cancelled");
            snapshot.read(path, crate::preparation::ASSET_LIMIT)
        };
        let mut actors = Vec::new();
        let mut motions = Motions::new();
        for id in 1..=9 {
            let captured = diagnostics.preload(
                &format!("common field character {id}"),
                cancelled,
                || -> Result<_> {
                    let model = mappings
                        .0
                        .get(&id)
                        .context("missing default field character")?;
                    let mut selected = Actors {
                        models: vec![ModelBinding::Shared(model.name().into())],
                        ..Default::default()
                    }
                    .assemble(&mut read, None)?;
                    prepare_clips(
                        selected.iter_mut().flat_map(|actor| &mut actor.parts),
                        &mut read,
                        None,
                        None,
                        &mut motions,
                    )?;
                    for part in selected.iter().flat_map(|actor| &actor.parts) {
                        for path in &part.textures {
                            read(path)?;
                        }
                    }
                    Ok(selected)
                },
            )?;
            if let Some(selected) = captured {
                actors.extend(selected);
            }
        }
        let geometry = actors
            .iter()
            .flat_map(|actor| &actor.parts)
            .filter_map(|part| {
                Some((
                    part.mesh.clone(),
                    Arc::downgrade(part.prepared_geometry.as_ref()?),
                ))
            })
            .collect();
        Ok(Self {
            actors,
            defaults,
            motions,
            binding_bytes,
            bindings,
            recent: Default::default(),
            geometry: Mutex::new(geometry),
        })
    }

    pub fn settings(bytes: &Arc<[u8]>, common: Option<&Self>) -> Result<Arc<settings::Settings>> {
        if let Some(defaults) = common.and_then(|common| common.defaults.as_ref())
            && crate::same_bytes(bytes, &defaults.settings_bytes)
        {
            return Ok(defaults.settings.clone());
        }
        Ok(Arc::new(
            serde_json::from_slice(bytes).context("decode field settings")?,
        ))
    }

    pub fn validate_effects(
        settings: &Arc<settings::Settings>,
        common: Option<&Self>,
    ) -> Result<()> {
        if common
            .and_then(|common| common.defaults.as_ref())
            .is_some_and(|defaults| Arc::ptr_eq(settings, &defaults.settings))
        {
            return Ok(());
        }
        settings.effects.validate()
    }

    pub(super) fn joints(
        bytes: &Arc<[u8]>,
        common: Option<&Self>,
    ) -> Result<Arc<BTreeMap<String, String>>> {
        if let Some(defaults) = common.and_then(|common| common.defaults.as_ref())
            && crate::same_bytes(bytes, &defaults.joint_bytes)
        {
            return Ok(defaults.joints.clone());
        }
        Ok(Arc::new(
            serde_json::from_slice(bytes).context(JOINT_MAPPINGS)?,
        ))
    }

    pub(super) fn bindings(
        bytes: &Arc<[u8]>,
        common: Option<&Self>,
    ) -> Result<Arc<SharedBankBindings>> {
        if let Some(common) = common {
            if crate::same_bytes(&common.binding_bytes, bytes) {
                return Ok(common.bindings.clone());
            }
        }
        Ok(Arc::new(
            serde_json::from_slice(bytes).context(actors::ANIMATION_BINDINGS)?,
        ))
    }

    pub(super) fn prepare_geometry(
        part: &mut ScenePart,
        bytes: Arc<[u8]>,
        common: Option<&Self>,
    ) -> Result<()> {
        use crate::field::attachments::Geometry;
        let geometry = if let Some(common) = common {
            let mut cache = common.geometry.lock().unwrap();
            match cache
                .get(&part.mesh)
                .and_then(Weak::upgrade)
                .filter(|geometry| geometry.matches(&bytes))
            {
                Some(geometry) => geometry,
                None => {
                    let geometry = Arc::new(Geometry::decode(bytes)?);
                    cache.insert(part.mesh.clone(), Arc::downgrade(&geometry));
                    geometry
                }
            }
        } else {
            Arc::new(Geometry::decode(bytes)?)
        };
        part.prepare_geometry(geometry);
        Ok(())
    }

    pub(super) fn prune(&self) {
        self.geometry
            .lock()
            .unwrap()
            .retain(|_, geometry| geometry.strong_count() > 0);
        self.recent
            .lock()
            .unwrap()
            .retain(|_, (_, motion)| motion.strong_count() > 0);
    }

    pub(super) fn decode(
        &self,
        path: &str,
        bytes: &Arc<[u8]>,
    ) -> Result<Arc<crate::animation::NamedMotion>> {
        if let Some((captured, motion)) = self.motions.get(path) {
            if crate::same_bytes(captured, bytes) {
                return Ok(motion.clone());
            }
        }
        let recent = self.recent.lock().unwrap();
        if let Some((captured, motion)) = recent.get(path) {
            if let Some((captured, motion)) = captured.upgrade().zip(motion.upgrade()) {
                if crate::same_bytes(&captured, bytes) {
                    return Ok(motion);
                }
            }
        }
        drop(recent);
        let motion = Arc::new(crate::animation::NamedMotion::decode(bytes)?);
        self.recent.lock().unwrap().insert(
            path.into(),
            (Arc::downgrade(bytes), Arc::downgrade(&motion)),
        );
        Ok(motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_and_joint_maps_reuse_only_matching_inputs() -> Result<()> {
        let root = tempfile::tempdir()?;
        let settings = settings::test_fixture();
        let bytes: Arc<[u8]> = serde_json::to_vec(&settings)?.into();
        let joints: Arc<[u8]> = Arc::from(&br#"{"native-root":"root"}"#[..]);
        std::fs::create_dir(root.path().join("fields"))?;
        std::fs::create_dir(root.path().join("meshes"))?;
        std::fs::create_dir(root.path().join("effects"))?;
        std::fs::write(root.path().join(settings::PATH), &bytes)?;
        std::fs::write(root.path().join(JOINT_MAPPINGS), &joints)?;
        std::fs::write(
            root.path().join("effects/default.ktx2"),
            b"captured image bytes",
        )?;
        std::fs::write(
            root.path().join("content.json"),
            serde_json::to_vec(&crate::library::Descriptor {
                version: crate::library::VERSION,
                game: crate::library::GAME.into(),
            })?,
        )?;
        let library = crate::library::Library::open(root.path(), &[])?;
        let defaults = Defaults::capture(&mut library.snapshot(), &|| false)?;
        assert!(Defaults::capture(&mut library.snapshot(), &|| true).is_err());
        let common = Common {
            actors: Vec::new(),
            defaults: Some(defaults),
            motions: BTreeMap::new(),
            binding_bytes: Arc::from([]),
            bindings: Arc::new(SharedBankBindings::default()),
            recent: Default::default(),
            geometry: Default::default(),
        };
        let defaults = common.defaults.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            &Common::settings(&bytes, Some(&common))?,
            &defaults.settings
        ));
        assert!(Arc::ptr_eq(
            &Common::joints(&joints, Some(&common))?,
            &defaults.joints
        ));
        let mut changed = settings.clone();
        changed.contact_shadow.alpha = 64;
        let changed = Common::settings(&serde_json::to_vec(&changed)?.into(), Some(&common))?;
        assert!(!Arc::ptr_eq(&changed, &defaults.settings));
        assert_eq!(changed.contact_shadow.alpha, 64);
        assert_eq!(defaults.settings.contact_shadow.alpha, 128);
        let mut invalid = settings.clone();
        invalid.effects.palette.clear();
        let invalid = Common::settings(&serde_json::to_vec(&invalid)?.into(), Some(&common))?;
        assert!(Common::validate_effects(&invalid, Some(&common)).is_err());
        Common::validate_effects(&defaults.settings, Some(&common))?;
        let changed = Common::joints(
            &Arc::from(&br#"{"native-root":"other"}"#[..]),
            Some(&common),
        )?;
        assert!(!Arc::ptr_eq(&changed, &defaults.joints));
        assert_eq!(changed["native-root"], "other");
        assert!(Common::settings(&Arc::from(&b"invalid"[..]), Some(&common)).is_err());
        assert!(Common::joints(&Arc::from(&b"invalid"[..]), Some(&common)).is_err());
        Ok(())
    }

    #[test]
    fn shared_field_curves_require_the_current_source_bytes() -> Result<()> {
        let skeleton = crate::animation::Skeleton::single("root");
        let clip = crate::animation::Motion {
            duration_frames: 12.,
            tracks: vec![],
        };
        let bytes: Arc<[u8]> = clip.encode(&skeleton)?.into();
        let common = Common {
            actors: vec![],
            defaults: None,
            binding_bytes: Arc::from(&br#"{"joint_maps":{},"banks":{}}"#[..]),
            bindings: Arc::new(SharedBankBindings::default()),
            motions: [(
                "idle.glb".into(),
                (
                    bytes.clone(),
                    Arc::new(crate::animation::NamedMotion::decode(&bytes)?),
                ),
            )]
            .into(),
            recent: Default::default(),
            geometry: Default::default(),
        };
        assert!(Arc::ptr_eq(
            &Common::bindings(&common.binding_bytes, Some(&common))?,
            &common.bindings
        ));
        let edited: Arc<[u8]> =
            Arc::from(&br#"{"joint_maps":{"edited":{"root":null}},"banks":{}}"#[..]);
        let fresh = Common::bindings(&edited, Some(&common))?;
        assert!(!Arc::ptr_eq(&fresh, &common.bindings));
        assert!(fresh.joint_maps.contains_key("edited"));
        assert!(Common::bindings(&Arc::from(&b"invalid"[..]), Some(&common)).is_err());
        let copied: Arc<[u8]> = bytes.to_vec().into();
        assert!(Arc::ptr_eq(
            &common.decode("idle.glb", &copied)?,
            &common.motions["idle.glb"].1
        ));
        let changed: Arc<[u8]> = crate::animation::Motion {
            duration_frames: 24.,
            ..clip
        }
        .encode(&skeleton)?
        .into();
        let decoded = common.decode("idle.glb", &changed)?;
        assert!(!Arc::ptr_eq(&decoded, &common.motions["idle.glb"].1));
        assert_eq!(
            decoded
                .bind(&["root".into()], &BTreeMap::new())?
                .duration_frames,
            24.
        );
        assert!(Arc::ptr_eq(&decoded, &common.decode("idle.glb", &changed)?));
        assert!(
            common
                .decode("idle.glb", &Arc::from(&b"invalid"[..]))
                .is_err()
        );
        let retired = Arc::downgrade(&decoded);
        drop(decoded);
        assert!(
            retired.upgrade().is_none(),
            "lookup retained a retired field's decoded clip"
        );
        common.prune();
        assert!(common.recent.lock().unwrap().is_empty());
        let mut part: ScenePart = serde_json::from_value(serde_json::json!({"mesh":"body.glb"}))?;
        Common::prepare_geometry(&mut part, bytes.clone(), Some(&common))?;
        let original = part.prepared_geometry.clone().unwrap();
        Common::prepare_geometry(&mut part, bytes.clone(), Some(&common))?;
        assert!(Arc::ptr_eq(
            &original,
            part.prepared_geometry.as_ref().unwrap()
        ));
        Common::prepare_geometry(&mut part, changed, Some(&common))?;
        assert!(!Arc::ptr_eq(
            &original,
            part.prepared_geometry.as_ref().unwrap()
        ));
        let retired = Arc::downgrade(part.prepared_geometry.as_ref().unwrap());
        drop(part);
        assert!(
            retired.upgrade().is_none(),
            "geometry lookup kept a retired scene alive"
        );
        common.prune();
        assert!(common.geometry.lock().unwrap().is_empty());
        Ok(())
    }

    #[test]
    #[ignore = "requires current named content at RESONANCE_TEST_ASSETS; CPU only"]
    fn installed_common_curves_match_fresh_fields_without_capturing_alternate_costumes()
    -> Result<()> {
        use crate::{diagnostics::Diagnostics, library::Library, prepared::Files};
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let library = Library::open(std::path::Path::new(&root), &[])?;
        let mut snapshot = library.snapshot();
        let common = Common::capture(&mut snapshot, &Diagnostics::new(true), &|| false)?;
        assert_eq!(common.actors.len(), 9);
        let files = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
        assert!(
            !files
                .keys()
                .any(|path| path.contains("/nobleman/") || path.contains("/maid/"))
        );
        let mut recent = Vec::new();
        for id in [332, 340, 341] {
            let start = std::time::Instant::now();
            let mut published = false;
            let mut snapshot = library.snapshot_with(&files)?;
            let shared = Definition::read(&mut snapshot, id)?.capture_scene(
                &mut snapshot,
                id,
                || false,
                Some(&common),
                &mut |snapshot, field| {
                    assert!(!published);
                    published = true;
                    assert!(snapshot.contains_key(&field.toon_ramp));
                    for part in field
                        .parts
                        .iter()
                        .chain(field.actors.iter().flat_map(|actor| &actor.parts))
                    {
                        assert!(snapshot.contains_key(&part.mesh));
                        assert!(
                            part.prepared_motions.is_none(),
                            "geometry publication must precede binding"
                        );
                    }
                    Ok(())
                },
            )?;
            assert!(published);
            let shared_time = start.elapsed();
            let start = std::time::Instant::now();
            let (_, fresh) = capture_scene(&mut library.snapshot(), id, || false)?;
            eprintln!(
                "field {id}: shared {:?}, fresh {:?}",
                shared_time,
                start.elapsed()
            );
            assert_eq!(
                serde_json::to_value(&shared)?,
                serde_json::to_value(&fresh)?
            );
            let mut reused = 0;
            for part in shared.actors.iter().flat_map(|actor| &actor.parts) {
                assert_eq!(
                    part.prepared_motions.as_ref().unwrap()._sources.len(),
                    part.clips
                        .iter()
                        .map(|clip| &clip.motion)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    "clip aliases must retain their shared source only once"
                );
                for (index, clip) in part.clips.iter().enumerate() {
                    if let Some((_, named)) = common.motions.get(&clip.motion) {
                        let expected = named.bind_shared(&part.bone_names, &clip.joints)?;
                        let actual = &part
                            .prepared_motion_snapshot()
                            .context("missing prepared curves")?[index];
                        assert!(Arc::ptr_eq(&expected, actual));
                        reused += 1;
                    }
                }
            }
            assert!(reused > 100, "common curves were not reused");
            recent.push(shared);
        }
        drop(recent);
        common.prune();
        assert!(common.recent.lock().unwrap().is_empty());
        // Only the fixed nine defaults keep geometry alive after scene retirement.
        assert!(
            common
                .geometry
                .lock()
                .unwrap()
                .values()
                .all(|geometry| geometry.strong_count() > 0)
        );
        let overlay = tempfile::tempdir()?;
        let mappings: ModelMappings = library
            .snapshot()
            .json(MODELS, crate::preparation::JSON_LIMIT)?;
        let broken = mappings.model_path(9)?;
        let path = overlay.path().join(&broken);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, b"invalid unused model")?;
        let changed =
            Library::open_paths(std::path::Path::new(&root), &[overlay.path().to_owned()])?;
        let diagnostics = Diagnostics::default();
        let warmed = Common::capture(&mut changed.snapshot(), &diagnostics, &|| false)?;
        assert_eq!(warmed.actors.len(), 8);
        assert!(warmed.actors.iter().all(|actor| actor.resource != 9));
        assert!(
            Common::capture(&mut changed.snapshot(), &Diagnostics::new(true), &|| false).is_err()
        );
        Ok(())
    }
}

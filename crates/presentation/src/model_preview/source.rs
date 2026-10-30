use anyhow::{Context, Result};
use bevy::{
    asset::io::{AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader},
    prelude::*,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, RwLock},
};
pub(super) struct Prepared {
    pub model: Arc<resonance_content::model_preview::ModelPreview>,
    pub files: Arc<resonance_content::prepared::Files>,
    pub sources: BTreeMap<String, String>,
}

#[derive(Resource, Clone)]
pub(super) struct Source {
    pub files: Arc<RwLock<Option<Arc<resonance_content::prepared::Files>>>>,
}

pub fn register(app: &mut App) {
    let source = Source {
        files: Default::default(),
    };
    app.insert_resource(source.clone());
    app.register_asset_source(
        "preview",
        AssetSourceBuilder::new(move || Box::new(source.clone())),
    );
}
impl Source {
    pub fn prepare(
        &self,
        record: &resonance_content::model_preview::ModelPreview,
        library: resonance_content::library::Library,
    ) -> Result<super::Pending> {
        let path = record
            .document
            .clone()
            .context("preview needs a named model document")?;
        super::Pending::spawn(move |cancelled| {
            capture(&library, &path, || {
                cancelled.load(std::sync::atomic::Ordering::Relaxed)
            })
        })
    }
}

fn capture(
    library: &resonance_content::library::Library,
    path: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Prepared> {
    use resonance_content::{model_preview::document, preparation};
    anyhow::ensure!(!cancelled(), "preview load cancelled");
    let mut snapshot = library.snapshot();
    let mut model = document::read(path, |path| snapshot.read(path, preparation::JSON_LIMIT))?;
    document::prepare(&mut model, |path| {
        anyhow::ensure!(!cancelled(), "preview load cancelled");
        snapshot.read(path, preparation::ASSET_LIMIT)
    })
    .with_context(|| format!("prepare model {path}"))?;
    if model.parts.iter().any(|part| {
        part.scene.outline_color.is_none() && part.scene.materials.iter().any(|m| m.color.is_some())
    }) {
        snapshot.read(document::TOON_RAMP, preparation::ASSET_LIMIT)?;
    }
    let sources = if model.behavior.is_some() {
        preparation::scripts(&mut snapshot, &cancelled)?
    } else {
        BTreeMap::new()
    };
    let files = resonance_content::prepared::Files::from_snapshot(
        snapshot,
        cancelled,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    Ok(Prepared {
        model: Arc::new(model),
        files: Arc::new(files),
        sources,
    })
}
impl AssetReader for Source {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.files
            .read()
            .unwrap()
            .as_ref()
            .and_then(|files| files.read(path.to_string_lossy().as_ref()).ok())
            .map(|b| VecReader::new(b.to_vec()))
            .ok_or_else(|| AssetReaderError::NotFound(path.into()))
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.into()))
    }
    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(path.into()))
    }
    async fn is_directory<'a>(&'a self, _: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_preview_captures_model_edits_mod_assets_and_current_motion_timing() -> Result<()> {
        use resonance_content::{
            animation::Motion,
            library::{Library, Mod},
        };
        use std::fs;
        let work = tempfile::tempdir()?;
        let root = work.path().join("base");
        let modification = work.path().join("mod");
        let owner = "meshes/monsters/torent";
        let descriptor = format!("{owner}/model.json");
        let mesh = format!("{owner}/body.glb");
        let texture = format!("{owner}/body.png");
        let motion = format!("{owner}/idle.glb");
        fs::create_dir_all(root.join(owner))?;
        fs::create_dir_all(modification.join(owner))?;
        fs::write(
            root.join("content.json"),
            br#"{"version":94,"game":"tales-of-symphonia"}"#,
        )?;
        let mut definition = serde_json::json!({
             "scale": 1, "elevation": 4, "hidden_geometry": [], "behavior": null,
            "parts": [{"animation": 4, "attached_to": null, "additive": false,
                "resource": 0, "mesh": mesh, "textures": [texture],
                    "materials": [], "translation": [0,0,0], "autoplay": false,
                    "texture_animations": [], "bone_names": ["root"],
                    "clips": [{"motion": motion, "slots":[4], "duration_seconds": 99}]}
            ]
        });
        fs::write(root.join(&descriptor), serde_json::to_vec(&definition)?)?;
        let mesh_bytes = resonance_asset_writer::gltf::pack_glb(
            &serde_json::json!({
                "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}],
                "nodes": [{"name": "root", "extras": {"resonance_bone": 0}}]
            }),
            &[],
        )?;
        fs::write(root.join(&mesh), &mesh_bytes)?;
        fs::write(root.join(&texture), b"original texture")?;
        fs::write(
            root.join(&motion),
            Motion {
                duration_frames: 30.,
                tracks: vec![],
            }
            .encode(&resonance_content::animation::Skeleton::single("root"))?,
        )?;
        let base = Library::open(&root, &[])?;
        let first = capture(&base, &descriptor, || false)?;
        assert_eq!(first.model.document.as_deref(), Some(descriptor.as_str()));
        assert_eq!(first.model.parts[0].scene.clips[0].duration_seconds, 1.);
        assert!(first.model.parts[0].scene.clips[0].pose_chains.is_empty());
        assert_eq!(&*first.files.read(&texture)?, b"original texture");
        assert!(first.sources.is_empty());

        definition["scale"] = 2.into();
        fs::write(
            modification.join(&descriptor),
            serde_json::to_vec(&definition)?,
        )?;
        fs::write(modification.join(&texture), b"mod texture")?;
        fs::write(
            modification.join(&motion),
            Motion {
                duration_frames: 90.,
                tracks: vec![],
            }
            .encode(&resonance_content::animation::Skeleton::single("root"))?,
        )?;
        let library = Library::open(
            &root,
            &[Mod {
                name: "model".into(),
                root: modification.clone(),
            }],
        )?;
        let second = capture(&library, &descriptor, || false)?;
        assert_eq!(second.model.scale, 2.);
        assert_eq!(second.model.parts[0].scene.clips[0].duration_seconds, 3.);
        assert_eq!(
            &*second.files.read(&mesh)?,
            mesh_bytes.as_slice(),
            "unchanged geometry is inherited"
        );
        assert_eq!(&*second.files.read(&texture)?, b"mod texture");
        assert!(
            second
                .files
                .overrides()
                .iter()
                .any(|change| change.path == texture && change.replacement == "model")
        );
        assert_eq!(
            &*first.files.read(&texture)?,
            b"original texture",
            "retained preview is immutable"
        );
        assert_eq!(capture(&base, &descriptor, || false)?.model.scale, 1.);
        assert!(capture(&library, &descriptor, || true).is_err());
        let reads = std::cell::Cell::new(0);
        assert!(
            capture(&library, &descriptor, || {
                reads.set(reads.get() + 1);
                reads.get() > 2
            })
            .is_err()
        );

        fs::write(modification.join(&motion), b"invalid edited motion")?;
        let error = match capture(&library, &descriptor, || false) {
            Ok(_) => anyhow::bail!("invalid motion reused old preview"),
            Err(error) => format!("{error:#}"),
        };
        assert!(
            error.contains(&motion) && error.contains(&descriptor),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn script_snapshot_uses_only_prepared_utf8_sources() {
        let mut files = resonance_content::prepared::Files::default();
        files.insert(
            "scripts/model/appearance.sym".into(),
            Arc::from(&b"verified"[..]),
        );
        files.insert("scripts/field.ssb".into(), Arc::from(&b"\xff"[..]));
        let sources = files.script_sources().unwrap();
        files.clear();
        assert_eq!(
            sources,
            BTreeMap::from([("model::appearance".into(), "verified".into())])
        );
        files.insert("scripts/model/broken.sym".into(), Arc::from(&b"\xff"[..]));
        assert!(files.script_sources().is_err());
    }
}

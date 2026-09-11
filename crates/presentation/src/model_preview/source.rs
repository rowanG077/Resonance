use anyhow::{Context, Result};
use bevy::{
    asset::io::{AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader},
    prelude::*,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
pub(super) type Bytes = BTreeMap<String, Arc<[u8]>>;

#[derive(Resource, Clone)]
pub(super) struct Source {
    pub root: PathBuf,
    pub bytes: Arc<RwLock<Bytes>>,
}
pub fn register(app: &mut App, root: &Path) {
    let source = Source {
        root: root.into(),
        bytes: Default::default(),
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
    ) -> Result<super::Pending> {
        let root = self.root.clone();
        let paths: std::collections::BTreeSet<_> = record
            .parts
            .iter()
            .flat_map(|p| std::iter::once(&p.scene.mesh).chain(&p.scene.textures))
            .cloned()
            .collect();
        super::Pending::spawn(move |cancelled| {
            let mut bytes = Bytes::new();
            for path in paths {
                anyhow::ensure!(
                    !cancelled.load(std::sync::atomic::Ordering::Relaxed),
                    "preview load cancelled"
                );
                resonance_content::validate_asset_path(&path)?;
                let data = std::fs::read(root.join(&path)).with_context(|| {
                    format!("missing preview asset {path}; recook its catalogue")
                })?;
                bytes.insert(path, data.into());
            }
            Ok(bytes)
        })
    }
}
impl AssetReader for Source {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.bytes
            .read()
            .unwrap()
            .get(path.to_string_lossy().as_ref())
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

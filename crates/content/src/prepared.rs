//! Immutable bytes captured from current named content. Media streams retain their own handles.
use crate::diagnostics::Diagnostics;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

#[derive(Clone)]
struct VerifiedBytes {
    bytes: Arc<[u8]>,
    digest: String,
}

#[derive(Clone)]
pub struct Files {
    bytes: BTreeMap<String, VerifiedBytes>,
    pub disk_bytes: u64,
    pub reused_bytes: u64,
    diagnostics: Diagnostics,
    overrides: Vec<crate::library::Override>,
    identity: Option<crate::save_identity::Identity>,
    library: Option<crate::library::Library>,
}
impl Default for Files {
    fn default() -> Self {
        Self::new(Diagnostics::new(true))
    }
}
impl Files {
    pub fn new(diagnostics: Diagnostics) -> Self {
        Self {
            bytes: BTreeMap::new(),
            disk_bytes: 0,
            reused_bytes: 0,
            diagnostics,
            overrides: Vec::new(),
            identity: None,
            library: None,
        }
    }
}

#[derive(Debug)]
struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("asset preparation cancelled")
    }
}
impl std::error::Error for Cancelled {}
#[derive(Default)]
pub struct Cache(BTreeMap<String, Weak<[u8]>>);
impl Files {
    /// The effective named namespace for a later, explicit preparation boundary
    /// such as a paused model viewer. Captured bytes themselves remain immutable.
    pub fn library(&self) -> Option<&crate::library::Library> {
        self.library.as_ref()
    }

    /// Derived preparation state, never a required file in the editable tree.
    pub fn with_identity(mut self, identity: crate::save_identity::Identity) -> Result<Self> {
        identity.validate()?;
        self.identity = Some(identity);
        Ok(self)
    }

    pub(crate) fn identity(&self) -> Option<&crate::save_identity::Identity> {
        self.identity.as_ref()
    }

    /// Transfer the effective named resources into the resident runtime store.
    /// Hashes identify the captured bytes; no cooker inventory admits or rejects
    /// them. Cache hits still follow reading the current resource so an edit or
    /// override cannot reuse an obsolete declaration's digest.
    pub fn from_snapshot(
        snapshot: crate::library::Snapshot,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        Self::from_captured(snapshot.into_captured(), cache, cancelled, diagnostics)
    }

    /// Read an immutable runtime view while the same preparation continues to
    /// discover dependencies, for example action-selected battle audio. Bytes
    /// are shared; this neither reopens paths nor starts a second snapshot.
    pub fn capture_snapshot(
        snapshot: &crate::library::Snapshot,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        Self::from_captured(snapshot.captured(), cache, cancelled, diagnostics)
    }

    fn from_captured(
        captured: crate::library::CapturedFiles,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        if cancelled() {
            return Err(Cancelled.into());
        }
        let mut result = Self::new(diagnostics);
        result.library = Some(captured.library);
        result.disk_bytes = captured.disk_bytes;
        result.overrides = captured.overrides;
        cache.0.retain(|_, bytes| bytes.strong_count() > 0);
        for (path, entry) in captured.files {
            if cancelled() {
                return Err(Cancelled.into());
            }
            let digest = entry.digest;
            let captured = entry.bytes;
            let bytes = if let Some(shared) = cache.0.get(&digest).and_then(Weak::upgrade) {
                result.reused_bytes += captured.len() as u64;
                shared
            } else {
                captured
            };
            cache.0.insert(digest.clone(), Arc::downgrade(&bytes));
            result.bytes.insert(path, VerifiedBytes { bytes, digest });
        }
        Ok(result)
    }

    /// Authored code comes from this verified snapshot, never a second disk read.
    pub fn script_sources(&self) -> Result<BTreeMap<String, String>> {
        self.iter()
            .filter_map(|(path, bytes)| {
                path.strip_prefix("scripts/")
                    .and_then(|path| path.strip_suffix(".sym"))
                    .map(|module| (path, module, bytes))
            })
            .map(|(path, module, bytes)| {
                Ok((
                    module.replace('/', "::"),
                    std::str::from_utf8(bytes)
                        .with_context(|| format!("invalid cooked script UTF-8: {path}"))?
                        .to_owned(),
                ))
            })
            .collect()
    }
    pub fn read(&self, path: &str) -> Result<Arc<[u8]>> {
        self.bytes
            .get(path)
            .map(|entry| entry.bytes.clone())
            .with_context(|| format!("unprepared cooked resource {path}"))
    }
    pub fn digest(&self, path: &str) -> Result<&str> {
        self.bytes
            .get(path)
            .map(|entry| entry.digest.as_str())
            .with_context(|| format!("unprepared cooked resource {path}"))
    }
    pub fn read_verified(&self, path: &str, digest: &str, limit: usize) -> Result<Arc<[u8]>> {
        let entry = self
            .bytes
            .get(path)
            .with_context(|| format!("unprepared cooked resource {path}"))?;
        ensure!(
            entry.bytes.len() <= limit && entry.digest == digest,
            "prepared resource digest or size differs: {path}"
        );
        Ok(entry.bytes.clone())
    }
    /// Explicit in-memory publication computes integrity metadata once.
    pub fn insert(&mut self, path: String, bytes: Arc<[u8]>) {
        self.identity = None;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        self.bytes.insert(path, VerifiedBytes { bytes, digest });
    }
    pub fn remove(&mut self, path: &str) {
        self.identity = None;
        self.bytes.remove(path);
    }
    pub fn clear(&mut self) {
        self.identity = None;
        self.bytes.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<[u8]>)> {
        self.bytes.iter().map(|(path, entry)| (path, &entry.bytes))
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.bytes.keys()
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn contains_key(&self, path: &str) -> bool {
        self.bytes.contains_key(path)
    }
    pub fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        serde_json::from_slice(&self.read(path)?).with_context(|| format!("decode {path}"))
    }
    /// Read a field from this captured named namespace.
    pub fn field(&self, map: u32) -> Result<crate::field::FieldAssets> {
        let mappings: crate::library::Mappings = self.json(crate::library::FIELDS)?;
        let path = mappings.resolve(crate::library::FIELDS, map, Some("field.json"))?;
        let definition: crate::field::document::Definition = self.json(&path)?;
        definition.assemble(map, |path| self.read(path))
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
    pub fn overrides(&self) -> &[crate::library::Override] {
        &self.overrides
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_bytes_updates_integrity_without_changing_retained_snapshots() {
        let mut files = Files::default();
        files.insert("sample.bin".into(), Arc::from(b"sample".as_slice()));
        let retained = files.clone();
        let digest = files.digest("sample.bin").unwrap().to_owned();
        let mut bytes = files.read_verified("sample.bin", &digest, 6).unwrap();
        assert!(Arc::ptr_eq(&bytes, &files.read("sample.bin").unwrap()));
        assert!(files.read_verified("sample.bin", &digest, 5).is_err());

        Arc::make_mut(&mut bytes)[0] = b'S';
        assert_eq!(files.read("sample.bin").unwrap().as_ref(), b"sample");
        files.insert("sample.bin".into(), bytes.clone());
        assert!(files.read_verified("sample.bin", &digest, 6).is_err());
        let updated = files.digest("sample.bin").unwrap();
        assert!(Arc::ptr_eq(
            &bytes,
            &files.read_verified("sample.bin", updated, 6).unwrap()
        ));
        assert_eq!(
            retained
                .read_verified("sample.bin", &digest, 6)
                .unwrap()
                .as_ref(),
            b"sample"
        );
        files.remove("sample.bin");
        assert!(files.read("sample.bin").is_err());
        assert!(files.digest("sample.bin").is_err());
        assert!(files.read_verified("sample.bin", &digest, 6).is_err());
    }
}

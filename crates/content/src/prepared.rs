//! Immutable bytes captured from current named content. Media streams retain their own handles.
use crate::diagnostics::Diagnostics;
use anyhow::{Context, Result};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub struct Files {
    bytes: BTreeMap<String, Arc<[u8]>>,
    pub disk_bytes: u64,
    diagnostics: Diagnostics,
    overrides: Vec<crate::library::Override>,
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
            diagnostics,
            overrides: Vec::new(),
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
impl Files {
    /// The effective named namespace for a later, explicit preparation boundary
    /// such as a paused model viewer. Captured bytes themselves remain immutable.
    pub fn library(&self) -> Option<&crate::library::Library> {
        self.library.as_ref()
    }

    pub fn from_snapshot(
        snapshot: crate::library::Snapshot,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        Self::from_captured(snapshot.into_captured(), cancelled, diagnostics)
    }

    /// Read an immutable runtime view while the same preparation continues to
    /// discover dependencies, for example action-selected battle audio. Bytes
    /// are shared; this neither reopens paths nor starts a second snapshot.
    pub fn capture_snapshot(
        snapshot: &crate::library::Snapshot,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        Self::from_captured(snapshot.captured(), cancelled, diagnostics)
    }

    fn from_captured(
        captured: crate::library::CapturedFiles,
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
        result.bytes = captured.files;
        Ok(result)
    }

    /// Authored code comes from the captured bytes, without a second disk read.
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
            .cloned()
            .with_context(|| format!("unprepared cooked resource {path}"))
    }
    pub fn insert(&mut self, path: String, bytes: Arc<[u8]>) {
        self.bytes.insert(path, bytes);
    }
    pub fn remove(&mut self, path: &str) {
        self.bytes.remove(path);
    }
    pub fn clear(&mut self) {
        self.bytes.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<[u8]>)> {
        self.bytes.iter()
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
    /// An omitted declaration has its default value. A declared file must exist
    /// in this preparation and decode successfully.
    pub fn json_or_default<T: serde::de::DeserializeOwned + Default>(
        &self,
        path: Option<&str>,
    ) -> Result<T> {
        path.map(|path| self.json(path))
            .transpose()
            .map(Option::unwrap_or_default)
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
    fn replacing_bytes_keeps_retained_snapshots() {
        let mut files = Files::default();
        files.insert("sample.bin".into(), Arc::from(b"sample".as_slice()));
        let retained = files.clone();
        files.insert("sample.bin".into(), Arc::from(b"edited".as_slice()));
        assert_eq!(&*files.read("sample.bin").unwrap(), b"edited");
        assert_eq!(&*retained.read("sample.bin").unwrap(), b"sample");
        files.remove("sample.bin");
        assert!(files.read("sample.bin").is_err());
    }
}

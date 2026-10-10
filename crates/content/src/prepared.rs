//! Verified immutable resource bytes. Movie payloads are verified but stay streamable.
use crate::{
    diagnostics::Diagnostics,
    field_preload::{Manifest, Role, SHARED_PATH, Shared},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Weak},
};

#[derive(Clone)]
struct VerifiedBytes {
    bytes: Arc<[u8]>,
    digest: String,
}

#[derive(Clone)]
pub struct Files {
    pub manifests: BTreeMap<u32, Manifest>,
    bytes: BTreeMap<String, VerifiedBytes>,
    pub disk_bytes: u64,
    pub reused_bytes: u64,
    diagnostics: Diagnostics,
    rejected: BTreeSet<String>,
}
impl Default for Files {
    fn default() -> Self {
        Self::new(Diagnostics::new(true))
    }
}
impl Files {
    pub fn new(diagnostics: Diagnostics) -> Self {
        Self {
            manifests: BTreeMap::new(),
            bytes: BTreeMap::new(),
            disk_bytes: 0,
            reused_bytes: 0,
            diagnostics,
            rejected: BTreeSet::new(),
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
        self.rejected.remove(&path);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        self.bytes.insert(path, VerifiedBytes { bytes, digest });
    }
    pub fn remove(&mut self, path: &str) {
        self.bytes.remove(path);
    }
    pub fn clear(&mut self) {
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
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// A failed verification must not become an unverified disk fallback.
    pub fn is_rejected(&self, path: &str) -> bool {
        self.rejected.contains(path)
    }

    pub fn load(
        root: &Path,
        paths: &[&str],
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        Self::load_with_diagnostics(root, paths, cache, cancelled, Diagnostics::new(true))
    }

    /// Runtime loading can collect independent failures and retain only verified
    /// resources. Required consumers still fail when their own inputs are absent.
    pub fn load_with_diagnostics(
        root: &Path,
        paths: &[&str],
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut result = Self::new(diagnostics);
        let mut inventory = BTreeMap::new();
        ensure!(!cancelled(), "asset preparation cancelled");
        let shared = (|| -> Result<Shared<serde_json::Value>> {
            serde_json::from_reader(std::io::BufReader::new(File::open(root.join(SHARED_PATH))?))
                .context("decode shared preparation inventory")
        })()
        .context("read shared preparation inventory");
        if let Some(shared) = result.diagnostics.attempt("asset inventory", shared)?
            && result
                .diagnostics
                .attempt("asset inventory", shared.validate_structure())?
                .is_some()
        {
            let files = result.decode_dependencies(&mut inventory, shared.files, &cancelled)?;
            result.collect_dependencies(&mut inventory, files)?;
        }
        for path in paths {
            ensure!(!cancelled(), "asset preparation cancelled");
            let manifest = (|| -> Result<Manifest<serde_json::Value>> {
                crate::validate_asset_path(path)?;
                serde_json::from_reader(std::io::BufReader::new(File::open(root.join(path))?))
                    .with_context(|| format!("decode preparation manifest {path}"))
            })()
            .with_context(|| format!("read preparation manifest {path}"));
            let Some(manifest) = result.diagnostics.attempt("asset inventory", manifest)? else {
                continue;
            };
            if result
                .diagnostics
                .attempt(
                    "asset inventory",
                    manifest
                        .validate_structure()
                        .with_context(|| format!("invalid preparation manifest {path}")),
                )?
                .is_none()
            {
                continue;
            }
            if !manifest.is_complete() {
                result.diagnostics.report(
                    "asset inventory",
                    anyhow::anyhow!(
                        "incomplete field preparation manifest {path}: {:?}",
                        manifest.missing_inputs
                    ),
                )?;
            }
            let files = result.decode_dependencies(&mut inventory, manifest.files, &cancelled)?;
            result.collect_dependencies(
                &mut inventory,
                files
                    .iter()
                    .map(|(path, file)| (path.clone(), file.clone())),
            )?;
            result.manifests.insert(
                manifest.map_id,
                Manifest {
                    version: manifest.version,
                    map_id: manifest.map_id,
                    inputs: manifest.inputs,
                    missing_inputs: manifest.missing_inputs,
                    files,
                },
            );
        }
        result.read_dependencies(root, inventory, cache, cancelled)
    }

    /// Prepare a scene inventory through the shared dependency checks.
    pub fn from_inventory(
        root: &Path,
        inventory: BTreeMap<String, crate::field_preload::File>,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        Self::default().with_dependencies(root, inventory, cache, cancelled)
    }

    /// Extend a candidate with verified resources. Diagnostic mode omits failed
    /// payloads and keeps checking the inventory; strict mode drops the candidate.
    pub fn with_dependencies(
        mut self,
        root: &Path,
        inventory: BTreeMap<String, crate::field_preload::File>,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        if cancelled() {
            return Err(Cancelled.into());
        }
        for path in inventory.keys() {
            self.rejected.remove(path);
        }
        let mut admitted = BTreeMap::new();
        self.collect_dependencies(&mut admitted, inventory)?;
        self.read_dependencies(root, admitted, cache, cancelled)
    }

    fn collect_dependencies(
        &mut self,
        inventory: &mut BTreeMap<String, crate::field_preload::File>,
        entries: impl IntoIterator<Item = (String, crate::field_preload::File)>,
    ) -> Result<()> {
        for (path, file) in entries {
            if self.rejected.contains(&path) {
                continue;
            }
            let valid = file.validate(&path).and_then(|()| {
                if let Some(previous) = inventory.get(&path) {
                    ensure!(
                        previous.sha256 == file.sha256 && previous.bytes == file.bytes,
                        "inconsistent dependency {path}"
                    );
                }
                Ok(())
            });
            if let Err(error) = valid {
                inventory.remove(&path);
                self.bytes.remove(&path);
                self.rejected.insert(path);
                self.diagnostics.report("asset dependency", error)?;
                continue;
            }
            if let Some(previous) = inventory.get_mut(&path) {
                previous.roles.extend(file.roles);
            } else {
                inventory.insert(path, file);
            }
        }
        Ok(())
    }

    fn decode_dependencies(
        &mut self,
        inventory: &mut BTreeMap<String, crate::field_preload::File>,
        entries: BTreeMap<String, serde_json::Value>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<BTreeMap<String, crate::field_preload::File>> {
        let mut files = BTreeMap::new();
        for (path, value) in entries {
            if cancelled() {
                return Err(Cancelled.into());
            }
            match serde_json::from_value(value) {
                Ok(file) => {
                    files.insert(path, file);
                }
                Err(error) => {
                    inventory.remove(&path);
                    self.bytes.remove(&path);
                    self.rejected.insert(path.clone());
                    self.diagnostics.report(
                        "asset dependency",
                        anyhow::Error::from(error).context(format!("invalid preload file {path}")),
                    )?;
                }
            }
        }
        Ok(files)
    }

    fn read_dependencies(
        mut self,
        root: &Path,
        inventory: BTreeMap<String, crate::field_preload::File>,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        cache.0.retain(|_, bytes| bytes.strong_count() > 0);
        for (path, entry) in inventory {
            if cancelled() {
                return Err(Cancelled.into());
            }
            let loaded = (|| -> Result<Option<Vec<u8>>> {
                if let Some(existing) = self.bytes.get(&path) {
                    ensure!(
                        existing.bytes.len() as u64 == entry.bytes
                            && existing.digest == entry.sha256,
                        "inconsistent dependency {path}"
                    );
                    if !entry.roles.contains(&Role::Movie) {
                        // This snapshot already owns these verified bytes. Streamed
                        // payloads and entries found only in Cache still require disk checks.
                        return Ok(None);
                    }
                }
                let mut file = File::open(root.join(&path))
                    .with_context(|| format!("read asset dependency {path}"))?;
                ensure!(
                    file.metadata()?.len() == entry.bytes,
                    "asset dependency size differs: {path}"
                );
                let mut hash = Sha256::new();
                let mut bytes = Vec::new();
                let mut buffer = [0; 64 * 1024];
                let mut read = 0u64;
                loop {
                    if cancelled() {
                        return Err(Cancelled.into());
                    }
                    let count = file.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    read += count as u64;
                    ensure!(
                        read <= entry.bytes,
                        "asset dependency grew while reading: {path}"
                    );
                    hash.update(&buffer[..count]);
                    if !entry.roles.contains(&Role::Movie) {
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    self.disk_bytes += count as u64;
                }
                ensure!(
                    read == entry.bytes,
                    "asset dependency shrank while reading: {path}"
                );
                ensure!(
                    format!("{:x}", hash.finalize()) == entry.sha256,
                    "asset dependency digest differs: {path}"
                );
                Ok(Some(bytes))
            })();
            let bytes = match loaded {
                Ok(bytes) => bytes,
                Err(error) => {
                    if error.is::<Cancelled>() {
                        return Err(error);
                    }
                    self.bytes.remove(&path);
                    self.rejected.insert(path.clone());
                    self.diagnostics.report("asset dependency", error)?;
                    continue;
                }
            };
            self.rejected.remove(&path);
            let Some(bytes) = bytes else {
                self.reused_bytes += entry.bytes;
                continue;
            };
            if !entry.roles.contains(&Role::Movie) {
                // Verify aliases too: a matching declared digest must not hide
                // a missing or corrupt payload, even while another field holds it.
                let bytes: Arc<[u8]> =
                    if let Some(shared) = cache.0.get(&entry.sha256).and_then(Weak::upgrade) {
                        self.reused_bytes += entry.bytes;
                        shared
                    } else {
                        bytes.into()
                    };
                cache.0.insert(entry.sha256.clone(), Arc::downgrade(&bytes));
                self.bytes.insert(
                    path,
                    VerifiedBytes {
                        bytes,
                        digest: entry.sha256,
                    },
                );
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_preload::{File as Entry, Inputs, VERSION};
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "resonance-prepared-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut files = BTreeMap::new();
        for (path, bytes, role) in [
            ("field.json", b"{}".as_slice(), Role::Field),
            ("one.bin", b"sample", Role::Data),
            ("alias.bin", b"sample", Role::Data),
            ("movie.bin", b"stream", Role::Movie),
        ] {
            fs::write(root.join(path), bytes).unwrap();
            files.insert(
                path.into(),
                Entry {
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    bytes: bytes.len() as u64,
                    roles: [role].into(),
                },
            );
        }
        let shared = Shared {
            version: VERSION,
            files: BTreeMap::from([("one.bin".into(), files.remove("one.bin").unwrap())]),
        };
        fs::write(root.join(SHARED_PATH), serde_json::to_vec(&shared).unwrap()).unwrap();
        let manifest = Manifest {
            version: VERSION,
            map_id: 1,
            inputs: Inputs {
                field: "field.json".into(),
                audio: Default::default(),
                movies: Default::default(),
            },
            missing_inputs: Default::default(),
            files,
        };
        fs::write(
            root.join("field.preload.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        Fixture(root)
    }
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

    #[test]
    fn verified_aliases_share_storage_and_movies_remain_streams() {
        let fixture = fixture();
        let mut cache = Cache::default();
        let first = Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).unwrap();
        assert!(Arc::ptr_eq(
            &first.read("one.bin").unwrap(),
            &first.read("alias.bin").unwrap()
        ));
        assert!(first.read("movie.bin").is_err());
        let next = Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).unwrap();
        assert!(Arc::ptr_eq(
            &first.read("one.bin").unwrap(),
            &next.read("one.bin").unwrap()
        ));
        let weak = Arc::downgrade(&next.read("one.bin").unwrap());
        drop(first);
        drop(next);
        assert!(
            weak.upgrade().is_none(),
            "cache must not retain retired fields"
        );
    }
    #[test]
    fn corrupt_missing_and_cancelled_preparation_fail_even_with_a_cache() {
        let fixture = fixture();
        let mut cache = Cache::default();
        let _lease =
            Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).unwrap();
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || true).is_err());
        fs::write(fixture.0.join("movie.bin"), b"broken").unwrap();
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
        fs::write(fixture.0.join("movie.bin"), b"stream").unwrap();
        fs::write(fixture.0.join("one.bin"), b"broken").unwrap();
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
        fs::remove_file(fixture.0.join("alias.bin")).unwrap();
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
    }

    #[test]
    fn extensions_reuse_their_snapshot_but_verify_new_paths_even_on_cache_hits() {
        let fixture = fixture();
        let mut cache = Cache::default();
        let active =
            Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).unwrap();
        let bytes = b"motion";
        fs::write(fixture.0.join("clip.motion"), bytes).unwrap();
        let entry = Entry {
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
            roles: [Role::Data].into(),
        };
        let inventory = BTreeMap::from([("clip.motion".into(), entry.clone())]);
        let loaded = active
            .clone()
            .with_dependencies(&fixture.0, inventory.clone(), &mut cache, || false)
            .unwrap();
        let reused = active
            .clone()
            .with_dependencies(&fixture.0, inventory.clone(), &mut cache, || false)
            .unwrap();
        assert!(Arc::ptr_eq(
            &loaded.read("clip.motion").unwrap(),
            &reused.read("clip.motion").unwrap()
        ));
        assert!(
            active
                .clone()
                .with_dependencies(&fixture.0, inventory.clone(), &mut cache, || true)
                .is_err()
        );
        fs::write(fixture.0.join("clip.motion"), b"broken").unwrap();
        assert!(
            active
                .clone()
                .with_dependencies(&fixture.0, inventory.clone(), &mut cache, || false)
                .is_err()
        );
        fs::remove_file(fixture.0.join("clip.motion")).unwrap();
        assert!(
            active
                .clone()
                .with_dependencies(&fixture.0, inventory.clone(), &mut cache, || false)
                .is_err()
        );
        let same_snapshot = loaded
            .clone()
            .with_dependencies(&fixture.0, inventory, &mut cache, || false)
            .unwrap();
        assert_eq!(same_snapshot.disk_bytes, loaded.disk_bytes);
        assert_eq!(
            same_snapshot.reused_bytes,
            loaded.reused_bytes + entry.bytes
        );
        assert!(Arc::ptr_eq(
            &same_snapshot.read("clip.motion").unwrap(),
            &loaded.read("clip.motion").unwrap()
        ));
        let invalid = BTreeMap::from([("../outside.motion".into(), entry.clone())]);
        assert!(
            active
                .clone()
                .with_dependencies(&fixture.0, invalid, &mut cache, || false)
                .is_err()
        );
        let inconsistent = BTreeMap::from([("one.bin".into(), entry)]);
        assert!(
            active
                .clone()
                .with_dependencies(&fixture.0, inconsistent, &mut cache, || false)
                .is_err()
        );
        assert!(active.read("clip.motion").is_err());
        assert_eq!(active.read("one.bin").unwrap().as_ref(), b"sample");
        assert_eq!(loaded.read("clip.motion").unwrap().as_ref(), bytes);
    }

    #[test]
    fn diagnostic_loading_collects_missing_size_and_digest_errors_without_using_cached_bytes() {
        let fixture = fixture();
        let mut cache = Cache::default();
        let retained =
            Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).unwrap();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(fixture.0.join("field.preload.json")).unwrap())
                .unwrap();
        let good = b"later verified dependency";
        fs::write(fixture.0.join("zz-good.bin"), good).unwrap();
        manifest.files.insert(
            "zz-good.bin".into(),
            Entry {
                sha256: format!("{:x}", Sha256::digest(good)),
                bytes: good.len() as u64,
                roles: [Role::Data].into(),
            },
        );
        fs::write(
            fixture.0.join("field.preload.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::remove_file(fixture.0.join("field.json")).unwrap();
        fs::write(fixture.0.join("alias.bin"), b"broken").unwrap();
        fs::write(fixture.0.join("one.bin"), b"wrong length").unwrap();
        let diagnostics = Diagnostics::default();
        let loaded = Files::load_with_diagnostics(
            &fixture.0,
            &["field.preload.json"],
            &mut cache,
            || false,
            diagnostics.clone(),
        )
        .unwrap();
        assert_eq!(diagnostics.entries().len(), 3);
        for path in ["field.json", "alias.bin", "one.bin"] {
            assert!(loaded.read(path).is_err());
            assert!(loaded.is_rejected(path));
        }
        assert_eq!(loaded.read("zz-good.bin").unwrap().as_ref(), good);
        assert_eq!(retained.read("one.bin").unwrap().as_ref(), b"sample");
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
    }

    #[test]
    fn shared_inventory_is_required_and_conflicting_local_entries_are_rejected() {
        let fixture = fixture();
        let mut cache = Cache::default();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(fixture.0.join("field.preload.json")).unwrap())
                .unwrap();
        let mut conflict = manifest.files["alias.bin"].clone();
        conflict.sha256 = "0".repeat(64);
        manifest.files.insert("one.bin".into(), conflict);
        fs::write(
            fixture.0.join("field.preload.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let loaded = Files::load_with_diagnostics(
            &fixture.0,
            &["field.preload.json"],
            &mut cache,
            || false,
            Diagnostics::default(),
        )
        .unwrap();
        assert!(loaded.is_rejected("one.bin"));
        assert!(loaded.read("one.bin").is_err());
        assert_eq!(loaded.read("alias.bin").unwrap().as_ref(), b"sample");
        assert_eq!(loaded.diagnostics().entries().len(), 1);
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
        fs::remove_file(fixture.0.join(SHARED_PATH)).unwrap();
        assert!(Files::load(&fixture.0, &[], &mut cache, || false).is_err());
    }

    #[test]
    fn malformed_entries_preserve_healthy_siblings_in_each_inventory() {
        for shared in [false, true] {
            for defect in [
                "path",
                "hash",
                "roles",
                "type",
                "missing",
                "unknown_role",
                "unknown_field",
                "shadowed",
            ] {
                let fixture = fixture();
                let mut entry = serde_json::json!({
                    "sha256": format!("{:x}", Sha256::digest(b"sample")),
                    "bytes": 6,
                    "roles": ["data"],
                });
                let path = match defect {
                    "path" => "../outside.bin",
                    "shadowed" => "one.bin",
                    _ => "bad.bin",
                };
                match defect {
                    "path" => {}
                    "hash" => entry["sha256"] = "invalid".into(),
                    "roles" => entry["roles"] = serde_json::json!([]),
                    "type" | "shadowed" => entry["bytes"] = "not a number".into(),
                    "missing" => {
                        entry.as_object_mut().unwrap().remove("bytes");
                    }
                    "unknown_role" => entry["roles"] = serde_json::json!(["unknown"]),
                    "unknown_field" => entry["extra"] = true.into(),
                    _ => unreachable!(),
                }
                let inventory_path = fixture.0.join(if shared {
                    SHARED_PATH
                } else {
                    "field.preload.json"
                });
                let mut inventory: serde_json::Value =
                    serde_json::from_slice(&fs::read(&inventory_path).unwrap()).unwrap();
                inventory["files"][path] = entry;
                fs::write(inventory_path, serde_json::to_vec(&inventory).unwrap()).unwrap();
                let mut cache = Cache::default();
                let files = Files::load_with_diagnostics(
                    &fixture.0,
                    &["field.preload.json"],
                    &mut cache,
                    || false,
                    Diagnostics::default(),
                )
                .unwrap();
                assert_eq!(files.read("field.json").unwrap().as_ref(), b"{}");
                if defect == "shadowed" {
                    assert!(
                        files.read("one.bin").is_err(),
                        "malformed declarations cannot reuse a shared entry"
                    );
                } else {
                    assert_eq!(files.read("one.bin").unwrap().as_ref(), b"sample");
                }
                assert_eq!(files.read("alias.bin").unwrap().as_ref(), b"sample");
                assert!(files.is_rejected(path));
                let failures = files.diagnostics().entries();
                assert_eq!(failures.len(), 1);
                assert!(
                    !failures[0].message.contains("read asset dependency"),
                    "malformed entries must be rejected before opening a file"
                );
                assert!(
                    Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err()
                );
                assert!(
                    Files::load_with_diagnostics(
                        &fixture.0,
                        &["field.preload.json"],
                        &mut cache,
                        || true,
                        Diagnostics::default()
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn diagnostic_loading_isolates_unsafe_paths_but_never_swallows_cancellation() {
        let fixture = fixture();
        let diagnostics = Diagnostics::default();
        let mut cache = Cache::default();
        let loaded = Files::load_with_diagnostics(
            &fixture.0,
            &["field.preload.json"],
            &mut cache,
            || false,
            diagnostics.clone(),
        )
        .unwrap();
        let entry = Entry {
            sha256: format!("{:x}", Sha256::digest(b"sample")),
            bytes: 6,
            roles: [Role::Data].into(),
        };
        let calls = AtomicUsize::new(0);
        fs::write(fixture.0.join("new.bin"), b"sample").unwrap();
        let error = loaded
            .clone()
            .with_dependencies(
                &fixture.0,
                BTreeMap::from([("new.bin".into(), entry.clone())]),
                &mut cache,
                || calls.fetch_add(1, Ordering::Relaxed) > 0,
            )
            .err()
            .unwrap();
        assert!(error.is::<Cancelled>());
        assert!(!diagnostics.has_errors());
        let inventory = BTreeMap::from([
            ("../outside.bin".into(), entry.clone()),
            ("new.bin".into(), entry.clone()),
        ]);
        let loaded = loaded
            .with_dependencies(&fixture.0, inventory.clone(), &mut cache, || false)
            .unwrap();
        assert!(loaded.is_rejected("../outside.bin"));
        assert_eq!(loaded.read("new.bin").unwrap().as_ref(), b"sample");
        assert!(
            Files::default()
                .with_dependencies(&fixture.0, inventory, &mut cache, || false)
                .is_err()
        );

        let manifest_bytes = fs::read(fixture.0.join("field.preload.json")).unwrap();
        let mut invalid: Manifest = serde_json::from_slice(&manifest_bytes).unwrap();
        invalid.files.insert("../outside.bin".into(), entry.clone());
        fs::write(
            fixture.0.join("field.preload.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        let unsafe_shared = Shared {
            version: VERSION,
            files: BTreeMap::from([("../outside.bin".into(), entry)]),
        };
        fs::write(
            fixture.0.join(SHARED_PATH),
            serde_json::to_vec(&unsafe_shared).unwrap(),
        )
        .unwrap();
        let paths = ["../outside.json", "field.preload.json"];
        let loaded = Files::load_with_diagnostics(
            &fixture.0,
            &paths,
            &mut cache,
            || false,
            diagnostics.clone(),
        )
        .unwrap();
        assert_eq!(loaded.read("alias.bin").unwrap().as_ref(), b"sample");
        assert_eq!(loaded.manifests.len(), 1);
        assert!(
            diagnostics
                .entries()
                .iter()
                .all(|entry| entry.message.contains("unsafe asset path"))
        );
        assert!(Files::load(&fixture.0, &paths, &mut cache, || false).is_err());
    }
}

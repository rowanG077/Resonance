//! Verified immutable resource bytes. Movie payloads are verified but stay streamable.
use crate::{
    diagnostics::Diagnostics,
    field_preload::{Manifest, Role},
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
pub struct Files {
    pub manifests: BTreeMap<u32, Manifest>,
    pub bytes: BTreeMap<String, Arc<[u8]>>,
    pub disk_bytes: u64,
    pub reused_bytes: u64,
    diagnostics: Diagnostics,
    rejected: BTreeSet<String>,
}
impl Default for Files {
    fn default() -> Self {
        Self {
            manifests: BTreeMap::new(),
            bytes: BTreeMap::new(),
            disk_bytes: 0,
            reused_bytes: 0,
            diagnostics: Diagnostics::new(true),
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
        self.bytes
            .iter()
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
        let mut result = Self {
            diagnostics,
            ..Self::default()
        };
        let mut inventory = BTreeMap::new();
        for path in paths {
            crate::validate_asset_path(path)?;
            ensure!(!cancelled(), "asset preparation cancelled");
            let manifest = (|| -> Result<Manifest> {
                serde_json::from_reader(std::io::BufReader::new(File::open(root.join(path))?))
                    .with_context(|| format!("decode preparation manifest {path}"))
            })()
            .with_context(|| format!("read preparation manifest {path}"));
            let Some(manifest) = result.diagnostics.attempt("asset inventory", manifest)? else {
                continue;
            };
            // Path safety remains mandatory even when the descriptor has other
            // errors. Never pass an invalid path to the runtime reader.
            for path in std::iter::once(&manifest.inputs.field)
                .chain(&manifest.inputs.audio)
                .chain(&manifest.inputs.movies)
                .chain(manifest.files.keys())
                .chain(&manifest.missing_inputs)
            {
                crate::validate_asset_path(path)?;
            }
            if result
                .diagnostics
                .attempt(
                    "asset inventory",
                    manifest
                        .validate()
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
            for (path, file) in &manifest.files {
                if result.rejected.contains(path) {
                    continue;
                }
                if let Some(previous) = inventory.get_mut(path) {
                    let previous: &mut crate::field_preload::File = previous;
                    if previous.sha256 != file.sha256 || previous.bytes != file.bytes {
                        result.diagnostics.report(
                            "asset inventory",
                            anyhow::anyhow!("inconsistent dependency {path}"),
                        )?;
                        inventory.remove(path);
                        result.rejected.insert(path.clone());
                        continue;
                    }
                    previous.roles.extend(&file.roles);
                } else {
                    inventory.insert(path.clone(), file.clone());
                }
            }
            result.manifests.insert(manifest.map_id, manifest);
        }
        result.with_dependencies(root, inventory, cache, cancelled)
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
        cache.0.retain(|_, bytes| bytes.strong_count() > 0);
        for (path, entry) in inventory {
            crate::validate_asset_path(&path)?;
            ensure!(!cancelled(), "asset preparation cancelled");
            let loaded = (|| -> Result<Vec<u8>> {
                if let Some(existing) = self.bytes.get(&path) {
                    ensure!(
                        existing.len() as u64 == entry.bytes
                            && format!("{:x}", Sha256::digest(existing)) == entry.sha256,
                        "inconsistent dependency {path}"
                    );
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
                Ok(bytes)
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
                cache.0.insert(entry.sha256, Arc::downgrade(&bytes));
                self.bytes.insert(path, bytes);
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
        let manifest = Manifest {
            version: VERSION,
            map_id: 1,
            inputs: Inputs {
                field: "field.json".into(),
                audio: Default::default(),
                movies: Default::default(),
            },
            missing_inputs: Default::default(),
            total_file_bytes: files.values().map(|f| f.bytes).sum(),
            files,
            scenes: vec![],
            features: Default::default(),
            scripts: vec![],
        };
        fs::write(
            root.join("field.preload.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        Fixture(root)
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
        fs::write(fixture.0.join("one.bin"), b"broken").unwrap();
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
        fs::remove_file(fixture.0.join("alias.bin")).unwrap();
        assert!(Files::load(&fixture.0, &["field.preload.json"], &mut cache, || false).is_err());
    }

    #[test]
    fn additional_dependencies_verify_on_cache_hits_and_fail_without_changing_the_active_snapshot()
    {
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
                .with_dependencies(&fixture.0, inventory, &mut cache, || false)
                .is_err()
        );
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
        manifest.total_file_bytes += good.len() as u64;
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
    fn diagnostic_loading_never_swallows_cancellation_or_unsafe_paths() {
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
        assert!(
            loaded
                .clone()
                .with_dependencies(
                    &fixture.0,
                    BTreeMap::from([("../outside.bin".into(), entry.clone())]),
                    &mut cache,
                    || false,
                )
                .is_err()
        );
        let calls = AtomicUsize::new(0);
        let error = loaded
            .with_dependencies(
                &fixture.0,
                BTreeMap::from([("one.bin".into(), entry)]),
                &mut cache,
                || calls.fetch_add(1, Ordering::Relaxed) > 0,
            )
            .err()
            .unwrap();
        assert!(error.is::<Cancelled>());
        assert!(!diagnostics.has_errors());
    }
}

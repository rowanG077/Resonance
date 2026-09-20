//! Verified immutable asset bytes. Movie payloads are verified but stay streamable.
use crate::field_preload::{Manifest, Role};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Weak},
};

#[derive(Clone, Default)]
pub struct Files {
    pub manifests: BTreeMap<u32, Manifest>,
    pub bytes: BTreeMap<String, Arc<[u8]>>,
    pub disk_bytes: u64,
    pub reused_bytes: u64,
}
#[derive(Default)]
pub struct Cache(BTreeMap<String, Weak<[u8]>>);
impl Files {
    /// Combine already verified resource leases before publishing a scene.
    pub fn include(&mut self, other: &Self) -> Result<()> {
        for (path, bytes) in &other.bytes {
            if let Some(previous) = self.bytes.get(path) {
                ensure!(previous == bytes, "conflicting prepared resource {path}");
            }
        }
        self.bytes
            .extend(other.bytes.iter().map(|(p, b)| (p.clone(), b.clone())));
        self.disk_bytes += other.disk_bytes;
        self.reused_bytes += other.reused_bytes;
        Ok(())
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
    pub fn load(
        root: &Path,
        paths: &[&str],
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let mut manifests = BTreeMap::new();
        let mut inventory = BTreeMap::new();
        for path in paths {
            crate::validate_asset_path(path)?;
            let manifest: Manifest =
                serde_json::from_reader(std::io::BufReader::new(File::open(root.join(path))?))?;
            manifest.validate()?;
            ensure!(
                manifest.is_complete(),
                "incomplete field preparation manifest {path}: {:?}",
                manifest.missing_inputs
            );
            for (path, file) in &manifest.files {
                if let Some(previous) = inventory.get_mut(path) {
                    let previous: &mut crate::field_preload::File = previous;
                    ensure!(
                        previous.sha256 == file.sha256 && previous.bytes == file.bytes,
                        "inconsistent dependency {path}"
                    );
                    previous.roles.extend(&file.roles);
                } else {
                    inventory.insert(path.clone(), file.clone());
                }
            }
            manifests.insert(manifest.map_id, manifest);
        }
        let mut result = Self::load_inventory(root, &inventory, cache, cancelled)?;
        result.manifests = manifests;
        Ok(result)
    }

    /// Shared dependency verification for fields, battles and their resource leases.
    pub fn load_inventory(
        root: &Path,
        inventory: &BTreeMap<String, crate::field_preload::File>,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let mut result = Self::default();
        cache.0.retain(|_, bytes| bytes.strong_count() > 0);
        for (path, entry) in inventory {
            crate::validate_asset_path(path)?;
            ensure!(
                entry.sha256.len() == 64
                    && entry.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && !entry.roles.is_empty(),
                "invalid dependency {path}"
            );
            ensure!(!cancelled(), "asset preparation cancelled");
            let mut file =
                File::open(root.join(path)).with_context(|| format!("read dependency {path}"))?;
            ensure!(
                file.metadata()?.len() == entry.bytes,
                "dependency size differs: {path}"
            );
            let mut hash = Sha256::new();
            let mut bytes = Vec::new();
            let mut buffer = [0; 64 * 1024];
            let mut read = 0u64;
            loop {
                ensure!(!cancelled(), "asset preparation cancelled");
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                read += count as u64;
                ensure!(read <= entry.bytes, "dependency grew while reading: {path}");
                hash.update(&buffer[..count]);
                if !entry.roles.contains(&Role::Movie) {
                    bytes.extend_from_slice(&buffer[..count]);
                }
                result.disk_bytes += count as u64;
            }
            ensure!(
                read == entry.bytes,
                "dependency shrank while reading: {path}"
            );
            ensure!(
                format!("{:x}", hash.finalize()) == entry.sha256,
                "dependency digest differs: {path}"
            );
            if !entry.roles.contains(&Role::Movie) {
                // Verify aliases too: a matching declared digest must not hide
                // a missing or corrupt payload, even while another field holds it.
                let bytes: Arc<[u8]> =
                    if let Some(shared) = cache.0.get(&entry.sha256).and_then(Weak::upgrade) {
                        result.reused_bytes += entry.bytes;
                        shared
                    } else {
                        bytes.into()
                    };
                cache.0.insert(entry.sha256.clone(), Arc::downgrade(&bytes));
                result.bytes.insert(path.clone(), bytes);
            }
        }
        Ok(result)
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
}

//! Verified immutable field bytes. Movie payloads are verified but stay streamable.
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
        let mut result = Self::default();
        let mut inventory = BTreeMap::new();
        for path in paths {
            crate::validate_asset_path(path)?;
            let manifest: Manifest = serde_json::from_reader(File::open(root.join(path))?)?;
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
            result.manifests.insert(manifest.map_id, manifest);
        }
        cache.0.retain(|_, bytes| bytes.strong_count() > 0);
        for (path, entry) in inventory {
            ensure!(!cancelled(), "field preparation cancelled");
            let mut file = File::open(root.join(&path))
                .with_context(|| format!("read field dependency {path}"))?;
            ensure!(
                file.metadata()?.len() == entry.bytes,
                "field dependency size differs: {path}"
            );
            let mut hash = Sha256::new();
            let mut bytes = Vec::new();
            let mut buffer = [0; 64 * 1024];
            let mut read = 0u64;
            loop {
                ensure!(!cancelled(), "field preparation cancelled");
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                read += count as u64;
                ensure!(
                    read <= entry.bytes,
                    "field dependency grew while reading: {path}"
                );
                hash.update(&buffer[..count]);
                if !entry.roles.contains(&Role::Movie) {
                    bytes.extend_from_slice(&buffer[..count]);
                }
                result.disk_bytes += count as u64;
            }
            ensure!(
                read == entry.bytes,
                "field dependency shrank while reading: {path}"
            );
            ensure!(
                format!("{:x}", hash.finalize()) == entry.sha256,
                "field dependency digest differs: {path}"
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
                cache.0.insert(entry.sha256, Arc::downgrade(&bytes));
                result.bytes.insert(path, bytes);
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
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "resonance-prepared-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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

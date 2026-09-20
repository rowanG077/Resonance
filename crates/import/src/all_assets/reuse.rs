//! Verified final publications only; decoded intermediates never enter this index.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};

thread_local! {
    static PUBLICATIONS: RefCell<Option<Sender<PathBuf>>> = const { RefCell::new(None) };
}

pub(crate) fn current() -> Option<Sender<PathBuf>> {
    PUBLICATIONS.with_borrow(Clone::clone)
}

/// Every scheduler worker inherits the enclosing package's publication channel.
pub(crate) fn inherit(sender: Option<Sender<PathBuf>>) -> Scope {
    Scope(PUBLICATIONS.replace(sender), PhantomData)
}

pub(crate) struct Scope(Option<Sender<PathBuf>>, PhantomData<*mut ()>);
impl Drop for Scope {
    fn drop(&mut self) {
        PUBLICATIONS.replace(self.0.take());
    }
}

/// Call only after a final write succeeds, including byte-identical early returns.
pub(crate) fn published(path: &Path) {
    PUBLICATIONS.with_borrow(|sender| {
        if let Some(sender) = sender {
            let _ = sender.send(path.to_owned());
        }
    });
}

pub(crate) struct Capture {
    scope: Scope,
    events: Receiver<PathBuf>,
}

impl Capture {
    pub(crate) fn start() -> Self {
        let (sender, events) = mpsc::channel();
        Self {
            scope: inherit(Some(sender)),
            events,
        }
    }

    pub(crate) fn finish(self) -> Result<BTreeSet<PathBuf>> {
        drop(self.scope);
        // Package schedulers join their workers before returning to this owner.
        let mut files = BTreeSet::new();
        loop {
            match self.events.try_recv() {
                Ok(path) => {
                    files.insert(path);
                }
                Err(mpsc::TryRecvError::Disconnected) => return Ok(files),
                Err(mpsc::TryRecvError::Empty) => {
                    anyhow::bail!("publication worker outlived its package")
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Receipt {
    key: String,
    pub paths: Vec<String>,
    pub units: usize,
    files: BTreeMap<String, String>,
}

impl Receipt {
    pub(super) fn register(&self, root: &Path) -> Result<()> {
        for (path, hash) in &self.files {
            crate::publication::verified(&root.join(path), hash)?;
        }
        Ok(())
    }
}

pub(super) struct Cache {
    root: PathBuf,
    reader: String,
}

impl Cache {
    pub(super) fn open(output: &Path) -> Result<Self> {
        Ok(Self {
            root: output.canonicalize()?,
            // Include linked readers/codecs and their build options, not only a
            // manually bumped package version. Different builds miss conservatively.
            reader: crate::media::hash_file(&std::env::current_exe()?)?,
        })
    }

    pub(super) fn key(&self, inputs: &impl Serialize) -> Result<String> {
        Ok(crate::digest(&serde_json::to_vec(&(
            1,
            &self.reader,
            inputs,
        ))?))
    }

    fn path(&self, key: &str) -> PathBuf {
        self.root.join(".cook-receipts").join(format!("{key}.json"))
    }

    fn final_path(&self, path: &str) -> Result<PathBuf> {
        resonance_content::validate_asset_path(path)?;
        let path = self.root.join(path).canonicalize()?;
        ensure!(
            path.starts_with(&self.root),
            "publication escapes output root"
        );
        Ok(path)
    }

    pub(super) fn restore(&self, key: &str) -> Option<Receipt> {
        let receipt: Receipt = serde_json::from_slice(&fs::read(self.path(key)).ok()?).ok()?;
        if receipt.key != key || receipt.files.is_empty() || receipt.paths.is_empty() {
            return None;
        }
        self.verify_paths(&receipt.paths, &receipt.files).ok()?;
        for (path, hash) in &receipt.files {
            if crate::media::hash_file(&self.final_path(path).ok()?)
                .ok()
                .as_ref()
                != Some(hash)
            {
                return None;
            }
        }
        Some(receipt)
    }

    pub(super) fn invalidate(&self, key: &str) -> Result<()> {
        match fs::remove_file(self.path(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn publish(
        &self,
        key: &str,
        paths: &[String],
        units: usize,
        files: BTreeSet<PathBuf>,
    ) -> Result<()> {
        ensure!(
            !paths.is_empty() && !files.is_empty(),
            "successful asset has no final publications"
        );
        let mut fingerprints = BTreeMap::new();
        for path in files {
            let path = path.canonicalize()?;
            let relative = path
                .strip_prefix(&self.root)?
                .to_str()
                .context("non-UTF8 publication path")?;
            resonance_content::validate_asset_path(relative)?;
            fingerprints.insert(relative.to_owned(), crate::media::hash_file(&path)?);
        }
        self.verify_paths(paths, &fingerprints)?;
        crate::write_atomic(
            &self.path(key),
            &serde_json::to_vec(&Receipt {
                key: key.into(),
                paths: paths.to_vec(),
                units,
                files: fingerprints,
            })?,
        )
    }

    fn verify_paths(&self, paths: &[String], files: &BTreeMap<String, String>) -> Result<()> {
        for path in paths {
            let actual = self.final_path(path)?;
            let relative = actual
                .strip_prefix(&self.root)?
                .to_str()
                .context("non-UTF8 publication path")?;
            ensure!(
                if actual.is_file() {
                    files.contains_key(relative)
                } else {
                    files
                        .keys()
                        .any(|file| file.starts_with(&format!("{relative}/")))
                },
                "untracked final publication {path}"
            );
        }
        Ok(())
    }
}

/// Skip preparation only when the compiler, original inputs and every final output match.
/// Returns true on reuse. A nested caller still reports all outputs to its parent receipt.
pub(crate) fn cook(
    output: &Path,
    inputs: &impl Serialize,
    paths: &[&str],
    prepare: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let cache = Cache::open(output)?;
    let key = cache.key(inputs)?;
    if let Some(receipt) = cache.restore(&key) {
        receipt.register(output)?;
        return Ok(true);
    }
    cache.invalidate(&key)?;
    let capture = Capture::start();
    prepare()?;
    let files = capture.finish()?;
    {
        // Receipts describe final content; an enclosing receipt must not capture this index.
        let _capture = inherit(None);
        cache.publish(
            &key,
            &paths
                .iter()
                .map(|path| (*path).to_owned())
                .collect::<Vec<_>>(),
            1,
            files.clone(),
        )?;
    }
    for path in files {
        published(&path);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_receipts_include_nested_workers_and_unchanged_writes() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        let cache = Cache::open(root)?;
        let key = cache.key(&("source", "dependencies", "options"))?;
        crate::write_atomic(&root.join("unchanged"), b"same")?;
        let capture = Capture::start();
        let mut dag = super::super::pool::Dag::new();
        for index in [0, 1] {
            dag.add(index.to_string(), [], move |_, _| {
                crate::write_atomic(&root.join(format!("mesh-{index}")), &[index])?;
                crate::write_atomic(&root.join("unchanged"), b"same")?;
                Ok(())
            });
        }
        for result in dag.run(2, || (), |_| {})? {
            result?;
        }
        let files = capture.finish()?;
        assert_eq!(files.len(), 3);
        assert!(current().is_none());
        cache.publish(&key, &["mesh-0".into()], 3, files)?;
        assert_eq!(cache.restore(&key).unwrap().units, 3);
        {
            let _publications = crate::publication::Session::start(root)?;
            cache.restore(&key).unwrap().register(root)?;
            assert!(crate::write_atomic(&root.join("unchanged"), b"conflict").is_err());
        }
        let saved = fs::read(cache.path(&key))?;
        let mut altered = cache.restore(&key).unwrap();
        fs::create_dir(root.join("untracked"))?;
        altered.paths.push("untracked".into());
        fs::write(cache.path(&key), serde_json::to_vec(&altered)?)?;
        assert!(cache.restore(&key).is_none());
        fs::write(cache.path(&key), saved)?;
        assert!(
            cache
                .restore(&cache.key(&("changed", "dependencies", "options"))?)
                .is_none()
        );
        assert!(
            cache
                .restore(&cache.key(&("source", "changed", "options"))?)
                .is_none()
        );
        assert!(
            cache
                .restore(&cache.key(&("source", "dependencies", "changed"))?)
                .is_none()
        );
        let reader = Cache {
            root: cache.root.clone(),
            reader: "changed reader".into(),
        };
        assert!(
            reader
                .restore(&reader.key(&("source", "dependencies", "options"))?)
                .is_none()
        );
        fs::remove_file(root.join("unchanged"))?;
        assert!(cache.restore(&key).is_none());
        crate::write_atomic(&root.join("unchanged"), b"same")?;
        assert!(cache.restore(&key).is_some());
        fs::write(root.join("mesh-1"), b"corrupt")?;
        assert!(cache.restore(&key).is_none());
        cache.invalidate(&key)?;
        let capture = Capture::start();
        crate::write_atomic(&root.join("mesh-1"), b"partially repaired")?;
        drop(capture); // Failed computations never publish a receipt.
        assert!(current().is_none() && !cache.path(&key).exists());
        Ok(())
    }

    #[test]
    #[ignore = "requires original disc 1; private title-field cook, no codecs or devices"]
    fn original_file_receipt_verifies_shared_meshes_outside_its_source_directory() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let source = crate::scene::title_source(&extracted, &executable)?;
        let output = tempfile::tempdir()?;
        let cache = Cache::open(output.path())?;
        let key = cache.key(&crate::media::hash_file(
            &extracted.join("files").join(&source),
        )?)?;
        let capture = Capture::start();
        let paths = super::super::cook_source(
            &extracted,
            output.path(),
            &source,
            Some(super::super::roles::Role::Field),
        )?;
        cache.publish(&key, &paths, 1, capture.finish()?)?;
        let receipt = cache
            .restore(&key)
            .context("fresh file receipt was incomplete")?;
        let mesh = receipt
            .files
            .keys()
            .find(|path| path.starts_with("meshes/"))
            .context("original field emitted no shared meshes")?;
        fs::remove_file(output.path().join(mesh))?;
        assert!(
            cache.restore(&key).is_none(),
            "missing shared geometry must force conversion"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires original disc 1; complete menu previews in a private output root"]
    fn original_menu_receipt_skips_preparation_and_repairs_missing_shared_curves() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let output = tempfile::tempdir()?;
        let root = output.path();
        let outer = Capture::start();
        assert!(!crate::menu::cook(&extracted, root)?);
        let cold = outer.finish()?;
        for family in ["meshes", "textures", "clips", "monsters", "figurines"] {
            ensure!(
                cold.iter().any(|path| path.starts_with(root.join(family))),
                "receipt omitted {family}"
            );
        }
        ensure!(
            !cold
                .iter()
                .any(|path| path.starts_with(root.join(".cook-receipts"))),
            "parent receipt captured a cache index"
        );
        let motion = cold
            .iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "motion"))
            .context("menu emitted no shared animation")?;
        let expected = crate::media::hash_file(motion)?;
        let outer = Capture::start();
        assert!(crate::menu::cook(&extracted, root)?);
        assert_eq!(outer.finish()?, cold);
        fs::remove_file(motion)?;
        assert!(
            !crate::menu::cook(&extracted, root)?,
            "missing shared output must force preparation"
        );
        assert_eq!(crate::media::hash_file(motion)?, expected);
        assert!(crate::menu::cook(&extracted, root)?);
        Ok(())
    }
}

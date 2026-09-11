use super::*;
use anyhow::Context;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotId(String);
impl SlotId {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        ensure!(
            (1..=64).contains(&name.len())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
            "slot ID must contain 1..64 ASCII letters, digits, hyphens or underscores"
        );
        Ok(Self(name))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn path(&self, kind: Kind, slot: &SlotId) -> PathBuf {
        self.root
            .join(kind.directory())
            .join(format!("slot-{}.json", slot.0))
    }
    pub fn list(&self, kind: Kind) -> Result<Vec<SlotId>> {
        let entries = match fs::read_dir(self.root.join(kind.directory())) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut slots = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path.extension().and_then(|s| s.to_str()) == Some("json")
                && let Some(name) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("slot-"))
                && let Ok(slot) = SlotId::new(name)
            {
                slots.push(slot);
            }
        }
        slots.sort();
        Ok(slots)
    }
    pub fn read(&self, kind: Kind, slot: &SlotId) -> Result<Vec<u8>> {
        read_bounded(&self.path(kind, slot))
    }
    pub fn write(&self, kind: Kind, slot: &SlotId, bytes: &[u8]) -> Result<()> {
        inspect(bytes)?;
        let path = self.path(kind, slot);
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).context("create save directory")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path).context("replace save slot")?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
    /// The owner keeps this task until completion before writing the same slot again.
    pub fn write_async(&self, kind: Kind, slot: SlotId, bytes: Vec<u8>) -> Result<WriteTask> {
        let store = self.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("save-writer".into())
            .spawn(move || {
                let _ = sender.send(store.write(kind, &slot, &bytes));
            })?;
        Ok(WriteTask(receiver))
    }
}

pub struct WriteTask(mpsc::Receiver<Result<()>>);
impl WriteTask {
    pub fn poll(&self) -> Option<Result<()>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("save writer stopped")))
            }
        }
    }
    pub fn wait(self) -> Result<()> {
        self.0.recv().context("save writer stopped")?
    }
}

pub fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_FILE_BYTES as u64,
        "save exceeds 1 MiB"
    );
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_FILE_BYTES, "save exceeds 1 MiB");
    Ok(bytes)
}

pub fn default_directory() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    return Ok(PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .context("LOCALAPPDATA is unavailable; specify --save-directory")?,
    )
    .join("Resonance"));
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(not(target_os = "macos"))]
        if let Some(data) = std::env::var_os("XDG_DATA_HOME").filter(|p| Path::new(p).is_absolute())
        {
            return Ok(PathBuf::from(data).join("resonance"));
        }
        let home_directory = PathBuf::from(
            std::env::var_os("HOME").context("HOME is unavailable; specify --save-directory")?,
        );
        #[cfg(target_os = "macos")]
        return Ok(home_directory.join("Library/Application Support/Resonance"));
        #[cfg(not(target_os = "macos"))]
        Ok(home_directory.join(".local/share/resonance"))
    }
}

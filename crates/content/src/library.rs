//! Named, editable content and ordered mod overrides. Each preparation takes a
//! snapshot; readers reuse captured bytes instead of reopening mutable inputs.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

mod stream;
pub use stream::StreamReader;

pub const VERSION: u32 = 107;
/// The game contract, independent of the release used to import its content.
pub const GAME: &str = "tales-of-symphonia";
pub const DESCRIPTOR: &str = "content.json";
pub const FIELDS: &str = "fields/mappings.json";
pub const MOVIES: &str = "movies/mappings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u32,
    pub game: String,
}

impl Descriptor {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported cooked layout {}; expected {VERSION}; recook the original game",
            self.version
        );
        ensure!(
            self.game == GAME,
            "unsupported cooked game {}; expected {GAME}",
            self.game
        );
        Ok(())
    }
}

/// One gameplay lookup namespace. IDs use canonical unsigned decimal keys; bank
/// members get a separate mapping under their named bank, not a new global ID.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Mappings(BTreeMap<u32, String>);

impl<'de> Deserialize<'de> for Mappings {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(MappingValues::<String>::deserialize(deserializer)?.0).map_err(de::Error::custom)
    }
}

/// Canonical decimal IDs shared by typed catalogues.
#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct MappingValues<T = serde_json::Value>(pub(crate) BTreeMap<u32, T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for MappingValues<T> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
            type Value = MappingValues<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object mapping decimal IDs to resources")
            }
            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut entries = BTreeMap::new();
                while let Some((key, target)) = map.next_entry::<String, T>()? {
                    let id = key.parse::<u32>().map_err(de::Error::custom)?;
                    if id.to_string() != key {
                        return Err(de::Error::custom(format!(
                            "noncanonical mapping ID {key:?}"
                        )));
                    }
                    if entries.insert(id, target).is_some() {
                        return Err(de::Error::custom(format!("duplicate mapping ID {id}")));
                    }
                }
                Ok(MappingValues(entries))
            }
        }
        deserializer.deserialize_map(Visitor(std::marker::PhantomData))
    }
}

impl Mappings {
    pub fn new(entries: BTreeMap<u32, String>) -> Result<Self> {
        for target in entries.values() {
            validate_path(target)?;
        }
        Ok(Self(entries))
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &str)> {
        self.0.iter().map(|(&id, path)| (id, path.as_str()))
    }

    pub fn target(&self, id: u32) -> Result<&str> {
        self.0
            .get(&id)
            .map(String::as_str)
            .with_context(|| format!("unmapped resource ID {id}"))
    }

    /// Resolve a named declaration which requires one gameplay identity.
    pub fn id(&self, target: &str) -> Result<u32> {
        let mut matches = self
            .iter()
            .filter_map(|(id, name)| (name == target).then_some(id));
        let id = matches
            .next()
            .with_context(|| format!("unmapped resource {target}"))?;
        anyhow::ensure!(matches.next().is_none(), "ambiguous resource {target}");
        Ok(id)
    }

    /// Resolve the category-relative binding into a cooked-root-relative path.
    /// `entry` selects a directory domain's conventional entry point.
    pub fn resolve(&self, mapping: &str, id: u32, entry: Option<&str>) -> Result<String> {
        validate_mapping_path(mapping)?;
        let target = self.target(id).with_context(|| mapping.to_owned())?;
        let directory = mapping.strip_suffix("mappings.json").unwrap();
        let mut path = format!("{directory}{target}");
        if let Some(entry) = entry {
            validate_path(entry)?;
            path.push('/');
            path.push_str(entry);
        }
        validate_path(&path)?;
        Ok(path)
    }
}

/// Logical resource paths have one spelling and never leave their content root.
pub fn validate_path(path: &str) -> Result<()> {
    crate::validate_asset_path(path)?;
    ensure!(
        !path.chars().any(char::is_control)
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "invalid content path {path:?}"
    );
    Ok(())
}

fn validate_mapping_path(path: &str) -> Result<()> {
    validate_path(path)?;
    ensure!(is_mapping(path), "expected a mappings.json path: {path}");
    Ok(())
}

fn is_mapping(path: &str) -> bool {
    path == "mappings.json" || path.ends_with("/mappings.json")
}

#[derive(Debug, Clone)]
pub struct Mod {
    pub name: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
struct Layer {
    name: String,
    root: PathBuf,
}

impl Layer {
    fn new(name: String, root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root).with_context(|| format!("open content layer {name}"))?;
        ensure!(root.is_dir(), "content layer {name} is not a directory");
        Ok(Self { name, root })
    }

    /// A regular file directly in this layer.
    fn path(&self, logical: &str) -> Result<Option<PathBuf>> {
        let path = self.root.join(logical);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                ensure_regular(metadata.file_type(), false, &self.name, logical)?;
                Ok(Some(path))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| path.display().to_string()),
        }
    }
}

/// Symbolic links are never content: not every supported platform has them,
/// and a link could leave its layer.
fn ensure_regular(kind: fs::FileType, directory: bool, layer: &str, path: &str) -> Result<()> {
    ensure!(
        !kind.is_symlink(),
        "symbolic links are not supported in content; copy the file or directory instead ({layer}: {path})"
    );
    ensure!(
        kind.is_file() || directory && kind.is_dir(),
        "content resource is not a regular file in {layer}: {path}"
    );
    Ok(())
}

/// Discover automatic mods in numeric priority order. Files (such as a README)
/// and hidden build directories are ignored; enabled directory names contain
/// exactly six digits and a name.
pub fn discover_mods(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read mods {}", directory.display()));
        }
    };
    let mut mods = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("non-UTF-8 mod directory name")?;
        let kind = entry.file_type()?;
        if kind.is_file() || name.starts_with('.') {
            continue;
        }
        ensure_regular(kind, true, "mods", name)?;
        ensure!(
            name.len() > 7
                && name.as_bytes()[..6].iter().all(u8::is_ascii_digit)
                && name.as_bytes()[6] == b'-'
                && !name[7..].trim().is_empty(),
            "invalid mod directory {name:?}; expected NNNNNN-modname"
        );
        mods.push(entry.path());
    }
    // Six-digit priorities sort numerically; equal priorities use the full name.
    mods.sort();
    Ok(mods)
}

#[derive(Debug)]
struct IndexedFile {
    physical: PathBuf,
    overrides: Vec<Override>,
    /// Composed documents are resolved once at startup, shared by preparations.
    resolved: Option<Arc<[u8]>>,
}

#[derive(Debug, Default)]
struct Index {
    files: BTreeMap<String, IndexedFile>,
    directories: BTreeSet<String>,
    documents: BTreeSet<String>,
}

impl Index {
    fn build(layers: &[Layer]) -> Result<Self> {
        let mut index = Self::default();
        let mut spelling = BTreeMap::new();
        let mut owners = BTreeMap::new();
        for layer in layers {
            let mut pending = vec![String::new()];
            while let Some(parent) = pending.pop() {
                for entry in fs::read_dir(layer.root.join(&parent))? {
                    let entry = entry?;
                    let name = entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("non-UTF-8 content name in {parent}"))?;
                    // Version control and editor state beside a mod is not content.
                    if name.starts_with('.') {
                        continue;
                    }
                    let path = if parent.is_empty() {
                        name
                    } else {
                        format!("{parent}/{name}")
                    };
                    validate_path(&path)?;
                    if let Some(previous) = spelling.insert(path.to_lowercase(), path.clone()) {
                        ensure!(
                            previous == path,
                            "case-conflicting content paths: {previous} and {path}"
                        );
                    }
                    let kind = entry.file_type()?;
                    ensure_regular(kind, true, &layer.name, &path)?;
                    let physical = layer.root.join(&path);
                    if kind.is_dir() {
                        ensure!(
                            !index.files.contains_key(&path),
                            "content path is both file and directory: {path}"
                        );
                        index.directories.insert(path.clone());
                        pending.push(path);
                        continue;
                    }
                    ensure!(
                        !index.directories.contains(&path),
                        "content path is both file and directory: {path}"
                    );
                    let selected = index
                        .files
                        .entry(path.clone())
                        .or_insert_with(|| IndexedFile {
                            physical: physical.clone(),
                            overrides: Vec::new(),
                            resolved: None,
                        });
                    if let Some(previous) = owners.insert(path.clone(), layer.name.clone()) {
                        selected.overrides.push(Override {
                            path: path.clone(),
                            previous,
                            replacement: layer.name.clone(),
                        });
                    }
                    selected.physical = physical;
                }
            }
        }
        index.resolve_documents()?;
        Ok(index)
    }

    fn resolve_documents(&mut self) -> Result<()> {
        let paths = self.files.keys().cloned().collect();
        let layouts = crate::authoring::layouts(&paths);
        // Documents are independent, so their owners are read and assembled
        // on every core.
        let workers = std::thread::available_parallelism().map_or(1, |count| count.get());
        let resolved = std::thread::scope(|scope| {
            let jobs = layouts
                .chunks(layouts.len().div_ceil(workers).max(1))
                .map(|layouts| {
                    scope.spawn(|| {
                        layouts
                            .iter()
                            .map(|layout| self.resolve(layout, &paths))
                            .collect::<Result<Vec<_>>>()
                    })
                })
                .collect::<Vec<_>>();
            jobs.into_iter()
                .map(|job| job.join().expect("document resolution panicked"))
                .collect::<Result<Vec<_>>>()
        })?;
        for (layout, file) in layouts.into_iter().zip(resolved.into_iter().flatten()) {
            self.documents.insert(layout.document.clone());
            self.files.insert(layout.document, file);
        }
        Ok(())
    }

    fn resolve(
        &self,
        layout: &crate::authoring::Layout,
        paths: &BTreeSet<String>,
    ) -> Result<IndexedFile> {
        ensure!(
            !self.files.contains_key(&layout.document),
            "{} has both a whole document and granular owners; use its source directory",
            layout.document
        );
        let mut overrides = Vec::new();
        let mut read = |path: &str| -> Result<serde_json::Value> {
            let source = self
                .files
                .get(path)
                .with_context(|| format!("missing authored input {path}"))?;
            let bytes = read_limited(&source.physical, crate::preparation::JSON_LIMIT)?;
            let value = crate::authoring::decode(&bytes)
                .with_context(|| format!("decode authored input {path}"))?;
            overrides.extend(source.overrides.iter().cloned());
            Ok(value)
        };
        let document = layout.assemble(paths, &mut read)?;
        let bytes = serde_json::to_vec(&document)?;
        ensure!(
            bytes.len() <= crate::preparation::JSON_LIMIT,
            "assembled document exceeds size limit: {}",
            layout.document
        );
        Ok(IndexedFile {
            // An assembled document is never opened as a file.
            physical: PathBuf::new(),
            overrides,
            resolved: Some(bytes.into()),
        })
    }
}

/// Immutable startup selection shared by every preparation. Granular documents
/// are assembled and frozen here. Media bytes, and a document stored as one
/// file (as test fixtures do; a cooked library has none), remain lazy and are
/// pinned per preparation. Namespace changes require a restart.
#[derive(Debug, Clone)]
pub struct Library {
    pub descriptor: Descriptor,
    index: Arc<Index>,
}

impl Library {
    /// Command-line layer order is explicit; the last directory has priority.
    /// Mods need no registration manifest or globally unique folder basename.
    pub fn open_paths(root: &Path, mods: &[PathBuf]) -> Result<Self> {
        let mods: Vec<_> = mods
            .iter()
            .enumerate()
            .map(|(index, root)| Mod {
                name: format!("mod {} ({})", index + 1, root.display()),
                root: root.clone(),
            })
            .collect();
        Self::open(root, &mods)
    }

    pub fn open(root: &Path, mods: &[Mod]) -> Result<Self> {
        let base = Layer::new("base".into(), root)?;
        let descriptor_path = base
            .path(DESCRIPTOR)?
            .context("missing content.json; recook the original game")?;
        let descriptor: Descriptor = serde_json::from_slice(&read_limited(&descriptor_path, 4096)?)
            .context("decode content.json")?;
        descriptor.validate()?;
        let mut names = BTreeSet::from(["base".to_owned()]);
        let mut roots = BTreeSet::from([base.root.clone()]);
        let mut layers = vec![base];
        for modification in mods {
            ensure!(
                !modification.name.trim().is_empty()
                    && !modification.name.chars().any(char::is_control)
                    && names.insert(modification.name.clone()),
                "invalid or duplicate mod name {:?}",
                modification.name
            );
            let layer = Layer::new(modification.name.clone(), &modification.root)?;
            ensure!(
                roots.insert(layer.root.clone()),
                "content layer is enabled twice: {}",
                layer.root.display()
            );
            ensure!(
                layer.path(DESCRIPTOR)?.is_none(),
                "mod {} cannot override content.json",
                layer.name
            );
            layers.push(layer);
        }
        let index = Arc::new(Index::build(&layers)?);
        Ok(Self { descriptor, index })
    }

    /// Startup-assembled document views for inspectors and authoring tools.
    /// This shares cached bytes and never reopens the underlying source files.
    pub fn resolved_documents(&self) -> impl Iterator<Item = (&str, Arc<[u8]>)> {
        self.index.documents.iter().map(|path| {
            (
                path.as_str(),
                self.index.files[path].resolved.as_ref().unwrap().clone(),
            )
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            library: self.clone(),
            bytes: BTreeMap::new(),
            mappings: BTreeMap::new(),
            overrides: Vec::new(),
            disk_bytes: 0,
            directories: BTreeMap::new(),
            streams: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub path: String,
    pub previous: String,
    pub replacement: String,
}

pub(crate) struct CapturedFiles {
    pub library: Library,
    pub disk_bytes: u64,
    pub overrides: Vec<Override>,
    pub files: BTreeMap<String, Arc<[u8]>>,
}

/// Captures each input on its first read. Parsing and compilation all
/// use these same bytes. Startup-resolved metadata stays fixed; a new preparation
/// can capture edits to ordinary media without recomposing the namespace.
pub struct Snapshot {
    library: Library,
    bytes: BTreeMap<String, Arc<[u8]>>,
    mappings: BTreeMap<String, Mappings>,
    overrides: Vec<Override>,
    disk_bytes: u64,
    directories: BTreeMap<String, BTreeSet<String>>,
    streams: BTreeMap<String, File>,
}

impl Snapshot {
    pub fn contains_key(&self, path: &str) -> bool {
        self.bytes.contains_key(path)
    }

    pub(crate) fn captured(&self) -> CapturedFiles {
        CapturedFiles {
            library: self.library.clone(),
            disk_bytes: self.disk_bytes,
            overrides: self.overrides.clone(),
            files: self.bytes.clone(),
        }
    }

    /// Enumerate the startup index without accessing the filesystem.
    pub fn files_under(&mut self, directory: &str) -> Result<&BTreeSet<String>> {
        validate_path(directory)?;
        ensure!(
            !self.library.index.files.contains_key(directory),
            "content namespace is not a directory: {directory}"
        );
        if !self.directories.contains_key(directory) {
            let prefix = format!("{directory}/");
            let files = self
                .library
                .index
                .files
                .range(prefix.clone()..)
                .take_while(|(path, _)| path.starts_with(&prefix))
                .map(|(path, _)| path.clone())
                .collect();
            self.directories.insert(directory.into(), files);
        }
        Ok(&self.directories[directory])
    }

    pub(crate) fn into_captured(self) -> CapturedFiles {
        CapturedFiles {
            library: self.library,
            disk_bytes: self.disk_bytes,
            overrides: self.overrides,
            files: self.bytes,
        }
    }

    pub fn overrides(&self) -> &[Override] {
        &self.overrides
    }

    pub fn descriptor(&self) -> &Descriptor {
        &self.library.descriptor
    }

    pub fn read(&mut self, path: &str, limit: usize) -> Result<Arc<[u8]>> {
        validate_path(path)?;
        if !self.bytes.contains_key(path) {
            let (bytes, overrides) = if let Some((selected, bytes)) = self
                .library
                .index
                .files
                .get(path)
                .and_then(|file| file.resolved.as_ref().map(|bytes| (file, bytes)))
            {
                ensure!(
                    bytes.len() <= limit,
                    "content resource exceeds {limit} bytes: {path}"
                );
                (bytes.clone(), selected.overrides.clone())
            } else if path == DESCRIPTOR {
                (
                    serde_json::to_vec_pretty(&self.library.descriptor)?.into(),
                    vec![],
                )
            } else {
                let (physical, overrides) = self.select(path)?;
                let bytes = read_limited(&physical, limit)
                    .with_context(|| format!("read content {path}"))?;
                self.disk_bytes += bytes.len() as u64;
                (bytes.into(), overrides)
            };
            self.bytes.insert(path.into(), bytes);
            self.overrides.extend(overrides);
        }
        let captured = &self.bytes[path];
        ensure!(
            captured.len() <= limit,
            "content resource exceeds {limit} bytes: {path}"
        );
        Ok(captured.clone())
    }

    /// Capture an optional resource from the startup selection. A new
    /// preparation refreshes ordinary media; resolved metadata stays at startup.
    /// Additions require reopening the library.
    pub fn read_optional(&mut self, path: &str, limit: usize) -> Result<Option<Arc<[u8]>>> {
        validate_path(path)?;
        if !self.bytes.contains_key(path)
            && path != DESCRIPTOR
            && self.select_optional(path)?.is_none()
        {
            return Ok(None);
        }
        self.read(path, limit).map(Some)
    }

    pub fn json<T: de::DeserializeOwned>(&mut self, path: &str, limit: usize) -> Result<T> {
        serde_json::from_slice(&self.read(path, limit)?)
            .with_context(|| format!("decode content {path}"))
    }

    pub fn mappings(&mut self, path: &str) -> Result<&Mappings> {
        validate_mapping_path(path)?;
        if !self.mappings.contains_key(path) {
            let mappings = self.json(path, crate::preparation::JSON_LIMIT)?;
            self.mappings.insert(path.into(), mappings);
        }
        Ok(&self.mappings[path])
    }

    pub fn resolve(&mut self, mapping: &str, id: u32, entry: Option<&str>) -> Result<String> {
        let path = self.mappings(mapping)?.resolve(mapping, id, entry)?;
        if !self.bytes.contains_key(&path) {
            self.select(&path)
                .with_context(|| format!("{mapping} ID {id} refers to {path}"))?;
        }
        Ok(path)
    }

    pub fn field(&mut self, id: u32) -> Result<String> {
        self.resolve(FIELDS, id, Some("field.json"))
    }

    pub fn movie(&mut self, id: u32) -> Result<String> {
        self.resolve(MOVIES, id, None)
    }

    /// Keep this handle alive for streamed playback. Do not reopen through the
    /// base root, which would lose mod selection and replacement-file identity.
    pub fn open_stream(&mut self, path: &str) -> Result<File> {
        validate_path(path)?;
        ensure!(path != DESCRIPTOR, "metadata is not a stream: {path}");
        if !self.streams.contains_key(path) {
            ensure!(
                self.library
                    .index
                    .files
                    .get(path)
                    .is_none_or(|file| file.resolved.is_none()),
                "resolved metadata is not a media stream: {path}"
            );
            let (physical, overrides) = self.select(path)?;
            let file =
                File::open(&physical).with_context(|| format!("open content stream {path}"))?;
            ensure!(
                file.metadata()?.is_file(),
                "content stream is not a file: {path}"
            );
            self.overrides.extend(overrides);
            self.streams.insert(path.into(), file);
        }
        // A repeated capture retains the same selected file even after atomic
        // path replacement. Media readers use positional I/O: duplicated OS
        // handles are not independent seek cursors on every platform.
        Ok(self.streams[path].try_clone()?)
    }

    fn select(&self, path: &str) -> Result<(PathBuf, Vec<Override>)> {
        self.select_optional(path)?
            .with_context(|| format!("missing content resource {path}"))
    }

    fn select_optional(&self, path: &str) -> Result<Option<(PathBuf, Vec<Override>)>> {
        ensure!(
            !self.library.index.directories.contains(path),
            "content resource is not a file: {path}"
        );
        Ok(self
            .library
            .index
            .files
            .get(path)
            .map(|selected| (selected.physical.clone(), selected.overrides.clone())))
    }
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| path.display().to_string())?;
    let length = file.metadata()?.len();
    ensure!(
        length <= limit as u64,
        "content resource exceeds {limit} bytes: {}",
        path.display()
    );
    let mut bytes = Vec::with_capacity(length as usize);
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "content resource grew beyond {limit} bytes: {}",
        path.display()
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests;

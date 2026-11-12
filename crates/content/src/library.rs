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

mod opened;
use opened::Opened;
mod stream;
pub use stream::StreamReader;
mod capture;
pub use capture::Capture;

pub const VERSION: u32 = 108;
/// The game contract, independent of the release used to import its content.
pub const GAME: &str = "tales-of-symphonia";
pub const DESCRIPTOR: &str = "content.json";
/// Layer name of the cooked game beneath every mod.
pub const BASE: &str = "base";
/// Optional description at the root of a mod. It is not content.
pub const MANIFEST: &str = "mod.json";
/// Root files of a mod which describe it and are never content.
const MOD_NOTES: [&str; 5] = [
    MANIFEST,
    "README.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
];
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

/// What a mod says about itself. A mod works without one; declaring `layout`
/// turns a stale mod into one clear startup error instead of scattered
/// failures in whichever documents changed shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Cooked layout version the mod's files were written for.
    pub layout: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Mods which supply this file, or any owner of this assembled document.
    mods: BTreeSet<String>,
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
                    // Nor is what a mod says about itself at its root.
                    if parent.is_empty() && layer.name != BASE && MOD_NOTES.contains(&&*name) {
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
                            mods: BTreeSet::new(),
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
                    // Only the selected file can explain a later failure.
                    selected.mods.clear();
                    if layer.name != BASE {
                        selected.mods.insert(layer.name.clone());
                    }
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
        let mut mods = BTreeSet::new();
        let mut read = |path: &str| -> Result<serde_json::Value> {
            let source = self
                .files
                .get(path)
                .with_context(|| format!("missing authored input {path}"))?;
            mods.extend(source.mods.iter().cloned());
            let bytes = read_limited(&source.physical, crate::preparation::JSON_LIMIT)?;
            let value = crate::authoring::decode(&bytes)
                .with_context(|| format!("decode authored input {}", source.physical.display()))?;
            overrides.extend(source.overrides.iter().cloned());
            Ok(value)
        };
        let assembled = layout.assemble(paths, &mut read).and_then(|mut document| {
            crate::authoring::allocate(&layout.document, &mut document)?;
            Ok(document)
        });
        let document = assembled
            .with_context(|| format!("assemble {}{}", layout.document, contributed(&mods)))?;
        let bytes = serde_json::to_vec(&document)?;
        ensure!(
            bytes.len() <= crate::preparation::JSON_LIMIT,
            "assembled document exceeds size limit: {}",
            layout.document
        );
        Ok(IndexedFile {
            // An assembled document is never opened as a file.
            physical: PathBuf::new(),
            mods,
            overrides,
            resolved: Some(bytes.into()),
        })
    }
}

/// Error suffix naming the mods behind a document, so a failure in a resolved
/// view points at the directories a player can disable or fix.
fn contributed(mods: &BTreeSet<String>) -> String {
    if mods.is_empty() {
        String::new()
    } else {
        let names: Vec<_> = mods.iter().map(String::as_str).collect();
        format!(" (with content from {})", names.join(", "))
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
    layers: Vec<Layer>,
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
        let base = Layer::new(BASE.into(), root)?;
        let descriptor_path = base
            .open(DESCRIPTOR)?
            .context("missing content.json; recook the original game")?;
        let descriptor: Descriptor = serde_json::from_slice(&read_opened(descriptor_path, 4096)?)
            .context("decode content.json")?;
        descriptor.validate()?;
        let mut names = BTreeSet::from([BASE.to_owned()]);
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
            if let Some(path) = layer.path(MANIFEST)? {
                let manifest: Manifest = serde_json::from_slice(&read_limited(&path, 65536)?)
                    .with_context(|| format!("decode {}", path.display()))?;
                ensure!(
                    manifest.layout == VERSION,
                    "{} was made for cooked layout {}, and this game uses layout {VERSION}; update or disable the mod",
                    layer.name,
                    manifest.layout
                );
            }
            layers.push(layer);
        }
        let index = Arc::new(Index::build(&layers)?);
        Ok(Self {
            descriptor,
            index,
            layers,
        })
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

    /// Error suffix naming the mods which supply `path`, or any owner of that
    /// resolved document. Empty for content that comes from the base game alone.
    pub fn attribution(&self, path: &str) -> String {
        self.index
            .files
            .get(path)
            .map(|file| contributed(&file.mods))
            .unwrap_or_default()
    }

    /// Every file a later layer replaced, in path order. Replacing base content
    /// is the purpose of a mod; one mod replacing another's file is a conflict
    /// the player should know about.
    pub fn overrides(&self) -> impl Iterator<Item = &Override> {
        self.index.files.values().flat_map(|file| &file.overrides)
    }

    /// Top-level names in a mod which the base library lacks: usually a
    /// misspelled category, which nothing would ever read.
    pub fn unfamiliar_roots(&self) -> Vec<(String, String)> {
        let entries = |layer: &Layer| -> BTreeSet<String> {
            fs::read_dir(&layer.root)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| !name.starts_with('.'))
                .collect()
        };
        let base = entries(&self.layers[0]);
        self.layers[1..]
            .iter()
            .flat_map(|layer| {
                entries(layer)
                    .into_iter()
                    .filter(|name| !base.contains(name))
                    .map(|name| (layer.name.clone(), name))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Every logical path of the library: selected files and resolved documents.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.index.files.keys().map(String::as_str)
    }

    /// Stored size of a selected file, or the size of a resolved document.
    pub fn size(&self, path: &str) -> Result<u64> {
        let file = self
            .index
            .files
            .get(path)
            .with_context(|| format!("missing content {path}"))?;
        Ok(match &file.resolved {
            Some(bytes) => bytes.len() as u64,
            None => fs::metadata(&file.physical)
                .with_context(|| file.physical.display().to_string())?
                .len(),
        })
    }

    /// Files which two or more mods supply at the same path.
    pub fn conflicts(&self) -> impl Iterator<Item = &Override> {
        self.overrides().filter(|entry| entry.previous != BASE)
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
            common: None,
            reused: None,
            source: None,
            read_locks: BTreeMap::new(),
            missing: BTreeSet::new(),
        }
    }

    /// A scene can inherit the fixed startup inputs, but never another scene.
    /// Tools use `snapshot()` to keep seeing current filesystem edits.
    pub fn snapshot_with(&self, common: &crate::prepared::Files) -> Result<Snapshot> {
        ensure!(
            common
                .library()
                .is_some_and(|library| library.layers == self.layers),
            "startup inputs belong to a different content namespace"
        );
        let mut snapshot = self.snapshot();
        snapshot.common = Some(common.startup_bytes()?);
        snapshot.overrides.extend_from_slice(common.overrides());
        Ok(snapshot)
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
    pub common: Option<Arc<crate::prepared::Bytes>>,
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
    common: Option<Arc<crate::prepared::Bytes>>,
    // A temporary read source, discarded when producing Files; never an inherited scene owner.
    reused: Option<Arc<crate::prepared::Files>>,
    source: Option<Arc<std::sync::Mutex<Snapshot>>>,
    read_locks: BTreeMap<String, Arc<std::sync::Mutex<()>>>,
    missing: BTreeSet<String>,
}

impl Snapshot {
    /// Runtime transitions may reuse requested bytes from their preceding
    /// preparation. Output Files own only those bytes, never this scene owner.
    pub fn reuse(&mut self, files: Arc<crate::prepared::Files>) -> Result<()> {
        // Views resolve first reads through their shared source alone.
        ensure!(
            self.source.is_none(),
            "reused inputs must be chosen before a capture is shared"
        );
        ensure!(
            files
                .library()
                .is_some_and(|library| self.uses_library(library)),
            "reused inputs belong to a different content namespace"
        );
        self.reused = Some(files);
        Ok(())
    }

    pub fn uses_library(&self, library: &Library) -> bool {
        self.library.layers == library.layers
    }

    pub fn contains_key(&self, path: &str) -> bool {
        self.bytes.contains_key(path)
            || self
                .common
                .as_ref()
                .is_some_and(|bytes| bytes.contains_key(path))
    }

    pub(crate) fn captured(&self) -> CapturedFiles {
        CapturedFiles {
            library: self.library.clone(),
            disk_bytes: self.disk_bytes,
            overrides: self.overrides.clone(),
            files: self.bytes.clone(),
            common: self.common.clone(),
        }
    }

    /// Enumerate the startup index without accessing the filesystem.
    pub fn files_under(&mut self, directory: &str) -> Result<&BTreeSet<String>> {
        if !self.directories.contains_key(directory)
            && let Some(source) = &self.source
        {
            let paths = source.lock().unwrap().files_under(directory)?.clone();
            self.directories.insert(directory.into(), paths);
        }
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
            common: self.common,
        }
    }

    pub fn overrides(&self) -> &[Override] {
        &self.overrides
    }

    pub fn descriptor(&self) -> &Descriptor {
        &self.library.descriptor
    }

    pub fn read(&mut self, path: &str, limit: usize) -> Result<Arc<[u8]>> {
        self.observe(path, limit, false)?
            .with_context(|| format!("missing content resource {path}"))
    }

    /// Absence is retained too. Namespace additions require reopening the library.
    pub fn read_optional(&mut self, path: &str, limit: usize) -> Result<Option<Arc<[u8]>>> {
        self.observe(path, limit, true)
    }

    fn observe(&mut self, path: &str, limit: usize, optional: bool) -> Result<Option<Arc<[u8]>>> {
        validate_path(path)?;
        if self.missing.contains(path) {
            ensure!(
                optional,
                "missing content resource {path} in this preparation"
            );
            return Ok(None);
        }
        if self.source.is_some() && !self.bytes.contains_key(path) {
            return self.observe_shared(path, limit, optional);
        }
        let checked = |bytes: Arc<[u8]>| -> Result<_> {
            ensure!(
                bytes.len() <= limit,
                "content resource exceeds {limit} bytes: {path}"
            );
            Ok(Some(bytes))
        };
        if let Some(bytes) = self
            .bytes
            .get(path)
            .or_else(|| self.common.as_ref().and_then(|bytes| bytes.get(path)))
        {
            return checked(bytes.clone());
        }
        if let Some((files, bytes)) = self
            .reused
            .as_ref()
            .and_then(|files| files.read(path).ok().map(|bytes| (files, bytes)))
        {
            let result = checked(bytes.clone())?;
            self.overrides.extend(
                files
                    .overrides()
                    .iter()
                    .filter(|row| row.path == path)
                    .cloned(),
            );
            self.bytes.insert(path.into(), bytes);
            return Ok(result);
        }
        let captured = if let Some(selected) = self.library.index.files.get(path)
            && let Some(bytes) = &selected.resolved
        {
            Some((bytes.clone(), selected.overrides.clone()))
        } else if path == DESCRIPTOR {
            Some((
                serde_json::to_vec_pretty(&self.library.descriptor)?.into(),
                vec![],
            ))
        } else {
            self.open_optional(path)?
                .map(|(file, overrides)| -> Result<_> {
                    let bytes =
                        read_opened(file, limit).with_context(|| format!("read content {path}"))?;
                    self.disk_bytes += bytes.len() as u64;
                    Ok((Arc::<[u8]>::from(bytes), overrides))
                })
                .transpose()?
        };
        let Some((bytes, overrides)) = captured else {
            let kind = if is_mapping(path) {
                "mapping"
            } else {
                "resource"
            };
            ensure!(optional, "missing content {kind} {path}");
            self.missing.insert(path.into());
            return Ok(None);
        };
        let bytes: Arc<[u8]> = bytes.into();
        let result = checked(bytes.clone())?;
        self.bytes.insert(path.into(), bytes);
        self.overrides.extend(overrides);
        Ok(result)
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
        if !self.contains_key(&path)
            && !self
                .reused
                .as_ref()
                .is_some_and(|files| files.contains_key(&path))
            && !self
                .source
                .as_ref()
                .is_some_and(|source| source.lock().unwrap().contains_key(&path))
        {
            self.select_optional(&path)?
                .with_context(|| format!("{mapping} ID {id} refers to missing {path}"))?;
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
        if !self.streams.contains_key(path)
            && let Some(source) = &self.source
        {
            let mut source = source.lock().unwrap();
            let file = source.open_stream(path)?;
            self.overrides.extend(
                source
                    .overrides
                    .iter()
                    .filter(|row| row.path == path)
                    .cloned(),
            );
            self.streams.insert(path.into(), file);
        }
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
            let (file, overrides) = self
                .open_optional(path)?
                .with_context(|| format!("missing content stream {path}"))?;
            self.overrides.extend(overrides);
            self.streams.insert(path.into(), file.file);
        }
        // A repeated capture retains the same selected file even after atomic
        // path replacement. Media readers use positional I/O: duplicated OS
        // handles are not independent seek cursors on every platform.
        Ok(self.streams[path].try_clone()?)
    }

    fn open_optional(&self, path: &str) -> Result<Option<(Opened, Vec<Override>)>> {
        let Some((physical, overrides)) = self.select_optional(path)? else {
            return Ok(None);
        };
        let layer = self
            .library
            .layers
            .iter()
            .rev()
            .find(|layer| layer.root.join(path) == physical)
            .context("selected content file has no owning layer")?;
        let file = layer
            .open(path)?
            .with_context(|| format!("selected content resource disappeared: {path}"))?;
        Ok(Some((file, overrides)))
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
    let metadata = file.metadata()?;
    read_opened(
        Opened {
            file,
            path: path.to_owned(),
            metadata,
        },
        limit,
    )
}

fn read_opened(opened: Opened, limit: usize) -> Result<Vec<u8>> {
    let Opened {
        file,
        path,
        metadata,
    } = opened;
    let length = metadata.len();
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

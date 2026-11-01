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

pub const VERSION: u32 = 102;
/// The game contract, independent of the release used to import its content.
pub const GAME: &str = "tales-of-symphonia";
pub const DESCRIPTOR: &str = "content.json";
pub const FIELDS: &str = "fields/mappings.json";
pub const MOVIES: &str = "movies/mappings.json";
const MAX_MAPPING_BYTES: usize = 16 * 1024 * 1024;

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

/// Canonical decimal IDs shared by typed catalogues and per-ID mod merging.
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

    fn path(&self, logical: &str) -> Result<Option<PathBuf>> {
        let Some(canonical) = self.entry(logical)? else {
            return Ok(None);
        };
        ensure!(
            canonical.is_file(),
            "content resource is not a file in {}: {logical}",
            self.name
        );
        Ok(Some(canonical))
    }

    fn entry(&self, logical: &str) -> Result<Option<PathBuf>> {
        let path = self.root.join(logical);
        let canonical = match fs::canonicalize(&path) {
            Ok(path) => path,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // A dangling override is invalid, not permission to fall back.
                ensure!(
                    !fs::symlink_metadata(&path).is_ok(),
                    "dangling content link: {}",
                    path.display()
                );
                return Ok(None);
            }
            Err(error) => return Err(error).with_context(|| path.display().to_string()),
        };
        ensure!(
            canonical.starts_with(&self.root),
            "content path escapes layer {}: {logical}",
            self.name
        );
        Ok(Some(canonical))
    }
}

/// Base plus enabled mods in ascending priority. Construction does no recursive
/// library scan; resources and mapping scopes are opened only when requested.
#[derive(Debug, Clone)]
pub struct Library {
    pub descriptor: Descriptor,
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
        Ok(Self { descriptor, layers })
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            library: self.clone(),
            bytes: BTreeMap::new(),
            mappings: BTreeMap::new(),
            overrides: Vec::new(),
            disk_bytes: 0,
            directories: BTreeMap::new(),
            missing: BTreeSet::new(),
            streams: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub path: String,
    /// None denotes a whole-file replacement; Some retains the ID's bank scope.
    pub id: Option<u32>,
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
/// use these same bytes. A new preparation takes a new snapshot to see edits.
pub struct Snapshot {
    library: Library,
    bytes: BTreeMap<String, Arc<[u8]>>,
    mappings: BTreeMap<String, Mappings>,
    overrides: Vec<Override>,
    disk_bytes: u64,
    directories: BTreeMap<String, BTreeSet<String>>,
    missing: BTreeSet<String>,
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

    /// Discover an authored namespace such as scripts/ from the effective view.
    /// File additions in mods participate without another hand-maintained index.
    /// The first enumeration is retained for this preparation, like file bytes.
    pub fn files_under(&mut self, directory: &str) -> Result<&BTreeSet<String>> {
        validate_path(directory)?;
        if !self.directories.contains_key(directory) {
            let mut files = BTreeSet::new();
            let mut spelling = BTreeMap::new();
            for layer in &self.library.layers {
                let Some(root) = layer.entry(directory)? else {
                    continue;
                };
                ensure!(
                    root.is_dir(),
                    "content namespace is not a directory: {directory} in {}",
                    layer.name
                );
                ensure!(
                    !fs::symlink_metadata(layer.root.join(directory))?.is_symlink(),
                    "directory aliases are unsupported in content namespaces: {directory}"
                );
                let mut pending = vec![directory.to_owned()];
                while let Some(parent) = pending.pop() {
                    for entry in fs::read_dir(layer.root.join(&parent))? {
                        let entry = entry?;
                        let name = entry
                            .file_name()
                            .into_string()
                            .map_err(|_| anyhow::anyhow!("non-UTF-8 content name in {parent}"))?;
                        let path = format!("{parent}/{name}");
                        validate_path(&path)?;
                        if let Some(previous) = spelling.insert(path.to_lowercase(), path.clone()) {
                            ensure!(
                                previous == path,
                                "case-conflicting content paths: {previous} and {path}"
                            );
                        }
                        let physical = layer.entry(&path)?.with_context(|| {
                            format!("content disappeared during discovery: {path}")
                        })?;
                        if physical.is_dir() {
                            ensure!(
                                !entry.file_type()?.is_symlink(),
                                "directory aliases are unsupported in content namespaces: {path}"
                            );
                            pending.push(path);
                        } else {
                            ensure!(physical.is_file(), "content resource is not a file: {path}");
                            files.insert(path);
                        }
                    }
                }
            }
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
        ensure!(
            !self.missing.contains(path),
            "missing content resource {path} in this preparation"
        );
        if !self.bytes.contains_key(path) {
            let (bytes, overrides) = if is_mapping(path) {
                let (mappings, overrides) = self.merge_mappings(path)?;
                let bytes = serde_json::to_vec_pretty(&mappings)?;
                ensure!(
                    bytes.len() <= limit,
                    "content resource exceeds {limit} bytes: {path}"
                );
                (bytes, overrides)
            } else if path == DESCRIPTOR {
                (serde_json::to_vec_pretty(&self.library.descriptor)?, vec![])
            } else {
                let (physical, overrides) = self.select(path)?;
                let bytes = read_limited(&physical, limit)
                    .with_context(|| format!("read content {path}"))?;
                self.disk_bytes += bytes.len() as u64;
                (bytes, overrides)
            };
            self.bytes.insert(path.into(), bytes.into());
            self.overrides.extend(overrides);
        }
        let captured = &self.bytes[path];
        ensure!(
            captured.len() <= limit,
            "content resource exceeds {limit} bytes: {path}"
        );
        Ok(captured.clone())
    }

    /// Capture an optional root without enumerating its entire resource tree.
    /// Absence is retained too: later additions require a new preparation.
    pub fn read_optional(&mut self, path: &str, limit: usize) -> Result<Option<Arc<[u8]>>> {
        validate_path(path)?;
        if self.missing.contains(path) {
            return Ok(None);
        }
        if self.bytes.contains_key(path) || path == DESCRIPTOR {
            return self.read(path, limit).map(Some);
        }
        let Some((physical, overrides)) = self.select_optional(path)? else {
            self.missing.insert(path.into());
            return Ok(None);
        };
        if is_mapping(path) {
            return self.read(path, limit).map(Some);
        }
        let bytes =
            read_limited(&physical, limit).with_context(|| format!("read content {path}"))?;
        self.disk_bytes += bytes.len() as u64;
        let bytes: Arc<[u8]> = bytes.into();
        self.bytes.insert(path.into(), bytes.clone());
        self.overrides.extend(overrides);
        Ok(Some(bytes))
    }

    pub fn json<T: de::DeserializeOwned>(&mut self, path: &str, limit: usize) -> Result<T> {
        serde_json::from_slice(&self.read(path, limit)?)
            .with_context(|| format!("decode content {path}"))
    }

    pub fn mappings(&mut self, path: &str) -> Result<&Mappings> {
        validate_mapping_path(path)?;
        if !self.mappings.contains_key(path) {
            let mappings = self.json(path, MAX_MAPPING_BYTES)?;
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
        ensure!(
            !is_mapping(path) && path != DESCRIPTOR,
            "metadata is not a stream: {path}"
        );
        ensure!(
            !self.missing.contains(path),
            "missing content stream {path} in this preparation"
        );
        if !self.streams.contains_key(path) {
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
        let mut selected: Option<(PathBuf, &str)> = None;
        let mut overrides = Vec::new();
        for layer in &self.library.layers {
            if let Some(physical) = layer.path(path)? {
                if let Some((_, previous)) = &selected {
                    overrides.push(Override {
                        path: path.into(),
                        id: None,
                        previous: (*previous).into(),
                        replacement: layer.name.clone(),
                    });
                }
                selected = Some((physical, &layer.name));
            }
        }
        Ok(selected.map(|(path, _)| (path, overrides)))
    }

    fn merge_mappings(&mut self, path: &str) -> Result<(MappingValues, Vec<Override>)> {
        let mut merged = BTreeMap::new();
        let mut owners: BTreeMap<u32, &str> = BTreeMap::new();
        let mut overrides = Vec::new();
        let mut found = false;
        for layer in &self.library.layers {
            let Some(physical) = layer.path(path)? else {
                continue;
            };
            let bytes = read_limited(&physical, MAX_MAPPING_BYTES)?;
            self.disk_bytes += bytes.len() as u64;
            let mappings: MappingValues = serde_json::from_slice(&bytes)
                .with_context(|| format!("decode {path} in {}", layer.name))?;
            found = true;
            for (id, target) in mappings.0 {
                if let serde_json::Value::String(target) = &target {
                    validate_path(target)
                        .with_context(|| format!("decode {path} in {}", layer.name))?;
                }
                if let Some(previous) = owners.insert(id, &layer.name) {
                    overrides.push(Override {
                        path: path.into(),
                        id: Some(id),
                        previous: previous.into(),
                        replacement: layer.name.clone(),
                    });
                }
                merged.insert(id, target);
            }
        }
        ensure!(found, "missing content mapping {path}");
        Ok((MappingValues(merged), overrides))
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

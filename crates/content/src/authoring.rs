//! Granular authored documents, assembled once into immutable startup resources.
//!
//! Schemas describe ownership, not patches. Independent records occupy independent
//! paths; sequences with behavioral ordering remain whole unless they have an
//! explicit stable identity and authored order.
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod json;
mod layouts;
pub(crate) use json::decode;
#[cfg(test)]
mod tests;

/// A physical ownership boundary within one logical runtime document.
#[derive(Clone)]
pub(crate) enum Node {
    /// One indivisible authored value.
    File,
    /// Fields with independent owners; remaining fields live in definition.json.
    Object(Vec<(&'static str, Node)>),
    /// Named records. Each key has its own file or directory.
    Map(Box<Node>),
    /// Positions are stable public IDs, not editable timeline offsets.
    Indexed(Box<Node>),
    /// Independently registered, uniquely identified entries with explicit order.
    Ordered {
        identity: Identity,
        entry: Box<Node>,
    },
}

#[derive(Clone)]
pub(crate) enum Identity {
    Value,
    Fields(&'static [&'static str]),
    First(&'static str),
    Model,
    Authored,
}

pub(crate) struct Layout {
    pub document: String,
    pub directory: String,
    pub node: Node,
}

/// Publish independent files for a supported document. Runtime paths are virtual;
/// consumers continue using their typed contracts after startup assembly.
pub fn split(path: &str, document: &Value) -> Result<Option<BTreeMap<String, Value>>> {
    let Some(layout) = layouts::for_document(path) else {
        return Ok(None);
    };
    let mut files = BTreeMap::new();
    layout.node.write(&layout.directory, document, &mut files)?;
    Ok(Some(files))
}

/// Assemble one document from its selected source paths. Native converters can
/// read an existing owner without materializing intermediate files on disk.
pub fn assemble(
    path: &str,
    paths: &BTreeSet<String>,
    mut read: impl FnMut(&str) -> Result<Value>,
) -> Result<Value> {
    layouts::for_document(path)
        .context("unsupported authored document")?
        .assemble(paths, &mut read)
}

impl Layout {
    pub(crate) fn assemble(
        &self,
        paths: &BTreeSet<String>,
        read: &mut impl FnMut(&str) -> Result<Value>,
    ) -> Result<Value> {
        // A misnamed owner would otherwise be ignored without a trace.
        let prefix = format!("{}/", self.directory);
        if let Some(stray) = paths
            .range(prefix.clone()..)
            .take_while(|path| path.starts_with(&prefix))
            .find(|path| path.ends_with(".json") && !self.node.owns(&self.directory, path))
        {
            anyhow::bail!("{stray} is not a source of {}", self.document);
        }
        let document = self
            .node
            .read(&self.directory, paths, read)?
            .with_context(|| format!("missing document owners for {}", self.document))?;
        ensure!(
            !is_removal(&document),
            "{} cannot be removed",
            self.document
        );
        Ok(document)
    }
}

/// Whether native publication has granular owners for this runtime contract.
pub fn supports(path: &str) -> bool {
    layouts::for_document(path).is_some()
}

/// Write a document's independent owners beneath `root`, once the split is
/// known to be lossless. Returns the written library paths.
pub fn publish(root: &Path, path: &str, document: &Value) -> Result<BTreeSet<String>> {
    let files = split(path, document)?.context("document has no granular ownership schema")?;
    validate_split(path, document, &files)?;
    for (path, value) in &files {
        let physical = root.join(path);
        fs::create_dir_all(physical.parent().context("owner has no directory")?)?;
        fs::write(physical, resonance_asset_writer::json::encode(value)?)?;
    }
    Ok(files.into_keys().collect())
}

/// Publish an edited document as a layer above `base`: its owners, plus a
/// removal marker for every named or registered entry the edit dropped and
/// empty definitions for settings reset to their defaults.
pub fn publish_over(
    root: &Path,
    path: &str,
    base: &Value,
    edited: &Value,
) -> Result<BTreeSet<String>> {
    let layout = layouts::for_document(path).context("unsupported authored document")?;
    let mut removed = BTreeMap::new();
    layout
        .node
        .removals(&layout.directory, base, edited, &mut removed)?;
    let mut files = publish(root, path, edited)?;
    for (path, marker) in removed {
        let physical = root.join(&path);
        fs::create_dir_all(physical.parent().context("owner has no directory")?)?;
        fs::write(physical, resonance_asset_writer::json::encode(&marker)?)?;
        files.insert(path);
    }
    Ok(files)
}

/// Check that publication preserves every original value and sequence order.
fn validate_split(path: &str, document: &Value, files: &BTreeMap<String, Value>) -> Result<()> {
    let layout = layouts::for_document(path).context("unsupported authored document")?;
    let paths = files.keys().cloned().collect();
    let reconstructed = layout.assemble(&paths, &mut |path| Ok(files[path].clone()))?;
    ensure!(
        reconstructed == *document,
        "granular publication changes {path}"
    );
    Ok(())
}

/// Discover supported source directories from the effective startup namespace.
/// This does not scan the filesystem or resolve any lower-priority layer.
pub(crate) fn layouts(paths: &BTreeSet<String>) -> Vec<Layout> {
    layouts::discover(paths)
}

/// A higher-priority owner (a file, or a directory's marker) retiring an entry
/// of a named or ordered collection. Fields and indexed positions cannot be retired.
fn is_removal(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|value| value.len() == 1 && value.get("$remove") == Some(&Value::Bool(true)))
}

impl Node {
    fn write(&self, path: &str, value: &Value, files: &mut BTreeMap<String, Value>) -> Result<()> {
        match self {
            Self::File => {
                files.insert(format!("{path}.json"), value.clone());
            }
            Self::Object(fields) => {
                let Some(object) = value.as_object() else {
                    files.insert(format!("{path}/definition.json"), value.clone());
                    return Ok(());
                };
                let mut rest = object.clone();
                for (name, node) in fields {
                    if let Some(value) = rest.remove(*name) {
                        node.write(&format!("{path}/{name}"), &value, files)?;
                    }
                }
                if !rest.is_empty() || object.is_empty() {
                    files.insert(format!("{path}/definition.json"), Value::Object(rest));
                }
            }
            Self::Map(node) => {
                let Some(entries) = value.as_object() else {
                    files.insert(format!("{path}/collection.json"), value.clone());
                    return Ok(());
                };
                files.insert(format!("{path}/collection.json"), Value::Object(Map::new()));
                for (key, value) in entries {
                    node.write(&format!("{path}/{}", component(key)), value, files)?;
                }
            }
            Self::Indexed(node) => {
                let entries = value
                    .as_array()
                    .with_context(|| format!("expected indexed collection at {path}"))?;
                files.insert(format!("{path}/collection.json"), Value::Null);
                for (id, value) in entries.iter().enumerate() {
                    node.write(&format!("{path}/{id}"), value, files)?;
                }
            }
            Self::Ordered { identity, entry } => {
                let entries = value
                    .as_array()
                    .with_context(|| format!("expected ordered collection at {path}"))?;
                files.insert(format!("{path}/collection.json"), Value::Null);
                let mut keys = BTreeSet::new();
                for (order, value) in entries.iter().enumerate() {
                    let key = if matches!(identity, Identity::Authored) {
                        format!("entry-{order:06}")
                    } else {
                        identity.key(value)?
                    };
                    ensure!(
                        keys.insert(key.clone()),
                        "duplicate registration {key} at {path}"
                    );
                    let root = format!("{path}/{}", component(&key));
                    files.insert(
                        format!("{root}/order.json"),
                        Value::from(order as i64 * 1024),
                    );
                    entry.write(&format!("{root}/entry"), value, files)?;
                }
            }
        }
        Ok(())
    }

    /// Each source is read once. A removal marker is returned as is, so the
    /// containing collection can drop the entry without reading its children.
    fn read(
        &self,
        path: &str,
        paths: &BTreeSet<String>,
        read: &mut impl FnMut(&str) -> Result<Value>,
    ) -> Result<Option<Value>> {
        let collection = match self {
            Self::Map(_) | Self::Indexed(_) | Self::Ordered { .. } => {
                read_optional(&format!("{path}/collection.json"), paths, read)?
            }
            _ => None,
        };
        if collection.as_ref().is_some_and(is_removal) {
            return Ok(collection);
        }
        if matches!(self, Self::Indexed(_) | Self::Ordered { .. }) {
            ensure!(
                collection.as_ref().is_none_or(Value::is_null),
                "sequence collection marker must be null: {path}"
            );
        }
        match self {
            Self::File => read_optional(&format!("{path}.json"), paths, read),
            Self::Object(fields) => {
                let marker = format!("{path}/definition.json");
                let mut result = match read_optional(&marker, paths, read)? {
                    Some(value) if is_removal(&value) => return Ok(Some(value)),
                    Some(Value::Object(value)) => value,
                    Some(value) => return Ok(Some(value)),
                    None => Map::new(),
                };
                let mut found = paths.contains(&marker);
                for (name, node) in fields {
                    ensure!(
                        !result.contains_key(*name),
                        "{marker}: {name} has its own owner"
                    );
                    if let Some(value) = node.read(&format!("{path}/{name}"), paths, read)? {
                        ensure!(!is_removal(&value), "{path}/{name} cannot be removed");
                        result.insert((*name).into(), value);
                        found = true;
                    }
                }
                Ok(found.then_some(Value::Object(result)))
            }
            Self::Map(node) => {
                match &collection {
                    Some(Value::Object(marker)) => {
                        ensure!(marker.is_empty(), "collection marker must be empty: {path}")
                    }
                    Some(marker) => return Ok(Some(marker.clone())),
                    None => {}
                }
                let keys = children(path, paths, matches!(node.as_ref(), Self::File));
                let mut result = Map::new();
                for key in keys {
                    let decoded = uncomponent(&key)?;
                    let value = node
                        .read(&format!("{path}/{key}"), paths, read)?
                        .with_context(|| format!("incomplete collection entry {path}/{key}"))?;
                    if is_removal(&value) {
                        continue;
                    }
                    ensure!(
                        result.insert(decoded, value).is_none(),
                        "duplicate collection entry at {path}"
                    );
                }
                let present = collection.is_some() || !result.is_empty();
                Ok(present.then_some(Value::Object(result)))
            }
            Self::Indexed(node) => {
                let keys = children(path, paths, matches!(node.as_ref(), Self::File));
                let mut entries = BTreeMap::new();
                for key in keys {
                    let id = key
                        .parse::<usize>()
                        .with_context(|| format!("invalid index at {path}/{key}"))?;
                    ensure!(id.to_string() == key, "noncanonical index at {path}/{key}");
                    let value = node
                        .read(&format!("{path}/{key}"), paths, read)?
                        .with_context(|| format!("missing indexed entry at {path}/{key}"))?;
                    ensure!(!is_removal(&value), "{path}/{key} cannot be removed");
                    entries.insert(id, value);
                }
                let present = collection.is_some() || !entries.is_empty();
                ensure!(
                    entries.keys().copied().eq(0..entries.len()),
                    "indexed collection has missing IDs at {path}"
                );
                Ok(present.then(|| Value::Array(entries.into_values().collect())))
            }
            Self::Ordered { identity, entry } => {
                let keys = children(path, paths, false);
                let mut entries = Vec::new();
                for key in keys {
                    let root = format!("{path}/{key}");
                    let value = entry
                        .read(&format!("{root}/entry"), paths, read)?
                        .with_context(|| format!("missing registration at {root}"))?;
                    if is_removal(&value) {
                        continue;
                    }
                    let order = read_optional(&format!("{root}/order.json"), paths, read)?
                        .with_context(|| format!("missing explicit order for {root}"))?;
                    let order = order
                        .as_i64()
                        .with_context(|| format!("expected integer order at {root}"))?;
                    ensure!(
                        matches!(identity, Identity::Authored)
                            || component(&identity.key(&value)?) == key,
                        "registration identity disagrees with path {root}"
                    );
                    entries.push((order, key, value));
                }
                let present = collection.is_some() || !entries.is_empty();
                entries.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
                Ok(present.then(|| {
                    Value::Array(entries.into_iter().map(|(_, _, value)| value).collect())
                }))
            }
        }
    }
}

impl Node {
    /// Where a marker retires the entry stored at `path`.
    fn marker(&self, path: &str) -> String {
        match self {
            Self::File => format!("{path}.json"),
            Self::Object(_) => format!("{path}/definition.json"),
            Self::Map(_) | Self::Indexed(_) | Self::Ordered { .. } => {
                format!("{path}/collection.json")
            }
        }
    }

    fn removals(
        &self,
        path: &str,
        base: &Value,
        edited: &Value,
        removed: &mut BTreeMap<String, Value>,
    ) -> Result<()> {
        let marker = || serde_json::json!({"$remove": true});
        match self {
            Self::File => {}
            Self::Object(fields) => {
                if let (Some(base), Some(edited)) = (base.as_object(), edited.as_object()) {
                    let separate =
                        |key: &String| fields.iter().any(|(name, _)| *name == key.as_str());
                    if base.keys().any(|key| !separate(key)) && edited.keys().all(separate) {
                        // Omitting this definition would inherit the base settings.
                        removed.insert(self.marker(path), Value::Object(Map::new()));
                    }
                }
                for (name, node) in fields {
                    match (base.get(*name), edited.get(*name)) {
                        (Some(base), Some(edited)) => {
                            node.removals(&format!("{path}/{name}"), base, edited, removed)?
                        }
                        (Some(_), None) => anyhow::bail!("{path}/{name} cannot be removed"),
                        _ => {}
                    }
                }
            }
            Self::Map(node) => {
                let (Some(base), Some(edited)) = (base.as_object(), edited.as_object()) else {
                    return Ok(());
                };
                for (key, base) in base {
                    let entry = format!("{path}/{}", component(key));
                    match edited.get(key) {
                        Some(edited) => node.removals(&entry, base, edited, removed)?,
                        None => {
                            removed.insert(node.marker(&entry), marker());
                        }
                    }
                }
            }
            Self::Indexed(node) => {
                let (Some(base), Some(edited)) = (base.as_array(), edited.as_array()) else {
                    return Ok(());
                };
                ensure!(
                    base.len() <= edited.len(),
                    "{path} cannot lose indexed entries"
                );
                for (id, (base, edited)) in base.iter().zip(edited).enumerate() {
                    node.removals(&format!("{path}/{id}"), base, edited, removed)?;
                }
            }
            Self::Ordered { identity, entry } => {
                let (Some(base), Some(edited)) = (base.as_array(), edited.as_array()) else {
                    return Ok(());
                };
                let key = |order: usize, value: &Value| -> Result<String> {
                    Ok(if matches!(identity, Identity::Authored) {
                        format!("entry-{order:06}")
                    } else {
                        component(&identity.key(value)?)
                    })
                };
                let mut kept = BTreeMap::new();
                for (order, value) in edited.iter().enumerate() {
                    kept.insert(key(order, value)?, value);
                }
                for (order, base) in base.iter().enumerate() {
                    let root = format!("{path}/{}/entry", key(order, base)?);
                    match kept.get(&key(order, base)?) {
                        Some(edited) => entry.removals(&root, base, edited, removed)?,
                        None => {
                            removed.insert(entry.marker(&root), marker());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn read_optional(
    path: &str,
    paths: &BTreeSet<String>,
    read: &mut impl FnMut(&str) -> Result<Value>,
) -> Result<Option<Value>> {
    paths.contains(path).then(|| read(path)).transpose()
}

fn children(path: &str, paths: &BTreeSet<String>, files: bool) -> BTreeSet<String> {
    let prefix = format!("{path}/");
    let mut result = BTreeSet::new();
    for path in paths
        .range(prefix.clone()..)
        .take_while(|path| path.starts_with(&prefix))
    {
        let tail = path.strip_prefix(&prefix).unwrap();
        if tail == "collection.json" {
            continue;
        }
        if files {
            if let Some(key) = tail.strip_suffix(".json").filter(|key| !key.contains('/')) {
                result.insert(key.to_owned());
            }
        } else if let Some((key, _)) = tail.split_once('/').filter(|_| tail.ends_with(".json")) {
            // Media may sit beside owners; only JSON makes an entry.
            result.insert(key.to_owned());
        }
    }
    result
}

impl Identity {
    fn key(&self, value: &Value) -> Result<String> {
        let scalar = |value: &Value| -> Result<String> {
            match value {
                Value::String(value) => Ok(value.clone()),
                Value::Number(value) => Ok(value.to_string()),
                _ => anyhow::bail!("registration identity must be a string or number"),
            }
        };
        match self {
            Self::Value => scalar(value),
            Self::Authored => {
                anyhow::bail!("authored registration identity belongs to its filename")
            }
            Self::Model => {
                if let Some(name) = value.as_str() {
                    return Ok(format!("shared-{name}"));
                }
                if let Some(group) = value.get("group") {
                    return Ok(format!("group-{}", scalar(group)?));
                }
                Ok(format!("local-{}", scalar(&value["resource"])?))
            }
            Self::First(field) => scalar(
                value[field]
                    .as_array()
                    .and_then(|values| values.first())
                    .with_context(|| format!("registration needs {field}"))?,
            ),
            Self::Fields(fields) if fields.len() == 1 => scalar(&value[fields[0]]),
            Self::Fields(fields) => {
                let values = fields
                    .iter()
                    .map(|field| scalar(&value[*field]))
                    .collect::<Result<Vec<_>>>()?;
                // One readable directory name, such as `colette--angelic-maiden`.
                Ok(values.join("--"))
            }
        }
    }
}

/// Reversible canonical names, including original keys containing slashes.
fn component(key: &str) -> String {
    if key.is_empty() {
        return "%".into();
    }
    if key == "collection" {
        return "%63ollection".into();
    }
    key.as_bytes()
        .iter()
        .map(|&byte| {
            if byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
fn uncomponent(key: &str) -> Result<String> {
    if key == "%" {
        return Ok(String::new());
    }
    let mut bytes = Vec::new();
    let mut chars = key.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let a = chars.next().context("truncated encoded collection name")?;
            let b = chars.next().context("truncated encoded collection name")?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(&[a, b])?, 16)?);
        } else {
            bytes.push(byte);
        }
    }
    let value = String::from_utf8(bytes)?;
    ensure!(
        component(&value) == key,
        "noncanonical collection name {key}"
    );
    Ok(value)
}

/// Owned JSON inputs, used by native publication to replace its own previous
/// output without deleting media or unrelated resources sharing a directory.
pub fn owned_sources(paths: &BTreeSet<String>) -> BTreeMap<String, BTreeSet<String>> {
    layouts(paths)
        .into_iter()
        .map(|layout| {
            let prefix = format!("{}/", layout.directory);
            let owned = paths
                .range(prefix.clone()..)
                .take_while(|path| path.starts_with(&prefix))
                .filter(|path| layout.node.owns(&layout.directory, path))
                .cloned()
                .collect();
            (layout.document, owned)
        })
        .collect()
}

impl Node {
    fn owns(&self, root: &str, path: &str) -> bool {
        match self {
            Self::File => path == format!("{root}.json"),
            Self::Object(fields) => {
                path == format!("{root}/definition.json")
                    || fields
                        .iter()
                        .any(|(name, node)| node.owns(&format!("{root}/{name}"), path))
            }
            Self::Map(node) | Self::Indexed(node) => {
                if path == format!("{root}/collection.json") {
                    return true;
                }
                let Some(tail) = path.strip_prefix(&format!("{root}/")) else {
                    return false;
                };
                let key = if matches!(node.as_ref(), Self::File) {
                    tail.strip_suffix(".json").filter(|key| !key.contains('/'))
                } else {
                    tail.split_once('/').map(|(key, _)| key)
                };
                key.is_some_and(|key| {
                    uncomponent(key).is_ok() && node.owns(&format!("{root}/{key}"), path)
                })
            }
            Self::Ordered { entry, .. } => {
                let Some(tail) = path.strip_prefix(&format!("{root}/")) else {
                    return false;
                };
                if tail == "collection.json" {
                    return true;
                }
                tail.split_once('/').is_some_and(|(key, tail)| {
                    uncomponent(key).is_ok()
                        && (tail == "order.json"
                            || entry.owns(&format!("{root}/{key}/entry"), path))
                })
            }
        }
    }
}

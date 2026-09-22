use crate::{Error, module_id};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use symphonia_script_compiler::SourceResolver;

/// Keep the cooked standard library separate from a source project's modules.
pub struct StandardSources<'a> {
    pub project: &'a dyn SourceResolver,
    pub standard: &'a dyn SourceResolver,
}

impl SourceResolver for StandardSources<'_> {
    fn source(&self, module: &str) -> Option<&str> {
        if module == "std" || module.starts_with("std::") {
            self.standard.source(module)
        } else {
            self.project.source(module)
        }
    }
}

/// One consistent source snapshot for a preparation operation. No file can be
/// changed between the cache comparison and the compiler reading that input.
#[derive(Debug)]
pub struct SourceTree {
    files: BTreeMap<String, (PathBuf, String)>,
}

impl SourceTree {
    /// A logical `field::start` lives at `field/start.sym` below root.
    /// Symlinks are rejected, so source discovery cannot escape the given root.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, Error> {
        Self::read(root.as_ref(), false)
    }

    /// Load editable project modules; the reserved standard library comes from
    /// the caller's immutable cooked snapshot and is not opened here.
    pub fn load_project(root: impl AsRef<Path>) -> Result<Self, Error> {
        Self::read(root.as_ref(), true)
    }

    fn read(root: &Path, project_only: bool) -> Result<Self, Error> {
        let root = fs::canonicalize(root).map_err(|source| Error::Io {
            path: root.into(),
            source,
        })?;
        let mut files = BTreeMap::new();
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(&directory).map_err(|source| Error::Io {
                path: directory,
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    path: root.clone(),
                    source,
                })?;
                let path = entry.path();
                if project_only
                    && path.parent() == Some(root.as_path())
                    && matches!(entry.file_name().to_str(), Some("std" | "std.sym"))
                {
                    continue;
                }
                let kind = entry.file_type().map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
                if kind.is_dir() {
                    pending.push(path);
                } else if kind.is_symlink() {
                    return Err(Error::SourcePath(path));
                } else if path.extension().is_some_and(|ext| ext == "sym") {
                    if !kind.is_file() {
                        return Err(Error::SourcePath(path));
                    }
                    let relative = path
                        .strip_prefix(&root)
                        .expect("directory entry remains under root");
                    let module = relative
                        .with_extension("")
                        .components()
                        .map(|part| {
                            part.as_os_str()
                                .to_str()
                                .ok_or_else(|| Error::SourcePath(path.clone()))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("::");
                    module_id(&module)?;
                    let source = fs::read_to_string(&path).map_err(|source| Error::Io {
                        path: path.clone(),
                        source,
                    })?;
                    files.insert(module, (path, source));
                }
            }
        }
        Ok(Self { files })
    }

    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn path(&self, module: &str) -> Option<&Path> {
        self.files.get(module).map(|(path, _)| path.as_path())
    }
}

impl SourceResolver for SourceTree {
    fn source(&self, module: &str) -> Option<&str> {
        self.files.get(module).map(|(_, source)| source.as_str())
    }
}

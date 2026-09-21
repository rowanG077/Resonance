use super::{Entry, PreparedEvent, Resources};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use symphonia_script_compiler::SourceResolver;
use symphonia_script_tools::{PreparationCache, SourceTree, StandardSources};

#[derive(Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum On {
    /// The first player-controlled update after arriving from another field.
    #[default]
    Arrival,
    /// Also run after a saved checkpoint has finished restoring.
    Entry,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    module: String,
    task: String,
    #[serde(default)]
    arguments: Vec<i32>,
    #[serde(default)]
    on: On,
}

#[derive(Clone)]
pub struct FieldEvent {
    on: On,
    event: Arc<PreparedEvent>,
}
impl FieldEvent {
    pub fn for_entry(&self, kind: crate::field::EntryKind) -> Option<Arc<PreparedEvent>> {
        (matches!(self.on, On::Entry) || kind == crate::field::EntryKind::Arrival)
            .then(|| self.event.clone())
    }
}

/// Optional source project, supplied explicitly at startup. Its bindings and
/// sources are reread at preparation, never by an active field or its tasks.
pub struct FieldScripts {
    root: PathBuf,
    cache: PreparationCache,
}
impl FieldScripts {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: Default::default(),
        }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn prepare(
        &mut self,
        map: u32,
        standard: &impl SourceResolver,
        resources: &mut impl Resources,
    ) -> Result<Option<FieldEvent>> {
        let path = self.root.join("fields.json");
        let bindings: BTreeMap<u32, Binding> = serde_json::from_slice(
            &fs::read(&path)
                .with_context(|| format!("read authored field bindings {}", path.display()))?,
        )
        .with_context(|| format!("decode authored field bindings {}", path.display()))?;
        let Some(binding) = bindings.get(&map) else {
            return Ok(None);
        };
        let project = SourceTree::load_project(&self.root)?;
        let sources = StandardSources {
            project: &project,
            standard,
        };
        let event = PreparedEvent::prepare(
            &mut self.cache,
            &sources,
            Entry {
                module: &binding.module,
                task: &binding.task,
                arguments: &binding.arguments,
            },
            resources,
        )
        .with_context(|| format!("prepare authored entry for field {map}"))?;
        Ok(Some(FieldEvent {
            on: binding.on,
            event: Arc::new(event),
        }))
    }
}

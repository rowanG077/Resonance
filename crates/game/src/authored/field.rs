use super::{Entry, PreparedEvent, Resources};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use symphonia_script_compiler::SourceResolver;
use symphonia_script_tools::{PreparationCache, SourceTree, StandardSources};

#[derive(Default, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum On {
    /// The first player-controlled update after arriving from another field.
    #[default]
    Arrival,
    /// Also run after a saved checkpoint has finished restoring.
    Entry,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    module: String,
    task: String,
    #[serde(default)]
    arguments: Vec<i32>,
    #[serde(default)]
    on: On,
}

/// The project's bindings and all non-standard modules captured for one
/// preparation. Compilation and save admission consume these exact inputs.
pub struct FieldProject {
    bindings: BTreeMap<u32, Binding>,
    sources: SourceTree,
}
impl FieldProject {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("fields.json");
        let bindings = serde_json::from_slice(
            &fs::read(&path)
                .with_context(|| format!("read authored field bindings {}", path.display()))?,
        )
        .with_context(|| format!("decode authored field bindings {}", path.display()))?;
        Ok(Self {
            bindings,
            sources: SourceTree::load_project(root)?,
        })
    }

    pub fn bind_identity(
        &self,
        library: &resonance_content::save_identity::Identity,
    ) -> Result<resonance_content::save_identity::Identity> {
        let sources = self
            .sources
            .modules()
            .map(|module| {
                let source = self
                    .sources
                    .source(module)
                    .expect("enumerated project module exists");
                Ok((
                    module,
                    symphonia_script_compiler::canonical_source(module, source)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        library.with_authored_project(&serde_json::to_vec(&(&self.bindings, sources))?)
    }
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

    pub fn capture(&self) -> Result<FieldProject> {
        FieldProject::load(&self.root)
    }

    pub fn prepare(
        &mut self,
        map: u32,
        standard: &impl SourceResolver,
        resources: &mut impl Resources,
    ) -> Result<Option<FieldEvent>> {
        let project = self.capture()?;
        self.prepare_captured(map, &project, standard, resources)
    }

    pub fn prepare_captured(
        &mut self,
        map: u32,
        project: &FieldProject,
        standard: &impl SourceResolver,
        resources: &mut impl Resources,
    ) -> Result<Option<FieldEvent>> {
        let Some(binding) = project.bindings.get(&map) else {
            return Ok(None);
        };
        let sources = StandardSources {
            project: &project.sources,
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

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::save_identity::{Identity, SCHEMA};

    #[test]
    fn project_identity_uses_captured_bindings_and_logical_sources() -> Result<()> {
        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let directory = Directory(
            std::env::temp_dir().join(format!("resonance-project-identity-{}", std::process::id())),
        );
        fs::create_dir(&directory.0)?;
        let root = directory.0.join("first");
        let relocated = directory.0.join("relocated");
        let bindings = r#"{"340":{"module":"entry","task":"run"}}"#;
        let source = "script field; message Caption = \"Welcome\"; const Key: string = \"lookup-key\"; pub task run() {}";
        for path in [&root, &relocated] {
            fs::create_dir(path)?;
            fs::write(path.join("fields.json"), bindings)?;
            fs::write(path.join("entry.sym"), source)?;
        }
        let library = Identity {
            schema: SCHEMA,
            content: [5; 32],
        };
        let captured = FieldProject::load(&root)?;
        let original = captured.bind_identity(&library)?;
        assert_ne!(
            original, library,
            "a selected project participates in admission"
        );
        assert_eq!(
            FieldProject::load(&relocated)?.bind_identity(&library)?,
            original
        );
        fs::create_dir(root.join("std"))?;
        fs::write(root.join("std/ignored.sym"), b"not project input")?;
        fs::write(
            root.join("fields.json"),
            r#"{ "340": { "on":"arrival", "arguments":[], "task":"run", "module":"entry" } }"#,
        )?;
        fs::write(
            root.join("entry.sym"),
            format!("// Mod notes\n{}", source.replace("Welcome", "Bienvenue")),
        )?;
        assert_eq!(
            FieldProject::load(&root)?.bind_identity(&library)?,
            original
        );
        fs::write(
            root.join("entry.sym"),
            source.replace("lookup-key", "different-key"),
        )?;
        assert_ne!(
            FieldProject::load(&root)?.bind_identity(&library)?,
            original
        );
        assert_eq!(
            captured.bind_identity(&library)?,
            original,
            "identity shares the compilation snapshot"
        );
        fs::write(root.join("entry.sym"), source)?;
        for changed in [
            bindings.replace("340", "341"),
            bindings.replace("run", "again"),
            bindings.replace("\"task\":\"run\"", "\"task\":\"run\",\"on\":\"entry\""),
            bindings.replace("\"task\":\"run\"", "\"task\":\"run\",\"arguments\":[1]"),
        ] {
            fs::write(root.join("fields.json"), changed)?;
            assert_ne!(
                FieldProject::load(&root)?.bind_identity(&library)?,
                original
            );
        }
        assert!(
            captured
                .bind_identity(&Identity {
                    schema: SCHEMA - 1,
                    content: [5; 32]
                })
                .is_err()
        );
        Ok(())
    }
}

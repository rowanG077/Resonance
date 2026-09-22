use crate::{Error, module_id};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use symphonia_script::{Program, authored::NativeDeclaration};
use symphonia_script_compiler::{AssetReference, ScriptKind, SourceResolver, compile};

/// A program and its exact inputs. VMs retain `program`, so source edits never
/// replace code beneath a running task.
#[derive(Debug)]
pub struct PreparedModule {
    pub kind: ScriptKind,
    pub program: Arc<Program>,
    pub assets: Vec<AssetReference>,
    pub sources: BTreeMap<String, String>,
}

/// A complete immutable batch for one scene or preview. The caller
/// resolves assets/glyphs and installs this batch only after preparation succeeds.
#[derive(Debug)]
pub struct Generation {
    modules: BTreeMap<String, Arc<PreparedModule>>,
    natives: Vec<NativeDeclaration>,
}

impl Generation {
    pub fn module(&self, name: &str) -> Option<&Arc<PreparedModule>> {
        self.modules.get(name)
    }

    pub fn modules(&self) -> impl Iterator<Item = (&str, &Arc<PreparedModule>)> {
        self.modules
            .iter()
            .map(|(name, module)| (name.as_str(), module))
    }
}

/// Process-local cache: rebuilding the compiler inherently discards its cache.
/// Only final programs are retained; nothing is written into cooked assets.
#[derive(Default)]
pub struct PreparationCache {
    latest: Option<Arc<Generation>>,
    compiled: BTreeMap<String, CachedModule>,
}

struct CachedModule {
    module: Arc<PreparedModule>,
    natives: Vec<NativeDeclaration>,
    missing: BTreeSet<String>,
}

struct ResolveInputs<'a, S> {
    sources: &'a S,
    missing: RefCell<BTreeSet<String>>,
}

impl<S: SourceResolver> SourceResolver for ResolveInputs<'_, S> {
    fn source(&self, module: &str) -> Option<&str> {
        let source = self.sources.source(module);
        if source.is_none() {
            self.missing.borrow_mut().insert(module.into());
        }
        source
    }
}

impl PreparationCache {
    /// Compile the full requested batch, then publish its generation atomically.
    /// Changed transitive sources and native signatures invalidate affected
    /// programs. Failure returns diagnostics without returning stale code or
    /// installing a partially prepared batch.
    pub fn prepare<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a str>,
        sources: &impl SourceResolver,
        natives: &[NativeDeclaration],
    ) -> Result<Arc<Generation>, Error> {
        let previous = self
            .latest
            .as_ref()
            .filter(|generation| generation.natives == natives);
        let mut modules = BTreeMap::new();
        let mut updates = BTreeMap::new();
        for entry in entries {
            module_id(entry)?;
            if modules.contains_key(entry) {
                continue;
            }
            let cached = self.compiled.get(entry).filter(|cached| {
                cached.natives == natives
                    && cached
                        .module
                        .sources
                        .iter()
                        .all(|(id, text)| sources.source(id) == Some(text.as_str()))
                    && cached.missing.iter().all(|id| sources.source(id).is_none())
            });
            let module = if let Some(cached) = cached {
                Arc::clone(&cached.module)
            } else {
                let inputs = ResolveInputs {
                    sources,
                    missing: RefCell::default(),
                };
                let compiled = compile(entry, &inputs, natives)?;
                let module = Arc::new(PreparedModule {
                    kind: compiled.kind,
                    program: Arc::new(compiled.program),
                    assets: compiled.assets,
                    sources: compiled.sources,
                });
                updates.insert(
                    entry.to_owned(),
                    CachedModule {
                        module: Arc::clone(&module),
                        natives: natives.to_vec(),
                        missing: inputs.missing.into_inner(),
                    },
                );
                module
            };
            modules.insert(entry.to_owned(), module);
        }
        if let Some(previous) = previous
            && previous.modules.len() == modules.len()
            && modules.iter().all(|(id, module)| {
                previous
                    .module(id)
                    .is_some_and(|old| Arc::ptr_eq(old, module))
            })
        {
            return Ok(Arc::clone(previous));
        }
        let generation = Arc::new(Generation {
            modules,
            natives: natives.to_vec(),
        });
        self.compiled.extend(updates);
        self.latest = Some(Arc::clone(&generation));
        Ok(generation)
    }
}

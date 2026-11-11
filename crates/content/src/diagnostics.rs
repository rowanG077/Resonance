//! Session error policy shared by loading, simulation and presentation.
use anyhow::{Result, ensure};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub scope: String,
    pub message: String,
    pub occurrences: u64,
}

#[derive(Debug, Clone)]
pub struct Diagnostics(Arc<State>);

#[derive(Debug)]
struct State {
    paranoid: bool,
    entries: Mutex<Vec<Diagnostic>>,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Diagnostics {
    pub fn new(paranoid: bool) -> Self {
        Self(Arc::new(State {
            paranoid,
            entries: Mutex::new(Vec::new()),
        }))
    }

    pub fn paranoid(&self) -> bool {
        self.0.paranoid
    }

    /// Record every occurrence, printing a repeated diagnostic only once. The
    /// caller must choose a coherent skip or fallback before using this method.
    pub fn report(&self, scope: &str, error: anyhow::Error) -> Result<()> {
        let message = format!("{error:#}");
        let mut entries = self.0.entries.lock().unwrap();
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.scope == scope && entry.message == message)
        {
            entry.occurrences = entry.occurrences.saturating_add(1);
        } else {
            let action = if self.paranoid() {
                "stopping: --paranoid"
            } else {
                "continuing"
            };
            if tracing::dispatcher::has_been_set() {
                tracing::error!("[{scope}] {message} ({action})");
            } else {
                // Importers and other tools can use diagnostics without a logger.
                eprintln!("ERROR [{scope}] {message} ({action})");
            }
            entries.push(Diagnostic {
                scope: scope.into(),
                message,
                occurrences: 1,
            });
        }
        if self.paranoid() { Err(error) } else { Ok(()) }
    }

    /// Omit a failed independent item in tolerant mode; preserve the original
    /// error in paranoid mode. This does not roll back a partially applied item.
    pub fn attempt<T>(&self, scope: &str, result: Result<T>) -> Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                self.report(scope, error)?;
                Ok(None)
            }
        }
    }

    /// Speculative preparation includes discovery. Cancellation must never be
    /// recovered as an optional miss, or publish a partially prepared value.
    /// Like `attempt`, this does not roll back side effects of the operation.
    pub fn preload<T>(
        &self,
        scope: &str,
        cancelled: &impl Fn() -> bool,
        prepare: impl FnOnce() -> Result<T>,
    ) -> Result<Option<T>> {
        ensure!(!cancelled(), "preload cancelled: {scope}");
        let result = prepare();
        ensure!(!cancelled(), "preload cancelled: {scope}");
        self.attempt(scope, result)
    }

    pub fn has_errors(&self) -> bool {
        !self.0.entries.lock().unwrap().is_empty()
    }

    pub fn entries(&self) -> Vec<Diagnostic> {
        self.0.entries.lock().unwrap().clone()
    }

    /// Stream diagnostics without cloning the inventory. A bounded writer can
    /// stop formatting as soon as the support-report budget is exhausted.
    pub fn write_summary(&self, out: &mut impl std::fmt::Write) -> std::fmt::Result {
        let entries = self
            .0
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        writeln!(
            out,
            "Content diagnostics: {} distinct errors",
            entries.len()
        )?;
        for entry in entries.iter().rev() {
            writeln!(
                out,
                "[{}] {} ({} occurrences)",
                entry.scope, entry.message, entry.occurrences
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_session_collects_distinct_errors_and_keeps_later_work() {
        let diagnostics = Diagnostics::default();
        let worker = diagnostics.clone();
        let results = [
            Err(anyhow::anyhow!("missing image")),
            Ok(7),
            Err(anyhow::anyhow!("missing mask")),
        ]
        .into_iter()
        .map(|result| worker.attempt("particle", result))
        .collect::<Result<Vec<_>>>()
        .unwrap();
        assert_eq!(results, [None, Some(7), None]);
        diagnostics
            .report("particle", anyhow::anyhow!("missing image"))
            .unwrap();
        let entries = diagnostics.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].occurrences, 2);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn paranoid_preserves_the_error_and_does_not_affect_another_session() {
        let paranoid = Diagnostics::new(true);
        let error = paranoid
            .report("model", anyhow::anyhow!("missing mesh"))
            .unwrap_err();
        assert_eq!(error.to_string(), "missing mesh");
        assert!(paranoid.has_errors());
        assert!(!Diagnostics::default().has_errors());
    }

    #[test]
    fn preloads_recover_discovery_errors_but_never_recover_cancellation() -> Result<()> {
        use std::cell::Cell;
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let result = diagnostics.preload::<()>("discovery", &|| false, || {
                Err(anyhow::anyhow!("bad mapping"))
            });
            assert_eq!(result.is_err(), paranoid);
            assert_eq!(diagnostics.entries().len(), 1);

            for success in [false, true] {
                let diagnostics = Diagnostics::new(paranoid);
                let cancelled = Cell::new(false);
                assert!(
                    diagnostics
                        .preload("discovery", &|| cancelled.get(), || {
                            cancelled.set(true);
                            if success {
                                Ok(7)
                            } else {
                                Err(anyhow::anyhow!("interrupted read"))
                            }
                        })
                        .is_err()
                );
                assert!(diagnostics.entries().is_empty());
            }
            let diagnostics = Diagnostics::new(paranoid);
            assert!(
                diagnostics
                    .preload("discovery", &|| true, || -> Result<()> {
                        panic!("cancelled work must not run")
                    })
                    .is_err()
            );
            assert!(diagnostics.entries().is_empty());
        }
        Ok(())
    }
}

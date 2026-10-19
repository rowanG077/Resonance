//! Session error policy shared by loading, simulation and presentation.
use anyhow::Result;
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
            eprintln!(
                "ERROR [{scope}] {message} ({})",
                if self.paranoid() {
                    "stopping: --paranoid"
                } else {
                    "continuing"
                }
            );
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

    pub fn has_errors(&self) -> bool {
        !self.0.entries.lock().unwrap().is_empty()
    }

    pub fn entries(&self) -> Vec<Diagnostic> {
        self.0.entries.lock().unwrap().clone()
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
}

//! Source preparation and authoring tools, independent of the game and cooker.
mod cache;
mod cli;
mod source;

pub use cache::{Generation, PreparationCache, PreparedModule};
pub use cli::run;
pub use source::{SourceTree, StandardSources};

use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("invalid module ID {0:?}; use ASCII identifiers separated by ::")]
    Module(String),
    #[error("source path is not a regular file or directory: {}", .0.display())]
    SourcePath(PathBuf),
    #[error(transparent)]
    Compile(#[from] symphonia_script_compiler::Diagnostic),
    #[error("formatting differs: {0}")]
    Formatting(String),
    #[error("{0}")]
    Usage(String),
    #[error("output: {0}")]
    Output(#[from] io::Error),
}

fn module_id(module: &str) -> Result<(), Error> {
    if module.split("::").all(|part| {
        let mut chars = part.bytes();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
    }) {
        Ok(())
    } else {
        Err(Error::Module(module.into()))
    }
}

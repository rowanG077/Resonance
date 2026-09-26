//! Readable SymphoniaScript authoring. Parsing and checking never require a game
//! world; native signatures are supplied by the same declarations used by hosts.
mod compile;
mod format;
mod syntax;

use std::collections::BTreeMap;
use symphonia_script::{Program, authored::NativeDeclaration};

pub use format::format;

/// A source module's execution host, declared before its imports and definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptKind {
    Field,
    Model,
    Battle,
    Library,
}

impl std::fmt::Display for ScriptKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Field => "field",
            Self::Model => "model",
            Self::Battle => "battle",
            Self::Library => "library",
        })
    }
}

/// Read and validate the required leading `script field|model|battle|library;` header.
pub fn script_kind(module: &str, source: &str) -> Result<ScriptKind, Diagnostic> {
    syntax::parse(module, source).map(|module| module.kind)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub line: u32,
    pub column: u32,
}
impl Location {
    pub(crate) fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            location: self.clone(),
            message: message.into(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}:{}:{}: {message}", .location.file, .location.line, .location.column)]
pub struct Diagnostic {
    pub location: Location,
    pub message: String,
}

/// Logical module IDs use `::`; callers choose their own filesystem or archive.
pub trait SourceResolver {
    fn source(&self, module: &str) -> Option<&str>;
}
impl SourceResolver for BTreeMap<String, String> {
    fn source(&self, module: &str) -> Option<&str> {
        self.get(module).map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetReference {
    /// Scalar passed to natives; resolves through this compiled module's bank.
    pub index: u32,
    pub kind: String,
    pub path: String,
}
#[derive(Debug)]
pub struct Compilation {
    pub kind: ScriptKind,
    pub program: Program,
    pub assets: Vec<AssetReference>,
    /// Exact transitive source inputs, useful for preparation-cache invalidation.
    pub sources: BTreeMap<String, String>,
}

/// Resolve a source graph, type-check it and produce an immutable VM program.
/// Native modules are imports too, but need no source file in the resolver.
pub fn compile(
    entry_module: &str,
    sources: &impl SourceResolver,
    natives: &[NativeDeclaration],
) -> Result<Compilation, Diagnostic> {
    compile::compile(entry_module, sources, natives)
}

/// Check performs the same validation as compilation, without retaining output.
pub fn check(
    entry_module: &str,
    sources: &impl SourceResolver,
    natives: &[NativeDeclaration],
) -> Result<(), Diagnostic> {
    compile(entry_module, sources, natives).map(|_| ())
}

/// The native API comes directly from host declarations, never a parallel file.
pub fn native_reference(natives: &[NativeDeclaration]) -> String {
    let mut natives = natives.to_vec();
    natives.sort_by_key(|native| native.name);
    let mut text = String::new();
    for native in natives {
        if native.suspends {
            text.push_str("await ");
        }
        text.push_str(native.name);
        text.push('(');
        text.push_str(
            &native
                .parameters
                .iter()
                .map(|ty| format!("{ty:?}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        text.push(')');
        if let Some(result) = native.result {
            text.push_str(&format!(" -> {result:?}"));
        }
        text.push('\n');
    }
    text
}

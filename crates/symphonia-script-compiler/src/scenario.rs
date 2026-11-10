//! Readable source for the original scenario VM, with lossless compilation.
//!
//! This profile retains the original memory and native ABI. It deliberately does
//! not lower through the authored VM's checked arithmetic or private locals.
mod auxiliary;
mod bank_reset;
mod catalog;
mod constant;
mod decode;
mod domains;
mod encode;
mod global_flow;
mod identity_flow;
mod operand_profiles;

pub use catalog::Definitions;
pub(crate) use decode::wrap_calls;
pub use decode::{
    Decompilation, IdentityUse, MessageIdentityUse, decompile, decompile_with_context,
};
pub(crate) use encode::validate_syntax;
pub use encode::{compile, compile_program};

use crate::{Diagnostic, Location};

/// Identity namespaces that share the original native instruction ABI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Context {
    #[default]
    Unspecified,
    Field,
    /// A known field resource, for scene-specific flag and model bindings.
    FieldScene(u16),
    World,
    Battle,
    Skit,
}
impl Context {
    /// Locate the innermost resource root, independent of enclosing directories.
    /// Callers with a known library root should pass a library-relative path.
    pub fn from_path(path: &std::path::Path) -> Self {
        let parts: Vec<_> = path.components().collect();
        for (index, part) in parts.iter().enumerate().rev() {
            // Resource names are not roots. An enclosing directory named
            // "skits" can still contain a complete fields/<scene>/scripts path.
            let field_root = part.as_os_str() == "fields"
                && parts
                    .get(index + 2)
                    .is_some_and(|part| part.as_os_str() == "scripts");
            if index > 0
                && !field_root
                && matches!(
                    parts[index - 1].as_os_str().to_str(),
                    Some("fields" | "skits")
                )
            {
                continue;
            }
            match part.as_os_str().to_str() {
                Some("fields") => {
                    let field = parts.get(index + 1).and_then(|part| {
                        let name = part.as_os_str().to_str()?;
                        let (prefix, digits) = name.rsplit_once('-')?;
                        (!prefix.is_empty()
                            && digits.len() == 4
                            && digits.bytes().all(|b| b.is_ascii_digit()))
                        .then(|| digits.parse().ok())
                        .flatten()
                    });
                    return field.map_or(Self::Field, Self::FieldScene);
                }
                Some("world") => return Self::World,
                Some("battle") => return Self::Battle,
                Some("skits") => return Self::Skit,
                _ => {}
            }
        }
        Self::Unspecified
    }

    pub(super) fn is_field(self) -> bool {
        matches!(self, Self::Field | Self::FieldScene(_))
    }
}

fn error(message: impl Into<String>) -> Diagnostic {
    Location {
        file: "scenario".into(),
        line: 1,
        column: 1,
    }
    .error(message)
}

fn literal_width(value: i32) -> &'static str {
    if i8::try_from(value).is_ok() {
        "s8"
    } else if i16::try_from(value).is_ok() {
        "s16"
    } else if u16::try_from(value).is_ok() {
        "u16"
    } else {
        "s32"
    }
}

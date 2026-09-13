//! SymphoniaScript: recovered scenario bytecode, inspection, and native-call metadata.
//! No game engine, filesystem, or emulator dependency.
pub mod authored;
pub mod message;
mod native;
pub use native::NativeCall;
mod program;
pub mod scenario;
pub mod semantics;
pub use program::{Op, Program, ProgramError, Width};

pub const GQSEAF_VM_CONSTANTS: &str = include_str!("../data/GQSEAF/vm_constants.json");

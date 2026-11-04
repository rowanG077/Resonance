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

/// Importer-owned transient byte outside the native variable allocation.
/// Nested scripted scenes reset it when returning to their caller; original
/// GameCube scripts use only the native variable region below 0x2000.
pub const COMPILER_TRANSIENT_BYTE: u16 = u16::MAX;

//! Checked-in sources embedded unchanged for immutable cooked publication.
//! `MODULES` uses logical `::` names. `FILES` paths are relative to `scripts/`.
include!(concat!(env!("OUT_DIR"), "/sources.rs"));

pub const PREVIEW_BINDINGS: &str = include_str!("../../../scripts/preview/bindings.json");

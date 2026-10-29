//! The library's small game and layout-version marker.
use anyhow::Result;
use resonance_content::library::{self, Descriptor};
use std::path::Path;

pub fn cook_descriptor(library: &Path) -> Result<()> {
    let descriptor = Descriptor {
        game: library::GAME.into(),
        version: library::VERSION,
    };
    crate::output::json(&library.join(library::DESCRIPTOR), &descriptor)?;
    Ok(())
}

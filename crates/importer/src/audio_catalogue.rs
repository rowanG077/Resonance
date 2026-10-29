//! One shared lookup for the native field and battle sound effect banks.
use crate::audio_effects;
use anyhow::{Result, ensure};
use resonance_content::library::Mappings;
use std::{collections::BTreeMap, path::Path};

pub fn cook_audio_catalogue(root: &Path, library: &Path) -> Result<()> {
    let output = library.join("audio/mappings.json");
    ensure!(!output.exists(), "audio mappings already exist");
    let mut entries = BTreeMap::new();
    for mapping in [
        audio_effects::cook_effects(root, library, None)?,
        audio_effects::cook_battle_cues(root, library, None)?,
    ] {
        for (id, target) in mapping.iter() {
            ensure!(
                entries.insert(id, target.to_owned()).is_none(),
                "duplicate audio cue ID {id}"
            );
        }
    }
    crate::output::json(&output, &Mappings::new(entries)?)?;
    Ok(())
}

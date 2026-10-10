//! Extend an owned encounter candidate with its verified audio dependency closure.
use anyhow::{Context, Result};
use resonance_content::{
    battle_audio::{self, Audio},
    prepared::{Cache, Files},
};
use std::path::Path;

pub fn prepare(
    root: &Path,
    files: Files,
    cache: &mut Cache,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, Option<Audio>)> {
    let decoded = (|| {
        let bytes = files.read_verified(
            battle_audio::PATH,
            files.digest(battle_audio::PATH)?,
            resonance_content::field_audio::MAX_MANIFEST_BYTES,
        )?;
        serde_json::from_slice::<
            Audio<resonance_content::field_audio::DecodedBank, serde_json::Value>,
        >(&bytes)
        .context("decode battle audio descriptor")
    })();
    let audio = match files
        .diagnostics()
        .attempt("battle audio descriptor", decoded)?
    {
        Some(audio) => audio.checked(files.diagnostics())?,
        None => None,
    };
    let inventory = audio
        .as_ref()
        .map(|audio| audio.files.clone())
        .unwrap_or_default();
    let files = files.with_dependencies(root, inventory, cache, cancelled)?;
    Ok((files, audio))
}

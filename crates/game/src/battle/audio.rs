//! Extend an owned encounter candidate with its verified audio dependency closure.
use anyhow::Result;
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
) -> Result<(Files, Audio)> {
    let audio: Audio = files.json(battle_audio::PATH)?;
    audio.validate()?;
    let files = files.with_dependencies(root, audio.files.clone(), cache, cancelled)?;
    Ok((files, audio))
}

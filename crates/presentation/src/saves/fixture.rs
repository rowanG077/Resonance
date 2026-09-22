//! Prepare development saves through the same field initialization as loading.
use super::*;
use std::{fs, io::Write, path::Path};

pub fn prepare_checkpoint_fixture(
    root: &Path,
    checkpoint: FieldCheckpoint,
    output: &Path,
) -> Result<()> {
    ensure!(!output.exists(), "checkpoint fixture already exists");
    let mut cache = loading::Cache::default();
    let package = new_game::FieldPackage::prepare(root, checkpoint.map_id, &mut cache, || false)?;
    let session =
        new_game::Session::load_prepared(root, package.files, Some(checkpoint), &mut cache)?;
    let state = session.field.checkpoint()?;
    let header = Header {
        identity: session.identity,
        label: "Oracle checkpoint".into(),
        location: format!("Field {}", state.map_id),
        played_ticks: state.played_ticks(),
        saved_unix_seconds: 0,
    };
    let bytes = resonance_persistence::encode(&header, &state)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(&bytes)?;
    Ok(())
}

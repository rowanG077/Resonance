use serde::de::DeserializeOwned;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn asset_root() -> PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"))
}

pub fn cooked<T: DeserializeOwned>(path: impl AsRef<Path>) -> T {
    let path = asset_root().join(path);
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

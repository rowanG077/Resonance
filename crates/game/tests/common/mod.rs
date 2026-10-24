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

// Some integration targets use only captured current documents.
#[allow(dead_code)]
pub fn cooked<T: DeserializeOwned>(path: impl AsRef<Path>) -> T {
    let path = asset_root().join(path);
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[allow(dead_code)]
pub fn field(id: u32) -> resonance_content::field::FieldAssets {
    let library = resonance_content::library::Library::open(&asset_root(), &[]).unwrap();
    resonance_content::field::document::capture_scene(&mut library.snapshot(), id, || false)
        .unwrap()
        .1
}

#[allow(dead_code)]
pub fn skits() -> resonance_content::skit::SkitCatalog {
    let library = resonance_content::library::Library::open(&asset_root(), &[]).unwrap();
    resonance_content::skit::document::capture(&mut library.snapshot(), &|| false).unwrap()
}

#[allow(dead_code)]
pub fn menu() -> resonance_content::menu_data::MenuData {
    use resonance_content::{
        diagnostics::Diagnostics, library::Library, preparation, prepared::Files,
    };
    let mut snapshot = Library::open(&asset_root(), &[]).unwrap().snapshot();
    let diagnostics = Diagnostics::new(true);
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false).unwrap();
    let files =
        Files::from_snapshot(snapshot, &mut Default::default(), || false, diagnostics).unwrap();
    resonance_content::menu_data::MenuData::load(&files).unwrap()
}

#[allow(dead_code)]
pub fn checkpoint_camera() -> resonance_events::camera::CameraSettings {
    let mut rig = resonance_events::camera::CameraRig::default();
    *rig.current_mut() = resonance_events::camera::EntryCamera::following(1).camera;
    rig.settings(1).unwrap()
}

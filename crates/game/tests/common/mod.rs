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
pub fn gameplay(
    library: &resonance_content::library::Library,
    strict: bool,
) -> anyhow::Result<resonance_content::prepared::Files> {
    use resonance_content::{diagnostics::Diagnostics, preparation, prepared::Files};
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(strict);
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    Files::from_snapshot(snapshot, || false, diagnostics)
}

#[allow(dead_code)]
pub fn menu() -> resonance_content::menu_data::MenuData {
    let library = resonance_content::library::Library::open(&asset_root(), &[]).unwrap();
    let files = gameplay(&library, true).unwrap();
    resonance_content::menu_data::MenuData::load(&files).unwrap()
}

#[allow(dead_code)]
pub fn checkpoint_camera() -> resonance_events::camera::CameraSettings {
    let mut rig = resonance_events::camera::CameraRig::default();
    *rig.current_mut() = resonance_events::camera::EntryCamera::following(1).camera;
    rig.settings(1).unwrap()
}

#[allow(dead_code)]
pub fn game_text() -> resonance_content::session::GameText {
    let mut files = resonance_content::prepared::Files::default();
    for path in [
        resonance_content::characters::PATH,
        resonance_content::titles::PATH,
        resonance_content::items::PATH,
        resonance_content::arte::PATH,
    ] {
        files.insert(
            path.into(),
            fs::read(asset_root().join(path)).unwrap().into(),
        );
    }
    resonance_content::session::GameText::load(&files).unwrap()
}

#[allow(dead_code)]
pub fn session() -> resonance_content::session::SessionData {
    let library = resonance_content::library::Library::open(&asset_root(), &[]).unwrap();
    let files = gameplay(&library, true).unwrap();
    resonance_content::session::SessionData::load(&files).unwrap()
}

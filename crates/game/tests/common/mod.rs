use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

pub fn asset_root() -> PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"))
}

#[allow(dead_code)]
pub fn library() -> resonance_content::library::Library {
    // Opening assembles every document; do it once per test binary.
    static LIBRARY: std::sync::OnceLock<resonance_content::library::Library> =
        std::sync::OnceLock::new();
    LIBRARY
        .get_or_init(|| resonance_content::library::Library::open(&asset_root(), &[]).unwrap())
        .clone()
}

/// Read through the library so granular documents arrive assembled.
fn read(paths: &[&str]) -> Vec<std::sync::Arc<[u8]>> {
    let library = library();
    let mut snapshot = library.snapshot();
    paths
        .iter()
        .map(|path| {
            snapshot
                .read(path, resonance_content::preparation::ASSET_LIMIT)
                .unwrap_or_else(|error| panic!("{path}: {error:#}"))
        })
        .collect()
}

/// Publish an edited document as a mod's granular owners, with removal
/// markers for the entries of the base document which the edit dropped.
#[allow(dead_code)]
pub fn write_mod(root: &Path, path: &str, document: &impl serde::Serialize) -> anyhow::Result<()> {
    resonance_content::authoring::publish_over(
        root,
        path,
        &cooked(path),
        &serde_json::to_value(document)?,
    )?;
    Ok(())
}

// Some integration targets use only captured current documents.
#[allow(dead_code)]
pub fn cooked<T: DeserializeOwned>(path: impl AsRef<Path>) -> T {
    let path = path.as_ref().to_str().unwrap();
    serde_json::from_slice(&read(&[path])[0]).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[allow(dead_code)]
pub fn field(id: u32) -> resonance_content::field::FieldAssets {
    let library = library();
    resonance_content::field::document::capture_scene(&mut library.snapshot(), id, || false)
        .unwrap()
        .1
}

#[allow(dead_code)]
pub fn skits() -> resonance_content::skit::SkitCatalog {
    let library = library();
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
    let library = library();
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
    let paths = [
        resonance_content::characters::PATH,
        resonance_content::titles::PATH,
        resonance_content::items::PATH,
        resonance_content::arte::PATH,
    ];
    for (path, bytes) in paths.into_iter().zip(read(&paths)) {
        files.insert(path.into(), bytes);
    }
    resonance_content::session::GameText::load(&files).unwrap()
}

#[allow(dead_code)]
pub fn session() -> resonance_content::session::SessionData {
    let library = library();
    let files = gameplay(&library, true).unwrap();
    resonance_content::session::SessionData::load(&files).unwrap()
}

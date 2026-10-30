use resonance_content::{diagnostics::Diagnostics, library::Library, preparation, prepared::Files};

pub fn files(root: &std::path::Path) -> Files {
    let mut snapshot = Library::open(root, &[]).unwrap().snapshot();
    let diagnostics = Diagnostics::new(true);
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false).unwrap();
    Files::from_snapshot(snapshot, || false, diagnostics).unwrap()
}

pub fn session(root: &std::path::Path) -> resonance_content::session::SessionData {
    resonance_content::session::SessionData::load(&files(root)).unwrap()
}

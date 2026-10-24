use resonance_content::{diagnostics::Diagnostics, library::Library, preparation, prepared::Files};

pub fn menu(root: &std::path::Path) -> resonance_content::menu_data::MenuData {
    let mut snapshot = Library::open(root, &[]).unwrap().snapshot();
    let diagnostics = Diagnostics::new(true);
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false).unwrap();
    let files =
        Files::from_snapshot(snapshot, &mut Default::default(), || false, diagnostics).unwrap();
    resonance_content::menu_data::MenuData::load(&files).unwrap()
}

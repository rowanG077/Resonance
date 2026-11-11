use super::*;

#[test]
fn ktx_replacements_and_mips_stay_with_the_preparation() -> Result<()> {
    let fixture = Fixture::new();
    let path = "meshes/sign/wood.ktx2";
    let encode =
        |value| resonance_asset_writer::ktx2::encode_levels(2, 2, &[&[value; 16], &[value; 4]]);
    let original = encode(17)?;
    let replacement = encode(31)?;
    fixture.write(&format!("base/{path}"), &original);
    fixture.write(&format!("mod/{path}"), &replacement);
    let library = fixture.open(&["mod"])?;
    let mut retained = library.snapshot();
    assert_eq!(retained.read(path, 4096)?.as_ref(), replacement);
    let changed = encode(97)?;
    fixture.write(&format!("mod/{path}"), &changed);
    assert_eq!(library.snapshot().read(path, 4096)?.as_ref(), changed);
    assert_eq!(retained.read(path, 4096)?.as_ref(), replacement);
    assert_eq!(
        fixture.open(&[])?.snapshot().read(path, 4096)?.as_ref(),
        original
    );
    let files = crate::prepared::Files::from_snapshot(
        retained,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    fs::remove_file(fixture.0.path().join(format!("mod/{path}")))?;
    fs::remove_file(fixture.0.path().join(format!("base/{path}")))?;
    assert_eq!(files.read(path)?.as_ref(), replacement);
    assert!(library.snapshot().read(path, 4096).is_err());
    Ok(())
}

#[test]
fn larger_replacements_keep_document_coordinates_and_selected_bytes() -> Result<()> {
    use resonance_asset_writer::{gpu, ktx2};
    for compressed in [false, true] {
        let fixture = Fixture::new();
        let path = "ui/cursor.ktx2";
        fixture.write(
            &format!("base/{path}"),
            &ktx2::encode_rgba8(2, 2, &[17; 16])?,
        );
        fixture.write(
            "base/ui/cursor.json",
            br#"{"image":"ui/cursor.ktx2","size":[3,5]}"#,
        );
        let replacement = if compressed {
            gpu::encode_rgba8(16, 16, &[31; 1024])?
        } else {
            ktx2::encode_rgba8(16, 16, &[31; 1024])?
        };
        fixture.write(&format!("mod/{path}"), &replacement);
        let library = fixture.open(&["mod"])?;
        fs::remove_file(fixture.0.path().join(format!("base/{path}")))?;
        let mut snapshot = library.snapshot();
        let mut cursor: crate::font::UiTexture = snapshot.json("ui/cursor.json", 4096)?;
        crate::ui_art::prepare_image(&mut cursor, |path| snapshot.read(path, 4096))?;
        assert_eq!([cursor.width, cursor.height], [3, 5]);
        assert_eq!(
            snapshot.read(path, replacement.len())?.as_ref(),
            replacement
        );
        assert_eq!(
            library
                .snapshot()
                .read_optional(path, replacement.len())?
                .unwrap()
                .as_ref(),
            replacement
        );
        assert!(
            library
                .snapshot()
                .read_optional(path, replacement.len() - 1)
                .is_err()
        );
    }
    Ok(())
}

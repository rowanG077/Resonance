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

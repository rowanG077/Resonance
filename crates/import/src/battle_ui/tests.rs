use super::*;

#[test]
#[ignore = "requires both extracted discs; private texture output"]
fn publishes_complete_hud_on_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in [1, 2] {
        let extracted = local.join(format!("disc{disc}"));
        let output = tempfile::tempdir()?;
        let paths = publish(&extracted, output.path())?;
        let art: Art = serde_json::from_slice(&fs::read(
            output.path().join(resonance_content::battle_ui::PATH),
        )?)?;
        art.validate()?;
        assert!(
            art.files()
                .all(|path| paths.iter().any(|published| published == path))
        );
        assert!(paths.iter().all(|path| output.path().join(path).is_file()));

        let sources = crate::source_assets::Sources::read(&extracted)?;
        let usual = fs::read(extracted.join("files").join(sources.usual))?;
        let portraits = section(section(&usual, 4)?, 2)?;
        let (width, height, pixels) = crate::tpl::decode(portraits)?.remove(0);
        let portrait = art.portraits[0].as_ref().unwrap();
        assert_eq!([portrait.width, portrait.height], [width, height]);
        assert_eq!(
            *crate::texture::pixels(&output.path().join(&portrait.path))?.as_raw(),
            pixels
        );
        let atlas = section(section(&usual, 4)?, 3)?;
        let texture = &crate::tpl::parse_tpl(atlas)?[0];
        assert_eq!(
            *crate::texture::pixels(&output.path().join(&art.font.texture))?.as_raw(),
            crate::tpl::decode_texture(atlas, texture)?
        );
    }
    Ok(())
}

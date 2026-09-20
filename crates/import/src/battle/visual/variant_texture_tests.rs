use super::*;

#[test]
fn body_variant_keeps_expression_channels_independent_and_zero_rows_disable_it() {
    // Original enemy100: one eight-frame expression channel on texture1;
    // formation variants occupy the four vertical rows of texture2.
    let mut metadata = [0; 0x1f0];
    metadata[0xce] = 1;
    metadata[0xcf] = 8;
    metadata[0xd3] = 1;
    metadata[0xee] = 2;
    metadata[0xef] = 4;
    let expressions = texture_layers(&metadata, None).unwrap();
    assert_eq!(expressions.len(), 1);
    assert_eq!((expressions[0].texture, expressions[0].frames), (1, 8));
    let variant = variant_texture(&metadata).unwrap().unwrap();
    assert_eq!((variant.texture, variant.frames), (2, 4));
    metadata[0xef] = 0;
    assert!(variant_texture(&metadata).unwrap().is_none());
    assert_eq!(texture_layers(&metadata, None).unwrap().len(), 1);
    metadata[0xce] = 5;
    assert!(texture_layers(&metadata, None).is_err());
    assert!(variant_texture(&metadata[..0xef]).is_err());
}

#[test]
#[ignore = "requires privately extracted US enemy archives; decodes complete original body atlases"]
fn original_formation_variants_use_existing_cmpr_body_rows_not_color_palettes() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let directory = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
    let table = word(&directory, 0x2c).unwrap() as usize;
    for (id, texture_index, size, hash) in [
        (
            73,
            1,
            [128, 512],
            "4b4e60561157c15076d8f42b685b8b64a46166b3319d011cf163ffc042292516",
        ),
        (
            100,
            2,
            [256, 1024],
            "82fdd8d5815aee44af85442a9636c242b612de028e46ed97fe7410543dc8b5e2",
        ),
        (
            101,
            2,
            [256, 1024],
            "4b89e421078a1475a7b4f6b4090c387b46f290d9ccb8a03e6d077166f8328498",
        ),
        (
            104,
            4,
            [256, 1024],
            "faa9de7f588b9a38a9b17188284f15f68628112d0cbbea992d03292b79568f65",
        ),
    ] {
        let start = word(&directory, table + id * 4).unwrap() as usize;
        let end = word(&directory, table + (id + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        assert_eq!(digest(&bytes), hash);
        let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
        let variant = variant_texture(metadata).unwrap().unwrap();
        assert_eq!((variant.texture, variant.frames), (texture_index, 4));
        let model = &bytes[word(&bytes, 0x18).unwrap() as usize..];
        let tpl = &model[word(model, 0).unwrap() as usize..word(model, 4).unwrap() as usize];
        let textures = crate::tpl::parse_tpl(tpl).unwrap();
        let texture = &textures[usize::from(variant.texture)];
        assert_eq!([texture.width, texture.height], size);
        assert_eq!(texture.format, 14);
        assert!(texture.palette_offset.is_none());
        let rgba = crate::tpl::decode_texture(tpl, texture).unwrap();
        assert_eq!(rgba.len(), usize::from(size[0]) * usize::from(size[1]) * 4);
        let row_bytes = rgba.len() / 4;
        let row = |index: usize| &rgba[index * row_bytes..(index + 1) * row_bytes];
        // The image decoder must retain distinct variant pixels in the full atlas.
        assert_ne!(row(0), row(1));
        assert_ne!(row(0), row(3));
        let expressions = texture_layers(metadata, None).unwrap();
        assert_eq!(expressions.len(), usize::from(id == 100));
        if id == 100 {
            assert_eq!((expressions[0].texture, expressions[0].frames), (1, 8));
        }
    }
}

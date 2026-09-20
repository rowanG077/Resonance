use super::*;

#[test]
#[ignore = "requires original extracted discs; decodes palettes and programs without encoding"]
fn original_effect_palette_windows_keep_native_count_after_base_selection() {
    for disc in 1..=2 {
        let extracted =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../local/extracted/disc{disc}"));
        let archive = MagicArchive::read(&extracted).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let mut cooker = test_cooker();
        cooker.textures = fixed_textures(&usual).unwrap();
        for package in [5, 9, 51, 109, 116, 118] {
            let source = archive.package(package).unwrap();
            let texture = magic_member(source, 8).unwrap().unwrap();
            cooker
                .textures
                .insert(TextureBank::Magic(package), texture.to_vec());
            let bank = magic_member(source, 4).unwrap().unwrap();
            for id in 1..=if package == 118 { 2 } else { 1 } {
                cooker
                    .program(
                        bank,
                        EffectId {
                            bank: EffectBank::Magic(package),
                            id,
                        },
                    )
                    .unwrap();
            }
        }
        cooker.result.validate().unwrap();
        assert!(!cooker.pending_images.is_empty());
    }
}

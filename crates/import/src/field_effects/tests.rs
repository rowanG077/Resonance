use super::*;
use std::io::{Cursor, Read};

#[test]
#[cfg(unix)]
#[ignore = "requires both original discs and frozen recipes; private output, no audio devices"]
fn original_field_effects_bind_shared_images_and_renamed_declarations() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let library = local.join("all-assets");
    for disc in [1, 2] {
        let root = tempfile::tempdir()?;
        let extracted = root.path().join("extracted");
        let output = root.path().join("prepared");
        fs::create_dir_all(extracted.join("sys"))?;
        fs::create_dir_all(extracted.join("files/Art"))?;
        let original = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(original.join("sys/main.dol"))?;
        let source = crate::cooked::Source::open(&library, disc, "sys/main.dol")?;
        let mut recipe = Recipe::read(&executable)?;
        let frozen: Recipe = source.document("embedded/field-effects.json")?;
        ensure!(
            serde_json::to_value(&recipe)? == serde_json::to_value(frozen)?,
            "field effect recipe changed"
        );
        assert_eq!(recipe.catalogue.entries.len(), 79);
        assert_eq!(recipe.effects.paralysis.missing_anchor_offset, [0.; 3]);
        assert_eq!(
            recipe.effects.sprites[&0].uv,
            [0., 64., 63., 127.].map(|v| v / 256.)
        );
        assert_eq!(
            recipe.effects.emotes.keys().copied().collect::<Vec<_>>(),
            (0..20).collect::<Vec<_>>()
        );
        let archive =
            crate::all_assets::roles::declared_path(&original.join("files"), &recipe.archive)?;
        let (images, _) = textures(&original, &output, &recipe.archive)?;
        let mut cab = cab::Cabinet::new(Cursor::new(fs::read(
            original.join("files").join(&archive),
        )?))?;
        let mut bytes = Vec::new();
        cab.read_file("EFFECT.TPL")?.read_to_end(&mut bytes)?;
        let expected = crate::tpl::decode(&bytes)?;
        assert_eq!(images.len(), 9);
        assert_eq!(images.len(), expected.len());
        for (image, (width, height, pixels)) in images.iter().zip(expected) {
            let image = image.image(0)?;
            assert_eq!((image.width, image.height), (width, height));
            assert_eq!(
                crate::texture::pixels(&output.join(&image.path))?.as_raw(),
                &pixels
            );
        }
        let dialogue: serde_json::Value = source.document("embedded/dialogue.json")?;
        let system = crate::all_assets::roles::declared_path(
            &original.join("files"),
            dialogue["system"].as_str().unwrap(),
        )?;
        fs::write(extracted.join("sys/main.dol"), &executable)?;
        fs::copy(
            original.join("sys/boot.bin"),
            extracted.join("sys/boot.bin"),
        )?;
        fs::copy(
            original.join("files").join(&archive),
            extracted.join("files/Art/Effects.bin"),
        )?;
        fs::copy(
            original.join("files").join(&system),
            extracted.join("files").join(&system),
        )?;
        recipe.archive = "art/effects.bin".into();
        let prepared = prepare(&extracted, &output, recipe)?;
        let effects: FieldEffects =
            serde_json::from_slice(&fs::read(output.join(&prepared.path))?)?;
        effects.validate()?;
        assert_eq!(effects.emote_texture, images[1].image(0)?.path);
        for (sprite, texture) in [(0, 0), (8, 2), (10, 2)] {
            assert_eq!(
                effects.sprites[&sprite].texture,
                images[texture].image(0)?.path
            );
        }
        assert_eq!(effects.refraction.sprite.texture, images[5].image(0)?.path);
        assert_eq!(prepared.particles[&25].texture, images[2].image(0)?.path);
        assert!(
            prepared
                .files
                .iter()
                .all(|path| path.starts_with("textures/") && output.join(path).is_file())
        );
        assert!(!output.join("sources.json").exists() && !output.join("intermediate").exists());
        fs::remove_file(extracted.join("files/Art/Effects.bin"))?;
        assert!(textures(&extracted, &output, "art/effects.bin").is_err());
    }
    Ok(())
}

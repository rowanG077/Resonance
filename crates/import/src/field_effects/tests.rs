use super::*;
use std::io::{Cursor, Read};

#[test]
#[cfg(unix)]
#[ignore = "requires both original discs and cook-all; no encoders or output devices"]
fn original_field_effects_bind_shared_images_and_renamed_declarations() -> Result<()> {
    use std::os::unix::fs::symlink;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let library = local.join("all-assets");
    let root = crate::temporary_path(&std::env::temp_dir().join("field-effects"));
    let extracted = root.join("extracted");
    let output = root.join("prepared");
    let result = (|| -> Result<()> {
        fs::create_dir_all(extracted.join("sys"))?;
        fs::create_dir_all(extracted.join("files/Art"))?;
        fs::create_dir_all(output.join("data/embedded"))?;
        symlink(library.join("assets"), output.join("assets"))?;
        for disc in [1, 2] {
            let original = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(original.join("sys/main.dol"))?;
            let source = crate::cooked::Source::open(&library, disc, "sys/main.dol")?;
            let mut recipe: Recipe = source.document("embedded/field-effects.json")?;
            assert_eq!(recipe.catalogue.entries.len(), 79);
            assert_eq!(recipe.effects.paralysis.missing_anchor_offset, [0.; 3]);
            assert_eq!(
                recipe.effects.sprites[&0].uv,
                [0., 64., 63., 127.].map(|v| v / 256.)
            );
            assert_eq!(
                serde_json::to_value(&recipe)?,
                serde_json::to_value(Recipe::read(&executable)?)?
            );
            assert_eq!(
                recipe.effects.emotes.keys().copied().collect::<Vec<_>>(),
                (0..20).collect::<Vec<_>>()
            );
            let archive =
                crate::all_assets::roles::declared_path(&original.join("files"), &recipe.archive)?;
            let (images, _) = textures(&original, &library, &recipe.archive)?;
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
                    crate::texture::pixels(&library.join(&image.path))?.as_raw(),
                    &pixels
                );
            }
            let mut dialogue: serde_json::Value = source.document("embedded/dialogue.json")?;
            let system = crate::all_assets::roles::declared_path(
                &original.join("files"),
                dialogue["system"].as_str().unwrap(),
            )?;
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
                extracted.join("files/Art/Windows.bin"),
            )?;
            recipe.archive = "art/effects.bin".into();
            dialogue["system"] = serde_json::json!("art/windows.bin");
            write_atomic(
                &output.join("data/embedded/field-effects.json"),
                &serde_json::to_vec(&recipe)?,
            )?;
            write_atomic(
                &output.join("data/embedded/dialogue.json"),
                &serde_json::to_vec(&dialogue)?,
            )?;
            let mut sources: BTreeMap<String, Vec<String>> =
                serde_json::from_slice(&fs::read(library.join("sources.json"))?)?;
            for (old, new) in [
                (archive.as_str(), "Art/Effects.bin"),
                (system.as_str(), "Art/Windows.bin"),
            ] {
                let paths = sources.remove(&format!("disc{disc}/{old}")).unwrap();
                sources.insert(format!("disc{disc}/{new}"), paths);
            }
            write_atomic(&output.join("sources.json"), &serde_json::to_vec(&sources)?)?;
            let prepared = cook(&extracted, &output)?;
            let effects: FieldEffects =
                serde_json::from_slice(&fs::read(output.join(&prepared.path))?)?;
            effects.validate()?;
            assert_eq!(effects.emote_texture, images[1].image(0)?.path);
            assert_eq!(effects.sprites[&0].texture, images[0].image(0)?.path);
            assert_eq!(effects.sprites[&8].texture, images[2].image(0)?.path);
            assert_eq!(effects.sprites[&10].texture, images[2].image(0)?.path);
            assert_eq!(effects.refraction.sprite.texture, images[5].image(0)?.path);
            assert_eq!(prepared.particles[&25].texture, images[2].image(0)?.path);
            assert!(
                prepared
                    .files
                    .iter()
                    .all(|path| path.starts_with("assets/") && output.join(path).is_file())
            );
            assert_eq!(fs::read_dir(output.join("effects"))?.count(), 1);
            assert!(
                !output.join("intermediate").exists() && !extracted.join("sys/main.dol").exists()
            );
            fs::remove_file(extracted.join("files/Art/Effects.bin"))?;
            assert!(cook(&extracted, &output).is_err());
        }
        Ok(())
    })();
    fs::remove_dir_all(root)?;
    result
}

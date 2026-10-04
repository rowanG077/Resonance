use super::*;
use std::io::{Cursor, Read};

#[test]
#[ignore = "requires the extracted original discs"]
fn field_sprites_and_palette_decode_from_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in [1, 2] {
        let executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
        let recipe = Recipe::read(&executable)?;
        recipe.blink.validate()?;
        recipe.shadow.validate()?;
        let effects = recipe.effects;
        effects.validate()?;
        let smoke = &effects.sprites[&resonance_content::effect::SMOKE_SPRITE];
        assert_eq!(smoke.uv_at(6), [0., 0., 63. / 256., 63. / 256.]);
        assert_eq!(smoke.uv_at(7), [64. / 256., 0., 127. / 256., 63. / 256.]);
        assert_eq!(
            smoke.uv_at(55),
            [192. / 256., 64. / 256., 255. / 256., 127. / 256.]
        );
        assert_ne!(
            effects.refraction.sprite.texture,
            effects.air_refraction.texture
        );
        assert!(effects.sprites[&23].additive);
        // Debris variants share one atlas.
        for (kind, uv) in [
            (52, [128., 0., 191., 63.]),
            (53, [192., 0., 255., 63.]),
            (54, [64., 64., 127., 127.]),
        ] {
            let debris = &effects.sprites[&kind];
            assert_eq!(debris.uv, uv.map(|v| v / 256.));
            assert_eq!(debris.texture, effects.sprites[&0].texture);
            assert!(!debris.additive);
            assert_eq!(debris.uv_at(162), debris.uv);
        }
        assert_eq!(effects.palette[33], [63, 63, 63, 255]);
        let electric = &effects.sprites[&42];
        assert_eq!(electric.uv_at(2), [32., 128., 62., 254.].map(|v| v / 256.));
        assert_eq!(electric.uv_at(6), electric.uv_at(0));
        assert_ne!(electric.uv_at(4), electric.uv_at(0));
        let sprite = &effects.sprites[&6];
        assert_eq!(sprite.uv, [129., 0., 192., 63.].map(|v| v / 256.));
        assert!(sprite.additive);
        let ring = &effects.sprites[&41];
        assert_eq!(ring.uv, [129., 0., 192., 63.].map(|v| v / 256.));
        assert_eq!(ring.texture, sprite.texture);
        assert!(ring.additive);
        // Station sprites retain their atlas regions on both discs.
        for (id, uv) in [
            (4, [0., 0., 63., 63.]),
            (5, [129., 0., 192., 63.]),
            (7, [0., 64., 63., 127.]),
            (22, [128., 192., 190., 254.]),
        ] {
            assert_eq!(effects.sprites[&id].uv, uv.map(|v| v / 256.));
            assert_eq!(effects.sprites[&id].texture, sprite.texture);
        }
        // Air refraction uses the full image except its padded border.
        assert_eq!(
            effects.air_refraction.uv,
            [0., 0., 254. / 256., 254. / 256.]
        );
        assert_eq!(effects.palette[48], [48, 48, 189, 255]);
    }
    Ok(())
}

#[test]
#[cfg(unix)]
#[ignore = "requires both original discs; private output, no audio devices"]
fn original_field_effects_bind_shared_images_and_renamed_declarations() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    for disc in [1, 2] {
        let root = tempfile::tempdir()?;
        let extracted = root.path().join("extracted");
        let output = root.path().join("prepared");
        fs::create_dir_all(extracted.join("sys"))?;
        fs::create_dir_all(extracted.join("files/Art"))?;
        let original = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(original.join("sys/main.dol"))?;
        let mut recipe = Recipe::read(&executable)?;
        assert_eq!(
            recipe.effects.sprites[&6].uv,
            [129., 0., 192., 63.].map(|v| v / 256.)
        );
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
        let system = crate::all_assets::roles::declared_path(
            &original.join("files"),
            // The system-art archive declaration in the original executable.
            &dol::text(&executable, 0x8017_A55C)?,
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

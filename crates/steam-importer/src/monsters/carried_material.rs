//! Real carried-package regression: preserve embedded outlines and expand neutral RGB only.
use super::*;

#[test]
#[ignore = "requires installed Steam; optionally writes only eight source-eligible monster models to RESONANCE_CARRIED_VARIANT_OUTPUT"]
fn installed_additive_carried_variants_expand_rgb_once_without_motion_changes() -> Result<()> {
    let source = Source::open(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
        ),
        crate::Language::English,
    )?;
    let temporary = tempfile::tempdir()?;
    let destination = std::env::var_os("RESONANCE_CARRIED_VARIANT_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let image = pe::Image::parse(&source.executable)?;
    const OWNERS: [u16; 8] = [71, 235, 236, 237, 238, 241, 242, 255];
    let mut eligible = Vec::new();
    for id in 0..count(&image)? as u16 {
        let bytes = source.require(package(&image, id)?)?;
        let metadata = metadata(&bytes)?;
        if metadata[0x1f0] != 0
            && metadata[0x130] & 1 == 0
            && let Some(bytes) = preview_model::member(&bytes, 0x160)?
        {
            let carried = Package::parse(bytes)?;
            if carried.len() == 6 && carried.reference(5)?.is_some() {
                eligible.push(id);
            }
        }
    }
    ensure!(
        eligible == OWNERS,
        "source additive variant directory changed"
    );
    models_selected(&source, &destination, Some(&OWNERS))?;
    for id in OWNERS {
        let bytes = source.require(package(&image, id)?)?;
        let metadata = metadata(&bytes)?;
        ensure!(metadata[0x1f0] != 0 && metadata[0x130] == 0);
        let carried = Package::parse(
            preview_model::member(&bytes, 0x160)?.context("missing carried slot0")?,
        )?;
        ensure!(carried.len() == 6 && carried.reference(2)?.is_none());
        let descriptor = carried.reference(5)?.context("missing additive variant")?;
        let stem = match id {
            71 => "elem14",
            235 => "bh22",
            236 => "ba00",
            237 => "ba01",
            238 => "ba02",
            241 => "ba05",
            242 => "ba06",
            255 => "ex04",
            _ => unreachable!(),
        };
        ensure!(descriptor == format!("{stem}_obj002.tomdlb_d"));
        let owner = format!("meshes/monsters/{}/carried-0", symbol(id));
        let reference = tempfile::tempdir()?;
        let (scene, mut native) = preview_model::part(
            &source,
            reference.path(),
            descriptor,
            &owner,
            "variant",
            &[],
        )?;
        ensure!(scene.clips.is_empty() && !scene.autoplay);
        let original = std::fs::read(reference.path().join(&scene.mesh))?;
        let mut vertices = 0;
        for mesh in &mut native.meshes {
            for color in &mut mesh.colors {
                let expected = match id {
                    71 => [129. / 255., 128. / 255., 129. / 255.],
                    236..=238 => [68. / 255., 64. / 255., 68. / 255.],
                    _ => [119. / 255., 128. / 255., 119. / 255.],
                };
                ensure!(color[..3] == expected);
                let alpha = color[3];
                ensure!(if id == 71 {
                    (0. ..=185. / 255.).contains(&alpha)
                } else {
                    alpha == 1.
                });
                for channel in &mut color[..3] {
                    *channel = (*channel * 2.).min(1.);
                }
                ensure!(color[3].to_bits() == alpha.to_bits());
                vertices += 1;
            }
        }
        ensure!(vertices != 0);
        let stem = descriptor.rsplit_once('.').unwrap().0;
        let (tints, _, _) = crate::model::material_parameters(
            &source.require(&format!("{stem}.mxd"))?,
            &native.materials,
        )?;
        let expected = native.glb(&tints)?;
        // Compare the actual production carried path against an independent
        // raw-source RGB edit; omitted or doubled normalization both fail.
        let actual = std::fs::read(destination.join(&scene.mesh))?;
        ensure!(actual == expected && actual != original);
        ensure!(scene.clips.is_empty() && !scene.autoplay);
        eprintln!(
            "enemy{id}: {vertices} additive core vertices RGB2; source alpha and no-motion variant retained"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY; reads only representative carried owners63/84/86"]
fn cooked_carried_material_owners_prepare_through_shared_reader() -> Result<()> {
    use resonance_content::{battle_enemy::document, diagnostics::Diagnostics, library::Library};
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let library = Library::open(&root, &[])?;
    let diagnostics = Diagnostics::new(true);
    for id in [63, 84, 86] {
        let captured = document::capture(&mut library.snapshot(), id, &diagnostics, &|| false)?;
        let artwork = captured.artwork.context("missing enemy artwork")?;
        let carried = artwork
            .source
            .attachments
            .get(&0)
            .context("missing native carried slot0")?;
        ensure!(
            carried.layers.len() == 1,
            "duplicate outline survived shared reader"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; publishes only source-declared carried owners"]
fn installed_publish_source_carried_owners() -> Result<()> {
    let source = Source::open(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
        ),
        crate::Language::English,
    )?;
    let library = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let image = pe::Image::parse(&source.executable)?;
    let mut selected = Vec::new();
    let mut slots = Vec::new();
    for id in 0..count(&image)? as u16 {
        let bytes = source.require(package(&image, id)?)?;
        let mut found = false;
        let before_body = metadata(&bytes)?[0x130] & 1 != 0;
        for slot in 0..usize::from(metadata(&bytes)?[0x1f0]) {
            let Some(bytes) = preview_model::member(&bytes, 0x160 + slot * 4)? else {
                continue;
            };
            let part = Package::parse(bytes)?;
            if part.reference(1)?.is_none() {
                continue;
            }
            found = true;
            let layers = 1
                + usize::from(part.reference(2)?.is_some())
                + usize::from(
                    slot == 0 && !before_body && part.len() == 6 && part.reference(5)?.is_some(),
                );
            slots.push((id, slot, layers));
        }
        if found {
            selected.push(id);
        }
    }
    ensure!(!selected.is_empty(), "source carried directory is empty");
    eprintln!("Source carried owners: {selected:?}");
    models_selected(&source, &library, Some(&selected))?;
    crate::battle_enemies::cook(&source, &library, Some(&selected))?;
    for &(id, slot, expected_layers) in &slots {
        let path = library.join(format!("battle/enemies/{}/enemy.json", symbol(id)));
        let owner: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let layers = owner["artwork"]["attachments"][slot.to_string()]["layers"]
            .as_array()
            .context("missing source carried slot")?;
        ensure!(
            layers.len() == expected_layers,
            "enemy{id} carried{slot} lost or invented an authored layer"
        );
    }
    eprintln!(
        "Published {} source-declared carried owners / {} slots; every slot retains exactly its authored layer count",
        selected.len(),
        slots.len()
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; publishes only monster/enemy63+86"]
fn installed_teddy_warrior_carried_bodies_keep_authored_layers_and_alpha() -> Result<()> {
    let source = Source::open(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
        ),
        crate::Language::English,
    )?;
    let library = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    models_selected(&source, &library, Some(&[63, 86]))?;
    crate::battle_enemies::cook(&source, &library, Some(&[63, 86]))?;
    let image = pe::Image::parse(&source.executable)?;
    for id in [63, 86] {
        let bytes = source.require(package(&image, id)?)?;
        let carried = Package::parse(
            preview_model::member(&bytes, 0x160)?.context("missing carried package")?,
        )?;
        ensure!(
            carried.reference(2)?.is_none(),
            "fixture now has an authored second outline"
        );
        let descriptor = carried.reference(1)?.context("missing carried body")?;
        let source_bytes = source.require(descriptor)?;
        let payload = crate::package::dependencies(&source_bytes)?;
        let mut native = crate::model::Model::read(&source_bytes, &source.require(&payload[0])?)?;
        let mut coloured = 0;
        for mesh in &mut native.meshes {
            for color in &mut mesh.colors {
                let alpha = color[3];
                if color[..3] != [0.; 3] {
                    ensure!(
                        color[..3] == [119. / 255., 128. / 255., 119. / 255.],
                        "unexpected native carried RGB fixture"
                    );
                    coloured += 1;
                }
                for channel in &mut color[..3] {
                    *channel = (*channel * 2.).min(1.);
                }
                ensure!(
                    color[3].to_bits() == alpha.to_bits(),
                    "initial alpha changed"
                );
            }
        }
        ensure!(coloured > 0, "missing coloured surface");
        let stem = descriptor
            .rsplit_once('.')
            .context("missing descriptor extension")?
            .0;
        let (tints, _, _) = crate::model::material_parameters(
            &source.require(&format!("{stem}.mxd"))?,
            &native.materials,
        )?;
        let expected = native.glb(&tints)?;
        let cooked: serde_json::Value = serde_json::from_slice(&std::fs::read(
            library.join(format!("battle/enemies/{}/enemy.json", symbol(id))),
        )?)?;
        let layers = cooked["artwork"]["attachments"]["0"]["layers"]
            .as_array()
            .context("missing carried layers")?;
        ensure!(
            layers.len() == 1,
            "generated duplicate outline survived source-absent ref2"
        );
        let mesh = layers[0]["mesh"].as_str().context("missing cooked mesh")?;
        ensure!(
            std::fs::read(library.join(mesh))? == expected,
            "published carried RGB/alpha differs from normalized native input"
        );
        eprintln!(
            "enemy{id} {descriptor}: one source layer, {coloured} neutral-RGB vertices expanded; initial alpha unchanged"
        );
    }
    Ok(())
}

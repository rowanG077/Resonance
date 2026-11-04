//! Source-declared layered wings share the GC color-scroll texture slot.
use super::*;

pub(super) fn wing_color_slot(scene: &mut resonance_content::ScenePart) -> Result<()> {
    ensure!(
        scene.textures.len() == 1
            && !scene.materials.is_empty()
            && scene.materials.iter().all(|material| {
                material
                    .color
                    .as_ref()
                    .is_some_and(|binding| binding.texture == 0)
                    && material.multiply.is_none()
            }),
        "layered wing does not have one unambiguous color atlas"
    );
    // EnemyWings scrolls the color atlas in semantic slot1. Steam combines
    // its feather geometry and palette into one image; duplicate the path so
    // the ordinary shared wing renderer retains that slot without new pixels,
    // shader modes, UV edits or a source-edition branch.
    scene.textures.push(scene.textures[0].clone());
    for material in &mut scene.materials {
        material.color.as_mut().unwrap().texture = 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wing_palette_uses_shared_scroll_slot_without_changing_art_or_motion() -> Result<()> {
        let original: resonance_content::ScenePart = serde_json::from_value(json!({
            "mesh":"wing.glb", "textures":["palette.ktx2"],
            "materials":[{"meshes":[0],"color":{"texture":0},"blend":true,"depth_write":false}],
            "clips":[{"motion":"wing-motion.glb","slots":[50]}]
        }))?;
        let mut converted = original.clone();
        wing_color_slot(&mut converted)?;
        assert_eq!(converted.textures, ["palette.ktx2", "palette.ktx2"]);
        assert_eq!(converted.materials[0].color.as_ref().unwrap().texture, 1);
        converted.textures.pop();
        converted.materials[0].color.as_mut().unwrap().texture = 0;
        assert_eq!(
            serde_json::to_value(converted)?,
            serde_json::to_value(&original)?
        );
        let mut ambiguous = original;
        ambiguous.materials[0].multiply = ambiguous.materials[0].color.clone();
        assert!(wing_color_slot(&mut ambiguous).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam and RESONANCE_TEST_LIBRARY; optionally writes only enemy215/235/239 effect models to RESONANCE_WING_EFFECT_OUTPUT"]
    fn installed_layered_wings_keep_source_layers_and_shared_palette_slot() -> Result<()> {
        let source = Source::open(
            Path::new(
                &std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let temporary = tempfile::tempdir()?;
        let destination = std::env::var_os("RESONANCE_WING_EFFECT_OUTPUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| temporary.path().to_owned());
        let image = pe::Image::parse(&source.executable)?;
        for (id, motion) in [(215, 60), (235, 61), (239, 50)] {
            let package = source.require(monsters::package(&image, id)?)?;
            let metadata = monsters::metadata(&package)?;
            ensure!(metadata[0x1f2] == 1, "wing attachment table changed");
            let row =
                preview_model::member(&package, 0x1d8)?.context("wing attachment table missing")?;
            ensure!(
                row[..2] == [0, motion] && row[3] == 0x70,
                "native layered wing record changed for enemy{id}"
            );
            let owner = format!("battle/enemies/{}/effects", monsters::symbol(id));
            let models = super::super::models(&source, &destination, &package, &owner)?;
            ensure!(
                models[&0].layers.len() == 1,
                "wing gained a source-absent shell"
            );
            let wing = &models[&0].layers[0].scene;
            ensure!(
                wing.textures.len() == 2
                    && wing.textures[0] == wing.textures[1]
                    && wing.materials.iter().all(|material| {
                        material
                            .color
                            .as_ref()
                            .is_some_and(|binding| binding.texture == 1)
                            && material.multiply.is_none()
                    })
                    && wing
                        .clips
                        .iter()
                        .any(|clip| clip.resource_slot == u16::from(motion)),
                "wing palette or authored motion was lost"
            );
            let path = format!("{owner}/effects.json");
            let mut document: serde_json::Value =
                serde_json::from_slice(&std::fs::read(library.join(&path))?)?;
            document["artwork"]["models"] = serde_json::to_value(&models)?;
            output::json(&destination.join(&path), &document)?;
            eprintln!(
                "enemy{id}: source-only effect layers, layered wing palette slot1, native motion{motion}"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam; compares accepted Bigfoot23/Gatekeeper245/Origin204/Mithos239 nonwing model recipes"]
    fn installed_nonwing_effect_recipes_keep_accepted_source_shells() -> Result<()> {
        let source = Source::open(
            Path::new(
                &std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let actual = tempfile::tempdir()?;
        let expected = tempfile::tempdir()?;
        let image = pe::Image::parse(&source.executable)?;
        let mut checked = 0;
        for id in [23, 245, 204, 239] {
            let package = source.require(monsters::package(&image, id)?)?;
            let owner = format!("battle/enemies/{}/effects", monsters::symbol(id));
            let converted = super::super::models(&source, actual.path(), &package, &owner)?;
            for (&slot, model) in &converted {
                if id == 239 && slot == 0 {
                    continue;
                }
                let body = preview_model::reference(&package, 0x180 + usize::from(slot) * 4)?
                    .context("effect body missing")?;
                let outline = preview_model::reference(&package, 0x198 + usize::from(slot) * 4)?
                    .unwrap_or_default();
                let mut motions =
                    preview_model::reference(&package, 0x1b0 + usize::from(slot) * 4)?
                        .map(|motion| vec![(0, motion)])
                        .unwrap_or_default();
                let metadata = monsters::metadata(&package)?;
                if metadata[0x1f2] != 0 {
                    let rows = preview_model::member(&package, 0x1d8)?
                        .context("attached models missing")?;
                    for row in rows[..usize::from(metadata[0x1f2]) * 40].chunks_exact(40) {
                        if row[0] == slot && row[1] != 0 {
                            let motion = u16::from(row[1]);
                            if let Some(name) =
                                preview_model::reference(&package, 0x20 + usize::from(motion) * 4)?
                                && !motions.iter().any(|(slot, _)| *slot == motion)
                            {
                                motions.push((motion, name));
                            }
                        }
                    }
                }
                let (mut prior, mut native) = preview_model::cook(
                    &source,
                    expected.path(),
                    [&body, &outline],
                    &format!("{owner}/models/model-{slot}"),
                    &motions,
                )?;
                normalize_model_color(
                    &source,
                    expected.path(),
                    &body,
                    &prior.parts[0].scene,
                    &mut native,
                )?;
                if outline.is_empty()
                    && super::super::embedded_outline::present_with_clips(
                        &native,
                        expected.path(),
                        &prior.parts[0].scene,
                    )?
                {
                    prior
                        .parts
                        .retain(|part| part.scene.outline_color.is_none());
                }
                for part in &mut prior.parts {
                    part.animation = None;
                    part.scene.autoplay = false;
                }
                ensure!(
                    serde_json::to_value(&model.layers)? == serde_json::to_value(&prior.parts)?,
                    "accepted nonwing recipe changed for enemy{id} slot{slot}"
                );
                for part in &model.layers {
                    ensure!(
                        std::fs::read(actual.path().join(&part.scene.mesh))?
                            == std::fs::read(expected.path().join(&part.scene.mesh))?,
                        "accepted nonwing geometry/normal/RGB changed"
                    );
                }
                checked += 1;
            }
        }
        ensure!(checked > 0, "representative effect models are absent");
        eprintln!("Preserved {checked} accepted nonwing model recipes");
        Ok(())
    }
}

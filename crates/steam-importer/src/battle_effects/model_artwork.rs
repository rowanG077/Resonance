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
}

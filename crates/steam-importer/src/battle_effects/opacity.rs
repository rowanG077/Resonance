//! Scene sampling consumes an explicit opacity binding in the shared format.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    declaration::Declaration, document::Definition, visual::ParticleTexture,
};
use std::collections::BTreeSet;

pub(crate) fn bind(bank: &mut Definition) -> Result<()> {
    let mut slots = BTreeSet::new();
    for declaration in &bank.templates {
        if let Declaration::Particle { visual, .. } = declaration
            && visual.scene_sample.is_some()
        {
            slots.insert(
                visual
                    .texture
                    .slot()
                    .context("scene sampler has no opacity atlas")?,
            );
        }
    }
    if slots.is_empty() {
        return Ok(());
    }
    let artwork = bank
        .artwork
        .as_mut()
        .context("scene sampler has no artwork")?;
    for slot in slots {
        let textures = artwork
            .textures
            .get_mut(&slot)
            .context("scene sampler opacity atlas is unbound")?;
        ensure!(!textures.is_empty(), "scene sampler opacity atlas is empty");
        if textures.len() == 1 {
            // Packed RGBA pages already contain the native coverage in A.
            // The shared scene-sampling pass reads only mask.a, independently
            // of its scene color; sharing these bytes does not apply alpha twice.
            textures.push(textures[0].clone());
        }
        ensure!(
            textures.len() == 2,
            "scene sampler needs one color and one opacity binding"
        );
        for declaration in &mut bank.templates {
            if let Declaration::Particle { visual, .. } = declaration
                && visual.scene_sample.is_some()
                && let ParticleTexture::Atlas {
                    slot: candidate,
                    dual,
                    ..
                } = &mut visual.texture
                && *candidate == slot
            {
                *dual = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        battle_effect::{EffectTexture, document::Artwork},
        font::UiTexture,
    };
    use std::collections::BTreeMap;

    #[test]
    fn scene_sampler_retains_the_alpha_binding_without_changing_other_draws() -> Result<()> {
        let sampler: Declaration = serde_json::from_value(serde_json::json!({
            "kind":"particle", "template":{"state":{"geometry":{"Size":{"value":[1.,1.,1.]}},"colors":[[128,128,128,255],[0,0,0,0]],"uv":[1,1,2,2]}},
            "visual":{"geometry":{"kind":"quad"},"orientation":"world","scene_sample":[12,12],"texture":{"kind":"atlas","slot":"bank-0","palette_set":1},"blend":"alpha"}
        }))?;
        let mut ordinary = sampler.clone();
        if let Declaration::Particle { visual, .. } = &mut ordinary {
            visual.scene_sample = None;
        }
        let texture = EffectTexture {
            images: vec![UiTexture {
                path: "mask.ktx2".into(),
                width: 4,
                height: 4,
            }],
            sampler: Default::default(),
            palette_sets: vec![vec![None; 256], vec![Some(0); 256]],
        };
        let mut bank = Definition {
            programs: Vec::new(),
            template_names: vec!["sampler".into(), "ordinary".into()],
            templates: vec![sampler, ordinary.clone()],
            artwork: Some(Artwork {
                textures: BTreeMap::from([(0, vec![texture.clone()]), (6, vec![texture.clone()])]),
                models: Default::default(),
            }),
        };
        let uv = bank.templates[0].template()?.state.uv;
        bind(&mut bank)?;
        bind(&mut bank)?;
        assert!(bank.templates[0].particle_visual()?.texture.dual());
        assert_eq!(bank.templates[0].template()?.state.uv, uv);
        assert_eq!(bank.templates[1], ordinary);
        let art = bank.artwork.context("missing artwork")?;
        assert_eq!(art.textures[&0].len(), 2);
        assert_eq!(art.textures[&0][0], texture);
        assert_eq!(art.textures[&0][1], texture);
        assert_eq!(art.textures[&6].len(), 1);
        Ok(())
    }
}

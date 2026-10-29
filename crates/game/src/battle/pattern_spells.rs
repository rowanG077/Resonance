//! Bind fixed spatial patterns to the existing native released-spell lifecycle.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, CONTACT_INITIALIZATION_TICKS, TRANSITION_TICKS},
};
use anyhow::Result;
use resonance_battle::{
    PreparedSpell, SpellEffect, SpellEffectPlacement, SpellLaunch, SpellOrigin, SpellProjectile,
};
use resonance_content::{
    battle_scene::{PatternOrigin, Scene, SpellRecipe},
    prepared::Files,
};

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    recipe: &SpellRecipe,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    recipe.validate(scene)?;
    let SpellRecipe::FixedPattern(pattern) = recipe;
    let recoil = recoil::Parameters::load(files)?;
    let mut spell = stored_spells::scene_spell(
        scene,
        match pattern.origin {
            PatternOrigin::SceneGround { toward_caster } => {
                SpellOrigin::SceneGround { toward_caster }
            }
            PatternOrigin::SceneFixed { position } => SpellOrigin::SceneFixed { position },
        },
        pattern.lifetime,
    )?;
    spell.effects.push(SpellEffect {
        at: TRANSITION_TICKS + 1,
        appearance: resources.effect_member(bank, pattern.startup_member)?,
        placement: SpellEffectPlacement::Scene([0.; 3]),
        deferred: pattern.startup_deferred,
        captured_heading_offset: Some(pattern.heading_offset),
    });
    let (sin, cos) = pattern.heading_offset.to_radians().sin_cos();
    for contact in &pattern.contacts {
        let source = &scene.projectiles[usize::from(contact.projectile)];
        let rule = source.hit_variant(usize::from(contact.hit_variant))?;
        let definition = resources.scene_projectile(
            files,
            &source.projectile,
            rule,
            &recoil,
            bank,
            &[(0, bank), (2, bank), (6, bank)],
        )?;
        let [x, y, z] = contact.offset;
        let offset = [x * cos + z * sin, y, z * cos - x * sin];
        let creation_tick = TRANSITION_TICKS + ACTIVE_START_TICKS + contact.tick;
        spell.projectiles.push(SpellProjectile {
            at: creation_tick + CONTACT_INITIALIZATION_TICKS,
            definition,
            origin_offset: offset,
            launch: SpellLaunch::Captured(pattern.heading_offset),
        });
        spell.effects.push(SpellEffect {
            at: creation_tick,
            appearance: resources.effect_member(bank, contact.effect_member)?,
            placement: SpellEffectPlacement::Scene(offset),
            deferred: true,
            captured_heading_offset: Some(pattern.heading_offset),
        });
    }
    Ok(spell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use resonance_battle::ResourceId;
    use std::{path::PathBuf, sync::Arc};

    #[test]
    #[ignore = "requires a full current cook at RESONANCE_TEST_ASSETS"]
    fn prepared_pattern_uses_changed_content_and_rejects_missing_dependencies() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
        );
        let library = resonance_content::library::Library::open(&root, &[])?;
        let mut snapshot = library.snapshot();
        // Ordinary encounter preparation captures these shared contact rules
        // before selecting any stored scenes.
        for path in [
            resonance_content::battle_recoil::PATH,
            resonance_content::battle_model::document::PARTY,
        ] {
            snapshot.read(path, resonance_content::preparation::JSON_LIMIT)?;
        }
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let (mut scene, source_bank) = resonance_content::battle_scene::document::capture(
            &mut snapshot,
            resonance_content::battle_scene::document::Kind::Technique,
            252,
            &diagnostics,
            &|| false,
        )?;
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        let bank = EffectResource {
            resource: ResourceId(1),
            members: Vec::new(),
            models: source_bank
                .art
                .iter()
                .flat_map(|art| &art.models)
                .map(|(&slot, part)| {
                    Ok((
                        slot,
                        super::super::model::effect(
                            &files,
                            part,
                            ResourceId(100 + u32::from(slot)),
                            false,
                        )?,
                    ))
                })
                .collect::<Result<_>>()?,
            bank: Some(Arc::new(source_bank)),
        };
        let mut resources = ActionResources::default();
        let baseline = prepare(
            &files,
            &scene,
            scene.spell_recipe.as_ref().unwrap(),
            &bank,
            &mut resources,
        )?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        resources.prepare_effects(&mut |_| Ok(None), &diagnostics)?;
        assert!(!diagnostics.has_errors());
        let mut recipe = scene.spell_recipe.clone().unwrap();
        let SpellRecipe::FixedPattern(pattern) = &mut recipe;
        pattern.lifetime += 10;
        pattern.heading_offset = 90.;
        pattern.contacts[0].tick += 3;
        pattern.contacts[0].offset = [100., 0., 0.];
        pattern.contacts[0].effect_member = pattern.startup_member;
        let altered = prepare(
            &files,
            &scene,
            &recipe,
            &bank,
            &mut ActionResources::default(),
        )?;
        assert_eq!(altered.duration, baseline.duration + 10);
        assert_eq!(
            altered.camera.unwrap().duration,
            baseline.camera.unwrap().duration + 10
        );
        assert_eq!(altered.projectiles[0].at, baseline.projectiles[0].at + 3);
        assert!(altered.projectiles[0].origin_offset[0].abs() < 0.00001);
        assert_eq!(altered.projectiles[0].origin_offset[2], -100.);
        assert_eq!(altered.effects[1].appearance, altered.effects[0].appearance);
        assert_eq!(
            altered.effects[1].at + CONTACT_INITIALIZATION_TICKS,
            altered.projectiles[0].at
        );
        assert!(matches!(
            altered.projectiles[0].launch,
            SpellLaunch::Captured(90.)
        ));
        let SpellRecipe::FixedPattern(pattern) = &mut recipe;
        pattern.contacts[0].effect_member = u16::MAX;
        let mut invalid = ActionResources::default();
        prepare(&files, &scene, &recipe, &bank, &mut invalid)?;
        assert!(
            invalid
                .prepare_effects(&mut |_| Ok(None), &diagnostics)
                .is_err()
        );
        scene.projectiles.clear();
        assert!(
            prepare(
                &files,
                &scene,
                &recipe,
                &bank,
                &mut ActionResources::default()
            )
            .is_err()
        );
        Ok(())
    }
}

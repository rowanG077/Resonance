//! Fairy Circle shares ordinary recovery, projectile and emitter-path scheduling.
use super::{ActionResources, EffectResource, recoil, stored_spells};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    PreparedSpell, RecoveryEffectPlacement, RecoveryTargets, SpellBenefit, SpellOrigin,
    SpellProjectile, SpellRecovery, SpellStartup,
    conditions::{Condition, Cure},
};
use resonance_content::{battle_scene::Scene, prepared::Files};

pub const CATALOGUE: u16 = 247;
pub const SCENE: u16 = 279;

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    // Native419f7f/41a24c schedule allied recovery180 and area contact190.
    let at = |age| stored_spells::TRANSITION_TICKS + stored_spells::ACTIVE_START_TICKS + age;
    ensure!(
        scene.effect_paths.len() == 4,
        "Fairy Circle needs four emitter paths"
    );
    let startup = resources.effect_member(bank, 1)?;
    let pulse = SpellRecovery {
        at: at(180),
        duration: 1,
        benefit: SpellBenefit::Cure(Cure::Ailments),
        targets: RecoveryTargets::LivingAllies,
        effect: Some((resources.effect_member(bank, 2)?, Default::default())),
        effect_placement: RecoveryEffectPlacement::RecipientCenter,
        flash: None,
    };
    // Curing first also makes a living petrified ally eligible for recovery.
    // Separate existing benefits preserve the native one-visit ordering.
    let mut recovery = vec![
        pulse,
        SpellRecovery {
            benefit: SpellBenefit::Hp(75),
            effect: None,
            ..pulse
        },
    ];
    for condition in [
        Condition::AttackUp,
        Condition::DefenseUp,
        Condition::MagicAttackUp,
    ] {
        recovery.push(SpellRecovery {
            benefit: SpellBenefit::Stat {
                condition,
                percent: 10,
                duration: 1800,
            },
            effect: None,
            ..pulse
        });
    }
    let source = scene
        .projectiles
        .get(1)
        .context("Fairy Circle has no area projectile")?;
    let projectile = resources.scene_projectile(
        files,
        &source.projectile,
        source.hit_variant(0)?,
        &recoil::Parameters::load(files)?,
        bank,
        &[(0, bank), (2, bank), (6, bank)],
    )?;
    Ok(PreparedSpell {
        recovery,
        projectiles: vec![SpellProjectile::new(
            at(190) + stored_spells::CONTACT_INITIALIZATION_TICKS,
            projectile,
        )],
        startup: Some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::DeferredEffect(startup),
        )),
        ..stored_spells::scene_spell(
            scene,
            SpellOrigin::SceneFixed { position: [0.; 3] },
            scene.duration,
        )?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires cooked Fairy Circle at RESONANCE_TEST_ASSETS"]
    fn fairy_circle_prepares_healing_attack_and_four_moving_emitters() -> Result<()> {
        let (_, files, bank) = super::super::pattern_spells::tests::cooked_scene(SCENE)?;
        let mut resources = ActionResources::default();
        let spell = stored_spells::prepare(&files, CATALOGUE, &bank, &bank, &bank, &mut resources)?;
        assert_eq!(spell.effect_paths.len(), 4);
        assert!(
            spell
                .effect_paths
                .iter()
                .all(|path| path.at == 61 && path.positions.len() == 361)
        );
        assert_eq!(spell.recovery.len(), 5);
        assert!(
            spell
                .recovery
                .iter()
                .all(|pulse| pulse.at == 242 && pulse.targets == RecoveryTargets::LivingAllies)
        );
        assert!(matches!(
            spell.recovery[0].benefit,
            SpellBenefit::Cure(Cure::Ailments)
        ));
        assert!(matches!(spell.recovery[1].benefit, SpellBenefit::Hp(75)));
        assert_eq!(spell.projectiles.len(), 1);
        assert_eq!(spell.projectiles[0].at, 253);
        resources.prepare_effects(
            &Default::default(),
            &mut |_| Ok(None),
            &resonance_content::diagnostics::Diagnostics::new(true),
        )?;
        Ok(())
    }
}

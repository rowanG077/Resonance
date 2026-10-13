//! Elemental seals temporarily enchant an ally's weapon.
use super::{ActionResources, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, Element, PreparedSpell, RecoveryTargets, SpellBenefit, SpellOrigin,
    SpellRecovery, conditions::Buff,
};
use resonance_content::{arte::Catalogue, battle_effect::Tints};

pub(super) fn native(catalogue: u16) -> Option<u16> {
    match catalogue {
        137 | 229..=231 => Some(276),
        138 | 232..=234 => Some(277),
        _ => None,
    }
}

pub(super) fn prepare(
    catalogue: u16,
    techniques: &Catalogue,
    bank: &EffectResource,
    tints: &Tints,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let index = techniques.definition(usize::from(catalogue))?.element;
    let element = index
        .checked_sub(1)
        .and_then(|index| Element::ALL.get(usize::from(index)))
        .copied()
        .context("elemental seal has no enchantment element")?;
    const ENCHANT_EFFECT: u16 = 47;
    let mut artwork = bank.clone();
    artwork.members = vec![ENCHANT_EFFECT];
    resources.effect(artwork)?;
    let effect = EffectAppearance {
        resource: bank.resource,
        member: ENCHANT_EFFECT,
    };
    let tint = tints
        .effect(usize::from(index))
        .context("missing elemental seal colors")?;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        projectiles: vec![],
        effects: vec![],
        recovery: vec![SpellRecovery {
            at: 0,
            benefit: SpellBenefit::Buff(Buff::Quartz(element)),
            targets: RecoveryTargets::Target,
            effect: Some((effect, tint)),
        }],
        origin: SpellOrigin::Caster,
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration: 90,
        startup: None,
        sound: None,
    })
}

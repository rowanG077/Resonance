//! First Aid restores one living ally and attaches its artwork to the recipient.
use super::{ActionResources, EffectResource};
use anyhow::Result;
use resonance_battle::{PreparedSpell, SpellOrigin, SpellRecovery};

pub const CATALOGUE: u16 = 98;
pub const KNIGHT_CATALOGUE: u16 = 221;

pub fn prepare(bank: &EffectResource, resources: &mut ActionResources) -> Result<PreparedSpell> {
    let effect = resources.effect_member(bank, 20)?;
    Ok(PreparedSpell {
        recovery: vec![SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: 11,
            benefit: resonance_battle::SpellBenefit::Hp(30),
            targets: resonance_battle::RecoveryTargets::Target,
            effect: Some((effect, Default::default())),
        }],
        ..PreparedSpell::new(SpellOrigin::Target, 90)
    })
}

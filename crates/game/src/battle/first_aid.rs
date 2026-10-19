//! First Aid restores one living ally and attaches its artwork to the recipient.
use super::{ActionResources, EffectResource};
use anyhow::Result;
use resonance_battle::{EffectAppearance, PreparedSpell, SpellOrigin, SpellRecovery};

pub const CATALOGUE: u16 = 98;
pub const KNIGHT_CATALOGUE: u16 = 221;

pub fn prepare(bank: &EffectResource, resources: &mut ActionResources) -> Result<PreparedSpell> {
    let effect = EffectAppearance {
        resource: bank.resource,
        member: 20,
    };
    let mut artwork = bank.clone();
    artwork.members = vec![effect.member];
    resources.effect(artwork)?;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles: Vec::new(),
        recovery: vec![SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: 11,
            benefit: resonance_battle::SpellBenefit::Hp(30),
            targets: resonance_battle::RecoveryTargets::Target,
            effect: Some((effect, Default::default())),
        }],
        origin: SpellOrigin::Target,
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration: 90,
        startup: None,
        sound: None,
    })
}

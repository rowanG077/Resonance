//! Resident support spells share recipient selection and the released spell clock.
use super::{ActionResources, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, PreparedSpell, RecoveryTargets, SpellBenefit, SpellOrigin, SpellRecovery,
    SpellStartup, conditions::Condition,
};
use resonance_content::battle_projectile::Table;

pub(super) fn native(catalogue: u16) -> Option<u16> {
    match catalogue {
        101 => Some(239),
        104 => Some(242),
        107 => Some(245),
        110 => Some(248),
        116 => Some(254),
        142 => Some(295),
        192 => Some(273),
        193 => Some(274),
        194 => Some(275),
        _ => None,
    }
}

pub fn prepare(
    catalogue: u16,
    parameters: &Table,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let spell = native(catalogue).context("unknown support spell")?;
    let duration = if catalogue == 142 {
        1
    } else {
        *parameters
            .support_durations
            .get(&spell)
            .context("missing support spell duration")?
    };
    let buff = |condition| SpellBenefit::Stat {
        condition,
        percent: 10,
        duration: 1800,
    };
    let distance_healing = |percent| SpellBenefit::HpByDistance {
        percent,
        distance_step: 200,
        penalty_per_step: 5,
        maximum_penalty: 25,
    };
    let (member, at, benefit) = match catalogue {
        101 => (
            Some(36),
            0,
            SpellBenefit::Cure(resonance_battle::conditions::Cure::Physical),
        ),
        104 => (
            Some(42),
            0,
            SpellBenefit::Cure(resonance_battle::conditions::Cure::AntiMagic),
        ),
        107 => (Some(41), 0, buff(Condition::AttackUp)),
        110 => (Some(22), 0, buff(Condition::DefenseUp)),
        116 => (Some(35), 141, SpellBenefit::Tp(15)),
        142 => (None, 1, SpellBenefit::Resurrect(0)),
        192 => (Some(20), 11, distance_healing(25)),
        193 => (Some(20), 11, SpellBenefit::Hp(35)),
        194 => (Some(20), 11, distance_healing(45)),
        _ => unreachable!(),
    };
    let effect = member.map(|member| EffectAppearance {
        resource: bank.resource,
        member,
    });
    if let Some(member) = member {
        let mut artwork = bank.clone();
        artwork.members = vec![member];
        resources.effect(artwork)?;
    }
    let delayed = at != 0;
    let startup_effect = catalogue == 116;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles: Vec::new(),
        recovery: vec![SpellRecovery {
            at,
            benefit,
            targets: RecoveryTargets::Target,
            effect: effect
                .filter(|_| !startup_effect)
                .map(|effect| (effect, Default::default())),
        }],
        // Immediate pulses select the recipient directly and need no retained point.
        origin: if delayed {
            SpellOrigin::Target
        } else {
            SpellOrigin::Caster
        },
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration,
        startup: effect
            .filter(|_| startup_effect)
            .map(|effect| (1, SpellStartup::Effect(effect))),
        sound: None,
    })
}

//! Bind an asset-authored spell release to this encounter's prepared effects,
//! projectiles and sounds. The simulation receives the same primitives as an
//! original spell and never learns which kind it is running.
use super::{ActionResources, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{
    PreparedSpell, RecoveryTargets, SpellBenefit, SpellEffect, SpellEffectPlacement, SpellLaunch,
    SpellOrigin, SpellProjectile, SpellRecovery, SpellStartup, conditions::Cure,
};
use resonance_content::{
    battle_projectile,
    battle_spell::{
        AuthoredSpell, Benefit, Launch, Origin, ShotProjectile, Targets, VisualPlacement,
    },
    items::BattleCure,
    prepared::Files,
};

pub(crate) struct Banks<'a> {
    pub common: &'a EffectResource,
    pub techniques: &'a EffectResource,
    /// The spell's own bank when it declares one, else the technique bank.
    pub spell: &'a EffectResource,
}

pub(crate) fn prepare(
    spell: &AuthoredSpell,
    files: &Files,
    projectiles: &battle_projectile::Table,
    recoil: &super::recoil::Parameters,
    banks: Banks<'_>,
    resources: &mut ActionResources,
    sound: Option<resonance_battle::Sound>,
) -> Result<PreparedSpell> {
    let mut effect = |member: u16| resources.effect_member(banks.spell, member);
    let startup = spell
        .startup
        .map(|member| Ok::<_, anyhow::Error>((0, SpellStartup::Effect(effect(member)?))))
        .transpose()?;
    let effects = spell
        .visuals
        .iter()
        .map(|visual| {
            Ok(SpellEffect {
                at: visual.at,
                appearance: effect(visual.effect)?,
                placement: match visual.placement {
                    VisualPlacement::Origin { offset } => SpellEffectPlacement::Scene(offset),
                    VisualPlacement::CasterGround => SpellEffectPlacement::CasterGround,
                    VisualPlacement::CasterCenter => SpellEffectPlacement::CasterCenter,
                    VisualPlacement::EnemyCenters => SpellEffectPlacement::EnemyCenters,
                },
                deferred: false,
                captured_heading_offset: Some(0.),
            })
        })
        .collect::<Result<_>>()?;
    let recovery = spell
        .pulses
        .iter()
        .map(|pulse| {
            Ok(SpellRecovery {
                at: pulse.at,
                duration: pulse.duration,
                benefit: match pulse.benefit {
                    Benefit::Hp { percent } => SpellBenefit::Hp(percent),
                    Benefit::Tp { amount } => SpellBenefit::Tp(amount),
                    Benefit::Cure { ailments } => SpellBenefit::Cure(match ailments {
                        BattleCure::Physical => Cure::Physical,
                        BattleCure::Magical => Cure::AntiMagic,
                        BattleCure::All => Cure::All,
                    }),
                    Benefit::Resurrect { percent } => SpellBenefit::Resurrect(percent),
                    Benefit::Haste { percent, duration } => {
                        SpellBenefit::Haste { percent, duration }
                    }
                    Benefit::Stat {
                        condition,
                        percent,
                        duration,
                    } => SpellBenefit::Stat {
                        condition,
                        percent,
                        duration,
                    },
                },
                targets: match pulse.targets {
                    Targets::Target => RecoveryTargets::Target,
                    Targets::Caster => RecoveryTargets::Caster,
                    Targets::LivingAllies => RecoveryTargets::LivingAllies,
                    Targets::LivingOpponents => RecoveryTargets::LivingOpponents,
                    Targets::Area { radius } => RecoveryTargets::Area {
                        radius,
                        inclusive: true,
                    },
                },
                effect: pulse
                    .effect
                    .map(|member| Ok::<_, anyhow::Error>((effect(member)?, Default::default())))
                    .transpose()?,
                effect_placement: Default::default(),
                flash: None,
            })
        })
        .collect::<Result<_>>()?;
    let shots = spell
        .projectiles
        .iter()
        .enumerate()
        .map(|(index, shot)| {
            let row = match &shot.projectile {
                ShotProjectile::Record(member) => projectiles
                    .records
                    .get(usize::from(*member))
                    .with_context(|| format!("unknown shared projectile record {member}"))?,
                ShotProjectile::Definition(definition) => definition.as_ref(),
            };
            let hit = super::hit::authored(&shot.hit, recoil)?;
            let mut definition = resources
                .projectile(
                    files,
                    row,
                    hit,
                    &[(0, banks.common), (1, banks.techniques), (2, banks.spell)],
                )
                .with_context(|| format!("spell projectile {index}"))?;
            {
                let definition = std::sync::Arc::make_mut(&mut definition);
                if let Some(contact) = definition.contact.as_mut() {
                    contact.cooldown = shot.hit.cooldown;
                }
                if let Some(velocity) = shot.velocity {
                    definition.velocity = velocity;
                }
            }
            let mut projectile = SpellProjectile::new(shot.at, definition);
            projectile.origin_offset = shot.offset;
            projectile.launch = match shot.launch {
                Launch::Live => SpellLaunch::Live,
                Launch::CasterAbove { scale, offset } => SpellLaunch::CasterAbove {
                    center_height_scale: scale,
                    height_offset: offset,
                },
                Launch::AtHeight { height } => SpellLaunch::AtHeight(height),
            };
            Ok(projectile)
        })
        .collect::<Result<_>>()?;
    Ok(PreparedSpell {
        projectiles: shots,
        effects,
        recovery,
        startup,
        sound,
        ..PreparedSpell::new(
            match spell.origin {
                Origin::Caster => SpellOrigin::Caster,
                Origin::Target => SpellOrigin::Target,
                Origin::TargetGround => SpellOrigin::TargetGround,
                Origin::Ground { toward_caster } => SpellOrigin::GroundAtRelease { toward_caster },
            },
            spell.duration,
        )
    })
}

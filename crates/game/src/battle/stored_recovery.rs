//! Stored healing and status effects share released-spell timing and recipients.
use super::{ActionResources, EffectResource, stored_spells};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, PreparedSpell, RecoveryEffectPlacement, RecoveryTargets, SpellBenefit,
    SpellOrigin, SpellRecovery, SpellStartup,
    conditions::{Buff, Condition, Cure},
};
use resonance_content::battle_scene::Scene;

pub const HEAL: u16 = 100;
pub const CURE: u16 = 119;
pub const ACID_RAIN: u16 = 226;

pub(super) fn prepare(
    catalogue: u16,
    scene: &Scene,
    bank: &EffectResource,
    techniques: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    if catalogue == ACID_RAIN {
        return acid_rain(scene, bank, resources);
    }
    use RecoveryTargets::{Area, LivingAllies, Target};
    let stat = |condition, percent| SpellBenefit::Stat {
        condition,
        percent,
        duration: 1800,
    };
    let (active_ticks, age, benefit, targets, secondary_member) = match catalogue {
        HEAL => (165, 0, SpellBenefit::Hp(60), Target, None),
        CURE => (165, 0, SpellBenefit::Hp(100), Target, None),
        102 => (
            180,
            21,
            SpellBenefit::Cure(Cure::Physical),
            Area {
                radius: scene.recovery_radius.context("missing Purify radius")?,
                inclusive: true,
            },
            None,
        ),
        103 => (195, 0, SpellBenefit::Cure(Cure::Physical), Target, None),
        105 => (
            195,
            31,
            SpellBenefit::Cure(Cure::AntiMagic),
            LivingAllies,
            None,
        ),
        106 => (195, 0, SpellBenefit::Cure(Cure::AntiMagic), Target, None),
        108 => (195, 31, stat(Condition::AttackUp, 10), LivingAllies, None),
        109 => (165, 0, stat(Condition::AttackUp, 15), Target, None),
        111 => (195, 31, stat(Condition::DefenseUp, 10), LivingAllies, None),
        112 => (195, 0, stat(Condition::DefenseUp, 15), Target, None),
        117 | 118 => (
            215,
            31,
            SpellBenefit::Hp(if catalogue == 117 { 9 } else { 10 }),
            Area {
                radius: scene
                    .recovery_radius
                    .context("missing healing area radius")?,
                inclusive: true,
            },
            None,
        ),
        120 => (
            210,
            31,
            SpellBenefit::Hp(15),
            Area {
                radius: scene
                    .recovery_radius
                    .context("missing Healing Circle radius")?,
                inclusive: true,
            },
            None,
        ),
        121 => (225, 91, SpellBenefit::Resurrect(30), Target, None),
        122 => (235, 81, SpellBenefit::Hp(70), LivingAllies, Some(2)),
        123 => (225, 0, SpellBenefit::Buff(Buff::Revive), Target, None),
        _ => anyhow::bail!("unknown stored recovery spell {catalogue}"),
    };
    let effect = EffectAppearance {
        resource: bank.resource,
        member: 1,
    };
    let mut artwork = bank.clone();
    artwork.members = vec![effect.member];
    if let Some(member) = secondary_member {
        artwork.members.push(member);
    }
    resources.effect(artwork)?;
    let recipient_member = match catalogue {
        102 => Some(37),
        105 => Some(42),
        108 => Some(41),
        111 => Some(22),
        _ => None,
    };
    let recipient_effect = if let Some(member) = recipient_member {
        Some(resources.effect_member(techniques, member)?)
    } else {
        secondary_member
            .map(|member| EffectAppearance { member, ..effect })
            .or_else(|| (age == 0).then_some(effect))
    };
    let mut recovery = vec![SpellRecovery {
        duration: if catalogue == 102 { 141 } else { 1 },
        effect_placement: match catalogue {
            102 | 103 | 105 | 108 | 109 | 111 | 112 | 123 => {
                RecoveryEffectPlacement::RecipientFacingCenter
            }
            106 => RecoveryEffectPlacement::RecipientFacingCenterUnscaled,
            _ => Default::default(),
        },
        flash: None,
        at: stored_spells::TRANSITION_TICKS + 1 + age,
        benefit,
        targets,
        effect: recipient_effect.map(|effect| (effect, Default::default())),
    }];
    let immunity = match catalogue {
        103 => Some(Buff::PhysicalBarrier { persistent: false }),
        106 => Some(Buff::MagicalBarrier { persistent: false }),
        _ => None,
    };
    if let Some(buff) = immunity {
        recovery.push(SpellRecovery {
            benefit: SpellBenefit::Buff(buff),
            effect: None,
            ..recovery[0]
        });
    }
    let repeats: &[u16] = match catalogue {
        117 => &[25, 50, 75, 100],
        118 => &[35, 70],
        120 => &[30, 60, 90],
        _ => &[],
    };
    if !repeats.is_empty() {
        let pulse = recovery[0];
        recovery.extend(repeats.iter().map(|delay| SpellRecovery {
            at: pulse.at + delay,
            ..pulse
        }));
    }
    Ok(PreparedSpell {
        recovery,
        startup: (age != 0).then_some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::DeferredEffect(effect),
        )),
        ..stored_spells::scene_spell(
            scene, // Ground circles and their upper layers must share the same floor origin.
            if matches!(catalogue, 117 | 118 | 120 | 121) {
                SpellOrigin::TargetGround
            } else {
                SpellOrigin::Target
            },
            active_ticks,
        )?
    })
}

fn acid_rain(
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    const ACTIVE_TICKS: u16 = 240;
    const APPLICATION_TICK: u16 = 120;
    const DEFENSE_AMOUNT: i16 = 15;
    const ACTOR_TINT: [u8; 3] = [32; 3];
    let effect = resources.effect_member(bank, 1)?;
    Ok(PreparedSpell {
        recovery: vec![SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: stored_spells::TRANSITION_TICKS
                + stored_spells::ACTIVE_START_TICKS
                + APPLICATION_TICK,
            benefit: SpellBenefit::DefenseDown {
                amount: DEFENSE_AMOUNT,
                duration: 0,
                tint: ACTOR_TINT,
            },
            targets: RecoveryTargets::LivingOpponents,
            effect: None,
        }],
        startup: Some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::DeferredEffect(effect),
        )),
        ..stored_spells::scene_spell(
            scene,
            SpellOrigin::SceneFixed { position: [0.; 3] },
            ACTIVE_TICKS,
        )?
    })
}

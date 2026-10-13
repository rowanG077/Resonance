//! Stored healing and status effects share released-spell timing and recipients.
use super::{ActionResources, EffectResource, stored_spells};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, PreparedSpell, RecoveryTargets, SpellBenefit, SpellOrigin, SpellRecovery,
    SpellStartup,
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
                inclusive: false,
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
    let mut recovery = vec![SpellRecovery {
        at: stored_spells::TRANSITION_TICKS + 1 + age,
        benefit,
        targets,
        effect: secondary_member
            .map(|member| EffectAppearance { member, ..effect })
            .or_else(|| (age == 0).then_some(effect))
            .map(|effect| (effect, Default::default())),
    }];
    let immunity = match catalogue {
        103 => Some(Buff::PhysicalAilmentGuard { persistent: false }),
        106 => Some(Buff::MagicalAilmentGuard { persistent: false }),
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
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles: Vec::new(),
        recovery,
        origin: SpellOrigin::Target,
        transition_ticks: stored_spells::TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(
            Some(
                scene
                    .stage_color
                    .context("missing recovery scene lighting")?,
            ),
            active_ticks,
        )?,
        camera: stored_spells::scene_camera(
            Some(scene.camera.context("missing recovery scene camera")?),
            active_ticks,
        )?,
        duration: stored_spells::TRANSITION_TICKS
            + active_ticks
            + stored_spells::SCENE_BOUNDARY_TICKS,
        startup: (age != 0).then_some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::Effect(effect),
        )),
        sound: None,
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
    let effect = EffectAppearance {
        resource: bank.resource,
        member: 1,
    };
    let mut artwork = bank.clone();
    artwork.members = vec![effect.member];
    resources.effect(artwork)?;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles: vec![],
        recovery: vec![SpellRecovery {
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
        origin: SpellOrigin::SceneFixed { position: [0.; 3] },
        transition_ticks: stored_spells::TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, ACTIVE_TICKS)?,
        camera: stored_spells::scene_camera(scene.camera, ACTIVE_TICKS)?,
        duration: stored_spells::TRANSITION_TICKS
            + ACTIVE_TICKS
            + stored_spells::SCENE_BOUNDARY_TICKS,
        startup: Some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::Effect(effect),
        )),
        sound: None,
    })
}

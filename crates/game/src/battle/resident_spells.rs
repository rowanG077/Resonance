//! Small spells share the resident effect bank and release scheduler.
use super::{ActionResources, EffectResource, recoil};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, HitShape, PreparedSpell, ProjectileContact, ProjectileDefinition,
    SpellEffect, SpellEffectPlacement, SpellOrigin,
};
use resonance_content::{battle_projectile, prepared::Files};
use std::sync::Arc;

pub const AQUA_EDGE: u16 = 62;
pub const ICICLE: u16 = 82;
pub const WIND_BLADE: u16 = 70;
pub const STONE_BLAST: u16 = 74;
pub const LIGHTNING: u16 = 78;

pub(super) fn source_spell(catalogue: u16) -> Option<u16> {
    match catalogue {
        AQUA_EDGE => Some(200),
        ICICLE => Some(220),
        WIND_BLADE | 215 => Some(208),
        STONE_BLAST | 217 => Some(212),
        LIGHTNING | 219 => Some(216),
        _ => None,
    }
}

pub(super) fn rule(
    table: &battle_projectile::Table,
    spell: u16,
) -> Result<&battle_projectile::ResidentSpell> {
    table
        .resident_spells
        .get(&spell)
        .with_context(|| format!("missing resident spell {spell} damage rules"))
}

pub fn prepare(
    files: &Files,
    table: &battle_projectile::Table,
    catalogue: u16,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let spell = source_spell(catalogue).context("unknown resident spell")?;
    let rule = rule(table, spell)?;
    let recoil = recoil::Parameters::load(files)?;
    if spell == 200 {
        return aqua_edge(files, table, rule, bank, resources, &recoil);
    }
    if spell == 220 {
        return icicle(files, table, rule, bank, resources, &recoil);
    }
    let mut hit = super::hit::authored(&rule.hit, &recoil)?;
    hit.overlimit_pause = false;
    let (mut projectile, startup_member, births) = match spell {
        208 => {
            let mut template = table
                .records
                .get(1)
                .context("missing Wind Blade contact")?
                .clone();
            template.contact.radius = 50.;
            template.contact.height = 50.;
            template.contact.recoil_direction = resonance_battle::RecoilDirection::AwayFromContact;
            (
                resources.projectile(files, &template, hit, &[])?,
                Some(27),
                &[12, 22, 32][..],
            )
        }
        212 => (
            Arc::new(ProjectileDefinition {
                lifetime: Some(10),
                velocity: [0.; 3],
                acceleration: [0.; 3],
                offset: [0.; 3],
                clamp_ground: false,
                active: None,
                birth: None,
                motion: Default::default(),
                effects: Default::default(),
                contact: Some(ProjectileContact {
                    hit,
                    cooldown: rule.hit.cooldown,
                    repeat_limit: 0,
                    radius: 85.,
                    height: 250.,
                    shape: HitShape::Cylinder,
                    offset: [0.; 3],
                    radius_growth: 0.,
                    height_growth: 0.,
                    survives_contact: true,
                    clashes: false,
                }),
            }),
            Some(21),
            &[32, 37, 42][..],
        ),
        216 => (
            resources.projectile(
                files,
                table
                    .records
                    .get(4)
                    .context("missing Lightning projectile")?,
                hit,
                &[(1, bank)],
            )?,
            None,
            &[22][..],
        ),
        _ => unreachable!(),
    };
    if let Some(contact) = Arc::make_mut(&mut projectile).contact.as_mut() {
        contact.cooldown = rule.hit.cooldown;
    }
    projectile.validate()?;
    let startup = startup_member.map(|member| EffectAppearance {
        resource: bank.resource,
        member,
    });
    if let Some(startup) = startup {
        let mut effects = bank.clone();
        effects.members = vec![startup.member];
        resources.effect(effects)?;
    }
    let (origin, startup_tick, duration) = if spell == 208 {
        // The release visit precedes the resident callback's first age update.
        (
            SpellOrigin::Target,
            6,
            rule.duration
                .checked_add(1)
                .context("Wind Blade duration overflow")?,
        )
    } else if spell == 216 {
        (
            SpellOrigin::SceneGround { toward_caster: 1. },
            1,
            rule.duration,
        )
    } else {
        (
            SpellOrigin::GroundAtRelease { toward_caster: 1. },
            0,
            rule.duration,
        )
    };
    let (startup, effects) = match (spell, startup) {
        (212, Some(appearance)) => (
            None,
            vec![SpellEffect {
                at: 0,
                appearance,
                placement: SpellEffectPlacement::Scene([0.; 3]),
                deferred: true,
                captured_heading_offset: Some(0.),
            }],
        ),
        (_, startup) => (startup, vec![]),
    };
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects,
        recovery: vec![],
        projectiles: births
            .iter()
            .map(|&at| resonance_battle::SpellProjectile::new(at, Arc::clone(&projectile)))
            .collect(),
        origin,
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration,
        startup: startup.map(|effect| {
            let effect = if spell == 208 {
                resonance_battle::SpellStartup::DeferredEffect(effect)
            } else {
                resonance_battle::SpellStartup::Effect(effect)
            };
            (startup_tick, effect)
        }),
        sound: None,
    })
}

fn aqua_edge(
    files: &Files,
    table: &battle_projectile::Table,
    rule: &battle_projectile::ResidentSpell,
    bank: &EffectResource,
    resources: &mut ActionResources,
    recoil: &recoil::Parameters,
) -> Result<PreparedSpell> {
    let row = table
        .records
        .get(8)
        .context("missing Aqua Edge projectile")?;
    let hit = super::hit::authored(&rule.hit, recoil)?;
    let projectile = resources.projectile(files, row, hit, &[(1, bank)])?;
    let mut projectiles = Vec::with_capacity(3);
    for blade in 0..3 {
        let mut definition = (*projectile).clone();
        let angle = blade as f32 * std::f32::consts::TAU / 3.;
        definition.velocity = [angle.sin(), 0., angle.cos()];
        definition.motion.aqua_edge = true;
        // The final contact precedes same-update retirement by the spell owner.
        definition.lifetime = Some(119);
        definition.contact.as_mut().unwrap().cooldown = rule.hit.cooldown;
        definition.validate()?;
        // Allocation precedes the first visible projectile visit by one update.
        projectiles.push(resonance_battle::SpellProjectile::new(
            2,
            Arc::new(definition),
        ));
    }
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles,
        recovery: vec![],
        origin: SpellOrigin::CasterRoot,
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration: rule
            .duration
            .checked_add(1)
            .context("Aqua Edge duration overflow")?,
        startup: None,
        sound: None,
    })
}

fn icicle(
    files: &Files,
    table: &battle_projectile::Table,
    rule: &battle_projectile::ResidentSpell,
    bank: &EffectResource,
    resources: &mut ActionResources,
    recoil: &recoil::Parameters,
) -> Result<PreparedSpell> {
    let mut template = table
        .records
        .get(1)
        .context("missing Icicle contact")?
        .clone();
    template.contact.shape = HitShape::Cylinder;
    template.contact.radius = 90.;
    template.contact.height = 180.;
    template.contact.recoil_direction = resonance_battle::RecoilDirection::AwayFromOwner;
    let mut projectiles = Vec::with_capacity(2);
    for (at, rule) in [
        (3, &rule.hit),
        (
            27,
            rule.followup_hit
                .as_ref()
                .context("missing Icicle follow-up rule")?,
        ),
    ] {
        let hit = super::hit::authored(rule, recoil)?;
        let mut projectile = resources.projectile(files, &template, hit, &[])?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .unwrap()
            .cooldown = rule.cooldown;
        projectiles.push(resonance_battle::SpellProjectile::new(at + 1, projectile));
    }
    let appearance = EffectAppearance {
        resource: bank.resource,
        member: 32,
    };
    let mut effects = bank.clone();
    effects.members = vec![appearance.member];
    resources.effect(effects)?;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![SpellEffect {
            at: 0,
            appearance,
            placement: SpellEffectPlacement::Scene([0.; 3]),
            deferred: true,
            captured_heading_offset: Some(0.),
        }],
        projectiles,
        recovery: vec![],
        origin: SpellOrigin::GroundAtRelease { toward_caster: 1. },
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration: rule
            .duration
            .checked_add(1)
            .context("Icicle duration overflow")?,
        startup: None,
        sound: None,
    })
}

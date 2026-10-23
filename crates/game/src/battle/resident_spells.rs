//! Small spells share the resident effect bank and release scheduler.
use super::{ActionResources, EffectResource, recoil};
use anyhow::{Context, Result};
use resonance_battle::{
    HitShape, PreparedSpell, ProjectileContact, ProjectileDefinition, SpellEffect,
    SpellEffectPlacement, SpellOrigin, SpellProjectile, SpellStartup,
};
use resonance_content::{battle_projectile, prepared::Files};
use std::sync::Arc;

pub const AQUA_EDGE: u16 = 62;
pub const ICICLE: u16 = 82;
pub const WIND_BLADE: u16 = 70;
pub const STONE_BLAST: u16 = 74;
pub const LIGHTNING: u16 = 78;

#[derive(Clone, Copy)]
#[repr(u16)]
pub(super) enum Spell {
    AquaEdge = 200,
    WindBlade = 208,
    StoneBlast = 212,
    Lightning = 216,
    Icicle = 220,
}

pub(super) fn source_spell(catalogue: u16) -> Option<Spell> {
    match catalogue {
        AQUA_EDGE => Some(Spell::AquaEdge),
        ICICLE => Some(Spell::Icicle),
        WIND_BLADE | 215 => Some(Spell::WindBlade),
        STONE_BLAST | 217 => Some(Spell::StoneBlast),
        LIGHTNING | 219 => Some(Spell::Lightning),
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
    let rule = rule(table, spell as u16)?;
    let recoil = recoil::Parameters::load(files)?;
    let mut prepared = PreparedSpell::new(
        SpellOrigin::GroundAtRelease { toward_caster: 1. },
        rule.duration,
    );
    let hit = || -> Result<_> {
        let mut hit = super::hit::authored(&rule.hit, &recoil)?;
        hit.overlimit_pause = false;
        Ok(hit)
    };
    let (mut projectile, births) = match spell {
        Spell::AquaEdge => {
            prepared.projectiles = aqua_edge(files, table, rule, bank, resources, &recoil)?;
            prepared.origin = SpellOrigin::CasterRoot;
            prepared.duration = rule
                .duration
                .checked_add(1)
                .context("Aqua Edge duration overflow")?;
            return Ok(prepared);
        }
        Spell::Icicle => {
            prepared.projectiles = icicle(files, table, rule, resources, &recoil)?;
            prepared.effects.push(scene_effect(bank, resources, 32)?);
            prepared.duration = rule
                .duration
                .checked_add(1)
                .context("Icicle duration overflow")?;
            return Ok(prepared);
        }
        Spell::WindBlade => {
            let hit = hit()?;
            let mut template = table
                .records
                .get(1)
                .context("missing Wind Blade contact")?
                .clone();
            template.contact.radius = 50.;
            template.contact.height = 50.;
            template.contact.recoil_direction = resonance_battle::RecoilDirection::AwayFromContact;
            let projectile = resources.projectile(files, &template, hit, &[])?;
            prepared.origin = SpellOrigin::Target;
            (projectile, &[12, 22, 32][..])
        }
        Spell::StoneBlast => (
            Arc::new(ProjectileDefinition {
                lifetime: Some(10),
                contact: Some(ProjectileContact {
                    hit: hit()?,
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
                ..Default::default()
            }),
            &[32, 37, 42][..],
        ),
        Spell::Lightning => {
            let hit = hit()?;
            prepared.origin = SpellOrigin::SceneGround { toward_caster: 1. };
            (
                resources.projectile(
                    files,
                    table
                        .records
                        .get(4)
                        .context("missing Lightning projectile")?,
                    hit,
                    &[(1, bank)],
                )?,
                &[22][..],
            )
        }
    };
    if let Some(contact) = Arc::make_mut(&mut projectile).contact.as_mut() {
        contact.cooldown = rule.hit.cooldown;
    }
    projectile.validate()?;
    match spell {
        Spell::WindBlade => {
            prepared.startup = Some((
                6,
                SpellStartup::DeferredEffect(resources.effect_member(bank, 27)?),
            ));
            // The release visit precedes the resident callback's first age update.
            prepared.duration = rule
                .duration
                .checked_add(1)
                .context("Wind Blade duration overflow")?;
        }
        Spell::StoneBlast => prepared.effects.push(scene_effect(bank, resources, 21)?),
        _ => {}
    }
    prepared.projectiles = births
        .iter()
        .map(|&at| SpellProjectile::new(at, Arc::clone(&projectile)))
        .collect();
    Ok(prepared)
}

fn aqua_edge(
    files: &Files,
    table: &battle_projectile::Table,
    rule: &battle_projectile::ResidentSpell,
    bank: &EffectResource,
    resources: &mut ActionResources,
    recoil: &recoil::Parameters,
) -> Result<Vec<SpellProjectile>> {
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
        projectiles.push(SpellProjectile::new(2, Arc::new(definition)));
    }
    Ok(projectiles)
}

fn icicle(
    files: &Files,
    table: &battle_projectile::Table,
    rule: &battle_projectile::ResidentSpell,
    resources: &mut ActionResources,
    recoil: &recoil::Parameters,
) -> Result<Vec<SpellProjectile>> {
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
        projectiles.push(SpellProjectile::new(at + 1, projectile));
    }
    Ok(projectiles)
}

fn scene_effect(
    bank: &EffectResource,
    resources: &mut ActionResources,
    member: u16,
) -> Result<SpellEffect> {
    Ok(SpellEffect {
        at: 0,
        appearance: resources.effect_member(bank, member)?,
        placement: SpellEffectPlacement::Scene([0.; 3]),
        deferred: true,
        captured_heading_offset: Some(0.),
    })
}

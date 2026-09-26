//! Original parameter bindings for the opening party's maintained martial tasks.
use super::{ActionBinding, EffectResource, HitResource, HitSelection, ProjectileResource};
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::ActionPhase;
use resonance_content::{arte, battle_action, battle_projectile, prepared::Files};

pub fn binding(files: &Files, technique: u16, id: u16) -> Result<ActionBinding> {
    let module = match technique {
        1 => "battle::demon_fang",
        35 => "battle::ray_thrust",
        _ => bail!("martial technique {technique} has no maintained sequence"),
    };
    let catalogue: arte::Catalogue = files.json(arte::PATH)?;
    let definition = catalogue.definition(usize::from(technique))?;
    let table: battle_action::Table = files.json(battle_action::MARTIAL_PATH)?;
    let source = phase_zero(&table, technique)?;
    Ok(ActionBinding {
        id,
        phase: ActionPhase::Actor,
        module: module.into(),
        entry: "attack".into(),
        duration: source.phases[0].duration,
        tp_cost: u16::from(definition.tp_cost),
    })
}

/// Bind the single original projectile row and its selected caller hit rule.
/// Effect resources already identify a prepared techniques bank and model set.
pub fn projectile(
    files: &Files,
    technique: u16,
    bank: &EffectResource,
) -> Result<ProjectileResource> {
    ensure!(matches!(technique, 1 | 35), "unprepared martial projectile");
    let table: battle_action::Table = files.json(battle_action::MARTIAL_PATH)?;
    let source = phase_zero(&table, technique)?;
    let phase = &source.phases[0];
    let hit = source
        .hits
        .get(phase.indices[1] as usize)
        .context("missing martial projectile row")?;
    ensure!(
        hit.emission == -2
            && hit.attachment_count == 1
            && hit.projectile_modifier == 0
            && hit.emission_operands[1..] == [0; 3],
        "martial projectile row needs another emission controller"
    );
    let member = u16::from(hit.emission_operands[0]);
    let projectiles: battle_projectile::Table = files.json(battle_projectile::PATH)?;
    let row = projectiles
        .records
        .get(usize::from(member))
        .context("missing martial projectile template")?;
    let rule = source
        .hit_rules
        .get(phase.indices[0] as usize + usize::from(hit.rule))
        .context("missing martial projectile hit rule")?;
    let effect = |bank_index: u8, member: u8| -> Result<Option<EffectResource>> {
        if member == 0 {
            return Ok(None);
        }
        ensure!(
            bank_index == 1,
            "martial projectile uses another effect bank"
        );
        let mut resource = bank.clone();
        resource.members = vec![u16::from(member)];
        Ok(Some(resource))
    };
    Ok(ProjectileResource {
        source: battle_projectile::PATH.into(),
        member,
        hit: HitResource {
            source: battle_action::MARTIAL_PATH.into(),
            selection: HitSelection::Technique {
                member: technique,
                phase: 0,
            },
            rule: hit.rule,
        },
        birth: effect(row.birth_effect.bank, row.birth_effect.member)?,
        trail: effect(row.trail_effect.bank, row.trail_effect.member)?,
        ground: effect(row.ground_effect.bank, row.ground_effect.member)?,
        clash: None,
        impact: effect(rule.impact_bank, rule.impact_effect)?,
    })
}

fn phase_zero(table: &battle_action::Table, technique: u16) -> Result<&battle_action::Bundle> {
    let source = table
        .records
        .get(usize::from(technique))
        .and_then(Option::as_ref)
        .context("missing martial technique")?;
    ensure!(
        source.phases.len() == 4,
        "martial technique has no ordinary phase selection"
    );
    Ok(source)
}

pub fn projectile_for_path(
    files: &Files,
    path: &str,
    bank: &EffectResource,
) -> Result<ProjectileResource> {
    let technique = match path {
        "battle/projectiles/martial/1" => 1,
        "battle/projectiles/martial/35" => 35,
        _ => bail!("unknown maintained martial projectile {path}"),
    };
    projectile(files, technique, bank)
}

/// Character identity and original clip; the encounter supplies its model ID.
pub fn motion(path: &str) -> Option<(u8, u16)> {
    Some(match path {
        "battle/motions/lloyd/0" => (1, 0),
        "battle/motions/lloyd/43" => (1, 43),
        "battle/motions/colette/0" => (2, 0),
        "battle/motions/colette/43" => (2, 43),
        _ => return None,
    })
}

pub fn effect(path: &str, bank: &EffectResource) -> Result<EffectResource> {
    ensure!(
        path == "battle/effects/techniques/demon_fang",
        "unknown maintained martial effect {path}"
    );
    let mut bank = bank.clone();
    bank.members = vec![3];
    Ok(bank)
}

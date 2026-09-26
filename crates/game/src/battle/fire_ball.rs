//! Original parameters used by the maintained Genis casting and Fire Ball tasks.
use super::{ActionBinding, EffectResource, HitResource, HitSelection, ProjectileResource};
use anyhow::{Context, Result, ensure};
use resonance_battle::ActionPhase;
use resonance_content::{arte, battle_action, battle_projectile, prepared::Files};

pub fn bindings(files: &Files, casting: u16, resident: u16) -> Result<[ActionBinding; 2]> {
    let catalogue: arte::Catalogue = files.json(arte::PATH)?;
    let technique = catalogue.definition(66)?;
    let table: battle_action::Table = files.json(battle_action::SPELL_PATH)?;
    let source = table
        .records
        .get(4)
        .and_then(Option::as_ref)
        .context("missing Fire Ball action")?;
    ensure!(
        source.phases.len() == 4,
        "Fire Ball requires ordinary phase selection"
    );
    Ok([
        ActionBinding {
            id: casting,
            phase: ActionPhase::Casting,
            module: "battle::genis_fire_ball".into(),
            entry: "run".into(),
            duration: 0,
            tp_cost: u16::from(technique.tp_cost),
        },
        ActionBinding {
            id: resident,
            phase: ActionPhase::Resident,
            module: "battle::fire_ball".into(),
            entry: "release".into(),
            duration: source.phases[0].duration,
            tp_cost: 0,
        },
    ])
}

pub fn projectile(files: &Files, bank: &EffectResource) -> Result<ProjectileResource> {
    let table: battle_projectile::Table = files.json(battle_projectile::PATH)?;
    let row = table
        .records
        .get(3)
        .context("missing Fire Ball projectile")?;
    let effect =
        |source: &resonance_content::battle_projectile::Effect| -> Result<Option<EffectResource>> {
            if source.member == 0 {
                return Ok(None);
            }
            ensure!(
                source.bank == 1,
                "Fire Ball projectile uses another effect bank"
            );
            let mut resource = bank.clone();
            resource.members = vec![u16::from(source.member)];
            Ok(Some(resource))
        };
    Ok(ProjectileResource {
        source: battle_projectile::PATH.into(),
        member: 3,
        hit: HitResource {
            source: battle_action::SPELL_PATH.into(),
            selection: HitSelection::Technique {
                member: 4,
                phase: 0,
            },
            rule: 0,
        },
        birth: effect(&row.birth_effect)?,
        trail: effect(&row.trail_effect)?,
        ground: effect(&row.ground_effect)?,
        clash: None,
        impact: None,
    })
}

pub fn effect(bank: &EffectResource) -> EffectResource {
    let mut bank = bank.clone();
    bank.members = vec![23];
    bank
}

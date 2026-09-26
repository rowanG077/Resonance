//! Opening companion inputs from the verified strategy and technique tables.
use super::ActionBinding;
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActionPhase, ActorId, CompanionDefinition, CompanionTechnique};
use resonance_content::{arte, battle_profile, prepared::Files};
use resonance_events::party::Member;
use std::collections::BTreeMap;

pub fn binding(character: u8, id: u16) -> Result<ActionBinding> {
    let entry = match character {
        2 => "colette",
        3 => "genis",
        _ => anyhow::bail!("companion policy is not prepared for character {character}"),
    };
    Ok(ActionBinding {
        id,
        phase: ActionPhase::Decision,
        module: "battle::companion".into(),
        entry: entry.into(),
        duration: 0,
        tp_cost: 0,
    })
}

pub fn reevaluation_binding(character: u8, id: u16) -> Result<ActionBinding> {
    let entry = match character {
        1 => "lloyd",
        2 => "colette",
        3 => "genis",
        _ => anyhow::bail!("normal reevaluation is not prepared for character {character}"),
    };
    Ok(ActionBinding {
        id,
        phase: ActionPhase::Decision,
        module: "battle::normal_control".into(),
        entry: entry.into(),
        duration: 0,
        tp_cost: 0,
    })
}

pub fn prepare(
    files: &Files,
    character: u8,
    actor: ActorId,
    member: &Member,
    actions: &BTreeMap<u16, u16>,
    level_difference: i8,
) -> Result<CompanionDefinition> {
    ensure!(
        matches!(character, 2 | 3),
        "unsupported companion character"
    );
    let profiles: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    let defaults = profiles.default_strategy[usize::from(character - 1)];
    let strategy = std::array::from_fn(|i| {
        if member.strategy[i] == 0 {
            defaults[i]
        } else {
            member.strategy[i]
        }
    });
    ensure!(
        strategy == defaults,
        "opening companion policy requires the default strategy"
    );
    let catalogue: arte::Catalogue = files.json(arte::PATH)?;
    let mut techniques = Vec::new();
    for &id in catalogue.learned_by(character)? {
        let id = u16::from(id);
        if !member.techniques.contains(&id) {
            continue;
        }
        ensure!(
            id == if character == 2 { 35 } else { 66 },
            "opening companion policy does not implement learned technique {id}"
        );
        let definition = catalogue.definition(usize::from(id))?;
        let raw_range = (i32::from(definition.cast_time_adjustment) << 16)
            | i32::from(definition.recovery_ticks as u16);
        let (maximum, minimum) = if definition.flags & 2 != 0 {
            if raw_range < 1000 {
                (raw_range as f32, raw_range as f32 - 100.)
            } else {
                (8000., 500.)
            }
        } else {
            (raw_range as f32, 0.)
        };
        techniques.push(CompanionTechnique {
            action: *actions
                .get(&id)
                .context("missing learned companion action")?,
            enabled: !member.disabled_techniques.contains(&id),
            flags: definition.flags,
            cost: definition.tp_cost.into(),
            learning_route: definition.learning_route,
            minimum,
            maximum,
        });
    }
    let policy = usize::from(strategy[1]);
    Ok(CompanionDefinition {
        actor,
        strategy,
        saved_position: member.strategy[2],
        level: member.level,
        level_difference,
        tp_limit: profiles.companion_policy.tp_limits[policy],
        healing_limit: profiles.companion_policy.healing_limits[policy],
        support_level_limit: profiles.companion_policy.support_level_limits[policy],
        techniques,
    })
}

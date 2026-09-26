//! Bind maintained enemy source to original table rows and prepared pose anchors.
use super::{ActionBinding, MeleeResource, MeleeSelection};
use anyhow::{Context, Result, bail};
use resonance_battle::ActionPhase;
use resonance_content::{battle_model, prepared::Files};

pub const ZOMBIE_ENTRIES: [&str; 5] = [
    "right_attack",
    "push_attack",
    "double_attack",
    "triple_attack",
    "counter_attack",
];

/// Keep every declared row available. The ordinary controller owns admission,
/// weights and special follow-up conditions, including the zero-weight row.
pub fn zombie_bindings(files: &Files, ids: [u16; 5]) -> Result<Vec<ActionBinding>> {
    let source: battle_model::Enemy = files.json(&battle_model::enemy_path(36))?;
    ZOMBIE_ENTRIES
        .iter()
        .zip(ids)
        .enumerate()
        .map(|(index, (entry, id))| {
            let row = source
                .actions
                .rows
                .get(index)
                .context("missing Zombie action")?;
            Ok(ActionBinding {
                id,
                phase: ActionPhase::Actor,
                module: "battle::enemy_zombie".into(),
                entry: (*entry).into(),
                duration: row.duration,
                tp_cost: u16::from(row.tp),
            })
        })
        .collect()
}

pub fn zombie_melee(path: &str, anchor_groups: &[Vec<u16>]) -> Result<MeleeResource> {
    let (action, row) = match path {
        "battle/melee/enemies/036/0/0" => (0, 0),
        "battle/melee/enemies/036/1/0" => (1, 0),
        "battle/melee/enemies/036/2/0" => (2, 0),
        "battle/melee/enemies/036/2/1" => (2, 1),
        _ => bail!("unknown Zombie contact {path}"),
    };
    Ok(MeleeResource {
        impact: None,
        source: battle_model::enemy_path(36),
        selection: MeleeSelection::Enemy { action },
        row,
        anchor_groups: anchor_groups.to_vec(),
    })
}

pub const GHOST_ENTRIES: [&str; 2] = ["strike", "spit"];

pub fn ghost_bindings(files: &Files, ids: [u16; 2]) -> Result<Vec<ActionBinding>> {
    let source: battle_model::Enemy = files.json(&battle_model::enemy_path(49))?;
    GHOST_ENTRIES
        .iter()
        .zip(ids)
        .enumerate()
        .map(|(index, (entry, id))| {
            let row = source
                .actions
                .rows
                .get(index)
                .context("missing Ghost action")?;
            Ok(ActionBinding {
                id,
                phase: ActionPhase::Actor,
                module: "battle::enemy_ghost".into(),
                entry: (*entry).into(),
                duration: row.duration,
                tp_cost: u16::from(row.tp),
            })
        })
        .collect()
}

pub fn ghost_melee(path: &str, anchor_groups: &[Vec<u16>]) -> Result<MeleeResource> {
    anyhow::ensure!(
        path == "battle/melee/enemies/049/0/0",
        "unknown Ghost contact {path}"
    );
    Ok(MeleeResource {
        source: battle_model::enemy_path(49),
        selection: MeleeSelection::Enemy { action: 0 },
        row: 0,
        anchor_groups: anchor_groups.to_vec(),
        impact: None,
    })
}

/// Birth member 1 uses this enemy's prepared model bank; the common clash
/// member is selected by template flag 0x2000, independently of that bank.
pub fn ghost_projectile(
    path: &str,
    mut birth: super::EffectResource,
    mut clash: super::EffectResource,
) -> Result<super::ProjectileResource> {
    anyhow::ensure!(
        path == "battle/projectiles/enemies/049/0",
        "unknown Ghost projectile {path}"
    );
    anyhow::ensure!(
        birth.source == battle_model::enemy_effects_path(49),
        "Ghost birth uses its owner bank"
    );
    anyhow::ensure!(
        clash.source == resonance_content::battle_effect::COMMON_PATH,
        "Ghost clash uses the common bank"
    );
    birth.members = vec![1];
    clash.members = vec![11];
    Ok(super::ProjectileResource {
        source: battle_model::enemy_projectiles_path(49),
        member: 0,
        hit: super::HitResource {
            source: battle_model::enemy_path(49),
            selection: super::HitSelection::Enemy,
            rule: 1,
        },
        birth: Some(birth),
        clash: Some(clash),
        trail: None,
        ground: None,
        impact: None,
    })
}

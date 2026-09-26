//! Bind ordinary enemy decisions to verified source rows before activation.
use super::ActionBinding;
use anyhow::{Result, ensure};
use resonance_battle::{ActionPhase, ActorId, EnemyChoice, EnemyDecisionDefinition};
use resonance_content::{battle_model, prepared::Files};

pub fn bindings(monster: u16, initialize: u16, decide: u16) -> Result<[ActionBinding; 2]> {
    let module = match monster {
        36 => "battle::enemy_zombie_ai",
        49 => "battle::enemy_ghost_ai",
        _ => anyhow::bail!("enemy {monster} has no maintained ordinary decision program"),
    };
    Ok(
        [(initialize, "initialize"), (decide, "decide")].map(|(id, entry)| ActionBinding {
            id,
            phase: ActionPhase::Decision,
            module: module.into(),
            entry: entry.into(),
            duration: 0,
            tp_cost: 0,
        }),
    )
}

pub fn definition(
    files: &Files,
    monster: u16,
    actor: ActorId,
    action_ids: &[u16],
    difficulty: u8,
) -> Result<EnemyDecisionDefinition> {
    let monster = u8::try_from(monster)?;
    let source: battle_model::Enemy = files.json(&battle_model::enemy_path(monster))?;
    let policy = &source.actions.policy;
    ensure!(
        policy.native == 0
            && policy.counter_count == 0
            && policy.counter_chance == 0
            && policy.overlimit_first_action == 0,
        "enemy native/counter/overlimit decisions are not prepared"
    );
    ensure!(
        usize::from(policy.ordinary_count) == source.actions.rows.len()
            && action_ids.len() == source.actions.rows.len(),
        "enemy action/decision row count differs"
    );
    let choices = source
        .actions
        .rows
        .iter()
        .zip(action_ids)
        .map(|(row, &action)| {
            ensure!(
                row.movement_clip == 0
                    && row.movement_rate.finite()? == 0.
                    && row.movement_speed.finite()? == 0.,
                "enemy custom approach motion is not prepared"
            );
            ensure!(
                row.technique == 0 && row.required_story_flag == 0 && row.required_monster == 0,
                "enemy special eligibility is not prepared"
            );
            Ok(EnemyChoice {
                action,
                weight: row.weight,
                requirements: row.requirements,
                target_policy: row.target_policy,
                guard_chance: row.guard_chance as i8,
                combo_at: row.combo_at,
                followup_chance: row.followup_chance as i8,
                range: row.range,
                tp: u16::from(row.tp),
                approach_minimum: f32::from(row.approach_minimum),
                approach_range: f32::from(row.approach_range),
            })
        })
        .collect::<Result<_>>()?;
    Ok(EnemyDecisionDefinition {
        actor,
        strategy: source.target_strategy,
        difficulty,
        choices,
        back_row: policy
            .back_row_actions
            .iter()
            .zip(&policy.back_row_weights)
            .take(usize::from(policy.back_row_count))
            .map(|(&row, &weight)| (row, weight))
            .collect(),
        walk_speed: source.profile.walk_speed.finite()?,
        turn_ticks: source.profile.turn_ticks,
        body_flags: source.profile.body_flags,
    })
}

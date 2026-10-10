//! Bind prepared enemy choices to executable battle actions.
use anyhow::{Context, Result, ensure};
use resonance_battle::{EnemyChoice, EnemyDecisionDefinition};
use resonance_content::battle_enemy;

pub fn definition(
    source: &battle_enemy::Definition,
    action_ids: &[resonance_battle::ActionKey],
    difficulty: u8,
) -> Result<EnemyDecisionDefinition> {
    let policy = &source.actions.policy;
    if let Some(reason) = &policy.unsupported_reason {
        anyhow::bail!("{reason}");
    }
    ensure!(
        action_ids.len() == source.actions.rows.len(),
        "enemy action/decision row count differs"
    );
    let choices = source
        .actions
        .rows
        .iter()
        .zip(action_ids)
        .map(|(row, &action)| choice(row, action))
        .collect::<Result<_>>()?;
    Ok(EnemyDecisionDefinition {
        strategy: source
            .target_strategy
            .context("unsupported enemy target strategy")?,
        difficulty,
        choices,
        back_row: policy
            .back_row
            .iter()
            .map(|row| (row.action, row.weight))
            .collect(),
        walk_speed: source.profile.walk_speed,
        walk_motion: None,
        turn_ticks: source.profile.turn_ticks,
    })
}

fn choice(
    row: &resonance_content::battle_action::EnemyAction,
    action: resonance_battle::ActionKey,
) -> Result<EnemyChoice> {
    if let Some(reason) = &row.unsupported_reason {
        anyhow::bail!("{reason}");
    }
    Ok(EnemyChoice {
        action,
        weight: row.weight,
        requirements: row.requirements.clone(),
        target_policy: row.target_policy,
        return_to_formation: row.return_to_formation,
        guard_chance: row.guard_chance,
        range: row.range,
        tp: u16::from(row.tp),
        approach_minimum: f32::from(row.approach_minimum),
        approach_range: f32::from(row.approach_range),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_action::{EnemyAction, EnemyRequirements, TargetPolicy};

    #[test]
    fn prepared_choices_preserve_eligibility_and_reject_unsupported_actions() -> Result<()> {
        let mut row = EnemyAction {
            weight: 1,
            target_policy: Some(TargetPolicy::Flying),
            requirements: EnemyRequirements {
                difficulty: 1..=2,
                hp_percent: Some(50),
                priority: true,
            },
            return_to_formation: false,
            range: [0, 0],
            approach_range: 100,
            approach_minimum: 0,
            guard_chance: -1,
            tp: 0,
            attack: Some(resonance_content::battle_action::EnemyAttack::Tail),
            projectile: None,
            unsupported_reason: None,
        };
        let prepared = choice(&row, resonance_battle::ActionKey(0))?;
        assert_eq!(prepared.action, resonance_battle::ActionKey(0));
        assert_eq!(prepared.requirements, row.requirements);
        assert_eq!(prepared.target_policy, Some(TargetPolicy::Flying));
        assert!(!prepared.return_to_formation);
        assert_eq!(prepared.guard_chance, -1);
        row.unsupported_reason = Some("custom action controller".into());
        assert!(
            choice(&row, resonance_battle::ActionKey(0))
                .unwrap_err()
                .to_string()
                .contains("custom action controller")
        );
        Ok(())
    }
}

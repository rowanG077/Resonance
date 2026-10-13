//! Bind prepared enemy choices to executable battle actions.
use anyhow::{Context, Result, ensure};
use resonance_battle::{EnemyChoice, EnemyDecisionDefinition};
use resonance_content::battle_enemy;
use std::collections::BTreeSet;

pub fn disabled_by_flags(
    rows: &[resonance_content::battle_action::EnemyAction],
    flags: &BTreeSet<u16>,
) -> Result<Vec<u8>> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.required_flag.is_some_and(|flag| !flags.contains(&flag)))
        .map(|(index, _)| Ok(u8::try_from(index)?))
        .collect()
}

pub fn definition(
    enemy_id: u8,
    source: &battle_enemy::Definition,
    action_ids: &[resonance_battle::ActionKey],
    difficulty: u8,
    model: Option<&resonance_battle::ModelDefinition>,
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
        .map(|(row, &action)| {
            use resonance_content::battle_action::EnemyApproach;
            let approach = match row.approach {
                EnemyApproach::Walk => None,
                EnemyApproach::Run => Some((
                    super::model::MotionRole::Run.id(),
                    0.5,
                    source.profile.run_speed,
                )),
                EnemyApproach::Custom { clip, rate, speed } => Some((clip, rate, speed)),
            };
            choice(
                row,
                action,
                approach.map(
                    |(clip, motion_rate, speed)| resonance_battle::ApproachParameters {
                        minimum: f32::from(row.approach_minimum),
                        maximum: f32::from(row.approach_range),
                        motion: super::model::motion(model, clip),
                        motion_rate,
                        speed,
                        turn_ticks: source.profile.turn_ticks,
                    },
                ),
            )
        })
        .collect::<Result<_>>()?;
    Ok(EnemyDecisionDefinition {
        enemy_id,
        disabled_choices: Vec::new(),
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
        walk_override: policy.walk_override,
        behavior: policy
            .behavior
            .as_ref()
            .map(|source| behavior(source, model))
            .transpose()?
            .flatten(),
        walk_motion: None,
        turn_ticks: source.profile.turn_ticks,
    })
}

fn behavior(
    source: &resonance_content::battle_action::EnemyBehavior,
    model: Option<&resonance_battle::ModelDefinition>,
) -> Result<Option<resonance_battle::EnemyBehavior>> {
    use resonance_battle::EnemyBehavior;
    use resonance_content::battle_action::EnemyBehavior as Source;
    Ok(Some(match source {
        Source::IndependentModel { .. } => return Ok(None),
        Source::Retreat {
            hp_percent,
            after_ticks,
        } => EnemyBehavior::Retreat {
            hp_percent: *hp_percent,
            after_ticks: *after_ticks,
        },
        Source::HealthStage {
            hp_percent,
            initial_actions,
            final_actions,
        } => EnemyBehavior::HealthStage {
            hp_percent: *hp_percent,
            initial_actions: initial_actions.clone(),
            final_actions: final_actions.clone(),
        },
        Source::CarriedAnimationRate { slot, rate } => EnemyBehavior::CarriedAnimationRate {
            slot: *slot,
            rate: *rate,
        },
        Source::ThreeStations {
            stations,
            change_at,
        } => EnemyBehavior::ThreeStations {
            stations: *stations,
            change_at: u32::from(*change_at),
        },
        Source::Transformation {
            reveal_hp_percent,
            revealed_bones,
            airborne_hp_percent,
            hover_height,
            initial_actions,
            final_actions,
        } => {
            let model = model.context("enemy transformation requires a prepared body")?;
            EnemyBehavior::Transformation {
                reveal_hp_percent: *reveal_hp_percent,
                revealed_bones: revealed_bones
                    .iter()
                    .map(|name| {
                        let bone = model
                            .skeleton
                            .bones
                            .iter()
                            .position(|bone| bone.name == *name)
                            .with_context(|| format!("missing transformation bone {name}"))?;
                        Ok(u16::try_from(bone)?)
                    })
                    .collect::<Result<_>>()?,
                airborne_hp_percent: *airborne_hp_percent,
                hover_height: *hover_height,
                initial_actions: initial_actions.clone(),
                final_actions: final_actions.clone(),
            }
        }
        Source::ModelParts {
            bone_scales,
            hidden_weapons,
        } => {
            let model = model.context("enemy part overrides require a prepared body")?;
            ensure!(
                hidden_weapons
                    .iter()
                    .all(|slot| model.weapons.iter().any(|weapon| weapon.slot == *slot)),
                "enemy part override has an unprepared weapon slot"
            );
            EnemyBehavior::ModelParts {
                bone_scales: bone_scales
                    .iter()
                    .map(|(name, scale)| {
                        let bone = model
                            .skeleton
                            .bones
                            .iter()
                            .position(|bone| bone.name == *name)
                            .with_context(|| format!("missing enemy part bone {name}"))?;
                        Ok((u16::try_from(bone)?, [*scale; 3]))
                    })
                    .collect::<Result<_>>()?,
                hidden_weapons: hidden_weapons.clone(),
            }
        }
    }))
}

fn choice(
    row: &resonance_content::battle_action::EnemyAction,
    action: resonance_battle::ActionKey,
    approach: Option<resonance_battle::ApproachParameters>,
) -> Result<EnemyChoice> {
    if let Some(reason) = &row.unsupported_reason {
        anyhow::bail!("{reason}");
    }
    Ok(EnemyChoice {
        action,
        weight: row.weight,
        chain_group: row.chain_group,
        followup_chance: row.followup_chance,
        requirements: row.requirements.clone(),
        target_policy: row.target_policy,
        target_allies: row.target_allies,
        return_to_formation: row.return_to_formation,
        guard_chance: row.guard_chance,
        range: row.range,
        tp: u16::from(row.tp),
        approach_minimum: f32::from(row.approach_minimum),
        approach_range: f32::from(row.approach_range),
        approach,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_action::{EnemyAction, EnemyRequirements, TargetPolicy};

    #[test]
    fn prepared_choices_preserve_eligibility_and_reject_unsupported_actions() -> Result<()> {
        let mut row = EnemyAction {
            voices: Default::default(),
            weight: 1,
            required_flag: None,
            cast_ticks: None,
            chain_group: 1,
            followup_chance: 25,
            target_policy: Some(TargetPolicy::Flying),
            target_allies: false,
            requirements: EnemyRequirements {
                difficulty: 1..=2,
                hp_percent: Some(50),
                max_enemies: Some(1),
                priority: true,
                once_per_battle: true,
                ..Default::default()
            },
            return_to_formation: false,
            range: [0, 0],
            approach_range: 100,
            approach_minimum: 0,
            approach: Default::default(),
            guard_chance: -1,
            tp: 0,
            execution: None,
            unsupported_reason: None,
        };
        let prepared = choice(&row, resonance_battle::ActionKey(0), None)?;
        assert_eq!(prepared.action, resonance_battle::ActionKey(0));
        assert_eq!(prepared.requirements, row.requirements);
        assert_eq!((prepared.chain_group, prepared.followup_chance), (1, 25));
        assert_eq!(prepared.target_policy, Some(TargetPolicy::Flying));
        assert!(!prepared.return_to_formation);
        assert_eq!(prepared.guard_chance, -1);
        let mut rows = [row.clone(), row.clone(), row.clone()];
        rows[1].required_flag = Some(73);
        assert_eq!(disabled_by_flags(&rows, &BTreeSet::new())?, vec![1]);
        assert!(disabled_by_flags(&rows, &BTreeSet::from([73]))?.is_empty());
        assert_eq!(
            choice(&rows[1], resonance_battle::ActionKey(1), None)?.action,
            resonance_battle::ActionKey(1)
        );
        row.unsupported_reason = Some("custom action controller".into());
        assert!(
            choice(&row, resonance_battle::ActionKey(0), None)
                .unwrap_err()
                .to_string()
                .contains("custom action controller")
        );
        Ok(())
    }
}

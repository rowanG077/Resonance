//! Initial conditions for controlled enemy-row captures, using real encounters.
//! These helpers do not claim that the ordinary AI would choose the row.
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActionKey, ActorId, Battle, EnemyChoiceId};
use resonance_content::{
    battle_action::{EnemyBehavior, TargetPolicy},
    battle_enemy::document::{self, Entry},
    battle_formation::{Formation, Formations},
    library::Snapshot,
    preparation::JSON_LIMIT,
};
use std::collections::BTreeSet;

#[derive(Debug, serde::Serialize)]
pub(super) struct Plan {
    pub formation: u16,
    pub event_flags: BTreeSet<u16>,
    pub owner_hp_percent: u8,
    pub requires_dead_ally: bool,
    pub entry_budget: u32,
}

fn entry_budget(presentation: Option<&resonance_content::battle_enemy::Presentation>) -> u32 {
    use resonance_content::battle_enemy::Opening;
    let ticks = presentation
        .and_then(|presentation| presentation.opening.as_ref())
        .map_or(0, |opening| match opening {
            Opening::Entrance(entrance) => entrance.ticks,
            Opening::Voice(voice) => voice.ticks,
        });
    (u32::from(ticks) + 180).max(600)
}

impl Plan {
    pub fn read(
        snapshot: &mut Snapshot,
        formations: &Formations,
        enemy: u16,
        row: usize,
        explicit: Option<u16>,
    ) -> Result<Self> {
        let path = snapshot.resolve(document::MAPPINGS, u32::from(enemy), Some("enemy.json"))?;
        let entry: Entry = snapshot.json(&path, JSON_LIMIT)?;
        let action = entry
            .rules
            .actions
            .rows
            .get(row)
            .context("unknown enemy capture row")?;
        ensure!(
            action.execution.is_some(),
            "enemy capture row has no authored execution: {:?}",
            action.unsupported_reason
        );
        let target_policy = action.target_policy.or(entry.rules.target_strategy);
        let requires_dead_ally =
            action.target_allies && target_policy == Some(TargetPolicy::NearestIncapacitated);
        let formation = formation(formations, enemy, row, requires_dead_ally, explicit)?;
        // Selection-only HP gates are metadata, not a reason to trigger an
        // unrelated retreat controller in this controlled-execution fixture.
        let mut owner_hp_percent = 100;
        // Lower HP is initial fixture data; the ordinary entry/warmup runs the
        // actual form-change controller rather than injecting a pose or alias.
        match entry.rules.actions.policy.behavior.as_ref() {
            Some(EnemyBehavior::HealthStage {
                hp_percent,
                final_actions,
                ..
            }) if u8::try_from(row).is_ok_and(|row| final_actions.contains(&row)) => {
                owner_hp_percent = owner_hp_percent.min(*hp_percent);
            }
            Some(EnemyBehavior::Transformation {
                airborne_hp_percent,
                final_actions,
                ..
            }) if u8::try_from(row).is_ok_and(|row| final_actions.contains(&row)) => {
                owner_hp_percent = owner_hp_percent.min(*airborne_hp_percent);
            }
            _ => {}
        }
        ensure!(owner_hp_percent > 0, "enemy capture needs a living owner");
        Ok(Self {
            formation,
            event_flags: action.required_flag.into_iter().collect(),
            owner_hp_percent,
            requires_dead_ally,
            entry_budget: entry_budget(entry.rules.presentation.as_ref()),
        })
    }
}

fn formation(
    formations: &Formations,
    enemy: u16,
    row: usize,
    requires_dead_ally: bool,
    explicit: Option<u16>,
) -> Result<u16> {
    let suitable = |formation: &Formation| {
        // Sword Dancer's later encounter exposes all nine authored rows. The
        // earlier variants deliberately disable rows and hide their body parts.
        let minimum_variant = if enemy == 191 && row >= 2 { 2 } else { 0 };
        (!requires_dead_ally || formation.actors.len() >= 2)
            && formation.actors.iter().any(|actor| {
                actor.variant >= minimum_variant
                    && formation
                        .resources
                        .get(usize::from(actor.resource))
                        .is_some_and(|resource| resource.enemy == enemy)
            })
    };
    if let Some(id) = explicit {
        ensure!(
            formations
                .records
                .get(usize::from(id))
                .is_some_and(suitable),
            "explicit formation {id} cannot supply enemy {enemy} row {row} context"
        );
        return Ok(id);
    }
    formations
        .records
        .iter()
        .enumerate()
        .filter(|(_, formation)| suitable(formation))
        .min_by_key(|(id, formation)| (formation.actors.len(), *id))
        .map(|(id, _)| u16::try_from(id).context("enemy capture formation exceeds request ID"))
        .context("no actual formation supplies the enemy capture context")?
}

/// Retain row identity alongside its prepared action for existing controllers.
pub(super) fn selected_action(
    core: &mut Battle,
    owner: ActorId,
    row: usize,
    expected: ActionKey,
) -> Result<()> {
    let observed = core
        .enemy_choices(owner)
        .and_then(|choices| choices.get(row))
        .context("enemy capture row was not prepared")?
        .action;
    ensure!(
        observed == expected,
        "enemy capture action differs from requested row"
    );
    let selected = core.select_fixture_enemy_row(owner, EnemyChoiceId(row))?;
    ensure!(
        selected == expected,
        "enemy capture selected a different action"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_formation::{Actor, Resource, Settings};

    fn encounter(enemy: u16, variant: u8, allies: usize, reserves: usize) -> Formation {
        Formation {
            settings: Settings {
                play_music: true,
                celebrate: true,
                entry_voice: true,
                escape_restricted: false,
            },
            resources: vec![
                Resource {
                    enemy,
                    hidden_name: false
                };
                1 + reserves
            ],
            actors: vec![
                Actor {
                    variant,
                    ..Default::default()
                };
                1 + allies
            ],
        }
    }

    #[test]
    fn enemy_fixture_waits_for_actual_entrance_without_shortening_it() -> Result<()> {
        use resonance_content::battle_enemy::{Opening, Presentation};
        let entrance: Opening = serde_json::from_value(serde_json::json!({"entrance": {
            "ticks":660,"gravity":0.0,"flash":null,"launches":[],"cues":[],
            "camera":{"radius":1150.0,"height_radius":1500.0,"focus_height":75.0,
                "elevation":5.0,"minimum_elevation":5.0,"yaw":-170.0,"yaw_step":0.0,
                "elevation_step":0.0,"focus_height_step":0.0}
        }}))?;
        let presentation = Presentation {
            opening: Some(entrance),
            defeat: resonance_content::battle_voice::TimedVoice {
                sound: None,
                ticks: 0,
            },
        };
        assert_eq!(entry_budget(None), 600);
        assert_eq!(entry_budget(Some(&presentation)), 840);
        assert!(
            matches!(presentation.opening, Some(Opening::Entrance(ref entrance)) if entrance.ticks == 660)
        );
        Ok(())
    }

    #[test]
    fn enemy_fixture_uses_active_allies_and_respects_explicit_context() -> Result<()> {
        let rows = Formations {
            records: vec![
                encounter(233, 0, 0, 1),
                encounter(233, 0, 2, 0),
                encounter(233, 0, 1, 0),
            ],
            pools: vec![],
        };
        assert_eq!(formation(&rows, 233, 14, false, None)?, 0);
        assert_eq!(formation(&rows, 233, 14, true, None)?, 2);
        assert_eq!(formation(&rows, 233, 14, true, Some(1))?, 1);
        assert!(formation(&rows, 233, 14, true, Some(0)).is_err());
        assert!(formation(&rows, 234, 0, false, None).is_err());
        Ok(())
    }

    #[test]
    fn enemy_fixture_uses_existing_final_sword_dancer_variant() -> Result<()> {
        let rows = Formations {
            records: (0..3)
                .map(|variant| encounter(191, variant, 0, 0))
                .collect(),
            pools: vec![],
        };
        assert_eq!(formation(&rows, 191, 0, false, None)?, 0);
        for row in 2..9 {
            assert_eq!(formation(&rows, 191, row, false, None)?, 2);
            assert!(formation(&rows, 191, row, false, Some(0)).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_ARTE_CATALOGUE and current cooked libraries; plans enemy rows without GPU or AI replay"]
    fn enemy_fixture_all_catalogue_rows_have_real_contexts() -> Result<()> {
        use resonance_content::{
            battle_rules,
            library::{Library, Mappings},
        };
        use serde_json::Value;
        let catalogue: Value = serde_json::from_slice(&std::fs::read(
            std::env::var_os("RESONANCE_ARTE_CATALOGUE").context("set RESONANCE_ARTE_CATALOGUE")?,
        )?)?;
        for (edition, root) in catalogue["libraries"]
            .as_object()
            .context("missing libraries")?
        {
            let library = Library::open(
                std::path::Path::new(root.as_str().context("missing library path")?),
                &[],
            )?;
            let mut snapshot = library.snapshot();
            let named: battle_rules::Formations =
                snapshot.json("battle/formations.json", JSON_LIMIT)?;
            let enemies: Mappings = snapshot.json(document::MAPPINGS, JSON_LIMIT)?;
            let formations = named.prepare(&enemies)?;
            let mut count = 0;
            let mut revival = 0;
            let mut changed_form = 0;
            for row in catalogue["cases"]
                .as_array()
                .context("missing cases")?
                .iter()
                .filter(|row| row["edition"] == *edition && row["category"] == "enemy")
            {
                let enemy = u16::try_from(
                    row["binding"]["enemy"]
                        .as_u64()
                        .context("missing enemy binding")?,
                )?;
                let index = usize::try_from(
                    row["binding"]["row"]
                        .as_u64()
                        .context("missing row binding")?,
                )?;
                let plan = Plan::read(&mut snapshot, &formations, enemy, index, None)
                    .with_context(|| format!("{}", row["id"]))?;
                count += 1;
                revival += usize::from(plan.requires_dead_ally);
                changed_form += usize::from(plan.owner_hp_percent < 100);
            }
            ensure!(count > 0, "no enemy cases were planned");
            eprintln!(
                "{edition}: {count} enemy contexts; {revival} dead-ally; {changed_form} lowered-HP form controllers"
            );
        }
        Ok(())
    }
}
